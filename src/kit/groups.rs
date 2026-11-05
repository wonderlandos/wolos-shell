// SPDX-License-Identifier: GPL-2.0-or-later
//! Groups of actions and the drawer of the WolOS Design System (ADR 0058 draft, DS-COMPONENTS):
//! the split button, the connected button group, the floating toolbar, the navigation drawer and
//! the date input. Specs: `design/components/{split-button,button-group,floating-toolbar,
//! navigation-drawer,date-input}.toml`; tests: `tests/kit_groups.rs`; look: `kit.css`.
use super::widgets as w;
use crate::widgets::calendar::days_in_month;
use gtk::prelude::*;
use std::rc::Rc;

/// A box with the accessible role it needs (a role is chosen when the widget is built).
fn group_box(o: gtk::Orientation, spacing: i32, role: gtk::AccessibleRole) -> gtk::Box {
    gtk::Box::builder()
        .orientation(o)
        .spacing(spacing)
        .accessible_role(role)
        .build()
}

/// A main action with a menu of related ones beside it ("Save", then Save As and Save a Copy):
/// the label button and a 24 px arrow, 2 px apart, both in the primary fill. `menu_name` names
/// the arrow for assistive technology ("More Ways to Save").
pub fn split_button(
    label: &str,
    menu_name: &str,
    items: Vec<w::MenuItem>,
    clicked: impl Fn() + 'static,
) -> gtk::Box {
    let root = group_box(gtk::Orientation::Horizontal, 2, gtk::AccessibleRole::Group);
    root.add_css_class("split-button");
    root.set_halign(gtk::Align::Start);
    let main = w::button(label, w::Kind::Primary, clicked);
    main.add_css_class("split-main");
    let arrow = gtk::MenuButton::builder()
        .icon_name("pan-down-symbolic")
        .tooltip_text(menu_name)
        .popover(&w::menu(items))
        .has_frame(false)
        .build();
    arrow.add_css_class("split-arrow");
    arrow.update_property(&[gtk::accessible::Property::Label(menu_name)]);
    root.append(&main);
    root.append(&arrow);
    root
}

/// Buttons that belong together, joined into one block with a hairline between them (Zoom Out,
/// Zoom In, Full Screen). Each one still acts alone; for exactly one pick use the segmented control.
/// `name` names the group.
pub fn button_group(name: &str, buttons: &[gtk::Button]) -> gtk::Box {
    let root = group_box(gtk::Orientation::Horizontal, 1, gtk::AccessibleRole::Group);
    root.add_css_class("button-group");
    root.set_halign(gtk::Align::Start);
    root.update_property(&[gtk::accessible::Property::Label(name)]);
    for b in buttons {
        b.add_css_class("group-button");
        root.append(b);
    }
    root
}

/// A pill that floats over content with the few actions of that content (an image viewer's
/// undo, redo, copy), at most one of them the primary. `name` names the toolbar.
pub fn floating_toolbar(
    name: &str,
    actions: &[gtk::Button],
    primary: Option<&gtk::Button>,
) -> gtk::Box {
    let root = group_box(
        gtk::Orientation::Horizontal,
        w::S1,
        gtk::AccessibleRole::Toolbar,
    );
    root.add_css_class("floating-toolbar");
    root.set_halign(gtk::Align::Center);
    root.update_property(&[gtk::accessible::Property::Label(name)]);
    for a in actions {
        root.append(a);
    }
    if let Some(p) = primary {
        p.add_css_class("floating-primary");
        root.append(p);
    }
    root
}

