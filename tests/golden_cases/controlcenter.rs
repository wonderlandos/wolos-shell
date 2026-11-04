// SPDX-License-Identifier: GPL-2.0-or-later
//! Control Center pages: [`control_center_cases`] (laptop and desktop, every detail page, the Kit
//! components) and the ones added in v0.12 wave 2: the Display page, the Sound page on a desktop, the narrow layout
//! (three tile columns) and the tile grid with tiles from a [`TileSource`] (ADR 0031 slot).
use super::Case;
use gtk::prelude::*;
use shell::kit::widgets as w;
use shell::system::{Fake, FakeDesktop, System};
use shell::ui;
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

/// A laptop in airplane mode (NET-3): every radio off, the Airplane Mode tile lit.
struct InFlight;

impl System for InFlight {
    fn battery(&self) -> Option<shell::status::Battery> {
        Fake.battery()
    }
    fn audio(&self) -> shell::status::Audio {
        Fake.audio()
    }
    fn network(&self) -> shell::status::Network {
        shell::status::Network::default()
    }
    fn brightness(&self) -> Option<u8> {
        Fake.brightness()
    }
    fn wifi_enabled(&self) -> Option<bool> {
        Some(false)
    }
    fn bluetooth(&self) -> Option<bool> {
        Some(false)
    }
    fn airplane(&self) -> Option<bool> {
        Some(true)
    }
    fn night_light(&self) -> Option<bool> {
        Fake.night_light()
    }
    fn accessibility(&self) -> shell::system::Accessibility {
        Fake.accessibility()
    }
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
        // NET-21: Airplane Mode is the first tile, lit; Wi-Fi and Bluetooth read Off.
        case("controlcenter-airplane", full, move || {
            cc::content_with(Rc::new(InFlight), ControlCenterPrefs::default(), "main")
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

/// Control Center: laptop (battery) and desktop variants, every module on, each detail page, and
/// its Kit components on their own.
pub fn control_center_cases() -> Vec<Case> {
    use shell::kit::controls as k;
    use shell::system::{Fake, FakeDesktop, System};
    use wol_core::prefs::ControlCenterPrefs;
    let cc = |name: &str, sys: fn() -> Rc<dyn System>, all: bool, page: &'static str| Case {
        name: name.into(),
        size: (368, 1),
        class: "preview",
        wallpaper: true,
        build: Box::new(move || {
            let mut p = ControlCenterPrefs::default();
            if all {
                for (key, _) in ControlCenterPrefs::MODULES {
                    p.set(key, true);
                }
            }
            ui::control_center::content_with(sys(), p, page)
        }),
    };
    let laptop: fn() -> Rc<dyn System> = || Rc::new(Fake);
    let desktop: fn() -> Rc<dyn System> = || Rc::new(FakeDesktop);
    let kit_case = |name: &str, build: fn() -> gtk::Widget| Case {
        name: format!("kit-{name}"),
        size: (368, 1),
        class: "preview",
        wallpaper: true,
        build: Box::new(move || {
            let p = w::glass_panel(w::S2);
            p.set_width_request(w::CONTROL_CENTER_WIDTH);
            p.append(&build());
            p.upcast()
        }),
    };
    vec![
        cc("controlcenter-desktop", desktop, false, "main"),
        cc("controlcenter-all", laptop, true, "main"),
        cc("controlcenter-wifi", laptop, false, "wifi"),
        cc("controlcenter-wifi-other", laptop, false, "wifi:other"),
        cc("controlcenter-bluetooth", laptop, false, "bluetooth"),
        cc("controlcenter-sound", laptop, false, "sound"),
        cc("controlcenter-focus", laptop, false, "focus"),
        kit_case("toggle-tile", || {
            let r = w::hbox(w::S2);
            r.set_homogeneous(true);
            let on = k::ToggleTile::new("night-light-symbolic", "Night Shift");
            on.set_on(true);
            r.append(&on.button);
            r.append(&k::ToggleTile::new("wolos-contrast-symbolic", "Increase Contrast").button);
            r.append(&k::ToggleTile::new("camera-photo-symbolic", "Screenshot").button);
            r.upcast()
        }),
        kit_case("device-row", || {
            let c = w::vbox(0);
            c.add_css_class("module");
            for (icon, t, s, on, tr) in [
                (
                    "network-wireless-signal-excellent-symbolic",
                    "Wonderland",
                    "Connected",
                    true,
                    Some("system-lock-screen-symbolic"),
                ),
                (
                    "bluetooth-active-symbolic",
                    "Magic Trackpad",
                    "",
                    false,
                    None,
                ),
                (
                    "audio-speakers-symbolic",
                    "Built-in Speakers",
                    "",
                    true,
                    Some("object-select-symbolic"),
                ),
            ] {
                c.append(&k::DeviceRow::new(icon, t, s, on, tr).button);
            }
            c.upcast()
        }),
        kit_case("detail-page", || {
            let p = k::DetailPage::new(
                "Bluetooth",
                Some(true),
                "Devices",
                Some("Bluetooth Settings…"),
            );
            p.set_rows(&[], "No paired devices");
            p.root.upcast()
        }),
        kit_case("slider-module", || {
            k::SliderModule::new(
                "Sound",
                "audio-volume-medium-symbolic",
                40.0,
                |_| {},
                Some("Output"),
            )
            .root
            .upcast()
        }),
        kit_case("now-playing", || {
            // Full card (widgets) with cover art missing: the player's app icon stands in; the
            // compact card (Control Center) with no player known: the note.
            let c = w::vbox(w::S2);
            let full = k::NowPlayingCard::new();
            full.set(&Fake.now_playing().unwrap());
            full.set_app_icon(Fake.now_playing_icon().as_deref());
            c.append(&full.root);
            let compact = k::NowPlayingCard::compact();
            compact.set(&Fake.now_playing().unwrap());
            c.append(&compact.root);
            c.upcast()
        }),
        kit_case("battery-tile", || {
            use shell::status::BatteryStatus as B;
            let r = w::hbox(w::S2);
            r.set_homogeneous(true);
            for (p, st, m, low) in [
                (82, B::Discharging, Some(191), false),
                (18, B::Discharging, Some(40), true),
                (7, B::Discharging, Some(12), false),
                (54, B::Charging, Some(80), false),
            ] {
                let t = k::BatteryTile::new();
                t.set(p, st, m, low);
                r.append(&t.button);
            }
            r.upcast()
        }),
    ]
}
