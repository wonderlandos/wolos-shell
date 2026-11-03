// SPDX-License-Identifier: GPL-2.0-or-later
//! Control Center pages added in v0.12 wave 2 (the other pages are in `golden.rs`
//! `control_center_cases`): the Display page, the Sound page on a desktop, the narrow layout
//! (three tile columns) and the tile grid with tiles from a [`TileSource`] (ADR 0031 slot).
use super::Case;
use shell::system::{Fake, FakeDesktop, System};
use shell::ui::control_center::{
    self as cc,
    tiles::{Tile, TileSource},
};
use std::rc::Rc;
use wol_core::prefs::ControlCenterPrefs;

/// Every module on.
fn all() -> ControlCenterPrefs {
    let mut p = ControlCenterPrefs::default();
    for (key, _) in ControlCenterPrefs::MODULES {
        p.set(key, true);
    }
    p
}

fn case(name: &str, width: i32, build: impl Fn() -> gtk::Widget + 'static) -> Case {
    Case {
        name: name.into(),
        size: (width + 48, 1),
        class: "preview",
        wallpaper: true,
        build: Box::new(build),
    }
}

/// Two plugin tiles, one on.
struct Plugins;

impl TileSource for Plugins {
    fn tiles(&self) -> Vec<Tile> {
        [
            (
                "org.wolos.awake.tile",
                "display-brightness-symbolic",
                "Stay Awake",
                true,
            ),
            (
                "org.wolos.silence.tile",
                "audio-volume-muted-symbolic",
                "Silence",
                false,
            ),
        ]
        .into_iter()
        .map(|(id, icon, label, on)| Tile {
            id: id.into(),
            icon: icon.into(),
            label: label.into(),
            on,
        })
        .collect()
    }
    fn toggle(&self, _id: &str, _on: bool) {}
}

pub fn cases() -> Vec<Case> {
    let full = shell::kit::widgets::CONTROL_CENTER_WIDTH;
    let laptop = || -> Rc<dyn System> { Rc::new(Fake) };
    vec![
        case("controlcenter-display", full, move || {
            cc::content_with(laptop(), all(), "display")
        }),
        case("controlcenter-desktop-sound", full, move || {
            // No sound service: System's devices only, the input half off.
            cc::content_sound(
                Rc::new(FakeDesktop),
                ControlCenterPrefs::default(),
                "sound",
                full,
                None,
            )
        }),
        case("controlcenter-narrow", cc::NARROW_WIDTH, move || {
            cc::content_sized(laptop(), all(), "main", cc::NARROW_WIDTH)
        }),
        case("controlcenter-plugin-tiles", full, move || {
            // The source is read on the first refresh, then cleared for the cases after this one.
            cc::set_tile_source(Some(Rc::new(Plugins)));
            let w = cc::content_with(laptop(), ControlCenterPrefs::default(), "main");
            cc::set_tile_source(None);
            w
        }),
    ]
}
