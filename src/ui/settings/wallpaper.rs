// SPDX-License-Identifier: GPL-2.0-or-later
//! Wallpaper: the Moon Gate (default; day in Light, night in Dark, in the active theme's dye), the
//! places (Cachoeira Station and the earlier ones, when installed), Dynamic (day, dusk and night
//! pictures following the clock) and your own photo. The page names what the desktop shows now.
//! The choice is one journaled line (`wolos/wallpaper`, same file as `wol wallpaper set`); the
//! desktop follows it. Previews are rendered once and cached (`form::wallpaper_thumb`).
use super::*;
use crate::copy::settings as copy;
use kit::pickers::{
    DYNAMIC, DYNAMIC_CHOICE, GATE_CHOICE, Look, PLACE_PREFIX, PLACES, gate_file, place_file,
    shipped_wallpaper, wallpaper_choice, wallpaper_name,
};

/// The preview's display shape (16:9).
const ASPECT: f64 = 16.0 / 9.0;

/// Shipped Dynamic pictures: (saved choice, file, name).
fn shipped() -> Vec<(String, PathBuf, String)> {
    DYNAMIC
        .iter()
        .filter_map(|n| {
            let f = shipped_wallpaper(n)?;
            let name = n
                .split('-')
                .map(|w| {
                    let mut c = w.chars();
                    c.next()
                        .map(|f| f.to_uppercase().chain(c).collect::<String>())
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>()
                .join(" ");
            Some((f.display().to_string(), f, name))
        })
        .collect()
}

/// Day and night pictures of a choice that follows Light and Dark, when both are installed.
fn day_night(f: impl Fn(bool) -> Option<PathBuf>) -> Option<[PathBuf; 2]> {
    Some([f(false)?, f(true)?])
}

/// Saves `choice` (a path or `dynamic`) as one journaled change and tells the desktop. False when
/// the save failed: the desktop is not told (nothing changed), and the caller puts the old tile
/// back (QA-SET-22: the new tile stayed checked and the shell reloaded for nothing).
fn choose(choice: &str, status: &gtk::Label) -> bool {
    let r = save_journaled(
        "wallpaper",
        shared::theme::WALLPAPER_FILE,
        Some(&format!("{choice}\n")),
    );
    say(status, if r.is_ok() { "" } else { SAVE_FAILED });
    if r.is_ok() {
        reload_shell();
    }
    r.is_ok()
}

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let p = w::page("Wallpaper");
    let status = status_label();
    let saved = wallpaper_choice();
    let pics = shipped();
    let tiles = w::wrap_row(w::S3, 4);
    let mut first: Option<gtk::ToggleButton> = None;
    // The tile whose picture is saved: a failed save checks it again (`going_back` keeps that
    // from saving it a second time).
    let current: Rc<RefCell<Option<gtk::ToggleButton>>> = Rc::default();
    let going_back = Rc::new(std::cell::Cell::new(false));
    let mut add = |t: &gtk::ToggleButton, choice: String| {
        if let Some(f) = &first {
            t.set_group(Some(f));
        } else {
            first = Some(t.clone());
        }
        if t.is_active() {
            *current.borrow_mut() = Some(t.clone());
        }
        let (st, current, going_back) = (status.clone(), current.clone(), going_back.clone());
        t.connect_toggled(move |t| {
            if !t.is_active() || going_back.get() {
                return;
            }
            if choose(&choice, &st) {
                *current.borrow_mut() = Some(t.clone());
            } else if let Some(old) = current.borrow().clone() {
                going_back.set(true);
                old.set_active(true);
                going_back.set(false);
            }
        });
        tiles.append(t);
    };
    let look = Look::current();
    let gate = day_night(|n| gate_file(&look.theme, n, ASPECT));
    // The default: the gate, or the dusk picture on an image without it.
    let default = match &gate {
        Some(_) => Some(GATE_CHOICE.to_string()),
        None => shipped_wallpaper(DYNAMIC[1]).map(|f| f.display().to_string()),
    };
    let follows = |choice: &str| match saved.as_deref() {
        Some(s) => s == choice,
        None => default.as_deref() == Some(choice),
    };
    // The Moon Gate: selected when chosen, by default, and when a saved photo is gone.
    let known = |s: &str| {
        s == DYNAMIC_CHOICE
            || s.starts_with(PLACE_PREFIX)
            || pics.iter().any(|x| x.0 == s)
            || std::path::Path::new(s).is_file()
    };
    let gate_on = follows(GATE_CHOICE) || saved.as_deref().is_some_and(|s| !known(s));
    if let Some(files) = &gate {
        let strips: Vec<&std::path::Path> = files.iter().map(|f| f.as_path()).collect();
        add(
            &form::wallpaper_strips_tile(&strips, "Moon Gate", gate_on),
            GATE_CHOICE.into(),
        );
    }
    for (id, name) in PLACES {
        let Some(files) = day_night(|n| place_file(id, n, ASPECT)) else {
            continue;
        };
        let choice = format!("{PLACE_PREFIX}{id}");
        let strips: Vec<&std::path::Path> = files.iter().map(|f| f.as_path()).collect();
        add(
            &form::wallpaper_strips_tile(&strips, name, follows(&choice)),
            choice,
        );
    }
    if !pics.is_empty() {
        let strips: Vec<&std::path::Path> = pics.iter().map(|x| x.1.as_path()).collect();
        add(
            &form::wallpaper_strips_tile(&strips, "Dynamic", follows(DYNAMIC_CHOICE)),
            DYNAMIC_CHOICE.into(),
        );
    }
    for (choice, file, name) in &pics {
        add(
            &form::wallpaper_tile(Some(file), name, follows(choice)),
            choice.clone(),
        );
    }
    let own = saved
        .as_deref()
        .filter(|s| *s != DYNAMIC_CHOICE && *s != GATE_CHOICE && !s.starts_with(PLACE_PREFIX))
        .filter(|s| !pics.iter().any(|x| x.0 == *s))
        .map(PathBuf::from)
        .filter(|f| f.is_file());
    if let Some(f) = own {
        add(
            &form::wallpaper_tile(Some(&f), "Your Photo", true),
            f.display().to_string(),
        );
    }
    p.append(&w::section(
        "",
        &[form::info_row(
            "Now Showing",
            &wallpaper_name(saved.as_deref(), &look, kit::now().hour),
        )],
    ));
    p.append(&w::section("", &[tiles.clone().upcast()]));
    if let Some(card) = tiles.parent() {
        card.add_css_class("preview-card");
    }
    p.append(&w::footnote(copy::WALLPAPER_SIDES));
    if !pics.is_empty() {
        p.append(&w::footnote(copy::WALLPAPER_DYNAMIC));
    }
    let (st, page, env) = (status.clone(), p.downgrade(), Rc::downgrade(e));
    p.append(&w::section(
        "",
        &[form::button_row(
            "Your own photo",
            copy::WALLPAPER_OWN_PHOTO,
            "Add Photo…",
            move || {
                let (Some(page), st, env) = (page.upgrade(), st.clone(), env.clone()) else {
                    return;
                };
                kit::pickers::choose_picture(&page, move |f| {
                    choose(&f.display().to_string(), &st);
                    // The new photo shows as the chosen tile.
                    if let Some(e) = env.upgrade() {
                        e.refresh();
                    }
                });
            },
        )],
    ));
    p.append(&status);
    p
}
