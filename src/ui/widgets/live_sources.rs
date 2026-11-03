// SPDX-License-Identifier: GPL-2.0-or-later
//! The live data sources of the desktop widgets: the fullscreen watch, system stats, calendar, music,
//! batteries, weather, custom (Lua and command) widgets, and the start-up that wires them.
use super::super::widgets_builtin::{SystemView, WeatherView};
use super::{Change, Content, Live, covers, local_now, mono_ms, place_key, today_utc};
use crate::widgets::{self as wd, media, power, weather};
use gtk::{gio, glib, prelude::*};
use shared::widgets::{Builtin, Kind, Place, Placed, Units};
use std::rc::Rc;

impl Live {
    /// Pauses the widgets while a window covers this display's desktop (maximized or full
    /// screen). Each display follows its own output's workspace (audit5 S6-2), not the focused
    /// one. The state comes from the shell's one compositor event reader (`ui::ipc::hypr_events`,
    /// read off the GTK thread): no private `.socket2.sock` reader, no request here (R2A-14,
    /// R2A-17).
    pub(super) fn watch_fullscreen(self: &Rc<Self>) {
        self.follow_fullscreen();
        let me = Rc::downgrade(self);
        crate::hypr::on_fullscreen_change(move |_| {
            // A display's desktop unplugged: its hook does nothing from then on.
            if let Some(l) = me.upgrade() {
                l.follow_fullscreen();
            }
        });
        let me = Rc::downgrade(self);
        crate::hypr::on_monitor_fullscreen_change(move || {
            if let Some(l) = me.upgrade() {
                l.follow_fullscreen();
            }
        });
        // The connector is known once the surface is on an output (and changes when the main
        // board moves with the main display): look again then, not only on the next change.
        if let Some(win) = self.board.win.borrow().clone() {
            let me = Rc::downgrade(self);
            win.connect_realize(move |win| {
                let Some(surf) = win.surface() else { return };
                let me = me.clone();
                surf.connect_enter_monitor(move |_, _| {
                    if let Some(l) = me.upgrade() {
                        l.follow_fullscreen();
                    }
                });
            });
        }
    }

    /// The output this board's surface is on (its connector); None before it is mapped.
    pub(super) fn connector(&self) -> Option<String> {
        let win = self.board.win.borrow().clone()?;
        let surf = win.surface()?;
        let mon = gtk::prelude::WidgetExt::display(&win).monitor_at_surface(&surf)?;
        Some(mon.connector()?.to_string())
    }

    pub(super) fn follow_fullscreen(self: &Rc<Self>) {
        let mode = crate::hypr::monitor_fullscreen(self.connector().as_deref());
        self.set_paused(covers(mode));
    }

