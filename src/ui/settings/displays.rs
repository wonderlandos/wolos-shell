// SPDX-License-Identifier: GPL-2.0-or-later
//! Displays: resolution, refresh rate, scale and arrangement next to the main display per monitor,
//! one Night Shift switch for all (ADR 0031 W5). The `monitor =` rule is built by
//! `crate::displays::rule`, applied live through `Action::Monitor`, and saved to the journaled
//! displays.lua only when the display took it. A changed resolution, refresh rate or scale then
//! waits for "Keep": 15 seconds without an answer put the display back (QA-LIVE-13).
use super::*;
use crate::displays::{self as disp, Geometry, Place};
use std::cell::Cell;
use std::rc::Weak;

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

/// How a mode change ended. The display is asked first; displays.lua is written only when it
/// took the mode, so a refused mode never comes back at the next login (audit5 S7-8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Settled {
    /// Live and saved.
    Saved,
    /// Live, but displays.lua could not be written: it lasts until the next login.
    NotSaved,
    /// The display did not take it (the sentence to show): nothing was saved, the menus go back.
    Refused(&'static str),
}

/// `live` is the display's answer; `save` writes displays.lua and runs only when it is Ok.
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
    page: Weak<Page>,
    monitor: crate::system::Monitor,
    res: Vec<(u32, u32, f64)>,
    /// The rates [`Self::rate_menu`] lists (they follow the resolution).
    rates: RefCell<Vec<f64>>,
    /// The scales [`Self::scale_menu`] lists: the usual five, plus the one it runs at.
    scales: Vec<(String, f64)>,
    /// The main display (name, size) a placement is next to.
    main: Option<(String, Geometry)>,
    /// The rule that pins the main display to 0x0, as it runs now (QA-GFX-1).
    main_rule: Option<String>,
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

    /// The `monitor =` rule for `c`; None when a value does not check.
    fn rule_for(&self, c: Choice) -> Option<String> {
        let &(w, h, first) = self.res.get(c.res)?;
        let hz = rates(&self.monitor, w, h)
            .get(c.rate)
            .copied()
            .unwrap_or(first);
        let scale = self.scales.get(c.scale).map_or(1.0, |s| s.1);
        let place = Place::ALL.get(c.place).copied().unwrap_or(Place::Auto);
        let main_now = self.main_now();
        let main = main_now.as_ref().map(|(n, g)| (n.as_str(), *g));
        let rule = disp::rule(&self.monitor.name, (w, h, hz), scale, place, main)?;
        // The main display sits at 0x0 unless somebody placed it by hand: that stays.
        Some(if main.is_some_and(|(n, _)| n == self.monitor.name) {
            let saved = prefs::monitor_rules(&read(&config(&self.rel)).unwrap_or_default());
            disp::keep_position(rule, &saved)
        } else {
            rule
        })
    }

    /// The main display's name and size as the page shows it now (a mode it took since the page
    /// was built included): a placement is measured from the main display as it runs.
    fn main_now(&self) -> Option<(String, Geometry)> {
        let (name, built) = self.main.as_ref()?;
        let now = self.page.upgrade().and_then(|p| {
            p.shown
                .borrow()
                .iter()
                .find(|m| &m.name == name)
                .map(geometry)
        });
        Some((name.clone(), now.unwrap_or(*built)))
    }

    /// This is the main display and `sent` is its new rule: the displays placed next to it, as
    /// (rule in force, rule at the same place for the new size) (QA-GFX-19 follow-up).
    fn followers(&self, sent: &str) -> Vec<(String, String)> {
        let Some((main, old)) = self.main_now().filter(|(n, _)| *n == self.monitor.name) else {
            return Vec::new();
        };
        let Some(new) = disp::rule_geometry(sent) else {
            return Vec::new();
        };
        let mut saved = prefs::monitor_rules(&read(&config(&self.rel)).unwrap_or_default());
        if let Some(p) = self.page.upgrade()
            && let Some(c) = p.confirm.borrow().as_ref()
        {
            saved.extend(c.saves().iter().cloned());
        }
        disp::follow_main(&saved, &main, old, new)
    }

    /// The main display's pinning rule, when this display is placed next to it and the saved rules
    /// do not pin it yet (QA-GFX-1).
    fn anchor(&self) -> Option<String> {
        let (main, _) = self
            .main
            .as_ref()
            .filter(|(n, _)| *n != self.monitor.name)?;
        // The file's rules, then what the page holds for Keep (a mode just changed is not in the
        // file yet; pinning from the file alone would put the old mode back).
        let mut saved = prefs::monitor_rules(&read(&config(&self.rel)).unwrap_or_default());
        if let Some(p) = self.page.upgrade()
            && let Some(c) = p.confirm.borrow().as_ref()
        {
            saved.extend(c.saves().iter().cloned());
        }
        disp::anchor_rule(&saved, main, self.main_rule.as_deref())
    }

    /// The menus go back to what the display runs, and `why` says so.
    fn refuse(&self, why: &str) {
        let live = self.live.get();
        self.cur.set(live);
        self.show(live);
        say(&self.status, why);
    }

    /// Asks the display for `c`. A new resolution, rate or scale (or any change while one waits
    /// for Keep) is held by the page until Keep and written then; anything else is written now.
    /// A refusal puts the menus back and writes nothing.
    fn apply(self: &Rc<Self>, c: Choice) {
        let Some(rule) = self.rule_for(c) else {
            return;
        };
        let before = self.live.get();
        let asks_keep = (before.res, before.rate, before.scale) != (c.res, c.rate, c.scale);
        let waiting = self
            .page
            .upgrade()
            .is_some_and(|p| p.confirm.borrow().is_some());
        // What puts this display back if nobody keeps the change: what it ran with before.
        let hold = (asks_keep || waiting)
            .then(|| self.rule_for(before))
            .flatten()
            .map(|previous| Hold {
                previous,
                restart: asks_keep,
            });
        match self.anchor() {
            // The main display is pinned first, so the compositor never sees a position next to
            // a main display it is free to move. If it refuses, nothing else is sent.
            Some(pin) => {
                let me = Rc::downgrade(self);
                self.ctx.act(
                    Action::Monitor(pin.clone()),
                    Rc::new(move |r| {
                        let Some(me) = me.upgrade() else { return };
                        match r {
                            Ok(()) => me.send(c, rule.clone(), hold.clone(), Some(pin.clone())),
                            Err(e) => me.refuse(e.message()),
                        }
                    }),
                );
            }
            None => self.send(c, rule, hold, None),
        }
    }

    /// Sends `rule` (`pin`: the main display's rule, applied just before, saved with it).
    fn send(self: &Rc<Self>, c: Choice, rule: String, hold: Option<Hold>, pin: Option<String>) {
        let me = Rc::downgrade(self);
        let sent = rule.clone();
        self.ctx.act(
            Action::Monitor(rule),
            Rc::new(move |r| {
                let Some(me) = me.upgrade() else { return };
                let rel = me.rel.clone();
                // Read before `took` moves the page to the new size.
                let followers = if r.is_ok() {
                    me.followers(&sent)
                } else {
                    Vec::new()
                };
                let rules: Vec<String> = pin
                    .iter()
                    .cloned()
                    .chain([sent.clone()])
                    .chain(followers.iter().map(|f| f.1.clone()))
                    .collect();
                // Held changes are written on Keep: a session that ends before that must not bring
                // the new mode back at the next login.
                let done = settle(r, || {
                    if hold.is_some() {
                        return Ok(());
                    }
                    save_displays(&rel, &lua_with(read(&config(&rel)), &rules))
                });
                match done {
                    Settled::Saved | Settled::NotSaved => {
                        me.live.set(c);
                        say(
                            &me.status,
                            if done == Settled::Saved {
                                ""
                            } else {
                                SAVE_FAILED
                            },
                        );
                        if let Some(page) = me.page.upgrade() {
                            page.took(&me.monitor.name, &sent);
                            // The displays next to the main one move with its new size.
                            for (_, moved) in &followers {
                                me.ctx.act(Action::Monitor(moved.clone()), Rc::new(|_| {}));
                                let name = moved.split(',').next().unwrap_or_default();
                                page.took(name, moved);
                            }
                            if let Some(h) = &hold {
                                page.hold(
                                    disp::Undo {
                                        name: me.monitor.name.clone(),
                                        rule: h.previous.clone(),
                                    },
                                    h.restart,
                                    rules,
                                );
                                for (before, moved) in &followers {
                                    let name = moved.split(',').next().unwrap_or_default();
                                    page.hold(
                                        disp::Undo {
                                            name: name.to_string(),
                                            rule: before.clone(),
                                        },
                                        false,
                                        Vec::new(),
                                    );
                                }
                            }
                        }
                    }
                    Settled::Refused(why) => me.refuse(why),
                }
            }),
        );
    }
}

