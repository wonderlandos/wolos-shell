// SPDX-License-Identifier: GPL-2.0-or-later
//! Menu bar goldens (A8): the WolOS menu with every power action (QA-FDAY-5), the live menu
//! without Lock Screen (QA-FDAY-7), and the red Stop while a screen recording runs (live-1 H4).
use super::Case;
use gtk::prelude::*;

/// Every label under `w`, in order.
fn labels(w: &gtk::Widget) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(l) = w.downcast_ref::<gtk::Label>() {
        out.push(l.label().to_string());
    }
    let mut c = w.first_child();
    while let Some(x) = c {
        out.extend(labels(&x));
        c = x.next_sibling();
    }
    out
}

/// The bar for an app of `class` with its Edit menu open under it: the titles File, Edit, View,
/// Window, Help, and the rows. `live` rows must be tappable, `grey` ones greyed out.
fn standard_bar(name: &str, class: &str, live: &[&str], grey: &[&str]) -> gtk::Widget {
    let b = shell::ui::menubar::content(&shell::system::Fake);
    b.preview_standard(name, class);
    let l = labels(b.root.upcast_ref());
    for want in ["File", "Edit", "View", "Window", "Help"] {
        assert!(l.iter().any(|x| x == want), "{want} in {l:?}");
    }
    let card = b.menu_card("Edit").expect("an Edit menu");
    for (rows, tappable) in [(live, true), (grey, false)] {
        for row in rows {
            let w = find_row(&card, row).unwrap_or_else(|| panic!("no row {row}"));
            assert_eq!(w.is_sensitive(), tappable, "{row} in {class}");
            assert_eq!(
                w.opacity() < 1.0,
                !tappable,
                "{row} in {class} looks {}",
                w.opacity()
            );
        }
    }
    let col = gtk::Box::new(gtk::Orientation::Vertical, 0);
    col.append(&b.root);
    col.append(&card);
    card.set_halign(gtk::Align::Start);
    card.set_margin_start(80);
    col.upcast()
}

/// The button of the menu row labelled `text`.
fn find_row(w: &gtk::Widget, text: &str) -> Option<gtk::Button> {
    if let Some(b) = w.downcast_ref::<gtk::Button>()
        && labels(w).iter().any(|l| l == text)
    {
        return Some(b.clone());
    }
    let mut c = w.first_child();
    while let Some(x) = c {
        if let Some(b) = find_row(&x, text) {
            return Some(b);
        }
        c = x.next_sibling();
    }
    None
}

pub fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "wolos-menu-has-sleep-restart".into(),
            size: (240, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let m = shell::ui::menubar::logo_menu_view(false);
                let l = labels(&m);
                for want in ["Sleep", "Restart…", "Shut Down…", "Lock Screen", "Log Out…"] {
                    assert!(l.iter().any(|x| x == want), "{want} in {l:?}");
                }
                m
            }),
        },
        Case {
            name: "wolos-menu-live".into(),
            size: (240, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let m = shell::ui::menubar::logo_menu_view(true);
                let l = labels(&m);
                assert!(!l.iter().any(|x| x == "Lock Screen"), "{l:?}");
                assert!(l.iter().any(|x| x == "Restart…"), "{l:?}");
                m
            }),
        },
        Case {
            name: "menubar-recording".into(),
            size: (900, 28),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let b = shell::ui::menubar::content(&shell::system::Fake);
                b.set_app("Files".into(), "wol-files".into());
                b.set_recording(true);
                b.root.clone().upcast()
            }),
        },
        Case {
            // QA-LIVE1-14: a Flatpak whose app id ends in `.desktop` read "org.telegram.desktop".
            name: "menubar-app-name".into(),
            size: (900, 28),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let installed = [shell::desktop::Entry {
                    id: "org.telegram.desktop.desktop".into(),
                    name: "Telegram".into(),
                    exec: vec!["/usr/bin/flatpak".into(), "run".into()],
                    icon: "org.telegram.desktop".into(),
                    wm_class: String::new(),
                }];
                let name = shell::apps::frontmost_name(
                    "org.telegram.desktop",
                    "Chat",
                    shell::apps::PINS,
                    &installed,
                );
                assert_eq!(name, "Telegram");
                let b = shell::ui::menubar::content(&shell::system::Fake);
                b.set_app(name, "org.telegram.desktop".into());
                let l = labels(b.root.upcast_ref());
                assert!(l.iter().any(|x| x == "Telegram"), "{l:?}");
                assert!(!l.iter().any(|x| x.contains("org.telegram")), "{l:?}");
                b.root.clone().upcast()
            }),
        },
        Case {
            // E2E-MENU: an app that publishes no menus gets File, Edit, View, Window and Help; the
            // Edit menu open, Undo and Redo live in the editor.
            name: "menubar-edit-texteditor".into(),
            size: (900, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                standard_bar(
                    "Text Editor",
                    "org.gnome.TextEditor",
                    &["Undo", "Redo"],
                    &[],
                )
            }),
        },
        Case {
            // A terminal has no Undo, Redo, Cut or Select All: those rows are greyed.
            name: "menubar-edit-foot".into(),
            size: (900, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                standard_bar(
                    "Terminal",
                    "foot",
                    &["Copy", "Paste"],
                    &["Undo", "Redo", "Cut", "Select All"],
                )
            }),
        },
        Case {
            // QA-LIVE3-35: the lock screen is the session owner's: one name, no account list (the
            // login window lists every account).
            name: "lock-single-user".into(),
            size: (900, 600),
            class: "lock-window",
            wallpaper: false,
            build: Box::new(|| {
                let passwd = "root:x:0:0::/root:/bin/sh\nalice:x:1000:1000:Alice Liddell:/home/alice:/bin/sh\nbo:x:1001:1001:Bo:/home/bo:/bin/sh\n";
                let owner = shell::ui::lock::lock_owner(Some("alice".into()), 1000, passwd);
                assert_eq!(owner, "alice");
                let panel = shell::kit::widgets::UserPanel::new("Alice Liddell");
                let screen = shell::ui::lock::lock_screen(&panel);
                let l = labels(&screen);
                assert!(l.iter().any(|x| x == "Alice Liddell"), "{l:?}");
                assert!(!l.iter().any(|x| x.contains("Bo")), "{l:?}");
                screen
            }),
        },
    ]
}
