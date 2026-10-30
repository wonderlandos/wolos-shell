//! Desktop & Dock: Dock and menu bar (shell prefs written to shell.json, applied with `reload`) and
//! Windows (tiling through the managed Hyprland include, applied live by `Action::Desktop`).
use super::*;
use wol_core::hyprprefs::Layout;
use wol_core::prefs::{AUTOHIDE_DELAY_MAX_MS, TILE_SIZE};
use wol_core::shortcuts::Group;

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let p = w::page("Desktop & Dock");
    let cur = Rc::new(Cell::new(load_shell()));
    let status = status_label();
    let t = |title: &str, get: fn(&ShellPrefs) -> bool, set: fn(&mut ShellPrefs, bool)| {
        let (c, st) = (cur.clone(), status.clone());
        form::pending_toggle_row(title, "", get(&cur.get()), move |on, done| {
            let mut v = c.get();
            set(&mut v, on);
            let ok = save_shell(v);
            if ok {
                c.set(v);
            }
            say(&st, if ok { "" } else { SAVE_FAILED });
            done(ok);
        })
    };
    p.append(&w::section(
        "Dock",
        &[
            t(
                "Magnification",
                |p| p.dock_magnify,
                |p, v| p.dock_magnify = v,
            ),
            t(
                "Automatically hide and show the Dock",
                |p| p.dock_autohide,
                |p, v| p.dock_autohide = v,
            ),
        ],
    ));
    p.append(&w::section(
        "",
        &[
            t(
                "Show recent applications in Dock",
                |p| p.dock_show_recents,
                |p, v| p.dock_show_recents = v,
            ),
            dock_slider(
                "Size",
                (f64::from(TILE_SIZE.0), f64::from(TILE_SIZE.1), 4.0),
                ("Small", "Large"),
                &cur,
                &status,
                |p| f64::from(p.tile_size()),
                |p, v| p.dock_tile_size = v as u32,
            ),
            dock_slider(
                "Delay before the Dock shows",
                (0.0, f64::from(AUTOHIDE_DELAY_MAX_MS), 100.0),
                ("None", "Long"),
                &cur,
                &status,
                |p| f64::from(p.autohide_delay_ms()),
                |p, v| p.dock_autohide_delay_ms = v as u32,
            ),
            dock_slider(
                "Minimize effect speed",
                (10.0, 200.0, 5.0),
                ("Slow", "Fast"),
                &cur,
                &status,
                |p| f64::from(p.dock_minimize_speed_pct),
                |p, v| p.dock_minimize_speed_pct = v as u8,
            ),
        ],
    ));
    p.append(&w::section(
        "Menu Bar",
        &[
            t(
                "Show battery percentage",
                |p| p.battery_percent,
                |p, v| p.battery_percent = v,
            ),
            t("Show date", |p| p.show_date, |p, v| p.show_date = v),
        ],
    ));
    p.append(&status);
    p.append(&windows(e));
    p
}

/// A Dock slider row: reads and writes one number of the shell prefs.
#[allow(clippy::too_many_arguments)]
fn dock_slider(
    title: &str,
    range: (f64, f64, f64),
    ends: (&str, &str),
    cur: &Rc<Cell<ShellPrefs>>,
    status: &gtk::Label,
    get: fn(&ShellPrefs) -> f64,
    set: fn(&mut ShellPrefs, f64),
) -> gtk::Widget {
    let (c, st) = (cur.clone(), status.clone());
    form::slider_row(
        title,
        range,
        get(&cur.get()).clamp(range.0, range.1),
        ends,
        move |v| {
            let mut p = c.get();
            set(&mut p, v);
            let ok = save_shell(p);
            if ok {
                c.set(p);
            }
            say(&st, if ok { "" } else { SAVE_FAILED });
        },
    )
}

/// Display names for the two tiling layouts.
const LAYOUTS: [(Layout, &str); 2] = [
    (Layout::Dwindle, "Split the focused window"),
    (Layout::Master, "Main window and stack"),
];

/// Space between tiled windows: Narrow … Wide, in steps the eye can tell apart.
const GAPS: (f64, f64, f64) = (0.0, 24.0, 4.0);

