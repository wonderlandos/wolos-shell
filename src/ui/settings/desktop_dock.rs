// SPDX-License-Identifier: GPL-2.0-or-later
//! Desktop & Dock: Dock and menu bar (shell prefs written to shell.json, applied with `reload`) and
//! Windows (tiling through the managed compositor include, applied live by `Action::Desktop`).
use super::*;
use wol_core::hyprprefs::Layout;
use wol_core::prefs::{AUTOHIDE_DELAY_MAX_MS, TILE_SIZE};
use wol_core::shortcuts::Group;

/// Row titles, as the page shows them and Settings search finds them (`super::search::ROWS`).
pub(super) mod rows {
    pub const MAGNIFY: &str = "Magnification";
    pub const AUTOHIDE: &str = "Automatically hide and show the Dock";
    pub const SIZE: &str = "Size";
    pub const DELAY: &str = "Delay before the Dock shows";
    pub const BATTERY: &str = "Show battery percentage";
    pub const DATE: &str = "Show date";
    pub const TILE: &str = "Tile windows";
    pub const ARRANGEMENT: &str = "Arrangement";
    pub const GAPS: &str = "Space between windows";
    pub const SHORTCUTS: &str = "Tiling shortcuts";
    pub const FLOATING: &str = "Keep Floating";
    pub const POSITION: &str = "Position on screen";
}

/// The edges the Dock may take: every edge but the bar's (`Layout::validate`).
pub(crate) fn dock_edges(bar: shared::layout::Edge) -> Vec<shared::layout::Edge> {
    use shared::layout::Edge;
    [Edge::Left, Edge::Bottom, Edge::Right, Edge::Top]
        .into_iter()
        .filter(|e| *e != bar)
        .collect()
}

fn edge_name(e: shared::layout::Edge) -> &'static str {
    use shared::layout::Edge;
    match e {
        Edge::Left => "Left",
        Edge::Bottom => "Bottom",
        Edge::Right => "Right",
        Edge::Top => "Top",
    }
}

/// `shell.json` with the Dock on `edge`, the prefs and the rest of the layout kept (the Dock
/// items follow it: their order stays, the shell lays them out along the new edge).
pub(crate) fn with_dock_edge(
    old: Option<&str>,
    edge: shared::layout::Edge,
) -> Result<String, String> {
    let mut l = prefs::layout(old);
    l.dock = Some(edge);
    prefs::with_layout(old, &l)
}

