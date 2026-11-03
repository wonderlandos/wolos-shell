// SPDX-License-Identifier: GPL-2.0-or-later
//! Theme picker golden (ADR 0031 themes v2, Ctrl+Cmd+T): the glass card with every built-in
//! theme as a preview card over the wallpaper. Settings > Appearance has its own page goldens.
use super::Case;
use gtk::prelude::*;
use shell::ui::theme_picker;
use std::rc::Rc;

pub fn cases() -> Vec<Case> {
    vec![Case {
        name: "theme-picker".into(),
        size: (560, 600),
        class: "preview",
        wallpaper: true,
        build: Box::new(|| {
            let (card, list) = theme_picker::content();
            let (cat, packs) = theme_picker::load();
            theme_picker::fill_with(&list, &cat, &packs, Rc::new(|| {}));
            // The panel at its own size, centred on the wallpaper (the shell window does the same).
            card.set_halign(gtk::Align::Center);
            card.set_valign(gtk::Align::Center);
            card.upcast()
        }),
    }]
}
