// SPDX-License-Identifier: GPL-2.0-or-later
//! Containers and collections of the WolOS Design System (ADR 0058 draft): cards (elevated,
//! filled, outlined), list items with a leading picture, dividers, section headers, the carousel,
//! skeletons, tables, trees, the split view and the floating action button. Specs:
//! `design/components/<id>.toml`; tests: `tests/kit_collection.rs`; look: `kit.css` ("Collections").
use super::widgets as w;
use gtk::{gio, prelude::*};
use std::collections::HashMap;
use std::rc::Rc;

/// How a card stands off the page: a shadow (elevated), a fill (filled) or a hairline (outlined).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardStyle {
    Elevated,
    Filled,
    Outlined,
}

/// A card: one subject, radius 12, the 12 px inset on all four sides.
pub fn card(style: CardStyle, body: &impl IsA<gtk::Widget>) -> gtk::Box {
    let b = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(w::S2)
        .accessible_role(gtk::AccessibleRole::Group)
        .build();
    b.add_css_class("surface-card");
    b.add_css_class(match style {
        CardStyle::Elevated => "card-elevated",
        CardStyle::Filled => "card-filled",
        CardStyle::Outlined => "card-outlined",
    });
    b.append(body);
    b
}

/// What leads a list item: nothing, a 16 px glyph, an avatar (initials) or a 32 px picture.
pub enum Lead<'a> {
    None,
    Icon(&'a str),
    Avatar(&'a str),
    Image(&'a str),
}

/// A list item of one, two or three lines: the title, then the supporting text (which wraps to two
/// lines for a three-line item), a leading part and an optional trailing control or text. The
/// item is [`super::widgets::list_row`] with a leading part; the trailing control is named by
/// the title.
pub fn list_item(
    lead: Lead,
    title: &str,
    supporting: &str,
    lines: u8,
    trailing: Option<&gtk::Widget>,
) -> gtk::Box {
    let row = w::list_row("", title, supporting, trailing);
    if lines >= 3 {
        row.add_css_class("list-item-3");
    }
    if lines >= 3
        && let Some(sub) = row
            .first_child()
            .and_then(|t| t.last_child())
            .and_downcast::<gtk::Label>()
    {
        sub.set_lines(2);
        sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
        sub.add_css_class("may-ellipsize");
    }
    let lead_w: Option<gtk::Widget> = match lead {
        Lead::None => None,
        Lead::Icon(n) => Some(w::icon(n, w::ICON).upcast()),
        Lead::Avatar(name) => Some(w::avatar(name, w::ICON_M).upcast()),
        Lead::Image(n) => Some(w::icon(n, w::ICON_M).upcast()),
    };
    if let Some(l) = lead_w {
        // Centred on the row like every part of a row (HIG section 6), also beside three lines.
        l.set_valign(gtk::Align::Center);
        row.prepend(&l);
    }
    row
}

/// A hairline between groups of content. `inset` keeps it off the edges by the 12 px inset
/// (between rows of a card); a full divider separates sections.
pub fn divider(inset: bool) -> gtk::Separator {
    let s = gtk::Separator::new(gtk::Orientation::Horizontal);
    s.add_css_class("divider");
    if inset {
        s.add_css_class("divider-inset");
    }
    s
}

/// The heading over a group of rows or cards, with at most one text action at the end ("See
/// All").
pub fn section_header(title: &str, action: Option<&gtk::Button>) -> gtk::Box {
    let b = w::hbox(w::S2);
    b.add_css_class("section-header");
    let t = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .hexpand(true)
        .accessible_role(gtk::AccessibleRole::Heading)
        .build();
    t.add_css_class("heading");
    b.append(&t);
    if let Some(a) = action {
        a.set_valign(gtk::Align::Center);
        b.append(a);
    }
    b
}

/// A row of items that scrolls sideways (featured apps, photos), with a previous and next
/// button at the ends that move by one item. `prev` and `next` are the buttons' names.
pub fn carousel(items: &[gtk::Widget], (prev, next): (&str, &str)) -> gtk::Box {
    let b = w::hbox(w::S2);
    b.add_css_class("carousel");
    let strip = w::hbox(w::S2);
    strip.add_css_class("carousel-strip");
    for i in items {
        strip.append(i);
    }
    let sc = gtk::ScrolledWindow::builder()
        .child(&strip)
        .hscrollbar_policy(gtk::PolicyType::External)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .hexpand(true)
        .build();
    let step = |sc: &gtk::ScrolledWindow, by: f64| {
        let a = sc.hadjustment();
        let item = a.page_size() / 2.0;
        a.set_value((a.value() + by * item).clamp(a.lower(), a.upper() - a.page_size()));
    };
    let (s1, s2) = (sc.downgrade(), sc.downgrade());
    let back = w::icon_button("go-previous-symbolic", prev, move || {
        if let Some(sc) = s1.upgrade() {
            step(&sc, -1.0)
        }
    });
    let fwd = w::icon_button("go-next-symbolic", next, move || {
        if let Some(sc) = s2.upgrade() {
            step(&sc, 1.0)
        }
    });
    for x in [&back, &fwd] {
        x.add_css_class("carousel-step");
        x.set_valign(gtk::Align::Center);
    }
    b.append(&back);
    b.append(&sc);
    b.append(&fwd);
    b
}

/// The shape of content still loading: grey bars where the lines will be (and a disc for an
/// avatar). Busy for assistive technology, named by `name` ("Loading apps").
pub fn skeleton(lines: u8, avatar: bool, name: &str) -> gtk::Box {
    let b = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(w::S3)
        .accessible_role(gtk::AccessibleRole::ProgressBar)
        .build();
    b.add_css_class("skeleton");
    b.update_property(&[gtk::accessible::Property::Label(name)]);
    b.update_state(&[gtk::accessible::State::Busy(true)]);
    if avatar {
        let a = w::hbox(0);
        a.add_css_class("skeleton-avatar");
        a.set_valign(gtk::Align::Center);
        b.append(&a);
    }
    let col = w::vbox(w::S2);
    col.set_hexpand(true);
    col.set_valign(gtk::Align::Center);
    for i in 0..lines.max(1) {
        let l = w::hbox(0);
        l.add_css_class("skeleton-line");
        // The last line is shorter, like the end of a paragraph.
        if i + 1 == lines.max(1) && lines > 1 {
            l.add_css_class("skeleton-short");
            l.set_halign(gtk::Align::Start);
        }
        col.append(&l);
    }
    b.append(&col);
    b
}

/// A table: column titles over rows of text, one cell per column, a row selectable. Numbers are
/// right-aligned in tabular figures (`numeric` lists those columns).
pub fn table(columns: &[&str], numeric: &[usize], rows: Vec<Vec<String>>) -> gtk::ColumnView {
    let rows = Rc::new(rows);
    let model = gtk::StringList::new(&[]);
    for i in 0..rows.len() {
        model.append(&i.to_string());
    }
    let sel = gtk::SingleSelection::builder()
        .model(&model)
        .autoselect(false)
        .build();
    let cv = gtk::ColumnView::builder()
        .model(&sel)
        .show_row_separators(true)
        .build();
    cv.add_css_class("table");
    for (ci, title) in columns.iter().enumerate() {
        let f = gtk::SignalListItemFactory::new();
        let right = numeric.contains(&ci);
        f.connect_setup(move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let l = w::label("", "");
            if right {
                l.set_xalign(1.0);
                l.add_css_class("tabular");
            }
            l.set_ellipsize(gtk::pango::EllipsizeMode::End);
            l.add_css_class("may-ellipsize");
            item.set_child(Some(&l));
        });
        let r = rows.clone();
        f.connect_bind(move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let idx = item
                .item()
                .and_downcast::<gtk::StringObject>()
                .and_then(|s| s.string().parse::<usize>().ok());
            if let (Some(l), Some(i)) = (item.child().and_downcast::<gtk::Label>(), idx) {
                l.set_label(
                    r.get(i)
                        .and_then(|row| row.get(ci))
                        .map_or("", String::as_str),
                );
            }
        });
        let col = gtk::ColumnViewColumn::builder()
            .title(*title)
            .factory(&f)
            .expand(ci == 0)
            .build();
        cv.append_column(&col);
    }
    cv
}