/// The Dock's position row (manual dock.md, changelog 0.12.0; QA-SET-26): which edge the Dock sits
/// on. Waits for the save; a failed one puts the menu back.
fn position_row(status: &gtk::Label) -> Option<gtk::Widget> {
    let text = read(&config(prefs::SHELL_FILE));
    let l = prefs::layout(text.as_deref());
    let at_edge = l.dock?;
    let edges = dock_edges(l.bar);
    let names: Vec<&str> = edges.iter().map(|e| edge_name(*e)).collect();
    let at = edges.iter().position(|e| *e == at_edge).unwrap_or(0);
    let st = status.clone();
    Some(form::pending_picker_row(
        rows::POSITION,
        "",
        &names,
        at,
        move |i, done| {
            let old = read(&config(prefs::SHELL_FILE));
            let ok = with_dock_edge(old.as_deref(), edges[i])
                .ok()
                .is_some_and(|t| save_journaled("shell", prefs::SHELL_FILE, Some(&t)).is_ok());
            if ok {
                reload_shell();
            }
            say(&st, if ok { "" } else { SAVE_FAILED });
            done(ok)
        },
    ))
}

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
            t(rows::MAGNIFY, |p| p.dock_magnify, |p, v| p.dock_magnify = v),
            t(
                rows::AUTOHIDE,
                |p| p.dock_autohide,
                |p, v| p.dock_autohide = v,
            ),
        ],
    ));
    let mut placement: Vec<gtk::Widget> = position_row(&status).into_iter().collect();
    placement.extend([
        dock_slider(
            rows::SIZE,
            (f64::from(TILE_SIZE.0), f64::from(TILE_SIZE.1), 4.0),
            ("Small", "Large"),
            &cur,
            &status,
            |p| f64::from(p.tile_size()),
            |p, v| p.dock_tile_size = v as u32,
        ),
        dock_slider(
            rows::DELAY,
            (0.0, f64::from(AUTOHIDE_DELAY_MAX_MS), 100.0),
            ("None", "Long"),
            &cur,
            &status,
            |p| f64::from(p.autohide_delay_ms()),
            |p, v| p.dock_autohide_delay_ms = v as u32,
        ),
    ]);
    p.append(&w::section("", &placement));
    p.append(&w::section(
        "Menu Bar",
        &[
            t(
                rows::BATTERY,
                |p| p.battery_percent,
                |p, v| p.battery_percent = v,
            ),
            t(rows::DATE, |p| p.show_date, |p, v| p.show_date = v),
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
    let knob = glib::WeakRef::<gtk::Scale>::new();
    let k2 = knob.clone();
    let row = form::slider_row(
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
            } else if let Some(s) = k2.upgrade() {
                // The knob goes back to the saved value, like every other slider here
                // (QA-SHADOW-56: it stayed where the failed save left it).
                s.set_value(get(&c.get()).clamp(range.0, range.1));
            }
            say(&st, if ok { "" } else { SAVE_FAILED });
        },
    );
    knob.set(control_in::<gtk::Scale>(&row).as_ref());
    row
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
    // The menu and the slider, so a failed save can put them back to the saved prefs.
    let (menu, slider) = (
        Rc::new(glib::WeakRef::<gtk::DropDown>::new()),
        Rc::new(glib::WeakRef::<gtk::Scale>::new()),
    );
    let (d1, s1, m1) = (desk.clone(), status.clone(), menu.clone());
    let arrangement = form::picker_row(rows::ARRANGEMENT, "", &names, at, move |i| {
        let (dk, m) = (d1.clone(), m1.clone());
        d1.set_or(
            &s1,
            |p| p.layout = LAYOUTS[i].0,
            move || {
                let at = LAYOUTS.iter().position(|l| l.0 == dk.get().layout);
                if let (Some(m), Some(at)) = (m.upgrade(), at) {
                    m.set_selected(u32::try_from(at).unwrap_or_default());
                }
            },
        )
    });
    let (d2, s2, pv, sl) = (
        desk.clone(),
        status.clone(),
        preview.clone(),
        slider.clone(),
    );
    let gaps = form::slider_row(
        rows::GAPS,
        GAPS,
        f64::from(d.looks.gaps).clamp(GAPS.0, GAPS.1),
        ("Narrow", "Wide"),
        move |v| {
            let (dk, pv2, sl) = (d2.clone(), pv.clone(), sl.clone());
            d2.set_or(
                &s2,
                |p| p.looks.gaps = v as u8,
                move || {
                    let looks = dk.get().looks;
                    pv2.set(&looks);
                    if let Some(s) = sl.upgrade() {
                        s.set_value(f64::from(looks.gaps).clamp(GAPS.0, GAPS.1));
                    }
                },
            );
            pv.set(&d2.get().looks);
        },
    );
    menu.set(control_in::<gtk::DropDown>(&arrangement).as_ref());
    slider.set(control_in::<gtk::Scale>(&gaps).as_ref());
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
                rows::TILE,
                crate::copy::DOCK_TILE_HINT,
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
            rows::SHORTCUTS,
            crate::copy::DOCK_TILING_SHORTCUTS_HINT,
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
        if desk.get().float_apps.is_empty() {
            fill_with(holder, desk, status, &[]);
            return;
        }
        let (h, dk, st) = (holder.clone(), desk.clone(), status.clone());
        // Every .desktop file, for the apps' names: the shared list, read off the UI thread
        // (R2A-21, R2A-24).
        glib::spawn_future_local(async move {
            let entries = crate::ui::ipc::installed().await;
            fill_with(&h, &dk, &st, &entries)
        });
    }
    fn fill_with(
        holder: &gtk::Box,
        desk: &Rc<Desk>,
        status: &gtk::Label,
        entries: &[crate::desktop::Entry],
    ) {
        while let Some(c) = holder.first_child() {
            holder.remove(&c);
        }
        let apps = desk.get().float_apps;
        let mut rows: Vec<gtk::Widget> = apps
            .iter()
            .map(|class| {
                let (h, dk, st, c) = (holder.clone(), desk.clone(), status.clone(), class.clone());
                let remove = w::icon_button("list-remove-symbolic", "Remove", move || {
                    dk.set_or(
                        &st,
                        |p| p.float_apps.retain(|x| *x != c),
                        refill(&h, &dk, &st),
                    );
                    fill(&h, &dk, &st);
                });
                let name = app_name(entries, class);
                let sub = if name == *class { "" } else { class.as_str() };
                form::row(&name, sub, &remove)
            })
            .collect();
        let (h, dk, st) = (holder.clone(), desk.clone(), status.clone());
        let add = w::button("Add App…", w::Kind::Default, || {});
        add.connect_clicked(move |b| {
            let taken = dk.get().float_apps;
            let (h, dk, st, b) = (h.clone(), dk.clone(), st.clone(), b.clone());
            // The compositor's client list and every .desktop file: off the UI thread.
            off_thread(
                move || {
                    let entries = crate::desktop::load_all();
                    let mut open: Vec<String> = crate::hypr::clients()
                        .into_iter()
                        .map(|c| c.class)
                        .filter(|c| wol_core::hyprprefs::is_valid_class(c) && !taken.contains(c))
                        .collect();
                    open.sort();
                    open.dedup();
                    open.into_iter()
                        .map(|c| (app_name(&entries, &c), c))
                        .collect::<Vec<(String, String)>>()
                },
                move |named| {
                    let items: Vec<w::MenuItem> = if named.is_empty() {
                        vec![(crate::copy::DOCK_ADD_APP_EMPTY, Box::new(|| {}))]
                    } else {
                        named
                            .iter()
                            .map(|(name, c)| {
                                let (h, dk, st, c) = (h.clone(), dk.clone(), st.clone(), c.clone());
                                let act: Box<dyn Fn()> = Box::new(move || {
                                    dk.set_or(
                                        &st,
                                        |p| p.float_apps.push(c.clone()),
                                        refill(&h, &dk, &st),
                                    );
                                    fill(&h, &dk, &st);
                                });
                                (name.as_str(), act)
                            })
                            .collect()
                    };
                    popup(&b, items);
                },
            );
        });
        let hint = if rows.is_empty() {
            "No apps keep floating"
        } else {
            ""
        };
        rows.push(form::row(hint, "", &add));
        holder.append(&w::section(rows::FLOATING, &rows));
    }
    /// What fills the list again after a failed save (Desk read the file back), while it shows.
    fn refill(holder: &gtk::Box, desk: &Rc<Desk>, status: &gtk::Label) -> impl FnOnce() + 'static {
        let (h, dk, st) = (holder.downgrade(), desk.clone(), status.clone());
        move || {
            if let Some(h) = h.upgrade() {
                fill(&h, &dk, &st);
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// QA-SET-26: the manual's Dock "Position on screen" did not exist.
    #[test]
    fn dock_moves_to_another_edge_and_keeps_the_rest() {
        use shared::layout::Edge;
        assert_eq!(
            dock_edges(Edge::Top),
            [Edge::Left, Edge::Bottom, Edge::Right]
        );
        assert!(!dock_edges(Edge::Bottom).contains(&Edge::Bottom));
        let old = prefs::keep_layout(
            None,
            ShellPrefs {
                dock_magnify: false,
                ..Default::default()
            },
        );
        let new = with_dock_edge(Some(&old), Edge::Left).unwrap();
        let l = prefs::layout(Some(&new));
        assert_eq!(l.dock, Some(Edge::Left));
        assert_eq!(
            l.dock_items,
            prefs::layout(Some(&old)).dock_items,
            "same items, same order"
        );
        assert!(!ShellPrefs::parse(Some(&new)).dock_magnify, "prefs kept");
        assert!(
            with_dock_edge(Some(&old), Edge::Top).is_err(),
            "not on the bar's edge"
        );
    }
}
