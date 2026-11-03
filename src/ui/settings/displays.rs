// SPDX-License-Identifier: GPL-2.0-or-later
//! Displays: resolution, refresh rate, scale and arrangement next to the main display per monitor,
//! one Night Shift switch for all (ADR 0031 W5). The `monitor =` rule is built by
//! `crate::displays::rule`, applied live through `Action::Monitor`, and saved to the journaled
//! displays.conf only when the display took it.
use super::*;
use crate::displays::{self as disp, Geometry, Place};
use std::cell::Cell;

// ---- Displays ----

fn resolutions(m: &crate::system::Monitor) -> Vec<(u32, u32, f64)> {
    let mut v: Vec<(u32, u32, f64)> = Vec::new();
    for mode in &m.modes {
        if let Some((w, h, r)) = wol_core::controls::parse_mode(mode)
            && !v.iter().any(|x| x.0 == w && x.1 == h)
        {
            v.push((w, h, r));
        }
    }
    if v.is_empty() {
        v.push((m.width, m.height, m.refresh));
    }
    v
}

/// Refresh rates at one resolution; the current rate when the display lists none.
fn rates(m: &crate::system::Monitor, w: u32, h: u32) -> Vec<f64> {
    let r = disp::refresh_rates(&m.modes, w, h);
    if r.is_empty() { vec![m.refresh] } else { r }
}

/// Lists `r` in the rate menu `d`.
fn set_rate_labels(d: &gtk::DropDown, r: &[f64]) {
    let l: Vec<String> = r.iter().map(|x| disp::hz_label(*x)).collect();
    let refs: Vec<&str> = l.iter().map(String::as_str).collect();
    w::set_options(d, &refs);
}

const SCALES: [(&str, f64); 5] = [
    ("100%", 1.0),
    ("125%", 1.25),
    ("150%", 1.5),
    ("175%", 1.75),
    ("200%", 2.0),
];

fn geometry(m: &crate::system::Monitor) -> Geometry {
    Geometry {
        width: m.width,
        height: m.height,
        scale: m.scale,
    }
}

/// What one display's menus say right now.
#[derive(Clone, Copy)]
struct Choice {
    res: usize,
    rate: usize,
    scale: usize,
    place: usize,
}

/// How a mode change ended. The display is asked first; displays.conf is written only when it
/// took the mode, so a refused mode never comes back at the next login (audit5 S7-8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Settled {
    /// Live and saved.
    Saved,
    /// Live, but displays.conf could not be written: it lasts until the next login.
    NotSaved,
    /// The display did not take it (the sentence to show): nothing was saved, the menus go back.
    Refused(&'static str),
}

/// `live` is the display's answer; `save` writes displays.conf and runs only when it is Ok.
fn settle(live: Result<(), ActionError>, save: impl FnOnce() -> std::io::Result<()>) -> Settled {
    match live {
        Err(e) => Settled::Refused(e.message()),
        Ok(()) => match save() {
            Ok(()) => Settled::Saved,
            Err(_) => Settled::NotSaved,
        },
    }
}

/// One display's section: its menus, what they ask for and what the display runs now.
struct Display {
    ctx: Rc<Ctx>,
    monitor: crate::system::Monitor,
    res: Vec<(u32, u32, f64)>,
    /// The rates [`Self::rate_menu`] lists (they follow the resolution).
    rates: RefCell<Vec<f64>>,
    /// The main display (name, size) a placement is next to.
    main: Option<(String, Geometry)>,
    rel: String,
    status: gtk::Label,
    /// What the menus ask for.
    cur: Cell<Choice>,
    /// What the display took last (the menus go back here when a change is refused).
    live: Cell<Choice>,
    /// Set while [`Self::show`] moves the menus (their handlers then do nothing).
    quiet: Cell<bool>,
    res_menu: gtk::DropDown,
    rate_menu: gtk::DropDown,
    scale_menu: gtk::DropDown,
    place_menu: Option<gtk::DropDown>,
}

impl Display {
    /// Moves every menu to `c` (the rate menu lists `c`'s resolution's rates) without applying.
    fn show(&self, c: Choice) {
        self.quiet.set(true);
        let (w, h, _) = self.res.get(c.res).copied().unwrap_or((
            self.monitor.width,
            self.monitor.height,
            self.monitor.refresh,
        ));
        let rates = rates(&self.monitor, w, h);
        if *self.rates.borrow() != rates || self.rate_menu.model().is_none() {
            set_rate_labels(&self.rate_menu, &rates);
            *self.rates.borrow_mut() = rates;
        }
        let at = |i: usize| u32::try_from(i).unwrap_or(0);
        self.res_menu.set_selected(at(c.res));
        self.rate_menu.set_selected(at(c.rate));
        self.scale_menu.set_selected(at(c.scale));
        if let Some(p) = &self.place_menu {
            p.set_selected(at(c.place));
        }
        self.quiet.set(false);
    }

