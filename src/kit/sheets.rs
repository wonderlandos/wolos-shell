// SPDX-License-Identifier: GPL-2.0-or-later
//! Surfaces over the content in the WolOS Design System (ADR 0058 draft): the question sheet
//! (our dialog), the bottom sheet, the side sheet, the search view, the rich tooltip and the
//! on-screen display. The sheet slides from the toolbar ([`super::widgets::sheet`]); a floating
//! window-modal dialog is not a WolOS part (RU-046). Specs: `design/components/<id>.toml`; tests:
//! `tests/kit_sheets.rs`; look: `kit.css` ("Sheets and overlays").
use super::widgets as w;
use gtk::prelude::*;

/// A question in a sheet: a title, one or two sentences, and its buttons at the end, the
/// default one last. The sheet reveals itself with [`gtk::Revealer::set_reveal_child`].
pub fn question_sheet(title: &str, text: &str, buttons: &[gtk::Button]) -> gtk::Revealer {
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(w::S2)
        .accessible_role(gtk::AccessibleRole::Dialog)
        .build();
    let t = w::label(title, "title");
    t.set_wrap(true);
    body.append(&t);
    let x = w::label(text, "");
    x.set_wrap(true);
    body.append(&x);
    body.update_relation(&[gtk::accessible::Relation::LabelledBy(&[t.upcast_ref()])]);
    body.update_relation(&[gtk::accessible::Relation::DescribedBy(&[x.upcast_ref()])]);
    let row = w::hbox(w::S2);
    row.add_css_class("question-buttons");
    row.set_halign(gtk::Align::End);
    for b in buttons {
        row.append(b);
    }
    body.append(&row);
    w::sheet(&body)
}

/// A sheet anchored to the bottom edge of a narrow window or a touch surface: a grab handle, then
/// its content (a short list of actions, details). Wide windows use [`question_sheet`] or a
/// [`side_sheet`].
pub fn bottom_sheet(child: &impl IsA<gtk::Widget>) -> gtk::Box {
    let b = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(w::S2)
        .accessible_role(gtk::AccessibleRole::Dialog)
        .build();
    b.add_css_class("bottom-sheet");
    b.set_valign(gtk::Align::End);
    let handle = w::hbox(0);
    handle.add_css_class("sheet-handle");
    handle.set_halign(gtk::Align::Center);
    b.append(&handle);
    b.append(child);
    b
}

/// A pane at the end of the window for details or options of what is selected (an inspector):
/// a header with its title and a close button, then its content. It narrows the content beside
/// it; it never covers it.
pub fn side_sheet(
    title: &str,
    close: &str,
    child: &impl IsA<gtk::Widget>,
    closed: impl Fn() + 'static,
) -> gtk::Box {
    let b = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(w::S3)
        .accessible_role(gtk::AccessibleRole::Group)
        .build();
    b.add_css_class("side-sheet");
    b.set_width_request(w::CONTROL_CENTER_WIDTH);
    // Its rows expand; the pane does not take the content's room for that.
    b.set_hexpand(false);
    let head = w::hbox(w::S2);
    head.add_css_class("side-sheet-head");
    let t = w::label(title, "title");
    t.set_hexpand(true);
    head.append(&t);
    head.append(&w::icon_button("window-close-symbolic", close, closed));
    b.append(&head);
    b.update_relation(&[gtk::accessible::Relation::LabelledBy(&[t.upcast_ref()])]);
    b.append(child);
    b
}

/// The search view: the field, then what it found as rows, or one quiet line when nothing
/// matches (`none`, "No apps match"). `rows` are list buttons ([`super::widgets::list_button`]).
pub fn search_view(field: &gtk::SearchEntry, rows: &[gtk::Widget], none: &str) -> gtk::Box {
    let b = w::vbox(w::S2);
    b.add_css_class("search-view");
    field.set_hexpand(true);
    b.append(field);
    if rows.is_empty() {
        let l = w::label(none, "secondary");
        l.add_css_class("search-none");
        b.append(&l);
    } else {
        let list = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .accessible_role(gtk::AccessibleRole::List)
            .build();
        list.add_css_class("search-results");
        for r in rows {
            list.append(r);
        }
        b.append(&list);
    }
    b
}

/// What a rich tooltip shows: a title, one or two sentences and at most one text action. It is
/// the child of [`rich_tooltip`]'s popover.
pub fn rich_tooltip_body(title: &str, text: &str, action: Option<&gtk::Button>) -> gtk::Box {
    let b = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(w::S1)
        .accessible_role(gtk::AccessibleRole::Tooltip)
        .build();
    b.add_css_class("rich-tooltip");
    let t = w::label(title, "title");
    t.set_wrap(true);
    b.append(&t);
    let x = w::label(text, "secondary");
    x.set_wrap(true);
    x.set_max_width_chars(40);
    b.append(&x);
    if let Some(a) = action {
        a.set_halign(gtk::Align::Start);
        b.append(a);
    }
    b
}

/// A rich tooltip on `anchor`: opens on hover or focus of the anchor after the tooltip delay and
/// stays while the pointer is over it, so its action can be reached.
pub fn rich_tooltip(anchor: &impl IsA<gtk::Widget>, body: &gtk::Box) -> gtk::Popover {
    let p = gtk::Popover::builder()
        .child(body)
        .has_arrow(false)
        .autohide(false)
        .build();
    p.add_css_class("rich-tooltip-popover");
    p.set_parent(anchor);
    p
}

/// The on-screen display: a glass pill with a 16 px glyph and a level bar (volume, brightness).
/// Returns the pill, its glyph and its bar.
pub fn osd_pill(icon_name: &str) -> (gtk::Box, gtk::Image, gtk::ProgressBar) {
    let g = w::glass_panel(w::S2);
    g.add_css_class("osd-pill");
    let row = w::hbox(w::S3);
    let icon = w::icon(icon_name, w::ICON);
    let bar = gtk::ProgressBar::new();
    bar.set_hexpand(true);
    bar.set_valign(gtk::Align::Center);
    row.append(&icon);
    row.append(&bar);
    g.append(&row);
    (g, icon, bar)
}
