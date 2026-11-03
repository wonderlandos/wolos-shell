// SPDX-License-Identifier: GPL-2.0-or-later
//! Tray goldens (SP-7): the menu bar with three apps' status items before the system items, as
//! `ui::tray_items` draws them: a picture the app sent, an icon name the theme has, and an item
//! with neither (the package icon). A passive item is not drawn.
use super::Case;
use gtk::prelude::*;
use shell::tray::{Item, Pixmap, Status};

/// A 32 px round badge in `rgb`, as an app would send it (straight RGBA).
fn badge(rgb: [u8; 3]) -> Pixmap {
    let n = 32;
    let mut rgba = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let (dx, dy) = (x as f32 - 15.5, y as f32 - 15.5);
            let a = (14.0 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
            rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], (a * 255.0) as u8]);
        }
    }
    Pixmap {
        width: n,
        height: n,
        rgba,
    }
}

fn items() -> Vec<Item> {
    let item = |id: &str, tip: &str| Item {
        id: format!(":1.{id}/StatusNotifierItem"),
        app: id.into(),
        tooltip: tip.into(),
        ..Default::default()
    };
    vec![
        Item {
            pixmap: Some(badge([0x2f, 0x9e, 0x5b])),
            ..item("chat", "Chat")
        },
        Item {
            icon_name: "emblem-synchronizing-symbolic".into(),
            ..item("sync", "Sync")
        },
        item("plain", "Plain"),
        Item {
            status: Status::Passive,
            icon_name: "mail-unread-symbolic".into(),
            ..item("quiet", "Quiet")
        },
    ]
}

pub fn cases() -> Vec<Case> {
    vec![Case {
        name: "tray-items".into(),
        size: (900, 28),
        class: "preview",
        wallpaper: true,
        build: Box::new(|| {
            shell::ui::tray_items::preview(items());
            let b = shell::ui::menubar::content(&shell::system::Fake);
            b.set_app("Files".into(), "wol-files".into());
            // Later cases draw bars without items again.
            shell::ui::tray_items::preview(Vec::new());
            b.root.clone().upcast()
        }),
    }]
}
