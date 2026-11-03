// SPDX-License-Identifier: GPL-2.0-or-later
//! App Store Plugins page (ADR 0031 W4), wide and narrow. The list is handed in, nothing is
//! downloaded: with a list (an update, one on, one off and two to get); with plugins that need
//! attention (asks for more than it was allowed, a newer version held back, one this WolOS cannot
//! run); while loading; when the list could not be loaded (Try Again); and empty.
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
        &[],
        "0.12.0",
    ))
}

/// ST6-1 and ST6-3: Weather was turned on before it asked for notifications (Allow); Clock stays
/// as it is while its newer version waits for a newer WolOS; Stars needs a newer WolOS and is
/// not offered.
pub fn attention() -> plugins::Listing {
    let mut clock = entry(
        "alice.clock",
        "World Clock",
        "3.0",
        "Times in other cities",
        &[],
    );
    clock.min_wolos = "0.13".into();
    let mut stars = entry("alice.stars", "Star Chart", "1.0", "The night sky", &[]);
    stars.min_wolos = "0.13".into();
    let index = PluginIndex {
        plugins: vec![
            entry(
                "alice.weather",
                "Weather",
                "1.3",
                "Forecast in the menu bar",
                &["http", "notifications"],
            ),
            clock,
            stars,
        ],
    };
    let have: Vec<(String, String)> = [("alice.weather", "1.3"), ("alice.clock", "2.0")]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
    plugins::Listing::Ready(plugins::rows(
        &index,
        &have,
        &["alice.weather".into(), "alice.clock".into()],
        &[("alice.weather".into(), vec!["notifications".into()])],
        "0.12.0",
    ))
}

/// The page while the list is being loaded.
pub fn loading() -> plugins::Listing {
    plugins::Listing::Loading
}

/// The list could not be loaded: one sentence and Try Again.
pub fn offline() -> plugins::Listing {
    plugins::Listing::Unavailable(shell::copy::STORE_PLUGINS_OFFLINE.into())
}

/// An index with no plugin in it.
pub fn empty() -> plugins::Listing {
    plugins::Listing::Ready(vec![])
}

/// A listing made on demand.
type Make = fn() -> plugins::Listing;

/// What a [`Case`] builds its widget with.
type Build = Box<dyn Fn() -> gtk::Widget>;

pub fn cases() -> Vec<Case> {
    let mut v = Vec::new();
    let page = |l: Make| -> Build {
        Box::new(move || {
            let s = store::content(
                Rc::new(shell::system::Fake),
                store::Env {
                    data_dirs: vec![],
                    pkg_search: false,
                    flatpaks: vec![],
                },
            );
            s.show_plugins(l());
            s.aw.root.clone().upcast()
        })
    };
    for (sw, sh, tag) in [(900, 600, "wide"), (480, 360, "narrow")] {
        v.push(Case {
            name: format!("store-plugins-list-{tag}"),
            size: (sw, sh),
            class: "preview-app",
            wallpaper: false,
            build: page(listing),
        });
    }
    // One size each: the states are one or two lines, not a layout.
    let states: [(&str, Make); 4] = [
        ("attention", attention),
        ("loading", loading),
        ("offline", offline),
        ("empty", empty),
    ];
    for (name, l) in states {
        v.push(Case {
            name: format!("store-plugins-{name}"),
            size: (900, 600),
            class: "preview-app",
            wallpaper: false,
            build: page(l),
        });
    }
    v
}
