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
