// SPDX-License-Identifier: GPL-2.0-or-later
//! Navigation parts of the WolOS Design System (ADR 0058 draft): tabs (primary, secondary,
//! scrollable), the navigation bar of a narrow window, the navigation rail, breadcrumbs, the
//! disclosure, the bottom app bar and the status bar. The top app bar, the sidebar (the
//! navigation drawer) and the window's own status line are parts of [`super::widgets::AppWindow`].
//! Specs: `design/components/<id>.toml`; tests: `tests/kit_navigation.rs`; look: `kit.css`.
use super::widgets as w;
use gtk::prelude::*;
use std::rc::Rc;

/// Primary tabs sit at the top of a pane and change all of it; secondary tabs sit inside a pane
/// and change a part of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabKind {
    Primary,
    Secondary,
}

/// A tab list over a stack of panels. `bar` is the tab list (for a toolbar or a pane head),
/// `stack` the panels; `root` holds both.
pub struct Tabs {
    pub root: gtk::Box,
    pub bar: gtk::Box,
    pub stack: gtk::Stack,
    pub tabs: Vec<gtk::ToggleButton>,
}

impl Tabs {
    /// `pages` are (title, panel); `scrollable` puts the tab list in a horizontal scroller for
    /// more tabs than fit (the window keeps its width).
    pub fn new(
        kind: TabKind,
        pages: Vec<(&str, gtk::Widget)>,
        selected: usize,
        scrollable: bool,
        picked: impl Fn(usize) + 'static,
    ) -> Tabs {
        let root = w::vbox(0);
        root.add_css_class("tabs");
        root.add_css_class(match kind {
            TabKind::Primary => "tabs-primary",
            TabKind::Secondary => "tabs-secondary",
        });
        let bar = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(0)
            .accessible_role(gtk::AccessibleRole::TabList)
            .build();
        bar.add_css_class("tab-list");
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .vhomogeneous(false)
            .build();
        stack.add_css_class("tab-panels");
        let picked = Rc::new(picked);
        let mut tabs: Vec<gtk::ToggleButton> = Vec::new();
        for (i, (title, panel)) in pages.into_iter().enumerate() {
            let t = gtk::ToggleButton::builder()
                .label(title)
                .accessible_role(gtk::AccessibleRole::Tab)
                .build();
            t.add_css_class("tab");
            if let Some(f) = tabs.first() {
                t.set_group(Some(f));
            }
            let holder = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .accessible_role(gtk::AccessibleRole::TabPanel)
                .build();
            holder.append(&panel);
            holder.update_relation(&[gtk::accessible::Relation::LabelledBy(&[t.upcast_ref()])]);
            let name = format!("tab-{i}");
            stack.add_named(&holder, Some(&name));
            t.update_relation(&[gtk::accessible::Relation::Controls(&[holder.upcast_ref()])]);
            // Picked before the handler: building the tabs tells nobody.
            t.set_active(i == selected);
            let (st, p) = (stack.downgrade(), picked.clone());
            t.connect_toggled(move |t| {
                t.update_state(&[gtk::accessible::State::Selected(Some(t.is_active()))]);
                if t.is_active() {
                    if let Some(s) = st.upgrade() {
                        s.set_visible_child_name(&name);
                    }
                    p(i);
                }
            });
            bar.append(&t);
            tabs.push(t);
        }
        for (i, t) in tabs.iter().enumerate() {
            t.update_state(&[gtk::accessible::State::Selected(Some(i == selected))]);
        }
        if selected < tabs.len() {
            stack.set_visible_child_name(&format!("tab-{selected}"));
        }
        if scrollable {
            bar.add_css_class("tab-list-scrollable");
            let sc = gtk::ScrolledWindow::builder()
                .child(&bar)
                .hscrollbar_policy(gtk::PolicyType::External)
                .vscrollbar_policy(gtk::PolicyType::Never)
                .build();
            sc.add_css_class("tab-scroller");
            root.append(&sc);
        } else {
            root.append(&bar);
        }
        root.append(&stack);
        Tabs {
            root,
            bar,
            stack,
            tabs,
        }
    }

    pub fn select(&self, i: usize) {
        if let Some(t) = self.tabs.get(i) {
            t.set_active(true);
        }
    }
}

/// One destination of a navigation bar or rail: a 16 px glyph over a short label.
fn destination(icon_name: &str, label: &str, class: &str) -> gtk::ToggleButton {
    let face = w::vbox(w::S1);
    let i = w::icon(icon_name, w::ICON);
    i.add_css_class("destination-icon");
    face.append(&i);
    face.append(&w::label(label, "destination-label"));
    let b = gtk::ToggleButton::builder().child(&face).build();
    b.add_css_class("destination");
    b.add_css_class(class);
    b
}

