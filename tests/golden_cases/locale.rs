// SPDX-License-Identifier: GPL-2.0-or-later
//! The pseudo-locale `xx` (ADR 0045) on real panes: every string the locale system owns shows
//! accented and 40 % longer, in brackets, so a label that is cut off, a row that no longer fits
//! or a sentence still written in the code instead of the catalog shows at once. The layout check
//! of the golden harness runs on these like on every case: a pane that overflows in `xx` fails.
use super::Case;
use gtk::prelude::*;
use shared::i18n::Lang;
use shell::kit::widgets as w;
use shell::system::{Fake, System};
use shell::ui;
use std::rc::Rc;
use wol_core::prefs::ControlCenterPrefs;

/// The Control Center in the pseudo-locale: every module on, and each detail page.
fn control_center() -> Vec<Case> {
    [
        ("main", true),
        ("wifi", false),
        ("bluetooth", false),
        ("sound", false),
        ("focus", false),
        ("display", false),
    ]
    .into_iter()
    .map(|(page, all)| Case {
        name: format!("controlcenter-xx-{page}"),
        size: (368, 1),
        class: "preview",
        wallpaper: true,
        build: Box::new(move || {
            shell::i18n::set_lang(Lang::Xx);
            let mut p = ControlCenterPrefs::default();
            if all {
                for (key, _) in ControlCenterPrefs::MODULES {
                    p.set(key, true);
                }
            }
            let sys: Rc<dyn System> = Rc::new(Fake);
            ui::control_center::content_with(sys, p, page)
        }),
    })
    .collect()
}

/// Settings > Sound, wide and narrow, in the pseudo-locale. Taller than the pane's usual golden,
/// so every row that holds catalog text is in the picture.
pub fn cases() -> Vec<Case> {
    let mut v = control_center();
    for (tag, size) in [
        ("wide", (w::APP_SIZE.0, 1500)),
        ("narrow", (w::APP_NARROW.0, 1900)),
    ] {
        v.push(Case {
            name: format!("settings-sound-xx-{tag}"),
            size,
            class: "preview-app",
            wallpaper: false,
            build: Box::new(|| {
                shell::i18n::set_lang(Lang::Xx);
                let a = ui::settings::content(Rc::new(Fake), &ui::settings::Host::demo());
                a.open("Sound");
                a.root.clone().upcast()
            }),
        });
    }
    v
}
