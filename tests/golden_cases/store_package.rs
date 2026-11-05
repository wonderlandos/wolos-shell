// SPDX-License-Identifier: GPL-2.0-or-later
//! App Store install page for a double-clicked `.wol` (ADR 0011): signed, unsigned and damaged
//! files, read through the real `verify`, wide and narrow; then the Plugins page with a list.
use super::Case;
use super::wol_file;
use gtk::prelude::*;
use shell::ui::store::{self, PackageFile};
use std::sync::Arc;

#[path = "store_plugins.rs"]
mod plugins_cases;

pub fn cases() -> Vec<Case> {
    let mut v = plugins_cases::cases();
    for (sw, sh, tag) in [(900, 600, "wide"), (480, 360, "narrow")] {
        for kind in ["signed", "unsigned", "damaged", "missing"] {
            v.push(Case {
                name: format!("store-package-{kind}-{tag}"),
                size: (sw, sh),
                class: "preview-app",
                wallpaper: false,
                build: Box::new(move || {
                    let d = wol_file::dir("golden");
                    let p = match kind {
                        "signed" => {
                            wol_file::write(&d, "Shredder.wol", wol_file::SHREDDER, None, true)
                        }
                        "unsigned" => wol_file::write(&d, "Wike.wol", wol_file::WIKE, None, false),
                        // QA-STORE-40: opened, then moved away: its own sentence, not "damaged".
                        "missing" => {
                            let p =
                                wol_file::write(&d, "Shredder.wol", wol_file::SHREDDER, None, true);
                            std::fs::remove_file(&p).expect("remove the file");
                            p
                        }
                        _ => wol_file::tampered(&wol_file::write(
                            &d,
                            "Shredder.wol",
                            wol_file::SHREDDER,
                            None,
                            true,
                        )),
                    };
                    let s = store::content(
                        Arc::new(shell::system::Fake),
                        store::Env {
                            data_dirs: vec![],
                            pkg_search: false,
                            flatpaks: vec![],
                            installed: Some(Default::default()),
                        },
                    );
                    s.show_package(PackageFile::read_with(
                        p.to_str().unwrap(),
                        &[wol_file::trusted()],
                        &d.join("records"),
                    ));
                    s.aw.root.clone().upcast()
                }),
            });
        }
    }
    v
}
