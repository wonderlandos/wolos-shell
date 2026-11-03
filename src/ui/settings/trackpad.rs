// SPDX-License-Identifier: GPL-2.0-or-later
//! Trackpad & Mouse: pointer speed, scrolling and clicking (desktop prefs, applied live, journaled).
use super::*;

/// Row titles, as the page shows them and Settings search finds them (`super::search::ROWS`).
pub(super) mod rows {
    pub const SPEED: &str = "Tracking speed";
    pub const TAP: &str = "Tap to click";
    pub const SECONDARY: &str = "Secondary click";
    /// Both the trackpad's and the mouse's.
    pub const NATURAL: &str = "Natural scrolling";
    pub const WHILE_TYPING: &str = "Ignore the trackpad while typing";
}

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let p = w::page("Trackpad & Mouse");
    let status = status_label();
    let d = e.desk.get();
    let toggle = |title: &str,
                  sub: &str,
                  on: bool,
                  set: fn(&mut wol_core::hyprprefs::DesktopPrefs, bool)| {
        let (dk, st) = (e.desk.clone(), status.clone());
        form::pending_toggle_row(title, sub, on, move |v, done| {
            dk.set_switch(&st, |p| set(p, v), done)
        })
    };
    // The slider, so a failed save can put it back to the saved speed (Desk::set_or).
    let slider = Rc::new(glib::WeakRef::<gtk::Scale>::new());
    let (dk, st, sl) = (e.desk.clone(), status.clone(), slider.clone());
    let speed = form::slider_row(
        rows::SPEED,
        (-1.0, 1.0, 0.1),
        f64::from(d.pointer_speed),
        ("Slow", "Fast"),
        move |v| {
            let (dk2, sl) = (dk.clone(), sl.clone());
            dk.set_or(
                &st,
                |p| p.pointer_speed = v as f32,
                move || {
                    if let Some(s) = sl.upgrade() {
                        s.set_value(f64::from(dk2.get().pointer_speed));
                    }
                },
            )
        },
    );
    slider.set(control_in::<gtk::Scale>(&speed).as_ref());
    p.append(&w::section("", &[speed]));
    p.append(&w::section(
        "Trackpad",
        &[
            toggle(rows::TAP, "Tap with one finger.", d.tap_to_click, |p, v| {
                p.tap_to_click = v
            }),
            toggle(
                rows::SECONDARY,
                "Click or tap with two fingers.",
                d.two_finger_secondary,
                |p, v| p.two_finger_secondary = v,
            ),
            toggle(
                rows::NATURAL,
                "Content follows your fingers.",
                d.touchpad_natural_scroll,
                |p, v| p.touchpad_natural_scroll = v,
            ),
            toggle(rows::WHILE_TYPING, "", d.disable_while_typing, |p, v| {
                p.disable_while_typing = v
            }),
        ],
    ));
    p.append(&w::section(
        "Mouse",
        &[toggle(
            rows::NATURAL,
            "Content follows the wheel like a trackpad.",
            d.mouse_natural_scroll,
            |p, v| p.mouse_natural_scroll = v,
        )],
    ));
    p.append(&status);
    p
}
