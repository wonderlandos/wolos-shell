// SPDX-License-Identifier: GPL-2.0-or-later
//! Settings opened on a row (`wol-settings privacy#screen-recording`, a Search result, a hit in
//! Settings' own search): after the page shows, the row is scrolled into view and lit
//! ([`TARGET_CLASS`]) until the person goes elsewhere. Panes are built the first time they show
//! and laid out a frame later, so the row is looked for on the frame clock, not after a wait.
//! The row is found by the title it shows: its English title and the one in the language in use.

use gtk::{glib, graphene, prelude::*};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Class of the lit row (kit.css).
pub const TARGET_CLASS: &str = "search-target";
/// Class of the row container a title belongs to ([`crate::kit::widgets::list_row`]).
const ROW_CLASS: &str = "list-row";
/// Frames to look for the row before giving up (a pane that fills in from a slow answer shows
/// the page without the row; ~5 s at 60 Hz). A bound on frames, not a wait.
pub const MAX_FRAMES: u32 = 300;
/// The row lands this share of the viewport below its top, not flush against it.
const LAND_AT: f64 = 0.25;

/// Every text `row` is shown as: its English title and the one in the language in use.
pub fn shown_as(row: &str) -> Vec<String> {
    let mut v = vec![row.to_lowercase()];
    let t = crate::i18n::tr(row).to_lowercase();
    if !v.contains(&t) {
        v.push(t);
    }
    v
}

/// The text a widget shows as a title: a label, or a button's own text.
fn text_of(w: &gtk::Widget) -> Option<String> {
    if let Some(l) = w.downcast_ref::<gtk::Label>() {
        return Some(l.label().to_string());
    }
    w.downcast_ref::<gtk::Button>()
        .and_then(|b| b.label())
        .map(|s| s.to_string())
}

/// The first visible label or button under `root`, in reading order, whose text is one of `names`
/// (lowercase; a trailing `…` does not count).
pub fn find_title(root: &gtk::Widget, names: &[String]) -> Option<gtk::Widget> {
    let mut child = root.first_child();
    while let Some(c) = child {
        if c.is_visible() {
            if let Some(t) = text_of(&c) {
                let t = t.trim().trim_end_matches('\u{2026}').to_lowercase();
                if names
                    .iter()
                    .any(|n| n.trim_end_matches('\u{2026}') == t.as_str())
                {
                    return Some(c);
                }
            }
            if let Some(f) = find_title(&c, names) {
                return Some(f);
            }
        }
        child = c.next_sibling();
    }
    None
}

/// The widget to light for a title: its row; a section's heading lights the whole section; else
/// the title itself.
pub fn row_of(title: &gtk::Widget) -> gtk::Widget {
    if title.has_css_class("heading")
        && let Some(section) = title.parent()
    {
        return section;
    }
    let mut up = title.parent();
    while let Some(p) = up {
        if p.has_css_class(ROW_CLASS) {
            return p;
        }
        up = p.parent();
    }
    title.clone()
}

/// Takes the light off whatever is lit under `root`.
pub fn clear(root: &gtk::Widget) {
    root.remove_css_class(TARGET_CLASS);
    let mut child = root.first_child();
    while let Some(c) = child {
        clear(&c);
        child = c.next_sibling();
    }
}

/// Scrolls the page in `scroll` so `row` shows near the top, and lights it (only it, in `stack`).
pub fn land(stack: &gtk::Stack, scroll: &gtk::ScrolledWindow, row: &gtk::Widget) {
    clear(stack.upcast_ref());
    row.add_css_class(TARGET_CLASS);
    let adj = scroll.vadjustment();
    // The row's top in the page's own coordinates (the viewport's child), whatever the scroll.
    let inner = scroll
        .child()
        .and_downcast::<gtk::Viewport>()
        .and_then(|v| v.child())
        .or_else(|| scroll.child());
    let Some(y) = inner.and_then(|i| row.compute_point(&i, &graphene::Point::new(0.0, 0.0))) else {
        return;
    };
    let top = f64::from(y.y()) - adj.page_size() * LAND_AT;
    adj.set_value(top.clamp(
        adj.lower(),
        (adj.upper() - adj.page_size()).max(adj.lower()),
    ));
}

/// Shows `row` of the page `stack` shows now: looked for each frame until the page is laid out
/// (at most [`MAX_FRAMES`]), then scrolled to and lit; the light goes when another page shows.
pub fn row(stack: &gtk::Stack, row: &str) {
    clear(stack.upcast_ref());
    let Some(scroll) = stack.visible_child().and_downcast::<gtk::ScrolledWindow>() else {
        return;
    };
    let names = shown_as(row);
    let frames = Cell::new(0u32);
    // The light goes when the person moves to another page.
    let handler: Rc<RefCell<Option<glib::SignalHandlerId>>> = Rc::default();
    {
        let h = handler.clone();
        let id = stack.connect_visible_child_name_notify(move |s| {
            clear(s.upcast_ref());
            if let Some(id) = h.borrow_mut().take() {
                s.disconnect(id);
            }
        });
        *handler.borrow_mut() = Some(id);
    }
    scroll.add_tick_callback(move |scroll, _| {
        frames.set(frames.get() + 1);
        if let Some(t) = find_title(scroll.upcast_ref(), &names) {
            let row = row_of(&t);
            // Laid out: the row has a height and the page a viewport.
            if row.height() > 0 && scroll.vadjustment().page_size() > 0.0 {
                if let Some(stack) = scroll.parent().and_downcast::<gtk::Stack>() {
                    land(&stack, scroll, &row);
                }
                return glib::ControlFlow::Break;
            }
        }
        if frames.get() >= MAX_FRAMES {
            return glib::ControlFlow::Break;
        }
        glib::ControlFlow::Continue
    });
}