/// One node of a [`tree`]: its label and its children.
pub struct TreeNode {
    pub label: String,
    pub children: Vec<TreeNode>,
}

/// A tree: rows that open and close to show their children, with a chevron and an indent of 16
/// per level. Right and Left open and close the focused row (the toolkit's tree expander).
pub fn tree(roots: Vec<TreeNode>, open: bool) -> gtk::ListView {
    // Each node gets a path id ("0", "0/2"); the model holds the ids, the map their labels.
    let mut labels: HashMap<String, String> = HashMap::new();
    let mut kids: HashMap<String, Vec<String>> = HashMap::new();
    fn walk(
        n: &[TreeNode],
        parent: &str,
        labels: &mut HashMap<String, String>,
        kids: &mut HashMap<String, Vec<String>>,
    ) {
        for (i, c) in n.iter().enumerate() {
            let id = if parent.is_empty() {
                i.to_string()
            } else {
                format!("{parent}/{i}")
            };
            labels.insert(id.clone(), c.label.clone());
            kids.entry(parent.to_string()).or_default().push(id.clone());
            walk(&c.children, &id, labels, kids);
        }
    }
    walk(&roots, "", &mut labels, &mut kids);
    let kids = Rc::new(kids);
    let labels = Rc::new(labels);
    let list = |ids: &[String]| {
        let m = gtk::StringList::new(&[]);
        for i in ids {
            m.append(i);
        }
        m
    };
    let root = list(kids.get("").map_or(&[][..], Vec::as_slice));
    let k2 = kids.clone();
    let tm = gtk::TreeListModel::new(root, false, open, move |obj| {
        let id = obj.downcast_ref::<gtk::StringObject>()?.string();
        let c = k2.get(id.as_str())?;
        let m = gtk::StringList::new(&[]);
        for i in c {
            m.append(i);
        }
        Some(m.upcast::<gio::ListModel>())
    });
    let sel = gtk::SingleSelection::builder()
        .model(&tm)
        .autoselect(false)
        .build();
    let f = gtk::SignalListItemFactory::new();
    f.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let ex = gtk::TreeExpander::new();
        ex.set_indent_for_icon(true);
        ex.set_child(Some(&w::label("", "")));
        item.set_child(Some(&ex));
    });
    f.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let Some(row) = item.item().and_downcast::<gtk::TreeListRow>() else {
            return;
        };
        let Some(ex) = item.child().and_downcast::<gtk::TreeExpander>() else {
            return;
        };
        let text = row
            .item()
            .and_downcast::<gtk::StringObject>()
            .and_then(|s| labels.get(s.string().as_str()).cloned())
            .unwrap_or_default();
        if let Some(l) = ex.child().and_downcast::<gtk::Label>() {
            l.set_label(&text);
        }
        ex.set_list_row(Some(&row));
    });
    let lv = gtk::ListView::builder().model(&sel).factory(&f).build();
    lv.add_css_class("tree");
    lv
}