fn destinations(
    root: &gtk::Box,
    items: &[(&str, &str)],
    selected: usize,
    class: &str,
    picked: impl Fn(usize) + 'static,
) -> Vec<gtk::ToggleButton> {
    let picked = Rc::new(picked);
    let mut v: Vec<gtk::ToggleButton> = Vec::new();
    for (i, (icon_name, label)) in items.iter().enumerate() {
        let b = destination(icon_name, label, class);
        if let Some(f) = v.first() {
            b.set_group(Some(f));
        }
        b.set_active(i == selected);
        let p = picked.clone();
        b.connect_toggled(move |b| {
            if b.is_active() {
                p(i)
            }
        });
        root.append(&b);
        v.push(b);
    }
    v
}

/// The bar of three to five destinations at the bottom of a narrow window (under
/// [`super::widgets::NARROW`]), in place of the sidebar. Every destination takes an equal share.
pub fn navigation_bar(
    items: &[(&str, &str)],
    selected: usize,
    picked: impl Fn(usize) + 'static,
) -> (gtk::Box, Vec<gtk::ToggleButton>) {
    let b = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .homogeneous(true)
        .accessible_role(gtk::AccessibleRole::Navigation)
        .build();
    b.add_css_class("navigation-bar");
    let v = destinations(&b, items, selected, "bar-destination", picked);
    (b, v)
}

/// The rail of three to seven destinations at the start of a window that is too narrow for the
/// sidebar and wide enough not to need the bottom bar (tablets, a half screen).
pub fn navigation_rail(
    items: &[(&str, &str)],
    selected: usize,
    picked: impl Fn(usize) + 'static,
) -> (gtk::Box, Vec<gtk::ToggleButton>) {
    let b = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(w::S1)
        .accessible_role(gtk::AccessibleRole::Navigation)
        .build();
    b.add_css_class("navigation-rail");
    b.set_valign(gtk::Align::Fill);
    let v = destinations(&b, items, selected, "rail-destination", picked);
    (b, v)
}

/// Where a page sits in a hierarchy: each parent is a link, the current page is plain text at
/// the end. `picked(i)` hears a click on parent `i`.
pub fn breadcrumb(trail: &[&str], picked: impl Fn(usize) + 'static) -> gtk::Box {
    let b = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(w::S1)
        .accessible_role(gtk::AccessibleRole::Navigation)
        .build();
    b.add_css_class("breadcrumb");
    b.set_halign(gtk::Align::Start);
    let picked = Rc::new(picked);
    for (i, seg) in trail.iter().enumerate() {
        if i + 1 == trail.len() {
            let l = w::label(seg, "crumb-current");
            l.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            l.add_css_class("may-ellipsize");
            b.append(&l);
            break;
        }
        let p = picked.clone();
        let c = gtk::Button::with_label(seg);
        c.add_css_class("crumb");
        c.connect_clicked(move |_| p(i));
        b.append(&c);
        let sep = w::icon("go-next-symbolic", w::ICON);
        sep.add_css_class("crumb-separator");
        b.append(&sep);
    }
    b
}

/// A header that shows and hides the part under it (advanced options, details). The header is a
/// button with a chevron that turns; the state is "expanded" for assistive technology.
pub fn disclosure(title: &str, open: bool, body: &impl IsA<gtk::Widget>) -> gtk::Expander {
    let e = gtk::Expander::builder()
        .label(title)
        .expanded(open)
        .child(body)
        .build();
    e.add_css_class("disclosure");
    e
}

/// The bar of two to four actions at the bottom of a narrow window or a touch surface (the
/// toolbar keeps them on a wide window). Actions sit at the start, the one primary action at the
/// end.
pub fn bottom_app_bar(actions: &[gtk::Widget], primary: Option<&gtk::Widget>) -> gtk::Box {
    let b = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(w::S2)
        .accessible_role(gtk::AccessibleRole::Toolbar)
        .build();
    b.add_css_class("bottom-app-bar");
    for a in actions {
        a.set_valign(gtk::Align::Center);
        b.append(a);
    }
    if let Some(p) = primary {
        b.append(&w::spacer());
        p.set_valign(gtk::Align::Center);
        b.append(p);
    }
    b
}

/// A status line for counts and quiet facts ("12 items, 3 selected"): never an action. The
/// window's own one is [`super::widgets::AppWindow::status`]; this is the same line for a pane.
pub fn status_bar(text: &str) -> gtk::Label {
    let l = gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .accessible_role(gtk::AccessibleRole::Status)
        .build();
    l.add_css_class("secondary");
    l.add_css_class("status-bar");
    l
}
