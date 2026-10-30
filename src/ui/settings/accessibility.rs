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
    let (dk, st) = (e.desk.clone(), status.clone());
    let mut pointer = vec![form::slider_row(
        "Pointer size",
        CURSOR,
        f64::from(d.cursor_size).clamp(CURSOR.0, CURSOR.1),
        ("Normal", "Large"),
        move |v| dk.set(&st, |p| p.cursor_size = v as u32),
    )];
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
