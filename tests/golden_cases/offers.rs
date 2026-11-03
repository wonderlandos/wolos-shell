// SPDX-License-Identifier: GPL-2.0-or-later
//! Notification offers on a banner: a sign-in code gets a Copy Code button, a build result
//! with numbers in it gets none.
use super::Case;
use gtk::prelude::*;
use shell::kit::widgets as w;
use shell::ui;

fn note(app: &str, summary: &str, body: &str) -> ui::notify::Note {
    ui::notify::Note {
        id: 1,
        app: app.into(),
        icon: String::new(),
        summary: summary.into(),
        body: body.into(),
        critical: false,
        timeout_ms: -1,
        image: String::new(),
        actions: vec![],
    }
}

pub fn cases() -> Vec<Case> {
    vec![Case {
        name: "banner-code".into(),
        size: (420, 1),
        class: "preview",
        wallpaper: true,
        build: Box::new(|| {
            let col = w::vbox(0);
            col.add_css_class("banners-preview");
            col.append(&ui::notify::banner(
                &note("Mail", "Sign-in code", "Your verification code is 938271."),
                1,
            ));
            col.append(&ui::notify::banner(
                &note(
                    "Terminal",
                    "Claude Code",
                    "Build 4821 finished: 412 passed, 0 failed",
                ),
                1,
            ));
            col.upcast()
        }),
    }]
}