/// Windows: Tile windows, and while on: a live picture, arrangement, spacing, apps that keep
/// floating and the way to the tiling shortcuts.
pub(super) fn windows(e: &Rc<Env>) -> gtk::Widget {
    let desk = &e.desk;
    let b = w::vbox(w::S6);
    let status = status_label();
    let d = desk.get();
    let more = w::vbox(w::S6);
    more.set_visible(d.tiling);
    let preview = kit::pickers::LooksPreview::new(
        kit::pickers::PRESET_THUMB,
        &d.looks,
        kit::pickers::Tokens::current(),
    );
    let names: Vec<&str> = LAYOUTS.iter().map(|l| l.1).collect();
    let at = LAYOUTS.iter().position(|l| l.0 == d.layout).unwrap_or(0);
    let (d1, s1) = (desk.clone(), status.clone());
    let arrangement = form::picker_row("Arrangement", "", &names, at, move |i| {
        d1.set(&s1, |p| p.layout = LAYOUTS[i].0)
    });
    let (d2, s2, pv) = (desk.clone(), status.clone(), preview.clone());
    let gaps = form::slider_row(
        "Space between windows",
        GAPS,
        f64::from(d.looks.gaps).clamp(GAPS.0, GAPS.1),
        ("Narrow", "Wide"),
        move |v| {
            d2.set(&s2, |p| p.looks.gaps = v as u8);
            pv.set(&d2.get().looks);
        },
    );
    let picture = w::vbox(0);
    picture.add_css_class("list-row");
    picture.add_css_class("preview-row");
    picture.append(&preview.root);
    for x in [&arrangement, &gaps, picture.upcast_ref()] {
        x.set_visible(d.tiling);
    }
    let (dk, st, m, a, g, pic) = (
        desk.clone(),
        status.clone(),
        more.clone(),
        arrangement.clone(),
        gaps.clone(),
        picture.clone(),
    );
    b.append(&w::section(
        "Windows",
        &[
            form::pending_toggle_row(
                "Tile windows",
                "New windows fill the screen side by side instead of floating.",
                d.tiling,
                move |on, done| {
                    let (m, a, g, pic) = (m.clone(), a.clone(), g.clone(), pic.clone());
                    dk.set_switch(
                        &st,
                        |p| p.tiling = on,
                        Box::new(move |ok| {
                            let now = on == ok;
                            for x in [m.upcast_ref::<gtk::Widget>(), &a, &g, pic.upcast_ref()] {
                                x.set_visible(now);
                            }
                            done(ok);
                        }),
                    );
                },
            ),
            picture.upcast(),
            arrangement,
            gaps,
        ],
    ));
    more.append(&floating(desk, &status));
    let env = e.clone();
    more.append(&w::section(
        "",
        &[form::button_row(
            "Tiling shortcuts",
            "Move, resize and focus tiled windows from the keyboard.",
            "Show…",
            move || {
                env.shortcut_group.set(Some(Group::Tiling));
                env.open(shortcuts::TITLE);
            },
        )],
    ));
    b.append(&more);
    b.append(&status);
    b.upcast()
}

/// Apps that keep floating while tiling is on: one row each, Add App lists the open windows.
fn floating(desk: &Rc<Desk>, status: &gtk::Label) -> gtk::Widget {
    let holder = w::vbox(0);
    fn fill(holder: &gtk::Box, desk: &Rc<Desk>, status: &gtk::Label) {
        while let Some(c) = holder.first_child() {
            holder.remove(&c);
        }
        let apps = desk.get().float_apps;
        let entries = if apps.is_empty() {
            Vec::new()
        } else {
            crate::desktop::load_all()
        };
        let mut rows: Vec<gtk::Widget> = apps
            .iter()
            .map(|class| {
                let (h, dk, st, c) = (holder.clone(), desk.clone(), status.clone(), class.clone());
                let remove = w::icon_button("list-remove-symbolic", "Remove", move || {
                    dk.set(&st, |p| p.float_apps.retain(|x| *x != c));
                    fill(&h, &dk, &st);
                });
                let name = app_name(&entries, class);
                let sub = if name == *class { "" } else { class.as_str() };
                form::row(&name, sub, &remove)
            })
            .collect();
        let (h, dk, st) = (holder.clone(), desk.clone(), status.clone());
        let add = w::button("Add App…", w::Kind::Default, || {});
        add.connect_clicked(move |b| {
            let taken = dk.get().float_apps;
            let entries = crate::desktop::load_all();
            let mut open: Vec<String> = crate::hypr::clients()
                .into_iter()
                .map(|c| c.class)
                .filter(|c| wol_core::hyprprefs::is_valid_class(c) && !taken.contains(c))
                .collect();
            open.sort();
            open.dedup();
            let named: Vec<(String, String)> = open
                .into_iter()
                .map(|c| (app_name(&entries, &c), c))
                .collect();
            let items: Vec<w::MenuItem> = if named.is_empty() {
                vec![("Open the app first, then add it here", Box::new(|| {}))]
            } else {
                named
                    .iter()
                    .map(|(name, c)| {
                        let (h, dk, st, c) = (h.clone(), dk.clone(), st.clone(), c.clone());
                        let act: Box<dyn Fn()> = Box::new(move || {
                            dk.set(&st, |p| p.float_apps.push(c.clone()));
                            fill(&h, &dk, &st);
                        });
                        (name.as_str(), act)
                    })
                    .collect()
            };
            let m = w::menu(items);
            m.set_parent(b);
            m.popup();
        });
        let hint = if rows.is_empty() {
            "No apps keep floating"
        } else {
            ""
        };
        rows.push(form::row(hint, "", &add));
        holder.append(&w::section("Keep floating", &rows));
    }
    fill(&holder, desk, status);
    holder.upcast()
}

/// Window class as the app's name (desktop entry id or command), else the class itself.
fn app_name(entries: &[crate::desktop::Entry], class: &str) -> String {
    entries
        .iter()
        .find(|e| {
            e.id.trim_end_matches(".desktop")
                .eq_ignore_ascii_case(class)
                || e.exec
                    .first()
                    .and_then(|x| x.rsplit('/').next())
                    .is_some_and(|x| x.eq_ignore_ascii_case(class))
        })
        .map(|e| e.name.clone())
        .unwrap_or_else(|| class.to_string())
}
