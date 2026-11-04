// SPDX-License-Identifier: GPL-2.0-or-later
//! Motion goldens (SP-4, `docs/modules/hig.md` section 7): goldens are stills, so each case runs
//! its surface's animation on a manual clock to the end and draws the state it lands in. The
//! frames between are checked as numbers on the way (start, middle, end), not as pictures.
use super::Case;
use gtk::{gdk, prelude::*};
use shell::kit::anim::Clock;
use shell::ui;
use std::sync::Arc;

const MS: u64 = 1_000_000;

/// Steps `clock` every 16 ms from `from_ms` to `to_ms` (a 60 Hz frame clock).
fn run(clock: &Clock, from_ms: u64, to_ms: u64) {
    let mut t = from_ms;
    while t <= to_ms {
        clock.step(t * MS);
        t += 16;
    }
}

fn wall(name: &str) -> gdk::Texture {
    let p = format!(
        "{}/../../design/wallpapers/{name}.svg",
        env!("CARGO_MANIFEST_DIR")
    );
    gdk::Texture::from_filename(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

/// A Moon Gate picture (design brand/wallpapers) decoded at the golden's size.
fn gate(mode: &str) -> gdk::Texture {
    let p = format!(
        "{}/../../design/brand/wallpapers/wolos-gate-{mode}-4k.svg",
        env!("CARGO_MANIFEST_DIR")
    );
    let pb = gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(&p, 480, 270, false)
        .unwrap_or_else(|e| panic!("{p}: {e}"));
    gdk::Texture::for_pixbuf(&pb)
}

fn note(id: u32, app: &str, summary: &str, body: &str) -> ui::notify::Note {
    ui::notify::Note {
        id,
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

/// The first Dock tile button under `root`.
fn first_tile(root: &gtk::Widget) -> gtk::Button {
    let mut todo = vec![root.clone()];
    while let Some(w) = todo.pop() {
        if w.has_css_class("dock-tile")
            && let Ok(b) = w.clone().downcast::<gtk::Button>()
        {
            return b;
        }
        let mut c = w.last_child();
        while let Some(x) = c {
            c = x.prev_sibling();
            todo.push(x);
        }
    }
    panic!("no dock tile")
}

pub fn cases() -> Vec<Case> {
    vec![
        Case {
            // Control Center opened: its modules entered 30 ms apart; at the end all are in.
            name: "motion-controlcenter-open".into(),
            size: (ui::control_center::NARROW_WIDTH + 112, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let root = ui::control_center::content(Arc::new(shell::system::Fake));
                let clock = Clock::manual();
                let moves = ui::control_center::open_stagger(&root, &clock);
                let mods = ui::control_center::main_modules(&root);
                assert!(mods.len() >= 3, "the main page has modules");
                assert!(mods.iter().all(|m| m.opacity() == 0.0), "start: none in");
                run(&clock, 0, 60);
                let first = mods[0].opacity();
                let last = mods[mods.len() - 1].opacity();
                assert!(first > last, "mid: the first is ahead ({first} vs {last})");
                run(&clock, 76, 700);
                assert!(mods.iter().all(|m| m.opacity() == 1.0), "end: all in");
                assert!(clock.is_empty());
                drop(moves);
                root
            }),
        },
        Case {
            // The Dock with the pointer on its first tile: the tile settled at 1.15.
            name: "motion-dock-hover".into(),
            size: (975, 140),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let running = ["foot".to_string()].into_iter().collect();
                let d = ui::dock::content(&running);
                let clock = Clock::manual();
                d.magnify.use_clock(&clock);
                let tile = first_tile(d.root.upcast_ref());
                d.magnify.hover(Some(&tile));
                run(&clock, 0, 48);
                let mid = d.magnify.scale_of(&tile);
                assert!(mid > 1.0 && mid < 1.2, "mid: on its way ({mid})");
                run(&clock, 64, 1000);
                assert_eq!(d.magnify.scale_of(&tile), ui::dock::MAGNIFY, "end: settled");
                assert!(clock.is_empty());
                d.root.clone().upcast()
            }),
        },
        Case {
            // A wallpaper change: the new picture faded in over 400 ms and replaced the old.
            name: "motion-wallpaper-crossfade".into(),
            size: (480, 270),
            class: "preview",
            wallpaper: false,
            build: Box::new(|| {
                let clock = Clock::manual();
                let f = ui::panels::CrossFade::new(&clock);
                let (day, dusk) = (wall("wonderland-day"), wall("wonderland-dusk"));
                f.show(&day);
                assert_eq!(f.progress(), 0.0, "the first picture shows at once");
                f.show(&dusk);
                run(&clock, 0, 192);
                let p = f.progress();
                assert!(p > 0.3 && p < 1.0, "mid: half way in ({p})");
                run(&clock, 208, 600);
                assert!(clock.is_empty());
                assert_eq!(
                    f.current().as_ref(),
                    Some(dusk.upcast_ref::<gdk::Paintable>()),
                    "end: the new picture"
                );
                f.root.set_size_request(480, 270);
                f.root.clone().upcast()
            }),
        },
        Case {
            // Day to night on the Moon Gate: no cross-fade, the ring light, the ribbon dye and the
            // sun move over 600 ms (kit::gate). The still is the frame at 300 ms: the ring already
            // lit for the night, the ribbons on their way, the sun still warm.
            name: "motion-wallpaper-gate-morph-mid".into(),
            size: (480, 270),
            class: "preview",
            wallpaper: false,
            build: Box::new(|| {
                let clock = Clock::manual();
                let f = ui::panels::CrossFade::new(&clock);
                let (day, night) = (gate("day"), gate("night"));
                let m = ui::panels::morph_between(&day, &night).expect("same size");
                f.show(&day);
                f.show_morph(m, &day, &night);
                assert_eq!(f.morph_progress(), 0.0, "starts on the day picture");
                run(&clock, 0, 300);
                let p = f.morph_progress();
                assert!(p > 0.4 && p < 0.6, "mid: half way ({p})");
                assert_eq!(
                    f.current().as_ref(),
                    Some(night.upcast_ref::<gdk::Paintable>()),
                    "the picture coming in is the night"
                );
                f.root.set_size_request(480, 270);
                f.root.clone().upcast()
            }),
        },
        Case {
            // The same morph run to its end hands the real night picture to the desktop.
            name: "motion-wallpaper-gate-morph-end".into(),
            size: (480, 270),
            class: "preview",
            wallpaper: false,
            build: Box::new(|| {
                let clock = Clock::manual();
                let f = ui::panels::CrossFade::new(&clock);
                let (day, night) = (gate("day"), gate("night"));
                let m = ui::panels::morph_between(&day, &night).expect("same size");
                f.show(&day);
                f.show_morph(m, &day, &night);
                run(&clock, 0, 700);
                assert!(clock.is_empty(), "nothing left moving");
                assert_eq!(f.morph_progress(), 1.0);
                assert_eq!(
                    f.current().as_ref(),
                    Some(night.upcast_ref::<gdk::Paintable>()),
                    "end: the real night picture, not the last frame"
                );
                f.root.set_size_request(480, 270);
                f.root.clone().upcast()
            }),
        },
        Case {
            // Notification Center opened: rows entered newest first, 30 ms apart.
            name: "motion-notifications-open".into(),
            size: (400, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                // The Center's list on its own (the panel's scroller has no natural height).
                let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
                for (i, (app, s, b)) in [
                    ("Mail", "Ana Ruiz", "Lunch on Friday?"),
                    ("Calendar", "Stand-up", "In 10 minutes"),
                    ("Files", "Copy finished", "3 files to Documents"),
                ]
                .into_iter()
                .enumerate()
                {
                    list.append(&ui::notify::banner(&note(i as u32 + 1, app, s, b), 1));
                }
                let clock = Clock::manual();
                let moves = ui::notify::stagger_rows(&list, &clock);
                assert_eq!(moves.len(), 3);
                run(&clock, 0, 32);
                let o: Vec<f32> = moves.iter().map(|a| a.value()).collect();
                assert!(o[0] > o[2], "mid: the newest leads ({o:?})");
                run(&clock, 48, 700);
                assert!(moves.iter().all(|a| a.value() == 1.0), "end: all in");
                list.upcast()
            }),
        },
        Case {
            // ⌘Tab moved on: the names cross-faded to the new selection (150 ms).
            name: "motion-switcher-select".into(),
            size: (900, 200),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let c = |a: &str, class: &str, f: i64| shell::hypr::Client {
                    focus: f,
                    ..shell::hypr::Client::new(a, class, "")
                };
                let apps = shell::switcher::apps(
                    &[
                        c("0x1", "foot", 0),
                        c("0x2", "org.gnome.Nautilus", 1),
                        c("0x3", "helium", 2),
                    ],
                    &[],
                );
                let s = ui::switcher::Strip::new(&apps, 1);
                let clock = Clock::manual();
                s.animate(&clock);
                s.select(2);
                run(&clock, 0, 64);
                let (a, b) = (s.name_opacity(1), s.name_opacity(2));
                assert!(
                    a > 0.0 && a < 1.0 && b > 0.0 && b < 1.0,
                    "mid: both ({a}, {b})"
                );
                run(&clock, 80, 400);
                assert_eq!((s.name_opacity(1), s.name_opacity(2)), (0.0, 1.0), "end");
                s.root.set_halign(gtk::Align::Center);
                s.root.set_valign(gtk::Align::Center);
                s.root.clone().upcast()
            }),
        },
    ]
}

/// Reduce motion: the same calls land in the same end state with no frame at all.
pub fn reduce_motion_snaps() {
    let clock = Clock::manual();
    clock.set_reduced(Some(true));
    let root = ui::control_center::content(Arc::new(shell::system::Fake));
    let _moves = ui::control_center::open_stagger(&root, &clock);
    assert!(
        ui::control_center::main_modules(&root)
            .iter()
            .all(|m| m.opacity() == 1.0),
        "Control Center modules in at once"
    );
    let d = ui::dock::content(&Default::default());
    d.magnify.use_clock(&clock);
    let tile = first_tile(d.root.upcast_ref());
    d.magnify.hover(Some(&tile));
    assert_eq!(d.magnify.scale_of(&tile), ui::dock::MAGNIFY);
    d.magnify.hover(None);
    assert_eq!(d.magnify.scale_of(&tile), 1.0);
    let f = ui::panels::CrossFade::new(&clock);
    let (day, dusk) = (wall("wonderland-day"), wall("wonderland-dusk"));
    f.show(&day);
    f.show(&dusk);
    assert_eq!(
        f.current().as_ref(),
        Some(dusk.upcast_ref::<gdk::Paintable>()),
        "the new picture at once"
    );
    assert!(clock.is_empty());
}
