// SPDX-License-Identifier: GPL-2.0-or-later
//! Clipboard history panel (⇧⌘V, ADR 0031 W5): search, pin, clear. What is kept is decided in
//! `crate::clipboard::History`; this file draws it. The pin file is read and written off the
//! GTK thread. A pick copies and closes, like a menu item (audit2 A2-15); no display means the
//! panel shows and nothing is recorded (audit2 A2-17).
use crate::clipboard::{self, History};
use crate::kit::widgets as w;
use gtk::{gdk, gio, glib, prelude::*};
use std::cell::RefCell;
use std::rc::Rc;

/// Rows shown for one search (the list is bounded anyway; this keeps a frame short).
const SHOWN: usize = 50;

/// The panel's redraw, set once it exists, so rows (drawn by it) can call it again.
type LateRedraw = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// The panel's widgets and state, for the shell and the goldens.
pub struct Panel {
    pub root: gtk::Box,
    pub search: gtk::SearchEntry,
    pub history: Rc<RefCell<History>>,
    render: Rc<dyn Fn()>,
}

impl Panel {
    /// The panel content around `history` (no window, no clipboard watch).
    pub fn new(history: History) -> Panel {
        let root = w::glass_panel(w::S2);
        let head = w::hbox(w::S2);
        head.append(&w::label("Clipboard", "title"));
        head.append(&w::spacer());
        let search = w::search_field("Search");
        let list = w::vbox(w::S1);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(480)
            .build();
        let history = Rc::new(RefCell::new(history));
        let render: LateRedraw = Default::default();
        // Clear is off while the history is empty (QA-VIS1-22).
        let clear = w::button("Clear", w::Kind::Default, || {});
        let draw: Rc<dyn Fn()> = {
            let (list, history, search, render, clear) = (
                list.clone(),
                history.clone(),
                search.clone(),
                render.clone(),
                clear.clone(),
            );
            Rc::new(move || {
                while let Some(c) = list.first_child() {
                    list.remove(&c);
                }
                let h = history.borrow();
                clear.set_sensitive(!h.entries().is_empty());
                let query = search.text();
                let found = h.search(&query);
                if h.entries().is_empty() {
                    list.append(&w::label("Copied text shows up here", "secondary"));
                } else if found.is_empty() {
                    list.append(&w::label("No matches", "secondary"));
                }
                let again = render.borrow().clone();
                for e in found.into_iter().take(SHOWN) {
                    list.append(&row(e, &history, again.clone()));
                }
            })
        };
        *render.borrow_mut() = Some(draw.clone());
        let (h2, r2) = (history.clone(), draw.clone());
        clear.connect_clicked(move |_| {
            h2.borrow_mut().clear();
            r2();
            // The system selection goes with the history (QA-LN-16): `wl-paste` gave the last text back.
            let Some(display) = gdk::Display::default() else {
                return;
            };
            let (cb, h) = (display.clipboard(), h2.clone());
            let cb2 = cb.clone();
            cb.read_text_async(None::<&gio::Cancellable>, move |r| {
                let sel = r.ok().flatten();
                if h.borrow().clear_wants_selection_empty(sel.as_deref()) {
                    let _ = cb2.set_content(None::<&gdk::ContentProvider>);
                }
            });
        });
        head.append(&clear);
        let r3 = draw.clone();
        search.connect_search_changed(move |_| r3());
        root.append(&head);
        root.append(&search);
        root.append(&scroll);
        root.append(&w::footnote(&limits_line()));
        draw();
        Panel {
            root,
            search,
            history,
            render: draw,
        }
    }

    /// Redraws the list (after the history changed from outside).
    pub fn refresh(&self) {
        (self.render)();
    }
}

/// What the history keeps and leaves out, said under the list whenever it is shown (QA-LN-17: an
/// image copy was silently not listed; QA-PWR-23: a 70 KiB copy pasted fine and was missing from
/// the list with no word why). Built from the limits, so the sentence cannot drift from them.
pub fn limits_line() -> String {
    crate::copy::clipboard_limits(clipboard::MAX, clipboard::MAX_TEXT / 1024)
}