    /// A menu moved: `next` of what they asked for so far becomes the request.
    fn pick(self: &Rc<Self>, next: impl FnOnce(Choice) -> Choice) {
        if self.quiet.get() {
            return;
        }
        let c = next(self.cur.get());
        self.cur.set(c);
        self.show(c);
        self.apply(c);
    }

    /// Asks the display for `c`; on Ok saves it to displays.conf, on a refusal puts the menus back.
    fn apply(self: &Rc<Self>, c: Choice) {
        let Some(&(w, h, first)) = self.res.get(c.res) else {
            return;
        };
        let hz = self.rates.borrow().get(c.rate).copied().unwrap_or(first);
        let scale = SCALES.get(c.scale).map_or(1.0, |s| s.1);
        let place = Place::ALL.get(c.place).copied().unwrap_or(Place::Auto);
        let main = self.main.as_ref().map(|(n, g)| (n.as_str(), *g));
        let Some(rule) = disp::rule(&self.monitor.name, (w, h, hz), scale, place, main) else {
            return;
        };
        let me = Rc::downgrade(self);
        self.ctx.act(
            Action::Monitor(rule.clone()),
            Rc::new(move |r| {
                let Some(me) = me.upgrade() else { return };
                let rel = me.rel.clone();
                let done = settle(r, || {
                    let text = prefs::displays_conf(read(&config(&rel)).as_deref(), &rule);
                    save_journaled("displays", &rel, Some(&text))
                });
                match done {
                    Settled::Saved => {
                        me.live.set(c);
                        say(&me.status, "");
                    }
                    Settled::NotSaved => {
                        me.live.set(c);
                        say(&me.status, SAVE_FAILED);
                    }
                    Settled::Refused(why) => {
                        let live = me.live.get();
                        me.cur.set(live);
                        me.show(live);
                        say(&me.status, why);
                    }
                }
            }),
        );
    }
}

/// One display's section: Resolution, Refresh Rate, Scale and, for a display that is not the
/// main one, Arrangement. Night Shift is not here: it is one setting for every display.
fn display_section(
    ctx: &Rc<Ctx>,
    m: &crate::system::Monitor,
    saved: &str,
    rel: &str,
    main: Option<&(String, Geometry)>,
    status: &gtk::Label,
) -> gtk::Widget {
    let res = resolutions(m);
    let is_main = main.is_some_and(|(n, _)| *n == m.name);
    let r0 = res
        .iter()
        .position(|r| r.0 == m.width && r.1 == m.height)
        .unwrap_or(0);
    let (rw, rh, _) = res
        .get(r0)
        .copied()
        .unwrap_or((m.width, m.height, m.refresh));
    let saved_rule = saved
        .lines()
        .filter_map(|l| l.trim().strip_prefix("monitor = "))
        .find(|r| r.split(',').next() == Some(m.name.as_str()));
    let place = match (main, saved_rule) {
        (Some((_, mg)), Some(r)) if !is_main => disp::place_of(r, *mg, geometry(m)),
        _ => Place::Auto,
    };
    let now = Choice {
        res: r0,
        rate: rates(m, rw, rh)
            .iter()
            .position(|r| (r - m.refresh).abs() < 0.01)
            .unwrap_or(0),
        scale: SCALES
            .iter()
            .position(|s| (s.1 - m.scale).abs() < 0.01)
            .unwrap_or(0),
        place: Place::ALL.iter().position(|p| *p == place).unwrap_or(0),
    };
    let labels: Vec<String> = res.iter().map(|r| format!("{} × {}", r.0, r.1)).collect();
    let lref: Vec<&str> = labels.iter().map(String::as_str).collect();
    let scales: Vec<&str> = SCALES.iter().map(|s| s.0).collect();
    let places: Vec<&str> = Place::ALL.iter().map(|p| p.label()).collect();
    let d = Rc::new(Display {
        ctx: ctx.clone(),
        monitor: m.clone(),
        res,
        rates: RefCell::default(),
        main: main.cloned(),
        rel: rel.to_string(),
        status: status.clone(),
        cur: Cell::new(now),
        live: Cell::new(now),
        quiet: Cell::new(false),
        res_menu: w::dropdown(&lref, 0),
        rate_menu: w::dropdown(&[], 0),
        scale_menu: w::dropdown(&scales, 0),
        place_menu: (!is_main).then(|| w::dropdown(&places, 0)),
    });
    d.show(now);
    let on = |menu: &gtk::DropDown, set: fn(Choice, usize) -> Choice| {
        let me = Rc::downgrade(&d);
        menu.connect_selected_notify(move |dd| {
            let i = usize::try_from(dd.selected()).unwrap_or(0);
            if let Some(me) = me.upgrade() {
                me.pick(|c| set(c, i))
            }
        });
    };
    // A new resolution offers its own rates: the highest one first.
    on(&d.res_menu, |c, res| Choice { res, rate: 0, ..c });
    on(&d.rate_menu, |c, rate| Choice { rate, ..c });
    on(&d.scale_menu, |c, scale| Choice { scale, ..c });
    let mut rows = vec![
        row("Resolution", &d.res_menu),
        row("Refresh Rate", &d.rate_menu),
        row("Scale", &d.scale_menu),
    ];
    if let Some(p) = &d.place_menu {
        on(p, |c, place| Choice { place, ..c });
        rows.push(row("Arrangement", p));
    }
    let title = if m.description.is_empty() {
        m.name.clone()
    } else {
        m.description.clone()
    };
    let section = w::section(&title, &rows);
    // The menus' handlers hold the display weakly: the section keeps it.
    let keep = d.clone();
    section.connect_destroy(move |_| {
        let _ = &keep;
    });
    section.upcast()
}