/// A change the page holds until Keep.
#[derive(Clone)]
struct Hold {
    /// The rule that puts this display back.
    previous: String,
    /// A new mode (the countdown starts over), not an arrangement.
    restart: bool,
}

/// `current` displays.lua text with `rules` written into it, in order.
fn lua_with(current: Option<String>, rules: &[String]) -> String {
    let mut text = current;
    for r in rules {
        text = Some(prefs::displays_lua(text.as_deref(), r));
    }
    text.unwrap_or_default()
}

/// displays.lua saved in the journal under "displays": Settings > Restore lists it, `wol undo`
/// takes it back. Not through `save_journaled`: its Undo toast only restores the file, and the
/// countdown ([`Page::hold`]) writes nothing before Keep.
fn save_displays(rel: &str, text: &str) -> std::io::Result<()> {
    wol_core::journal::Journal::for_config(&PathBuf::from(kit::config_home()))
        .save(
            "displays",
            "settings",
            &[(rel, Some(text))],
            wol_core::journal::now(),
        )
        .map(drop)
}

/// One display's section: Resolution, Refresh Rate, Scale and, for a display that is not the
/// main one, Arrangement. Night Shift is not here: it is one setting for every display.
fn display_section(
    page: &Rc<Page>,
    m: &crate::system::Monitor,
    saved: &str,
    main: Option<&(String, Geometry)>,
    main_rule: Option<&str>,
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
    let rules = prefs::monitor_rules(saved);
    let saved_rule = rules
        .iter()
        .map(String::as_str)
        .find(|r| r.split(',').next() == Some(m.name.as_str()));
    let place = match (main, saved_rule) {
        (Some((_, mg)), Some(r)) if !is_main => disp::place_of(r, *mg, geometry(m)),
        _ => Place::Auto,
    };
    let scales = disp::scale_choices(m.scale);
    let now = Choice {
        res: r0,
        rate: rates(m, rw, rh)
            .iter()
            .position(|r| (r - m.refresh).abs() < 0.01)
            .unwrap_or(0),
        scale: disp::scale_index(&scales, m.scale),
        place: Place::ALL.iter().position(|p| *p == place).unwrap_or(0),
    };
    let labels: Vec<String> = res.iter().map(|r| format!("{} × {}", r.0, r.1)).collect();
    let lref: Vec<&str> = labels.iter().map(String::as_str).collect();
    let scale_labels: Vec<&str> = scales.iter().map(|s| s.0.as_str()).collect();
    let places: Vec<&str> = Place::ALL.iter().map(|p| p.label()).collect();
    let d = Rc::new(Display {
        ctx: page.ctx.clone(),
        page: Rc::downgrade(page),
        monitor: m.clone(),
        res,
        rates: RefCell::default(),
        main: main.cloned(),
        main_rule: main_rule.map(str::to_string),
        scales: scales.clone(),
        rel: page.rel.clone(),
        status: status.clone(),
        cur: Cell::new(now),
        live: Cell::new(now),
        quiet: Cell::new(false),
        res_menu: w::dropdown(&lref, 0),
        rate_menu: w::dropdown(&[], 0),
        scale_menu: w::dropdown(&scale_labels, 0),
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

/// The sheet that asks "Keep these display settings?" while the countdown runs.
struct KeepSheet {
    revealer: gtk::Revealer,
    line: gtk::Label,
}

/// The Displays page: the sections (built again when a display comes or goes), the countdown for a
/// changed mode, and the displays it shows.
struct Page {
    ctx: Rc<Ctx>,
    root: gtk::Box,
    body: gtk::Box,
    rel: String,
    /// The displays the sections were built from (updated as changes are taken).
    shown: RefCell<Vec<crate::system::Monitor>>,
    confirm: RefCell<Option<disp::Confirm>>,
    timer: RefCell<Option<glib::SourceId>>,
    sheet: RefCell<Option<KeepSheet>>,
    /// The window's close request is hooked (once, when the first countdown starts).
    close_hooked: Cell<bool>,
    /// The page's own status line (the sections have theirs): where the revert says what it did.
    note: RefCell<Option<gtk::Label>>,
}

impl Page {
    /// The displays now: the snapshot, without the ones the desktop no longer shows.
    fn displays(&self) -> Vec<crate::system::Monitor> {
        let live: Vec<String> = gtk::gdk::Display::default()
            .map(|d| {
                let list = d.monitors();
                (0..list.n_items())
                    .filter_map(|i| list.item(i).and_downcast::<gtk::gdk::Monitor>())
                    .filter_map(|m| m.connector().map(|c| c.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        disp::still_connected(self.ctx.sys.monitors(), &live)
    }

    /// Builds the sections again from the displays now.
    fn fill(self: &Rc<Self>) {
        while let Some(c) = self.body.first_child() {
            self.body.remove(&c);
        }
        let mons = self.displays();
        *self.shown.borrow_mut() = mons.clone();
        if mons.is_empty() {
            let me = Rc::downgrade(self);
            let retry = w::button(
                crate::copy::settings::DISPLAYS_NONE_RETRY,
                w::Kind::Default,
                move || {
                    if let Some(me) = me.upgrade() {
                        me.fill();
                    }
                },
            );
            self.body.append(&w::empty_state(
                "video-display-symbolic",
                crate::copy::settings::DISPLAYS_NONE_TITLE,
                crate::copy::settings::DISPLAYS_NONE,
                Some(retry),
            ));
            return;
        }
        let status = status_label();
        *self.note.borrow_mut() = Some(status.clone());
        let saved = read(&config(&self.rel)).unwrap_or_default();
        // The main display (Dock, desktop) is `crate::displays::main_of`: the built-in panel,
        // else the first the desktop lists. Control Center asks the same function.
        let main_mon = disp::main_of(&mons);
        let main = main_mon.map(|m| (m.name.clone(), geometry(m)));
        let main_rule = main_mon.and_then(|m| {
            disp::rule(
                &m.name,
                (m.width, m.height, m.refresh),
                m.scale,
                Place::Auto,
                main.as_ref().map(|(n, g)| (n.as_str(), *g)),
            )
        });
        for m in &mons {
            self.body.append(&display_section(
                self,
                m,
                &saved,
                main.as_ref(),
                main_rule.as_deref(),
                &status,
            ));
        }
        // Night Shift is one setting for every display (`Action::NightLight` has no output), so
        // one switch outside the display sections; it flips back when the action fails (audit5
        // S7-9). Without hyprsunset there is nothing to switch, so no row (QA-SET-23).
        if let Some(on) = self.ctx.sys.night_light() {
            let (night_ctx, night_status) = (self.ctx.clone(), status.clone());
            self.body.append(&w::section(
                "",
                &[row_sub(
                    "Night Shift",
                    "Warmer colours after dark",
                    &w::pending_toggle(on, move |on, done| {
                        night_ctx.act_switch(Action::NightLight(on), &night_status, done)
                    }),
                )],
            ));
        }
        self.body.append(&status);
    }

    /// The desktop's display list or the snapshot moved: builds the sections again when a display
    /// came or went or runs another mode (a change this page took is already in `shown`).
    fn follow(self: &Rc<Self>) {
        if self.confirm.borrow().is_some() {
            // Mid-countdown the sections stay as they are; Keep or Undo builds them again.
            return;
        }
        if disp::monitors_changed(&self.shown.borrow(), &self.displays()) {
            self.fill();
        }
    }

    /// `rule` (`NAME,WxH@R,POS,SCALE`) was taken by display `name`: `shown` says so too.
    fn took(&self, name: &str, rule: &str) {
        let parts: Vec<&str> = rule.split(',').collect();
        let Some((w, h, hz)) = parts.get(1).and_then(|m| wol_core::controls::parse_mode(m)) else {
            return;
        };
        let scale = parts.get(3).and_then(|s| s.parse::<f64>().ok());
        if let Some(m) = self.shown.borrow_mut().iter_mut().find(|m| m.name == name) {
            (m.width, m.height, m.refresh) = (w, h, hz);
            if let Some(s) = scale {
                m.scale = s;
            }
        }
    }

    /// The countdown sheet: over the window (so it shows on any pane), made on first use.
    fn show_sheet(self: &Rc<Self>) {
        if self.sheet.borrow().is_none() {
            let title = w::label(crate::copy::settings::DISPLAYS_KEEP_TITLE, "title");
            title.set_wrap(true);
            let line = w::label("", "secondary");
            line.set_wrap(true);
            let (keep_me, revert_me) = (Rc::downgrade(self), Rc::downgrade(self));
            let keep = w::button("Keep", w::Kind::Primary, move || {
                if let Some(p) = keep_me.upgrade() {
                    p.keep();
                }
            });
            let revert = w::button("Undo", w::Kind::Default, move || {
                if let Some(p) = revert_me.upgrade() {
                    p.revert();
                }
            });
            let buttons = w::hbox(w::S2);
            buttons.append(&w::spacer());
            buttons.append(&revert);
            buttons.append(&keep);
            let col = w::vbox(w::S2);
            col.append(&title);
            col.append(&line);
            col.append(&buttons);
            let revealer = w::sheet(&col);
            let overlay = self
                .root
                .ancestor(gtk::Overlay::static_type())
                .and_then(|o| o.downcast::<gtk::Overlay>().ok());
            match overlay {
                Some(o) => {
                    revealer.set_halign(gtk::Align::Center);
                    revealer.set_valign(gtk::Align::Start);
                    revealer.set_width_request(w::SHEET_WIDTH);
                    revealer.add_css_class("toolbar-sheet");
                    o.add_overlay(&revealer);
                }
                // No window overlay (a bare preview): the sheet is the first thing on the page.
                None => self
                    .root
                    .insert_child_after(&revealer, self.root.first_child().as_ref()),
            }
            *self.sheet.borrow_mut() = Some(KeepSheet { revealer, line });
        }
        let s = self.sheet.borrow();
        if let Some(s) = s.as_ref() {
            let left = self
                .confirm
                .borrow()
                .as_ref()
                .map_or(0, disp::Confirm::left);
            s.line
                .set_label(&crate::copy::settings::displays_keep_line(left));
            s.revealer.set_reveal_child(true);
            s.revealer.set_can_target(true);
        }
    }

    /// A change the page holds until Keep: the sheet asks, and the countdown starts (a new mode
    /// starts it over, an arrangement made meanwhile leaves the clock alone).
    fn hold(self: &Rc<Self>, undo: disp::Undo, restart: bool, saves: Vec<String>) {
        let fresh = {
            let mut c = self.confirm.borrow_mut();
            match c.as_mut() {
                Some(c) if restart => {
                    c.change(undo, saves);
                    true
                }
                Some(c) => {
                    c.hold(undo, saves);
                    false
                }
                None => {
                    *c = Some(disp::Confirm::arm(undo, saves));
                    true
                }
            }
        };
        self.show_sheet();
        // Closing the window with the question open is no answer: the display goes back first.
        if !self.close_hooked.replace(true)
            && let Some(win) = self.root.root().and_downcast::<gtk::Window>()
        {
            let me = Rc::downgrade(self);
            win.connect_close_request(move |_| {
                if let Some(p) = me.upgrade() {
                    p.revert_now();
                }
                glib::Propagation::Proceed
            });
        }
        if !fresh && self.timer.borrow().is_some() {
            return;
        }
        if let Some(old) = self.timer.borrow_mut().take() {
            old.remove();
        }
        let me = Rc::downgrade(self);
        let id = glib::timeout_add_local(
            std::time::Duration::from_millis(disp::tick_millis()),
            move || {
                let Some(p) = me.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                p.tick()
            },
        );
        *self.timer.borrow_mut() = Some(id);
    }

    /// One second of the countdown.
    fn tick(self: &Rc<Self>) -> glib::ControlFlow {
        let tick = self.confirm.borrow_mut().as_mut().map(disp::Confirm::tick);
        match tick {
            Some(disp::Tick::Left(_)) => {
                self.show_sheet();
                glib::ControlFlow::Continue
            }
            Some(disp::Tick::Revert) => {
                // This source ends with the return below: the revert must not remove it.
                self.timer.borrow_mut().take();
                self.revert();
                glib::ControlFlow::Break
            }
            None => glib::ControlFlow::Break,
        }
    }

    /// Ends the countdown; the sheet goes. Returns what it was waiting on.
    fn stop(&self) -> Option<disp::Confirm> {
        if let Some(old) = self.timer.borrow_mut().take() {
            old.remove();
        }
        if let Some(s) = self.sheet.borrow().as_ref() {
            s.revealer.set_reveal_child(false);
            s.revealer.set_can_target(false);
        }
        self.confirm.borrow_mut().take()
    }

    /// Keep: what was held is written to displays.lua now, in one journal entry (Settings >
    /// Restore lists it), and the page follows the displays again.
    fn keep(self: &Rc<Self>) {
        if let Some(c) = self.stop()
            && !c.saves().is_empty()
        {
            let text = lua_with(read(&config(&self.rel)), c.saves());
            if save_displays(&self.rel, &text).is_err()
                && let Some(n) = self.note.borrow().as_ref()
            {
                say(n, SAVE_FAILED);
            }
        }
        self.follow();
    }

    /// The window closed without an answer: the same revert, waited for (the process ends next).
    fn revert_now(&self) {
        let Some(c) = self.stop() else { return };
        for u in c.rules() {
            let _ = self
                .ctx
                .sys
                .spawn(Action::Monitor(u.rule.clone()))
                .recv_blocking();
        }
    }

    /// Revert (the button, the end of the countdown): the displays go back to what they ran with.
    /// Nothing was written to displays.lua, so there is nothing to undo there.
    fn revert(self: &Rc<Self>) {
        let Some(c) = self.stop() else { return };
        let failed = false;
        let rules = c.rules();
        let pending = Rc::new(Cell::new(rules.len()));
        let failed = Rc::new(Cell::new(failed));
        for u in rules {
            let (me, pending, failed) = (Rc::downgrade(self), pending.clone(), failed.clone());
            self.ctx.act(
                Action::Monitor(u.rule.clone()),
                Rc::new(move |r| {
                    failed.set(failed.get() || r.is_err());
                    pending.set(pending.get().saturating_sub(1));
                    if pending.get() == 0
                        && let Some(me) = me.upgrade()
                    {
                        me.fill();
                        if let Some(n) = me.note.borrow().as_ref() {
                            say(
                                n,
                                if failed.get() {
                                    crate::copy::settings::DISPLAYS_REVERT_FAILED
                                } else {
                                    crate::copy::settings::DISPLAYS_REVERTED
                                },
                            );
                        }
                    }
                }),
            );
        }
    }
}

pub(super) fn page(ctx: &Rc<Ctx>) -> gtk::Box {
    let p = w::page("Displays");
    let body = w::vbox(w::S6);
    p.append(&body);
    let page = Rc::new(Page {
        ctx: ctx.clone(),
        root: p.clone(),
        body,
        rel: prefs::DISPLAYS_FILE.to_string(),
        shown: RefCell::default(),
        confirm: RefCell::default(),
        timer: RefCell::default(),
        sheet: RefCell::default(),
        close_hooked: Cell::default(),
        note: RefCell::default(),
    });
    page.fill();
    // A display comes or goes (the desktop's list) or the snapshot changes: the sections follow.
    if let Some(d) = gtk::gdk::Display::default() {
        let (me, list) = (Rc::downgrade(&page), d.monitors());
        let id = list.connect_items_changed(move |_, _, _, _| {
            if let Some(me) = me.upgrade() {
                me.follow();
            }
        });
        let (list, id) = (list.downgrade(), RefCell::new(Some(id)));
        p.connect_destroy(move |_| {
            if let (Some(l), Some(id)) = (list.upgrade(), id.borrow_mut().take()) {
                l.disconnect(id);
            }
        });
    }
    if let Some(rx) = ctx.sys.watch() {
        let me = Rc::downgrade(&page);
        glib::spawn_future_local(async move {
            while rx.recv().await.is_ok() {
                let Some(me) = me.upgrade() else { break };
                me.follow();
            }
        });
    }
    // The page lives as long as its widgets; closing the window without an answer is no answer: the
    // display goes back, and the sheet's overlay goes with the window.
    // The window going away unroots the page: the display goes back, and the process may end next.
    let closing = Rc::downgrade(&page);
    p.connect_root_notify(move |w| {
        if w.root().is_none()
            && let Some(page) = closing.upgrade()
        {
            page.revert_now();
        }
    });
    let keep = page.clone();
    p.connect_destroy(move |_| {
        if keep.confirm.borrow().is_some() {
            keep.revert_now();
        }
        if let Some(s) = keep.sheet.borrow_mut().take()
            && let Some(parent) = s.revealer.parent().and_downcast::<gtk::Overlay>()
        {
            parent.remove_overlay(&s.revealer);
        }
    });
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
        assert!(!saved.get(), "displays.lua untouched after a refusal");
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
        let section = &section[..section.find("/// The sheet that asks").unwrap()];
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
