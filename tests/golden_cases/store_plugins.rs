// SPDX-License-Identifier: GPL-2.0-or-later
//! App Store Plugins page with a list (ADR 0031 W4): an update, one on, one off and two to get,
//! wide and narrow. The list is handed in, nothing is downloaded.
use super::super::Case;
use gtk::prelude::*;
use shell::ui::store::{self, plugins};
use std::rc::Rc;
use wopkg::index::plugins::{Entry, PluginIndex};

fn entry(id: &str, name: &str, version: &str, summary: &str, perms: &[&str]) -> Entry {
    Entry {
        id: id.into(),
        name: name.into(),
        version: version.into(),
        summary: summary.into(),
        author: "Alice Liddell".into(),
        license: "MIT".into(),
        kinds: vec!["menubar".into()],
        permissions: perms.iter().map(|p| p.to_string()).collect(),
        ..Default::default()
    }
}

pub fn listing() -> plugins::Listing {
    let index = PluginIndex {
        plugins: vec![
            entry(
                "alice.weather",
                "Weather",
                "1.3",
                "Forecast in the menu bar",
                &["http"],
            ),
            entry(
                "alice.pomodoro",
                "Pomodoro",
                "0.4",
                "Focus timer with breaks",
                &["notifications"],
            ),
            entry(
                "alice.clock",
                "World Clock",
                "2.0",
                "Times in other cities",
                &[],
            ),
            entry(
                "alice.notes",
                "Sticky Notes",
                "1.0",
                "Notes on the desktop",
                &[],
            ),
            entry(
                "alice.tea",
                "Tea Timer",
                "1.1",
                "Rings when the tea is ready",
                &["notifications"],
            ),
        ],
    };
    let have: Vec<(String, String)> = [
        ("alice.weather", "1.2"),
        ("alice.pomodoro", "0.4"),
        ("alice.clock", "2.0"),
    ]
    .iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect();
    plugins::Listing::Ready(plugins::rows(
        &index,
        &have,
        &["alice.pomodoro".into()],
        "0.12.0",
    ))
}

pub fn cases() -> Vec<Case> {
    let mut v = Vec::new();
    for (sw, sh, tag) in [(900, 600, "wide"), (480, 360, "narrow")] {
        v.push(Case {
            name: format!("store-plugins-list-{tag}"),
            size: (sw, sh),
            class: "preview-app",
            wallpaper: false,
            build: Box::new(|| {
                let s = store::content(
                    Rc::new(shell::system::Fake),
                    store::Env {
                        data_dirs: vec![],
                        pkg_search: false,
                        flatpaks: vec![],
                    },
                );
                s.show_plugins(listing());
                s.aw.root.clone().upcast()
            }),
        });
    }
    v
}
