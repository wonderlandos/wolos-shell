//! Draws a `shared::view` tree (ADR 0020) with Kit parts: app windows from libwol
//! (`ui::app_host`) and Lua desktop widgets. What the person does goes to the [`Sink`].
//! A new tree of the same shape updates the parts in place (a text field keeps its focus and
//! cursor while the app changes a label next to it); a different shape rebuilds that branch.
use crate::kit::data as d;
use crate::kit::widgets as w;
use gtk::{gio, glib, prelude::*};
use shared::view::{Menu, MenuEntry, Node, Role, Tool, Toolbar};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// What the person did to the part with this id.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Clicked(u32),
    Edited(u32, String),
    Toggled(u32, bool),
    Slid(u32, f64),
    /// Menu or toolbar id, and the key chosen.
    Chose(u32, String),
}

/// Receives [`Act`]s; quiet while the view itself sets values.
pub struct Sink {
    send: Box<dyn Fn(Act)>,
    quiet: Cell<bool>,
}

impl Sink {
    pub fn new(send: impl Fn(Act) + 'static) -> Rc<Sink> {
        Rc::new(Sink {
            send: Box::new(send),
            quiet: Cell::new(false),
        })
    }

    fn act(&self, a: Act) {
        if !self.quiet.get() {
            (self.send)(a);
        }
    }
}

fn role(r: Role) -> d::Role {
    match r {
        Role::Display => d::Role::Display,
        Role::Large => d::Role::Large,
        Role::Title => d::Role::Title,
        Role::Body => d::Role::Body,
        Role::Secondary => d::Role::Secondary,
        Role::Caption => d::Role::Caption,
    }
}

fn spacing(s: u8) -> i32 {
    i32::from(s)
}

/// One drawn node: its part and its children's.
struct Drawn {
    node: Node,
    widget: gtk::Widget,
    /// The part a value lives in when `widget` is a wrapper (toggle with a label, list rows).
    inner: Option<gtk::Widget>,
    /// The id the part's signal handler reports (patched with the node).
    id: Rc<Cell<u32>>,
    children: Vec<Drawn>,
}

fn image(source: &str, size: u16) -> gtk::Image {
    let i = if source.starts_with('/') {
        gtk::Image::from_file(source)
    } else {
        w::icon(source, i32::from(size))
    };
    i.set_pixel_size(i32::from(size));
    i.set_valign(gtk::Align::Center);
    i
}

fn slider(min: f64, max: f64, value: f64, id: Rc<Cell<u32>>, sink: &Rc<Sink>) -> gtk::Scale {
    let s = gtk::Scale::with_range(gtk::Orientation::Horizontal, min, max, (max - min) / 100.0);
    s.set_value(value);
    s.set_draw_value(false);
    s.set_hexpand(true);
    s.add_css_class("slider");
    // The knob stays inside its trough (kit.css), in a list row or not.
    s.add_css_class("row-slider");
    let sink = sink.clone();
    s.connect_value_changed(move |s| sink.act(Act::Slid(id.get(), s.value())));
    s
}