/// One entry: the text (click copies and closes), Pin/Unpin and Remove.
fn row(
    e: &clipboard::Entry,
    history: &Rc<RefCell<History>>,
    render: Option<Rc<dyn Fn()>>,
) -> gtk::Box {
    let r = w::hbox(w::S1);
    let icon = if e.pinned {
        "view-pin-symbolic"
    } else {
        "edit-paste-symbolic"
    };
    let pick = w::list_button(icon, &clipboard::preview(&e.text), "");
    pick.set_hexpand(true);
    let text = e.text.clone();
    pick.connect_clicked(move |b| {
        b.display().clipboard().set_text(&text);
        if let Some(win) = b.root().and_downcast::<gtk::Window>() {
            win.set_visible(false);
        }
    });
    r.append(&pick);
    let (tip, pin) = if e.pinned {
        ("Unpin", false)
    } else {
        ("Pin", true)
    };
    let (h, text, again) = (history.clone(), e.text.clone(), render.clone());
    r.append(&w::icon_button("view-pin-symbolic", tip, move || {
        if h.borrow_mut().set_pinned(&text, pin) {
            save_pins(&h.borrow());
            if let Some(f) = &again {
                f();
            }
        }
    }));
    let (h, text, pinned) = (history.clone(), e.text.clone(), e.pinned);
    r.append(&w::icon_button(
        "edit-delete-symbolic",
        "Remove",
        move || {
            if h.borrow_mut().remove(&text) {
                if pinned {
                    save_pins(&h.borrow());
                }
                if let Some(f) = &render {
                    f();
                }
            }
        },
    ));
    r
}

/// Writes the pins off the GTK thread; a failed write keeps them for this session only.
fn save_pins(h: &History) {
    let pins = h.pinned_texts();
    glib::spawn_future_local(async move {
        let home = wol_core::journal::state_home();
        let r = gio::spawn_blocking(move || clipboard::save_pins(&home, &pins)).await;
        if let Ok(Err(e)) = r {
            eprintln!("wol-shell: clipboard pins not saved: {e}");
        }
    });
}

/// The panel window: watches the display clipboard (event-driven) and loads the pins.
pub fn panel(app: &gtk::Application) -> gtk::Window {
    let p = Rc::new(Panel::new(History::default()));
    let win = super::panels::panel(app, "clipboard", "wol-panel-clipboard", [false; 4], &p.root);
    // A fresh look each time it opens: the search starts empty and has the keyboard.
    let p2 = p.clone();
    win.connect_show(move |_| {
        p2.search.set_text("");
        p2.search.grab_focus();
    });
    let p2 = p.clone();
    glib::spawn_future_local(async move {
        let home = wol_core::journal::state_home();
        if let Ok(pins) = gio::spawn_blocking(move || clipboard::load_pins(&home)).await {
            let mut h = History::with_pins(pins);
            for e in p2.history.borrow().entries().iter().rev() {
                h.push(&e.text);
            }
            *p2.history.borrow_mut() = h;
            p2.refresh();
        }
    });
    if let Some(d) = gdk::Display::default() {
        d.clipboard().connect_changed(move |cb| {
            let p = p.clone();
            // A copy marked as a password (password managers) is never read into the history.
            let hinted = cb
                .formats()
                .contain_mime_type(clipboard::PASSWORD_HINT_MIME);
            cb.read_text_async(None::<&gio::Cancellable>, move |r| {
                if let Ok(Some(t)) = r
                    && p.history.borrow_mut().push_copy(&t, hinted)
                {
                    p.refresh();
                }
            });
        });
    }
    win
}

#[cfg(test)]
mod tests {
    /// QA-PWR-23: the panel names its limits, from the constants.
    #[test]
    fn the_panel_says_how_much_it_keeps() {
        let l = super::limits_line();
        assert!(
            l.contains(&format!("last {} copies", crate::clipboard::MAX)),
            "{l}"
        );
        assert!(l.contains("64 KB"), "{l}");
        assert!(l.contains("images are not kept"), "{l}");
    }

    /// Audit2 A2-15/A2-17 carried over from the old panel: a pick closes, no expect on the display.
    #[test]
    fn a_pick_closes_and_no_display_never_panics() {
        let src = include_str!("clipboard.rs");
        let code = &src[..src.find("#[cfg(test)]").unwrap_or(src.len())];
        assert!(code.contains("win.set_visible(false)"));
        assert!(!code.contains(concat!(".expect", "(")));
        assert!(code.contains("gio::spawn_blocking(move || clipboard::load_pins"));
    }
}
