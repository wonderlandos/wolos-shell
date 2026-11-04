// SPDX-License-Identifier: GPL-2.0-or-later
//! System Monitor (`wol-monitor`, docs/modules/monitor.md): what the computer is doing, live.
//! Three layers, each in its own place:
//! - data: `wol_core::monitor` (pure parsers and the one sampler, tested on fixture trees);
//! - [`feed`]: the only code here that reads or changes the system (sampler thread, D-Bus,
//!   signals), always off the GTK thread;
//! - [`model`]: a minute of history and the rows the pages show, pure;
//! - the pages ([`overview`], [`apps`], [`processes`], [`disks`], [`network`], [`startup`]):
//!   Kit widgets that draw the model and hand the person's choices to callbacks ([`Actions`]).
pub mod apps;
pub mod confirm;
pub mod demo;
pub mod disks;
pub mod feed;
pub mod model;
pub mod network;
pub mod overview;
pub mod processes;
pub mod startup;
pub mod tile;

use crate::copy::monitor as copy;

/// Application id (desktop file `org.wolos.monitor.desktop`, window class) and program.
pub const APP_ID: &str = "org.wolos.monitor";
pub const BIN: &str = "wol-monitor";
use crate::kit::{self, widgets as w};
use gtk::{glib, prelude::*};
use model::{AppRow, Model};
use std::cell::RefCell;
use std::rc::Rc;
use wol_core::monitor::procs::Target;
use wol_core::monitor::sampler::Want;

/// Done callback of an action that runs elsewhere: true when it worked.
pub type Done = Box<dyn FnOnce(bool)>;
/// Done callback of a Quit or End Process: what reached the processes.
pub type Sent = Box<dyn FnOnce(feed::Sent)>;