fn draw(node: &Node, sink: &Rc<Sink>) -> Drawn {
    let id = Rc::new(Cell::new(0));
    let mut inner = None;
    let mut children = Vec::new();
    let widget: gtk::Widget = match node {
        Node::Stack {
            spacing: s,
            children: c,
        }
        | Node::Row {
            spacing: s,
            children: c,
        } => {
            let b = if matches!(node, Node::Row { .. }) {
                w::hbox(spacing(*s))
            } else {
                w::vbox(spacing(*s))
            };
            for n in c {
                let d = draw(n, sink);
                if matches!(node, Node::Row { .. }) {
                    d.widget.set_valign(gtk::Align::Center);
                    // A spacer in a row pushes sideways only.
                    if matches!(n, Node::Spacer) {
                        d.widget.set_vexpand(false);
                    }
                }
                b.append(&d.widget);
                children.push(d);
            }
            b.upcast()
        }
        Node::List { children: c } => {
            let rows: Vec<gtk::Widget> = c
                .iter()
                .map(|n| {
                    let d = draw(n, sink);
                    let row = w::hbox(w::S3);
                    row.add_css_class("list-row");
                    d.widget.set_hexpand(true);
                    d.widget.set_valign(gtk::Align::Center);
                    row.append(&d.widget);
                    children.push(d);
                    row.upcast()
                })
                .collect();
            w::section("", &rows).upcast()
        }
        Node::Label { text, role: r } => d::text(text, role(*r), d::Tone::Text).upcast(),
        Node::Button { id: i, label } => {
            id.set(*i);
            let (sink, id) = (sink.clone(), id.clone());
            w::button(label, w::Kind::Default, move || {
                sink.act(Act::Clicked(id.get()))
            })
            .upcast()
        }
        Node::Spacer => {
            let s = w::spacer();
            s.set_vexpand(true);
            s.upcast()
        }
        Node::TextField {
            id: i,
            text,
            placeholder,
        } => {
            id.set(*i);
            let e = w::text_field(placeholder);
            e.set_text(text);
            e.set_hexpand(true);
            let (sink, id) = (sink.clone(), id.clone());
            e.connect_changed(move |e| sink.act(Act::Edited(id.get(), e.text().into())));
            e.upcast()
        }
        Node::Toggle { id: i, on, label } => {
            id.set(*i);
            let (s2, id2) = (sink.clone(), id.clone());
            let t = w::toggle(*on, move |on| s2.act(Act::Toggled(id2.get(), on)));
            if label.is_empty() {
                t.upcast()
            } else {
                let row = w::hbox(w::S3);
                row.append(&w::label(label, ""));
                row.append(&w::spacer());
                row.append(&t);
                inner = Some(t.upcast());
                row.upcast()
            }
        }
        Node::Slider {
            id: i,
            value,
            min,
            max,
        } => {
            id.set(*i);
            slider(*min, *max, *value, id.clone(), sink).upcast()
        }
        Node::Image { source, size } => image(source, *size).upcast(),
    };
    Drawn {
        node: node.clone(),
        widget,
        inner,
        id,
        children,
    }
}

fn same_shape(a: &Node, b: &Node) -> bool {
    use Node::*;
    match (a, b) {
        (
            Stack {
                spacing: s1,
                children: c1,
            },
            Stack {
                spacing: s2,
                children: c2,
            },
        )
        | (
            Row {
                spacing: s1,
                children: c1,
            },
            Row {
                spacing: s2,
                children: c2,
            },
        ) => s1 == s2 && c1.len() == c2.len(),
        (List { children: c1 }, List { children: c2 }) => c1.len() == c2.len(),
        (Label { role: r1, .. }, Label { role: r2, .. }) => r1 == r2,
        (Toggle { label: l1, .. }, Toggle { label: l2, .. }) => l1.is_empty() == l2.is_empty(),
        (Slider { min: a, max: b, .. }, Slider { min: c, max: e, .. }) => a == c && b == e,
        (Image { source: a, size: b }, Image { source: c, size: e }) => a == c && b == e,
        (Button { .. }, Button { .. })
        | (Spacer, Spacer)
        | (TextField { .. }, TextField { .. }) => true,
        _ => false,
    }
}