/// The modal drawer of a window too narrow for its sidebar: a title, then the destinations as
/// rows of a glyph and a label, the picked one tinted. It is the navigation bar's and the rail's
/// bigger sibling; it opens over the content and closes when a destination is picked.
pub fn navigation_drawer(
    title: &str,
    items: &[(&str, &str)],
    selected: usize,
    picked: impl Fn(usize) + 'static,
) -> (gtk::Box, Vec<gtk::ToggleButton>) {
    let root = group_box(
        gtk::Orientation::Vertical,
        w::S1,
        gtk::AccessibleRole::Navigation,
    );
    root.add_css_class("navigation-drawer");
    let head = w::label(title, "title");
    head.add_css_class("drawer-title");
    head.set_xalign(0.0);
    root.append(&head);
    root.update_property(&[gtk::accessible::Property::Label(title)]);
    let picked = Rc::new(picked);
    let mut v: Vec<gtk::ToggleButton> = Vec::new();
    for (i, (icon_name, label)) in items.iter().enumerate() {
        let face = w::hbox(w::S3);
        face.append(&w::icon(icon_name, w::ICON));
        let text = w::label(label, "");
        text.set_xalign(0.0);
        face.append(&text);
        let b = gtk::ToggleButton::builder().child(&face).build();
        b.add_css_class("drawer-item");
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
    (root, v)
}

/// A day typed as `YYYY-MM-DD`; `None` unless it is a real day.
pub fn parse_date(text: &str) -> Option<(i32, u32, u8)> {
    let mut p = text.trim().split('-');
    let (y, m, d) = (p.next()?, p.next()?, p.next()?);
    if p.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    let (y, m, d): (i32, u32, u8) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    ((1..=12).contains(&m) && d >= 1 && u32::from(d) <= days_in_month(y, m)).then_some((y, m, d))
}

/// A date typed in a field, with the calendar button beside it. The field says the shape it
/// wants (`2026-10-26`); what is not a real day marks the field invalid, in words an assistive
/// technology reads, never in colour alone. `open_picker` runs for the calendar button, `typed`
/// hears a real day.
pub struct DateInput {
    pub root: gtk::Box,
    pub entry: gtk::Entry,
    pub button: gtk::Button,
}

impl DateInput {
    pub fn new(
        name: &str,
        picker_name: &str,
        open_picker: impl Fn() + 'static,
        typed: impl Fn(i32, u32, u8) + 'static,
    ) -> DateInput {
        let root = group_box(
            gtk::Orientation::Horizontal,
            w::S2,
            gtk::AccessibleRole::Group,
        );
        root.add_css_class("date-input");
        root.set_halign(gtk::Align::Start);
        let entry = w::text_field("YYYY-MM-DD");
        entry.set_width_chars(12);
        entry.set_max_length(10);
        entry.update_property(&[gtk::accessible::Property::Label(name)]);
        let button = w::icon_button("x-office-calendar-symbolic", picker_name, open_picker);
        root.append(&entry);
        root.append(&button);
        // Valid until ten characters say otherwise; the state is there from the start.
        entry.update_state(&[gtk::accessible::State::Invalid(
            gtk::AccessibleInvalidState::False,
        )]);
        entry.connect_changed(move |e| {
            let text = e.text();
            let bad = !text.is_empty() && text.len() == 10 && parse_date(&text).is_none();
            let state = if bad {
                gtk::AccessibleInvalidState::True
            } else {
                gtk::AccessibleInvalidState::False
            };
            e.update_state(&[gtk::accessible::State::Invalid(state)]);
            if bad {
                e.add_css_class("invalid");
            } else {
                e.remove_css_class("invalid");
            }
            if let Some((y, m, d)) = parse_date(&text) {
                typed(y, m, d)
            }
        });
        DateInput {
            root,
            entry,
            button,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_date;

    #[test]
    fn a_typed_day_is_a_real_day() {
        assert_eq!(parse_date("2026-10-26"), Some((2026, 10, 26)));
        assert_eq!(parse_date(" 2024-02-29 "), Some((2024, 2, 29)));
        assert_eq!(parse_date("2026-02-29"), None, "2026 is no leap year");
        assert_eq!(parse_date("2026-13-01"), None);
        assert_eq!(parse_date("2026-10-00"), None);
        assert_eq!(parse_date("26-10-26"), None);
        assert_eq!(parse_date("2026-10-26-1"), None);
        assert_eq!(parse_date(""), None);
    }
}