pub(super) fn page(ctx: &Rc<Ctx>) -> gtk::Box {
    let p = w::page("Displays");
    let mons = ctx.sys.monitors();
    if mons.is_empty() {
        p.append(&w::empty_state(
            "video-display-symbolic",
            "No displays found",
            crate::copy::settings::DISPLAYS_NONE,
            None,
        ));
        return p;
    }
    let status = status_label();
    let rel = format!("{}/displays.conf", prefs::HYPR_DIR);
    let saved = read(&config(&rel)).unwrap_or_default();
    // The main display (Dock, desktop) is the first the desktop lists (`crate::displays::main_of`).
    let main = mons.first().map(|m| (m.name.clone(), geometry(m)));
    for m in &mons {
        p.append(&display_section(
            ctx,
            m,
            &saved,
            &rel,
            main.as_ref(),
            &status,
        ));
    }
    // Night Shift is one setting for every display (`Action::NightLight` has no output), so one
    // switch outside the display sections; it flips back when the action fails (audit5 S7-9).
    let (night_ctx, night_status) = (ctx.clone(), status.clone());
    p.append(&w::section(
        "",
        &[row_sub(
            "Night Shift",
            "Warmer colours after dark",
            &w::pending_toggle(ctx.sys.night_light().unwrap_or(false), move |on, done| {
                night_ctx.act_switch(Action::NightLight(on), &night_status, done)
            }),
        )],
    ));
    p.append(&status);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A refused mode writes nothing (the next login would bring it back); a save that does not
    /// work is reported, not dropped (audit5 S7-8).
    #[test]
    fn only_a_mode_the_display_took_is_saved() {
        let saved = Cell::new(false);
        let r = settle(Err(ActionError::Failed), || {
            saved.set(true);
            Ok(())
        });
        assert_eq!(r, Settled::Refused(ActionError::Failed.message()));
        assert!(!saved.get(), "displays.conf untouched after a refusal");
        assert_eq!(
            settle(Err(ActionError::Invalid), || Ok(())),
            Settled::Refused(ActionError::Invalid.message())
        );
        assert_eq!(settle(Ok(()), || Ok(())), Settled::Saved);
        assert_eq!(
            settle(Ok(()), || Err(std::io::Error::other("disk full"))),
            Settled::NotSaved
        );
    }

    /// One Night Shift switch for the page, not one per display: `Action::NightLight` has no
    /// output (audit5 S7-9). The empty state names no program (Wonderland Doctrine, S7-16).
    #[test]
    fn night_shift_is_one_row_and_the_empty_state_is_copy() {
        let src = include_str!("displays.rs");
        let code = src.split("#[cfg(test)]").next().unwrap();
        let section = &code[code.find("fn display_section(").unwrap()..];
        let section = &section[..section.find("\npub(super) fn page(").unwrap()];
        assert!(
            !section.contains("NightLight"),
            "no per-display Night Shift"
        );
        assert_eq!(code.matches("\"Night Shift\"").count(), 1);
        assert!(
            !code.to_lowercase().contains("hyprland is"),
            "no compositor name"
        );
        assert!(code.contains("copy::settings::DISPLAYS_NONE"));
        assert!(crate::copy::lint(crate::copy::settings::DISPLAYS_NONE).is_empty());
    }
}