/// Bring `d` to `new`: in place when the shape is the same, else a new part in its place.
/// Returns the new part when it was replaced (the parent swaps it in).
fn patch(d: &mut Drawn, new: &Node, sink: &Rc<Sink>) -> Option<gtk::Widget> {
    if !same_shape(&d.node, new) {
        let old = d.widget.clone();
        *d = draw(new, sink);
        let parent = old.parent()?;
        if let Some(b) = parent.downcast_ref::<gtk::Box>() {
            b.insert_child_after(&d.widget, Some(&old));
            b.remove(&old);
        }
        return Some(d.widget.clone());
    }
    match new {
        Node::Stack { children, .. } | Node::Row { children, .. } | Node::List { children } => {
            for (c, n) in d.children.iter_mut().zip(children) {
                patch(c, n, sink);
            }
        }
        Node::Label { text, .. } => {
            if let Some(l) = d.widget.downcast_ref::<gtk::Label>() {
                let t = if matches!(
                    new,
                    Node::Label {
                        role: Role::Caption,
                        ..
                    }
                ) {
                    text.to_uppercase()
                } else {
                    text.clone()
                };
                if l.label() != t {
                    l.set_label(&t);
                }
            }
        }
        Node::Button { id, label } => {
            d.id.set(*id);
            if let Some(b) = d.widget.downcast_ref::<gtk::Button>()
                && b.label().as_deref() != Some(label.as_str())
            {
                b.set_label(label);
            }
        }
        Node::TextField {
            id,
            text,
            placeholder,
        } => {
            d.id.set(*id);
            if let Some(e) = d.widget.downcast_ref::<gtk::Entry>() {
                if e.text() != *text {
                    e.set_text(text);
                }
                e.set_placeholder_text(Some(placeholder));
            }
        }
        Node::Toggle { id, on, label } => {
            d.id.set(*id);
            let part = d.inner.as_ref().unwrap_or(&d.widget);
            if let Some(s) = part.downcast_ref::<gtk::Switch>()
                && s.is_active() != *on
            {
                s.set_active(*on);
            }
            if d.inner.is_some()
                && let Some(l) = d.widget.first_child().and_downcast::<gtk::Label>()
            {
                l.set_label(label);
            }
        }
        Node::Slider { id, value, .. } => {
            d.id.set(*id);
            if let Some(s) = d.widget.downcast_ref::<gtk::Scale>()
                && s.value() != *value
            {
                s.set_value(*value);
            }
        }
        Node::Spacer | Node::Image { .. } => {}
    }
    d.node = new.clone();
    None
}

/// A drawn tree that follows new versions of itself.
pub struct View {
    root: gtk::Box,
    drawn: RefCell<Drawn>,
    sink: Rc<Sink>,
}

impl View {
    pub fn new(node: &Node, sink: Rc<Sink>) -> View {
        let root = w::vbox(0);
        root.add_css_class("app-view");
        let drawn = draw(node, &sink);
        root.append(&drawn.widget);
        View {
            root,
            drawn: RefCell::new(drawn),
            sink,
        }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    /// Show `node`, reusing every part that kept its place.
    pub fn update(&self, node: &Node) {
        self.sink.quiet.set(true);
        let mut d = self.drawn.borrow_mut();
        if !same_shape(&d.node, node) {
            self.root.remove(&d.widget);
            *d = draw(node, &self.sink);
            self.root.append(&d.widget);
        } else {
            patch(&mut d, node, &self.sink);
        }
        self.sink.quiet.set(false);
    }
}

/// A window's menu bar and toolbar. Rebuilt only when the app's data changes, so an open menu
/// stays open while the app updates a label.
pub struct Chrome {
    window: gtk::ApplicationWindow,
    bar: gtk::Box,
    actions: gtk::Box,
    sink: Rc<Sink>,
    drawn: RefCell<(Vec<Menu>, Option<Toolbar>)>,
    /// Action names in use, for taking their shortcuts off again.
    names: RefCell<Vec<String>>,
}

/// GIO action name of menu `menu`'s entry `entry` (keys may hold `_`, which GIO refuses).
fn action_name(menu: u32, entry: usize) -> String {
    format!("kit-{menu}-{entry}")
}

impl Chrome {
    /// A bar under the title row, hidden until the app has menus; the toolbar lives in the title
    /// row's action box.
    pub fn new(window: &gtk::ApplicationWindow, aw: &w::AppWindow, sink: Rc<Sink>) -> Chrome {
        let bar = w::hbox(0);
        bar.add_css_class("app-menubar");
        bar.set_margin_start(w::S2);
        bar.set_visible(false);
        if let Some(head) = aw.root.first_child() {
            aw.root.insert_child_after(&bar, Some(&head));
        }
        Chrome {
            window: window.clone(),
            bar,
            actions: aw.actions.clone(),
            sink,
            drawn: RefCell::default(),
            names: RefCell::default(),
        }
    }

    pub fn update(&self, app: &gtk::Application, menus: &[Menu], toolbar: Option<&Toolbar>) {
        {
            let d = self.drawn.borrow();
            if d.0 == menus && d.1.as_ref() == toolbar {
                return;
            }
        }
        *self.drawn.borrow_mut() = (menus.to_vec(), toolbar.cloned());
        self.menus(app, menus);
        self.tools(toolbar);
    }

