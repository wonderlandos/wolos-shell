// SPDX-License-Identifier: GPL-2.0-or-later
//! Accessibility: contrast, transparency, motion, pointer size and zoom. Desktop prefs apply live
//! through one `hyprctl` batch and are journaled; contrast restyles every shell process.
use super::*;

/// Pointer size in px: normal … large, in steps the eye can tell apart.
const CURSOR: (f64, f64, f64) = (24.0, 64.0, 8.0);

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let p = w::page("Accessibility");
    let status = status_label();
    let d = e.desk.get();
    let a = e.sys().accessibility();
    let toggle = |title: &str,
                  sub: &str,
                  on: bool,
                  set: fn(&mut wol_core::hyprprefs::DesktopPrefs, bool)| {
        let (dk, st) = (e.desk.clone(), status.clone());
        form::pending_toggle_row(title, sub, on, move |v, done| {
            dk.set_switch(&st, |p| set(p, v), done)
        })
    };
    let contrast = {
        let (ctx, st) = (e.ctx.clone(), status.clone());
        form::pending_toggle_row(
            "Increase contrast",
            "Darker borders and text that stands out more.",
            a.increase_contrast,
            move |on, done| ctx.act_switch(Action::IncreaseContrast(on), &st, done),
        )
    };
    p.append(&w::section(
        "Display",
        &[
            contrast,
            toggle(
                "Reduce transparency",
                "Solid windows and menus instead of frosted glass.",
                d.reduce_transparency,
                |p, v| p.reduce_transparency = v,
            ),
            toggle(
                "Reduce motion",
                "Windows and spaces appear without sliding or zooming.",
                d.reduce_motion,
                |p, v| p.reduce_motion = v,
            ),
        ],
    ));
    // The slider, so a failed save can put it back to the saved size (Desk::set_or).
    let slider = Rc::new(glib::WeakRef::<gtk::Scale>::new());
    let (dk, st, sl) = (e.desk.clone(), status.clone(), slider.clone());
    let size = form::slider_row(
        "Pointer size",
        CURSOR,
        f64::from(d.cursor_size).clamp(CURSOR.0, CURSOR.1),
        ("Normal", "Large"),
        move |v| {
            let (dk2, sl) = (dk.clone(), sl.clone());
            dk.set_or(
                &st,
                |p| p.cursor_size = v as u32,
                move || {
                    if let Some(s) = sl.upgrade() {
                        s.set_value(f64::from(dk2.get().cursor_size).clamp(CURSOR.0, CURSOR.1));
                    }
                },
            )
        },
    );
    slider.set(control_in::<gtk::Scale>(&size).as_ref());
    let mut pointer = vec![size];
    if let Some(on) = a.zoom {
        let (ctx, st) = (e.ctx.clone(), status.clone());
        pointer.push(form::pending_toggle_row(
            "Zoom",
            "Magnifies the screen around the pointer.",
            on,
            move |v, done| ctx.act_switch(Action::Zoom(v), &st, done),
        ));
    }
    p.append(&w::section("Pointer and Zoom", &pointer));
    p.append(&w::footnote(
        "Control Center can show these switches too: turn on Accessibility Shortcuts in Control Center settings.",
    ));
    p.append(&status);
    p
}