/// Two panes side by side with a handle between them that a person drags (a list and its
/// detail, a sidebar of one's own). The start pane keeps its width when the window grows.
pub fn split_view(
    start: &impl IsA<gtk::Widget>,
    end: &impl IsA<gtk::Widget>,
    position: i32,
) -> gtk::Paned {
    let p = gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .start_child(start)
        .end_child(end)
        .resize_start_child(false)
        .shrink_start_child(false)
        .shrink_end_child(false)
        .position(position)
        .wide_handle(false)
        .build();
    p.add_css_class("split-view");
    p
}

/// The floating action button: the one main action of a touch or narrow surface that has no
/// toolbar for it (a new message in a phone-sized window). A 16 px glyph in a 40 px accent
/// square of radius 12; `label` makes it the extended pill with words ("New Note"). Its name is
/// the label, or the tooltip when it shows a glyph only.
pub fn fab(
    icon_name: &str,
    label: &str,
    tooltip: &str,
    clicked: impl Fn() + 'static,
) -> gtk::Button {
    let face = w::hbox(w::S2);
    face.set_halign(gtk::Align::Center);
    face.append(&w::icon(icon_name, w::ICON));
    let b = gtk::Button::builder()
        .child(&face)
        .halign(gtk::Align::End)
        .valign(gtk::Align::End)
        .build();
    b.add_css_class("fab");
    if label.is_empty() {
        b.set_tooltip_text(Some(tooltip));
    } else {
        face.append(&w::label(label, ""));
        b.add_css_class("fab-extended");
    }
    b.update_property(&[gtk::accessible::Property::Label(if label.is_empty() {
        tooltip
    } else {
        label
    })]);
    b.connect_clicked(move |_| clicked());
    b
}