    fn menus(&self, app: &gtk::Application, menus: &[Menu]) {
        for n in self.names.borrow_mut().drain(..) {
            self.window.remove_action(&n);
            app.set_accels_for_action(&format!("win.{n}"), &[]);
        }
        let bar = gio::Menu::new();
        for m in menus {
            let mut section = gio::Menu::new();
            let sub = gio::Menu::new();
            for (i, e) in m.entries.iter().enumerate() {
                match e {
                    MenuEntry::Separator => {
                        sub.append_section(None, &section);
                        section = gio::Menu::new();
                    }
                    MenuEntry::Item {
                        key,
                        label,
                        shortcut,
                        enabled,
                        checked,
                    } => {
                        let name = action_name(m.id, i);
                        // Only a checked item carries state: plain items draw no tick mark.
                        let a = if *checked {
                            gio::SimpleAction::new_stateful(&name, None, &true.to_variant())
                        } else {
                            gio::SimpleAction::new(&name, None)
                        };
                        a.set_enabled(*enabled);
                        let (sink, id, key) = (self.sink.clone(), m.id, key.clone());
                        a.connect_activate(move |_, _: Option<&glib::Variant>| {
                            sink.act(Act::Chose(id, key.clone()))
                        });
                        self.window.add_action(&a);
                        let full = format!("win.{name}");
                        section.append(Some(label), Some(&full));
                        if let Some(accel) = crate::app_host::accelerator(shortcut) {
                            app.set_accels_for_action(&full, &[accel.as_str()]);
                        }
                        self.names.borrow_mut().push(name);
                    }
                }
            }
            sub.append_section(None, &section);
            bar.append_submenu(Some(&m.title), &sub);
        }
        while let Some(c) = self.bar.first_child() {
            self.bar.remove(&c);
        }
        self.bar.set_visible(!menus.is_empty());
        if !menus.is_empty() {
            self.bar
                .append(&gtk::PopoverMenuBar::from_model(Some(&bar)));
        }
    }

    fn tools(&self, toolbar: Option<&Toolbar>) {
        while let Some(c) = self.actions.first_child() {
            self.actions.remove(&c);
        }
        let Some(t) = toolbar else { return };
        for tool in &t.tools {
            match tool {
                Tool::Spacer => self.actions.append(&w::spacer()),
                Tool::Button {
                    key,
                    label,
                    icon,
                    enabled,
                } => {
                    let (sink, id, k) = (self.sink.clone(), t.id, key.clone());
                    let press = move || sink.act(Act::Chose(id, k.clone()));
                    let b: gtk::Button = if icon.is_empty() {
                        w::button(label, w::Kind::Default, press)
                    } else {
                        w::icon_button(icon, label, press)
                    };
                    b.set_tooltip_text(Some(label));
                    b.set_sensitive(*enabled);
                    b.set_valign(gtk::Align::Center);
                    self.actions.append(&b);
                }
            }
        }
    }
}

/// Tallest an app window grows to fit its content before it scrolls.
pub const APP_MAX_HEIGHT: i32 = 640;

/// An app window's content: the Kit app frame around the view.
pub fn app_frame(view: &View) -> w::AppWindow {
    let aw = w::AppWindow::new(false, false);
    let body = view.widget();
    body.set_margin_top(w::S4);
    body.set_margin_bottom(w::S4);
    body.set_margin_start(w::S4);
    body.set_margin_end(w::S4);
    let scroll = gtk::ScrolledWindow::builder()
        .child(body)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .propagate_natural_width(true)
        .max_content_height(APP_MAX_HEIGHT)
        .vexpand(true)
        .build();
    aw.content.add_named(&scroll, Some("main"));
    aw
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes() {
        let l = |t: &str| Node::Label {
            text: t.into(),
            role: Role::Body,
        };
        assert!(same_shape(&l("a"), &l("b")), "text changes in place");
        let s = |c: Vec<Node>| Node::Stack {
            spacing: 8,
            children: c,
        };
        assert!(same_shape(&s(vec![l("a")]), &s(vec![l("b")])));
        assert!(!same_shape(&s(vec![l("a")]), &s(vec![])));
        assert!(!same_shape(&l("a"), &Node::Spacer));
        let t = |label: &str| Node::Toggle {
            id: 1,
            on: true,
            label: label.into(),
        };
        assert!(same_shape(&t("a"), &t("b")));
        assert!(!same_shape(&t("a"), &t("")), "the wrapper comes and goes");
    }
}