    pub(super) fn read_system(self: &Rc<Self>) {
        let src = self
            .stats
            .borrow_mut()
            .get_or_insert_with(|| {
                std::sync::Arc::new(std::sync::Mutex::new(
                    wol_core::system::stats::StatsSource::new(std::sync::Arc::new(
                        wol_core::system::PosixHost,
                    )),
                ))
            })
            .clone();
        let me = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let s = match src.lock() {
                Ok(mut s) => s.read(),
                Err(_) => return,
            };
            let Some(l) = me.upgrade() else { return };
            let mut cpu = l.cpu.borrow_mut();
            cpu.push(s.cpu_percent);
            *l.system.borrow_mut() = Some(SystemView {
                cpu: cpu.values(),
                mem_used: s.memory.used,
                mem_total: s.memory.total,
                load: s.load.map(|x| (x.one, x.five, x.fifteen)),
                uptime: s.uptime_secs,
            });
            drop(cpu);
            l.show_kind(Builtin::System);
        });
    }

    pub(super) fn read_events(self: &Rc<Self>) {
        let now = local_now();
        self.events_day.set(now.day);
        let Some((start, end)) = today_utc(&now) else {
            return;
        };
        let me = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let ev = gio::spawn_blocking(move || crate::widgets::eds::read(&start, &end))
                .await
                .ok()
                .flatten();
            if let Some(l) = me.upgrade() {
                *l.events.borrow_mut() = ev;
                l.show_kind(Builtin::Calendar);
            }
        });
    }

    pub(super) fn watch_music(self: &Rc<Self>) {
        let (tx, rx) = async_channel::unbounded();
        media::watch(tx);
        let me = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            while let Ok(p) = rx.recv().await {
                let Some(l) = me.upgrade() else { break };
                *l.music.borrow_mut() = p;
                l.show_kind(Builtin::Music);
            }
        });
    }

    pub(super) fn watch_batteries(self: &Rc<Self>) {
        self.upower.set(Some(true));
        let (tx, rx) = async_channel::unbounded();
        power::watch(tx);
        let me = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            while let Ok(v) = rx.recv().await {
                let Some(l) = me.upgrade() else { break };
                match v {
                    Some(devs) => *l.batteries.borrow_mut() = devs,
                    None => {
                        // No UPower: the built-in battery, on the widget schedule.
                        l.upower.set(Some(false));
                        *l.batteries.borrow_mut() = power::builtin(&*l.sys);
                        l.sync();
                    }
                }
                l.show_kind(Builtin::Battery);
            }
        });
    }

    /// Weather for one widget's place (`only`), or every place on the desktop.
    pub(super) fn refresh_weather(self: &Rc<Self>, only: Option<&str>, force: bool) {
        let mut places: Vec<(Place, Units)> = Vec::new();
        for p in self.board.widgets() {
            if p.kind != Kind::Builtin(Builtin::Weather) || only.is_some_and(|o| o != p.id) {
                continue;
            }
            if let Some(pl) = &p.options.place {
                let u = Self::units(&p);
                if !places
                    .iter()
                    .any(|(q, v)| place_key(q, *v) == place_key(pl, u))
                {
                    places.push((pl.clone(), u));
                }
            }
        }
        for (pl, u) in places {
            let me = Rc::downgrade(self);
            glib::spawn_future_local(async move {
                let cache = wd::cache_home();
                let p2 = pl.clone();
                let out = gio::spawn_blocking(move || {
                    weather::load(&cache, &p2, u, wd::unix_now(), force, |url| {
                        wol_core::widgets::http_get(
                            url,
                            wol_core::widgets::HTTP_TIMEOUT,
                            wol_core::widgets::MAX_OUTPUT,
                        )
                        .map_err(|e| e.message())
                    })
                })
                .await
                .unwrap_or(weather::Outcome::Offline);
                let Some(l) = me.upgrade() else { return };
                let view = match out {
                    weather::Outcome::Fresh(f) => WeatherView::Ready {
                        place: pl.clone(),
                        forecast: f,
                        stale: None,
                    },
                    weather::Outcome::Stale(f, age) => WeatherView::Ready {
                        place: pl.clone(),
                        forecast: f,
                        stale: Some(age),
                    },
                    weather::Outcome::Offline => WeatherView::Offline(pl.clone()),
                };
                l.weather.borrow_mut().insert(place_key(&pl, u), view);
                l.show_kind(Builtin::Weather);
            });
        }
    }

    pub(super) fn run_custom(self: &Rc<Self>, wid: &str) {
        let Some(p) = self.board.widgets().into_iter().find(|p| p.id == wid) else {
            return;
        };
        let Kind::Custom(pkg_id) = &p.kind else {
            return;
        };
        let Some(pkg) = self.board.packages().into_iter().find(|k| &k.id == pkg_id) else {
            return self.show(|q| q.id == wid);
        };
        let Ok(m) = &pkg.manifest else {
            return self.show(|q| q.id == wid);
        };
        let Some(src) = m.source.clone() else {
            return self.show(|q| q.id == wid);
        };
        let desk = self.board.desktop();
        let allowed: Vec<String> = desk
            .grants
            .iter()
            .filter(|g| &g.widget == pkg_id)
            .map(|g| g.host.clone())
            .collect();
        if let Some(h) = src.host()
            && !allowed.contains(&h)
        {
            return self.show(|q| q.id == wid);
        }
        let (dir, name, wid) = (pkg.dir.clone(), m.name.clone(), wid.to_string());
        let me = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let home = wol_core::widgets::home();
            let r = gio::spawn_blocking(move || {
                wol_core::widgets::read_source(&src, &dir, &home, |h| {
                    allowed.iter().any(|a| a == h)
                })
            })
            .await;
            let Some(l) = me.upgrade() else { return };
            match r {
                Ok(Ok(v)) => {
                    l.custom.borrow_mut().insert(wid.clone(), v);
                    l.show(|q| q.id == wid);
                }
                Ok(Err(e)) => {
                    // Keep showing the last good data; without any, say what went wrong.
                    if !l.custom.borrow().contains_key(&wid) {
                        l.board.set_content(
                            &wid,
                            Content::Error {
                                message: crate::copy::widget_source(&e, &name),
                                name,
                                details: vec![e.message()],
                            },
                        );
                    }
                    eprintln!("wol-shell: widget {wid}: {}", e.message());
                }
                Err(_) => {}
            }
        });
    }

    pub(super) fn on_change(self: &Rc<Self>, c: Change) {
        match c {
            Change::Added(id) | Change::Updated(id) | Change::Granted(id) => {
                self.sync();
                let Some(p) = self.board.widgets().into_iter().find(|p| p.id == id) else {
                    return;
                };
                self.show(|q| q.id == id);
                match &p.kind {
                    Kind::Builtin(Builtin::Weather) => self.refresh_weather(Some(&id), false),
                    Kind::Custom(_) => self.start_custom(&p),
                    _ => {}
                }
            }
            Change::Timer(id, press) => {
                if let Some(p) = self.board.widgets().into_iter().find(|p| p.id == id) {
                    self.counter(&p, |c| c.press(press, mono_ms()));
                    self.show(|q| q.id == id);
                    self.sync();
                }
            }
            Change::Lua(file, button) => {
                self.lua.borrow_mut().click(&file, button);
                self.show_lua(&file);
            }
            Change::Removed(p) => {
                self.timers.borrow_mut().remove(&p.id);
                self.custom.borrow_mut().remove(&p.id);
                self.unschedule(&format!("custom:{}", p.id));
                self.sync();
            }
            Change::Edit(_) => {}
            Change::Replaced => self.start_all(),
        }
    }

    pub(super) fn start_custom(self: &Rc<Self>, p: &Placed) {
        let Kind::Custom(pkg_id) = &p.kind else {
            return;
        };
        let refresh = self
            .board
            .packages()
            .into_iter()
            .find(|k| &k.id == pkg_id)
            .and_then(|k| k.manifest.as_ref().ok().map(|m| m.refresh))
            .unwrap_or(0);
        if refresh > 0 {
            self.schedule(&format!("custom:{}", p.id), refresh as u64);
        }
        self.run_custom(&p.id);
        self.arm();
    }

    /// Everything from scratch (start, `wol shell reload`).
    pub(super) fn start_all(self: &Rc<Self>) {
        self.custom.borrow_mut().clear();
        self.sync();
        self.show(|_| true);
        self.refresh_weather(None, false);
        for p in self.board.widgets() {
            if matches!(p.kind, Kind::Custom(_)) {
                self.start_custom(&p);
            }
        }
    }
}