/// What the person's choices do; the window wires the real ones (`feed`), goldens none.
pub struct Actions {
    /// Quit an app (`force`: it is paused, so it is killed).
    pub quit: Rc<dyn Fn(&AppRow, bool, Sent)>,
    /// End one process.
    pub end: Rc<dyn Fn(Target, &'static str, Sent)>,
    pub switch: startup::Switch,
    pub login_items: Rc<dyn Fn()>,
    /// The visible page changed: what the sampler should read.
    pub want: Rc<dyn Fn(Want)>,
    /// The Startup page opened and has nothing yet.
    pub load_startup: Rc<dyn Fn()>,
}

impl Actions {
    /// Every choice does nothing (goldens, demo).
    pub fn none() -> Actions {
        Actions {
            quit: Rc::new(|_, _, d| d(feed::Sent::All)),
            end: Rc::new(|_, _, d| d(feed::Sent::All)),
            switch: Rc::new(|_, _, d| d(true)),
            login_items: Rc::new(|| {}),
            want: Rc::new(|_| {}),
            load_startup: Rc::new(|| {}),
        }
    }
}

/// What a page needs the sampler to read beyond the always-on numbers.
pub fn want_for(page: &str) -> Want {
    Want {
        procs: page == copy::PROCESSES,
        disks: page == copy::DISKS,
        pss: page == copy::APPS,
    }
}

pub struct Monitor {
    pub aw: w::AppWindow,
    pub model: Rc<RefCell<Model>>,
    overview: overview::Page,
    apps: apps::Page,
    procs: Rc<processes::Page>,
    disks: disks::Page,
    network: network::Page,
    startup: startup::Page,
    pub filter: gtk::SearchEntry,
    end: gtk::Button,
    pub confirm: Rc<confirm::Confirm>,
    actions: Actions,
}

/// The window's content over `model`.
pub fn content(model: Rc<RefCell<Model>>, actions: Actions) -> Rc<Monitor> {
    let aw = w::AppWindow::new(true, true);
    Rc::new_cyclic(|me: &std::rc::Weak<Monitor>| {
        let quit: apps::Quit = {
            let me = me.clone();
            Rc::new(move |row: &AppRow, force: bool| {
                if let Some(m) = me.upgrade() {
                    m.ask_quit(row.clone(), force);
                }
            })
        };
        let fold: processes::Fold = {
            let (me, model) = (me.clone(), model.clone());
            Rc::new(move |pid| {
                {
                    let mut m = model.borrow_mut();
                    if !m.folded.remove(&pid) {
                        m.folded.insert(pid);
                    }
                }
                if let Some(m) = me.upgrade() {
                    m.procs.update(&m.model.borrow());
                }
            })
        };
        let overview = overview::Page::new();
        let apps = apps::Page::new(quit);
        let procs = processes::Page::new(fold);
        let disks = disks::Page::new();
        let network = network::Page::new();
        let login = actions.login_items.clone();
        let startup = startup::Page::new(actions.switch.clone(), move || login());
        aw.source_section(copy::TITLE);
        aw.source_item(
            "utilities-system-monitor-symbolic",
            copy::OVERVIEW,
            &overview.root,
        );
        aw.source_item("view-app-grid-symbolic", copy::APPS, &apps.root);
        aw.source_item("view-list-symbolic", copy::PROCESSES, &procs.root);
        aw.source_item("drive-harddisk-symbolic", copy::DISKS, &disks.root);
        aw.source_item("network-wired-symbolic", copy::NETWORK, &network.root);
        aw.source_item("system-run-symbolic", copy::STARTUP, &startup.root);
        let filter = w::search_field(copy::FILTER);
        let end = {
            let me = me.clone();
            w::button(copy::END, w::Kind::Destructive, move || {
                if let Some(m) = me.upgrade() {
                    m.ask_end();
                }
            })
        };
        end.set_sensitive(false);
        // Processes only (`page_changed`); the first page is the Overview.
        end.set_visible(false);
        filter.set_visible(false);
        aw.actions.append(&end);
        aw.actions.append(&filter);
        {
            let e = end.clone();
            *procs.on_select.borrow_mut() = Some(Box::new(move |r| {
                e.set_sensitive(r.is_some());
                // A frozen process can only be force quit, as a paused app.
                e.set_label(if r.is_some_and(|r| r.proc.frozen) {
                    copy::FORCE_QUIT
                } else {
                    copy::END
                });
            }));
        }
        {
            let me = me.clone();
            filter.connect_search_changed(move |f| {
                if let Some(m) = me.upgrade() {
                    m.model.borrow_mut().filter = f.text().to_string();
                    m.procs.update(&m.model.borrow());
                }
            });
        }
        {
            let me = me.clone();
            aw.content.connect_visible_child_name_notify(move |_| {
                if let Some(m) = me.upgrade() {
                    m.page_changed();
                }
            });
        }
        let confirm = confirm::Confirm::new(&aw);
        Monitor {
            aw,
            model,
            overview,
            apps,
            procs,
            disks,
            network,
            startup,
            filter,
            end,
            confirm,
            actions,
        }
    })
}

impl Monitor {
    /// The visible page's name.
    pub fn page(&self) -> String {
        self.aw
            .content
            .visible_child_name()
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    pub fn open(&self, page: &str) {
        self.aw.select(page);
    }

    fn page_changed(&self) {
        let page = self.page();
        let procs = page == copy::PROCESSES;
        self.filter.set_visible(procs);
        self.end.set_visible(procs);
        (self.actions.want)(want_for(&page));
        if page == copy::STARTUP && self.model.borrow().startup.is_none() {
            (self.actions.load_startup)();
        }
        self.update();
    }

    /// Draws the model: the visible page and the status bar (hidden pages catch up when shown).
    pub fn update(&self) {
        let m = self.model.borrow();
        match self.page().as_str() {
            copy::APPS => self.apps.update(&m),
            copy::PROCESSES => self.procs.update(&m),
            copy::DISKS => self.disks.update(&m),
            copy::NETWORK => self.network.update(&m),
            copy::STARTUP => self.startup.update(&m),
            _ => self.overview.update(&m),
        }
        let procs = (!m.procs.is_empty()).then_some(m.procs.len());
        self.aw
            .status
            .set_text(&copy::status(m.last.apps.len(), procs));
    }

    fn ask_quit(self: &Rc<Self>, row: AppRow, force: bool) {
        let me = Rc::downgrade(self);
        let (body, button) = if force {
            (copy::FORCE_QUIT_BODY, copy::FORCE_QUIT)
        } else {
            (copy::QUIT_BODY, copy::QUIT)
        };
        self.confirm
            .ask(&copy::quit_title(&row.name), body, button, move || {
                let Some(m) = me.upgrade() else { return };
                let c = m.confirm.clone();
                (m.actions.quit)(&row, force, Box::new(move |s| tell(&c, s, copy::NOT_QUIT)));
            });
    }

    fn ask_end(self: &Rc<Self>) {
        let Some(r) = self.procs.selected() else {
            return;
        };
        let me = Rc::downgrade(self);
        let sig = model::end_signal(&r.proc);
        let (body, button) = if r.proc.frozen {
            (copy::FORCE_QUIT_BODY, copy::FORCE_QUIT)
        } else {
            (copy::END_BODY, copy::END)
        };
        let target = Target {
            pid: r.proc.pid,
            start: r.proc.start,
        };
        self.confirm.ask(
            &copy::end_title(&r.proc.name, r.proc.pid),
            body,
            button,
            move || {
                let Some(m) = me.upgrade() else { return };
                let c = m.confirm.clone();
                (m.actions.end)(target, sig, Box::new(move |s| tell(&c, s, copy::NOT_ENDED)));
            },
        );
    }
}

/// After a Quit or End Process: nothing to say when every process took it; otherwise the
/// sheet says what happened (`refused`: the sentence for a refusal).
fn tell(c: &confirm::Confirm, s: feed::Sent, refused: &str) {
    match s {
        feed::Sent::All => {}
        feed::Sent::Partly => c.note(copy::PARTLY_ENDED),
        feed::Sent::Gone => c.note(copy::ALREADY_ENDED),
        feed::Sent::Refused => c.note(refused),
    }
}

/// The real choices: signals and D-Bus through [`feed`], off the GTK thread.
fn actions(
    wants: feed::Wants,
    model: std::rc::Weak<RefCell<Model>>,
    redraw: Rc<dyn Fn()>,
) -> Actions {
    let load = {
        let (model, redraw) = (model.clone(), redraw.clone());
        Rc::new(move || {
            let (model, redraw) = (model.clone(), redraw.clone());
            feed::load_startup(move |rows| {
                if let Some(m) = model.upgrade() {
                    m.borrow_mut().startup = Some(rows);
                    redraw();
                }
            });
        })
    };
    Actions {
        quit: Rc::new(|row, force, done| {
            feed::send(
                row.targets.clone(),
                if force { "KILL" } else { "TERM" },
                done,
            );
        }),
        end: Rc::new(|t, sig, done| feed::send(vec![t], sig, done)),
        switch: Rc::new(feed::switch_unit),
        login_items: Rc::new(|| kit::spawn(&["wol-settings".to_owned(), "General".to_owned()])),
        want: Rc::new(move |w| wants.set(w)),
        load_startup: load,
    }
}

pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder().application_id(APP_ID).build();
    app.connect_activate(|app| {
        if let Some(win) = app.active_window() {
            win.present();
            return;
        }
        kit::style::install("");
        let demo = std::env::var_os("WOL_MONITOR_DEMO").is_some();
        let wants = feed::Wants::default();
        // Built empty; the first sample arrives within a second.
        let model = Rc::new(RefCell::new(if demo {
            demo::model()
        } else {
            Model::default()
        }));
        let redraw: Rc<RefCell<Option<std::rc::Weak<Monitor>>>> = Rc::default();
        let draw: Rc<dyn Fn()> = {
            let r = redraw.clone();
            Rc::new(move || {
                if let Some(m) = r.borrow().as_ref().and_then(std::rc::Weak::upgrade) {
                    m.update();
                }
            })
        };
        let acts = if demo {
            Actions::none()
        } else {
            actions(wants.clone(), Rc::downgrade(&model), draw)
        };
        let mon = content(model, acts);
        *redraw.borrow_mut() = Some(Rc::downgrade(&mon));
        let win = gtk::ApplicationWindow::builder()
            .application(app)
            .title(copy::TITLE)
            .default_width(w::APP_SIZE.0)
            .default_height(w::APP_SIZE.1)
            .decorated(false)
            .build();
        win.set_child(Some(&mon.aw.root));
        kit::snapshot_on_map(&win, "monitor");
        if let Some(page) = std::env::args().nth(1) {
            mon.open(&page);
        }
        mon.page_changed();
        if !demo {
            let rx = feed::start(wants);
            let m = Rc::downgrade(&mon);
            {
                let m = m.clone();
                feed::load_entries(move |entries| {
                    if let Some(mon) = m.upgrade() {
                        mon.model.borrow_mut().entries = entries;
                    }
                });
            }
            glib::spawn_future_local(async move {
                while let Ok(s) = rx.recv().await {
                    let Some(mon) = m.upgrade() else { return };
                    mon.model.borrow_mut().push(s);
                    mon.update();
                }
            });
        }
        // The window owns the monitor: it goes when the window does (the sampler sees the
        // channel close and stops).
        let keep = RefCell::new(Some(mon));
        win.connect_close_request(move |_| {
            keep.borrow_mut().take();
            glib::Propagation::Proceed
        });
        win.present();
    });
    app.run_with_args::<&str>(&[]);
    glib::ExitCode::SUCCESS
}
