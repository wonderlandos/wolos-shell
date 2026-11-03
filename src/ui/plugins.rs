// SPDX-License-Identifier: GPL-2.0-or-later
//! Shell plugin host (ADR 0031 W2, docs/modules/plugins.md): the enabled plugins'
//! `[[menubar]]` items, `[[dock]]` entries, `[[widget]]` desktop cards, `[[tile]]` Control Center
//! tiles and `[[hotkey]]` shortcuts, placed by the layout in `shell.json` (`wol_core::prefs`).
//!
//! - Loading is `wol_core::plugins` (the same folders, checks and enable list as `wol plugin`),
//!   read off the GTK thread ([`read_snapshot`]) and applied on it ([`Host::apply`]).
//! - Every script entry is its own Lua state (`wol_lua::plugin::Script`) with the host's
//!   [`DEADLINES`]: one failing or stopped entry shows a sentence in its own place and never
//!   touches another item of the bar. [`MAX_FAILURES`](shared::plugins::MAX_FAILURES) failures in
//!   a row (`wol_core::plugins::Health`) turn the plugin off and leave a toast.
//! - Commands and web reads of a script run on their own thread ([`Live`]): a render gets the
//!   last answer at once, so `wol.run` never holds the menu bar.
//! - The user plugin folder, each plugin folder in it and `plugins.json` are watched ([`watch`]):
//!   a change reloads every surface's host.
//!
//! Each surface (menu bar, Dock, desktop) owns its own [`Host`] for the kinds it draws; tiles and
//! hotkeys need no Lua and are free functions ([`tiles`], [`set_tile`], [`hotkeys`]).
use crate::kit::{self, lifetime::Lifetime, widgets as w};
use crate::ui::app_view::{Act, Sink, View};
use gtk::{gio, glib, prelude::*};
use shared::layout::{BarItem, DockItem, Layout};
use shared::plugins::{Kind, Manifest, Permission, Side, is_icon_file};
use shared::view::Node;
use shared::widgets::Size;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use wol_core::plugins::{self as cp, Enabled, Health, Plugin};
use wol_core::shortcuts::{self, Bind, Combo, Group, Overrides, Shortcut};
use wol_lua::plugin::{Entry, Script};
use wol_lua::{Deadlines, Services, Stamp};

/// Wall-clock limit of each entry point (R2C-3): the scripts run on the GTK thread.
pub const DEADLINES: Deadlines = Deadlines::DEFAULT;
/// Largest output of one `wol.run` or `wol.http_get`.
pub const RUN_OUTPUT: usize = 256 * 1024;
/// Commands and web reads in flight (and answers kept) per host.
pub const MAX_RUNS: usize = 32;
/// What a script reads from `wol.run` / `wol.http_get` before the first answer is back.
pub const RUNNING: &str = "Running…";
/// Kinds each surface runs.
pub const BAR_KINDS: &[Kind] = &[Kind::Menubar];
pub const DESKTOP_KINDS: &[Kind] = &[Kind::Widget];
/// Desktop widget ids of plugin cards start with this (`lua:plugin-...`, see [`widget_key`]).
pub const WIDGET_PREFIX: &str = "plugin-";
/// Settings > Keyboard ids of plugin hotkeys: `plugin:<plugin>/<id>`.
pub const SHORTCUT_PREFIX: &str = "plugin:";
/// Title of the notification a plugin host posts.
pub const TOAST_TITLE: &str = "Plugins";
/// Quiet time after a change in a watched folder before the host reads it again.
const SETTLE: Duration = Duration::from_millis(300);
/// How often a host looks for answers while commands are running.
const ANSWER_POLL: Duration = Duration::from_millis(250);

// ---- Where plugins live ----

/// The plugin folders (user first) and the stores (`plugins.json`, state, toggles).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    pub dirs: Vec<(PathBuf, bool)>,
    pub config_home: PathBuf,
    pub state_home: PathBuf,
}

impl Paths {
    /// This session's XDG folders.
    pub fn user() -> Paths {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let xdg = |var: &str, under_home: &str| {
            std::env::var_os(var)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| home.join(under_home))
        };
        Paths {
            dirs: cp::dirs(&xdg("XDG_DATA_HOME", ".local/share")),
            config_home: xdg("XDG_CONFIG_HOME", ".config"),
            state_home: xdg("XDG_STATE_HOME", ".local/state"),
        }
    }

    /// The user plugin folder (ADR 0031: reloaded on change) and the folder of `plugins.json`.
    pub fn watched(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = self
            .dirs
            .iter()
            .filter(|(_, system)| !system)
            .map(|(d, _)| d.clone())
            .collect();
        if let Some(p) = cp::enabled_path(&self.config_home).parent() {
            v.push(p.to_path_buf());
        }
        v
    }
}

// ---- What is read from disk ----

/// One script file of an enabled plugin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub plugin: String,
    pub kind: Kind,
    pub id: String,
    /// The text, or why it could not be read.
    pub text: Result<String, String>,
}

/// Everything a host needs from disk, read off the GTK thread. Two equal snapshots mean
/// nothing a host draws changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub plugins: Vec<Plugin>,
    pub enabled: Enabled,
    pub sources: Vec<Source>,
}

impl Snapshot {
    /// Enabled plugins with a valid manifest, in name order.
    pub fn on(&self) -> impl Iterator<Item = (&Plugin, &Manifest)> {
        self.plugins.iter().filter_map(|p| {
            let m = p.manifest.as_ref().ok()?;
            self.enabled.is_enabled(&m.id).then_some((p, m))
        })
    }

    pub fn manifest(&self, plugin: &str) -> Option<(&Plugin, &Manifest)> {
        self.on().find(|(_, m)| m.id == plugin)
    }

    /// What the plugin was granted from what it declares (a newer manifest asks again).
    pub fn granted(&self, plugin: &str) -> Vec<Permission> {
        self.manifest(plugin)
            .map(|(_, m)| {
                m.permissions
                    .iter()
                    .copied()
                    .filter(|p| self.enabled.allows(plugin, *p))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether `plugin` may run `argv` from a `[[toggle]]`, `[[dock]]` or `[[hotkey]]` entry
    /// (audit C8-1): a `wol` verb goes through the helper policy (`WOL_HELPER`); anything else
    /// runs as the person, so it needs the `run` permission declared and granted.
    pub fn may_run(&self, plugin: &str, argv: &[String]) -> bool {
        shared::plugins::is_wol_argv(argv) || self.granted(plugin).contains(&Permission::Run)
    }
}

/// Why a toggle, Dock or hotkey command of a plugin did not run (audit C8-1).
pub const NOT_ALLOWED_TO_RUN: &str =
    "it runs a command as you and was not allowed to (Settings > Plugins)";

/// Reads the plugins, the enable list and the scripts of `kinds`. Blocks: run it off the GTK
/// thread ([`load`]); tests call it directly.
pub fn read_snapshot(paths: &Paths, kinds: &[Kind]) -> Snapshot {
    let plugins = cp::load_all(&paths.dirs);
    let enabled = cp::load_enabled(&paths.config_home);
    let mut sources = Vec::new();
    for p in &plugins {
        let Ok(m) = &p.manifest else { continue };
        if !enabled.is_enabled(&m.id) {
            continue;
        }
        for (kind, id, file) in m.scripts() {
            if !kinds.contains(&kind) {
                continue;
            }
            let text = std::fs::read_to_string(p.dir.join(file))
                .map_err(|_| format!("{file}: the file could not be read as text."));
            sources.push(Source {
                plugin: m.id.clone(),
                kind,
                id: id.to_owned(),
                text,
            });
        }
    }
    Snapshot {
        plugins,
        enabled,
        sources,
    }
}

// ---- Services: what scripts reach ----

/// Answers of commands and web reads, by (plugin, what was asked).
#[derive(Default)]
struct Answers {
    done: HashMap<String, Result<String, String>>,
    running: HashSet<String>,
}

/// Runs jobs on their own thread and hands back the last answer for the same question.
#[derive(Clone, Default)]
pub struct Runner {
    answers: Arc<Mutex<Answers>>,
    /// A job finished since the last [`Runner::take_fresh`].
    fresh: Arc<AtomicBool>,
}

impl Runner {
    /// The last answer to `key` (or [`RUNNING`] before the first), and a new run of `job`
    /// unless one is still going or [`MAX_RUNS`] are.
    pub fn ask(
        &self,
        key: String,
        job: impl FnOnce() -> Result<String, String> + Send + 'static,
    ) -> Result<String, String> {
        let mut a = self.answers.lock().unwrap_or_else(|e| e.into_inner());
        let last = a.done.get(&key).cloned();
        if !a.running.contains(&key) && a.running.len() < MAX_RUNS {
            a.running.insert(key.clone());
            let (answers, fresh) = (self.answers.clone(), self.fresh.clone());
            std::thread::spawn(move || {
                let r = job();
                let mut a = answers.lock().unwrap_or_else(|e| e.into_inner());
                a.running.remove(&key);
                if a.done.len() >= MAX_RUNS && !a.done.contains_key(&key) {
                    a.done.clear();
                }
                a.done.insert(key, r);
                fresh.store(true, Ordering::Release);
            });
        }
        last.unwrap_or_else(|| Err(RUNNING.into()))
    }

    pub fn busy(&self) -> bool {
        !self
            .answers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .running
            .is_empty()
    }

    /// True once after a job finished.
    pub fn take_fresh(&self) -> bool {
        self.fresh.swap(false, Ordering::AcqRel)
    }
}

/// argv as the user, limited (`wol_core::widgets::run_limited`), in the plugin folder. `wol`
/// verbs see the plugin as a helper (`WOL_HELPER`): policy, approval and the audit log apply,
/// as for `wol plugin run`.
pub fn run_argv(
    plugin: &str,
    dir: &Path,
    argv: &[String],
    timeout_secs: u32,
) -> Result<String, String> {
    let mut cmd: Vec<String> = Vec::new();
    if argv.first().is_some_and(|a| a == "wol") {
        cmd.extend(["env".to_owned(), format!("WOL_HELPER={plugin}")]);
    }
    cmd.extend(argv.iter().cloned());
    wol_core::widgets::run_limited(
        &cmd,
        dir,
        Duration::from_secs(u64::from(timeout_secs.max(1))),
        RUN_OUTPUT,
    )
    .map(|b| String::from_utf8_lossy(&b).into_owned())
    .map_err(|e| e.message())
}

/// Plugin notifications on their way to wold, across every host (audit C8-4).
const MAX_NOTIFY: usize = 4;
static NOTIFY_IN_FLIGHT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Takes one of the [`MAX_NOTIFY`] posting slots; false when all are taken.
fn notify_slot() -> bool {
    if NOTIFY_IN_FLIGHT.fetch_add(1, Ordering::AcqRel) < MAX_NOTIFY {
        return true;
    }
    NOTIFY_IN_FLIGHT.fetch_sub(1, Ordering::AcqRel);
    false
}

/// The shell's services: this machine for time, notifications, clipboard and panels; the
/// plugin stores for state and toggles; commands and web reads through a [`Runner`].
pub struct Live {
    paths: Paths,
    snap: Rc<RefCell<Snapshot>>,
    runner: Runner,
}

impl Live {
    fn dir(&self, plugin: &str) -> PathBuf {
        self.snap
            .borrow()
            .manifest(plugin)
            .map(|(p, _)| p.dir.clone())
            .unwrap_or_else(|| PathBuf::from("/"))
    }
}

impl Services for Live {
    fn now(&self) -> Stamp {
        wol_lua::Live.now()
    }
    fn system(&self) -> wol_lua::SystemInfo {
        wol_lua::Live.system()
    }
    fn notify(&self, title: &str, body: &str) -> Result<(), String> {
        // Audit C8-4: each post waits on wold off the GTK thread; at most MAX_NOTIFY at once
        // (wol-lua already limits each script to NOTIFY_PER_WINDOW a minute).
        if !notify_slot() {
            return Err(wol_lua::TOO_MANY_NOTIFICATIONS.into());
        }
        let (t, b) = (title.to_owned(), body.to_owned());
        std::thread::spawn(move || {
            let _ = wol_lua::Live.notify(&t, &b);
            NOTIFY_IN_FLIGHT.fetch_sub(1, Ordering::AcqRel);
        });
        Ok(())
    }
    fn run(&self, plugin: &str, argv: &[String], timeout_secs: u32) -> Result<String, String> {
        let (p, dir, a) = (plugin.to_owned(), self.dir(plugin), argv.to_vec());
        let key = format!("{plugin}\0run\0{}", argv.join("\0"));
        self.runner
            .ask(key, move || run_argv(&p, &dir, &a, timeout_secs))
    }
    fn http_get(&self, plugin: &str, url: &str) -> Result<String, String> {
        let host = shared::widgets::manifest::url_host(url).unwrap_or_default();
        if !self.snap.borrow().enabled.allows_host(plugin, &host) {
            return Err(format!(
                "{host}: not allowed for this plugin yet (Settings > Plugins)"
            ));
        }
        let u = url.to_owned();
        self.runner.ask(format!("{plugin}\0http\0{url}"), move || {
            wol_core::widgets::http_get(&u, wol_core::widgets::HTTP_TIMEOUT, RUN_OUTPUT)
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .map_err(|e| e.message())
        })
    }
    fn open(&self, _plugin: &str, target: &str) -> Result<(), String> {
        let uri = if target.contains("://") {
            target.to_owned()
        } else {
            gio::File::for_path(target).uri().to_string()
        };
        gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>)
            .map_err(|_| crate::copy::PLUGIN_OPEN_FAILED.to_owned())
    }
    fn toggle_get(&self, plugin: &str, toggle: &str) -> bool {
        cp::toggle_get(&self.paths.state_home, plugin, toggle)
    }
    fn toggle_set(&self, plugin: &str, toggle: &str, on: bool) -> Result<(), String> {
        let job = toggle_job(&self.paths, &self.snap.borrow(), plugin, toggle, on)?;
        let key = format!("{plugin}\0toggle\0{toggle}\0{on}");
        match self.runner.ask(key, job) {
            Err(m) if m != RUNNING => Err(m),
            _ => Ok(()),
        }
    }
    fn clipboard_text(&self, plugin: &str) -> Result<String, String> {
        wol_lua::Live.clipboard_text(plugin)
    }
    fn show(&self, plugin: &str, panel: &str) -> Result<(), String> {
        wol_lua::Live.show(plugin, panel)
    }
    fn state_load(&self, plugin: &str) -> Option<String> {
        cp::load_state(&self.paths.state_home, plugin)
    }
    fn state_save(&self, plugin: &str, json: &str) -> Result<(), String> {
        cp::save_state(&self.paths.state_home, plugin, json)
    }
}

/// Runs toggle `toggle`'s `on`/`off` command of an enabled plugin, then sets its flag file.
fn toggle_job(
    paths: &Paths,
    snap: &Snapshot,
    plugin: &str,
    toggle: &str,
    on: bool,
) -> Result<impl FnOnce() -> Result<String, String> + Send + 'static, String> {
    let (p, m) = snap
        .manifest(plugin)
        .ok_or_else(|| format!("{plugin}: not enabled"))?;
    let t = m
        .toggle
        .iter()
        .find(|t| t.id == toggle)
        .ok_or_else(|| format!("{plugin}: no toggle {toggle}"))?;
    let argv = if on { t.on.clone() } else { t.off.clone() };
    if !argv.is_empty() && !snap.may_run(plugin, &argv) {
        return Err(format!("{plugin}: {NOT_ALLOWED_TO_RUN}"));
    }
    let (state, dir, plugin, toggle) = (
        paths.state_home.clone(),
        p.dir.clone(),
        plugin.to_owned(),
        toggle.to_owned(),
    );
    Ok(move || {
        if !argv.is_empty() {
            run_argv(&plugin, &dir, &argv, wol_lua::plugin::DEFAULT_TIMEOUT)?;
        }
        cp::toggle_set(&state, &plugin, &toggle, on).map(|()| String::new())
    })
}

// ---- The host ----

/// One script entry: its own Lua state and what it showed last.
struct Slot {
    plugin: String,
    kind: Kind,
    id: String,
    script: Result<Script, String>,
    shown: Option<Result<Node, String>>,
}

/// What a plugin entry shows now: its tree, or the sentence why not.
pub type Shown = Result<Node, String>;

/// A plugin card for the desktop gallery and the widget board.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    /// `lua:` widget id ([`widget_key`]).
    pub key: String,
    pub plugin: String,
    pub id: String,
    pub name: String,
    pub sizes: Vec<Size>,
}

/// A plugin Dock entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockTile {
    pub plugin: String,
    pub id: String,
    pub name: String,
    /// Icon name, or a file inside the plugin folder (absolute).
    pub icon: String,
    pub exec: Vec<String>,
}

/// The enabled plugins of one surface, their scripts and their failure streaks.
pub struct Host {
    kinds: &'static [Kind],
    paths: Paths,
    snap: Rc<RefCell<Snapshot>>,
    services: Rc<dyn Services>,
    runner: Runner,
    slots: Vec<Slot>,
    health: Health,
    toasts: Vec<String>,
    /// Bumped on every [`Host::apply`]: timers of an older load stop.
    generation: u64,
}

impl Host {
    /// The shell's host for `kinds`, with the live services. Nothing is loaded yet: [`load`].
    pub fn new(paths: Paths, kinds: &'static [Kind]) -> Host {
        let snap: Rc<RefCell<Snapshot>> = Rc::default();
        let runner = Runner::default();
        let services = Rc::new(Live {
            paths: paths.clone(),
            snap: snap.clone(),
            runner: runner.clone(),
        });
        Host::build(paths, kinds, snap, services, runner)
    }

    /// A host with other services (tests, previews).
    pub fn with_services(paths: Paths, kinds: &'static [Kind], services: Rc<dyn Services>) -> Host {
        Host::build(paths, kinds, Rc::default(), services, Runner::default())
    }

    fn build(
        paths: Paths,
        kinds: &'static [Kind],
        snap: Rc<RefCell<Snapshot>>,
        services: Rc<dyn Services>,
        runner: Runner,
    ) -> Host {
        Host {
            kinds,
            paths,
            snap,
            services,
            runner,
            slots: Vec::new(),
            health: Health::default(),
            toasts: Vec::new(),
            generation: 0,
        }
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    pub fn kinds(&self) -> &'static [Kind] {
        self.kinds
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Takes a fresh read. False (nothing done) when it equals the one loaded. Every script
    /// loads again (a stopped one runs again) and the failure streaks start over.
    pub fn apply(&mut self, s: Snapshot) -> bool {
        if *self.snap.borrow() == s && self.generation > 0 {
            return false;
        }
        self.slots.clear();
        self.health = Health::default();
        self.snap.replace(s);
        let snap = self.snap.borrow().clone();
        for src in &snap.sources {
            let Some((p, m)) = snap.manifest(&src.plugin) else {
                continue;
            };
            let granted = snap.granted(&m.id);
            let script = src.text.clone().and_then(|text| {
                Script::load_within(
                    Entry {
                        kind: src.kind,
                        plugin: &m.id,
                        id: &src.id,
                        dir: &p.dir,
                    },
                    &text,
                    &granted,
                    self.services.clone(),
                    DEADLINES,
                )
            });
            self.slots.push(Slot {
                plugin: m.id.clone(),
                kind: src.kind,
                id: src.id.clone(),
                script,
                shown: None,
            });
        }
        self.generation += 1;
        true
    }

    /// Turns `plugin` off now (three failures): saved to `plugins.json`, so every surface and
    /// `wol plugin list` agree, and its entries leave this host.
    fn turn_off(&mut self, plugin: &str, toast: String) {
        let mut e = self.snap.borrow().enabled.clone();
        e.disable(plugin);
        if let Err(m) = cp::save_enabled(&self.paths.config_home, &e) {
            self.toasts.push(m);
        }
        self.snap.borrow_mut().enabled = e;
        self.snap
            .borrow_mut()
            .sources
            .retain(|s| s.plugin != plugin);
        self.slots.retain(|s| s.plugin != plugin);
        self.toasts.push(toast);
    }

    /// Counts the outcome of one call of `plugin`.
    fn outcome<T>(&mut self, plugin: &str, r: Result<T, String>) -> Result<T, String> {
        match r {
            Ok(v) => {
                self.health.ok(plugin);
                Ok(v)
            }
            Err(m) => {
                let name = self.name(plugin);
                if let Some(toast) = self.health.failed(plugin, &name, &m) {
                    self.turn_off(plugin, toast);
                }
                Err(m)
            }
        }
    }

    fn slot(&mut self, kind: Kind, plugin: &str, id: &str) -> Option<&mut Slot> {
        self.slots
            .iter_mut()
            .find(|s| s.kind == kind && s.plugin == plugin && s.id == id)
    }

    pub fn has(&self, kind: Kind, plugin: &str, id: &str) -> bool {
        self.slots
            .iter()
            .any(|s| s.kind == kind && s.plugin == plugin && s.id == id)
    }

    /// The plugin's name (its id when it has none).
    pub fn name(&self, plugin: &str) -> String {
        self.snap
            .borrow()
            .manifest(plugin)
            .map_or_else(|| plugin.to_owned(), |(_, m)| m.name.clone())
    }

    /// Runs the entry's `render`. None: no such entry (off, removed, another surface's kind).
    pub fn render(&mut self, kind: Kind, plugin: &str, id: &str) -> Option<Shown> {
        let slot = self.slot(kind, plugin, id)?;
        let r = match &mut slot.script {
            Ok(s) => guarded(|| s.render()),
            Err(m) => Err(m.clone()),
        };
        slot.shown = Some(r.clone());
        Some(self.outcome(plugin, r))
    }

    /// What the entry showed last (None: not rendered yet, or gone).
    pub fn shown(&self, kind: Kind, plugin: &str, id: &str) -> Option<Shown> {
        self.slots
            .iter()
            .find(|s| s.kind == kind && s.plugin == plugin && s.id == id)?
            .shown
            .clone()
    }

    /// A menu bar item's popover tree (Ok(None): the item has none).
    pub fn popover(&mut self, plugin: &str, id: &str) -> Option<Result<Option<Node>, String>> {
        let slot = self.slot(Kind::Menubar, plugin, id)?;
        let r = match &mut slot.script {
            Ok(s) => guarded(|| s.popover()),
            Err(m) => Err(m.clone()),
        };
        Some(self.outcome(plugin, r))
    }

    /// A button in the entry's tree: its handler, then a new render.
    pub fn click(&mut self, kind: Kind, plugin: &str, id: &str, button: u32) -> Option<Shown> {
        let slot = self.slot(kind, plugin, id)?;
        let r = match &mut slot.script {
            Ok(s) => guarded(|| s.click(button)),
            Err(m) => Err(m.clone()),
        };
        match self.outcome(plugin, r) {
            Ok(()) => self.render(kind, plugin, id),
            Err(m) => {
                if let Some(s) = self.slot(kind, plugin, id) {
                    s.shown = Some(Err(m.clone()));
                }
                Some(Err(m))
            }
        }
    }

    /// Seconds between renders of the entry (0: only after a click).
    pub fn refresh(&self, kind: Kind, plugin: &str, id: &str) -> Option<u32> {
        let s = self
            .slots
            .iter()
            .find(|s| s.kind == kind && s.plugin == plugin && s.id == id)?;
        Some(s.script.as_ref().map_or(0, Script::refresh))
    }

    /// Toasts to show (a plugin turned off, `plugins.json` not writable); taken once.
    pub fn take_toasts(&mut self) -> Vec<String> {
        std::mem::take(&mut self.toasts)
    }

    pub fn runner(&self) -> &Runner {
        &self.runner
    }

    /// The bar's three slots: the layout's items, plugin items of plugins that are on and
    /// loaded, then the `[[menubar]]` entries the layout does not place, by their `side` (left:
    /// end of the left slot; centre: end of the centre; right: before the system items, in
    /// plugin order, as macOS status items sit left of the clock).
    pub fn bar(&self, layout: &Layout) -> [Vec<BarItem>; 3] {
        let live = |p: &str, id: &str| self.has(Kind::Menubar, p, id);
        let mut slots = [
            layout.left.clone(),
            layout.centre.clone(),
            layout.right.clone(),
        ];
        for s in &mut slots {
            s.retain(|i| match i {
                BarItem::Plugin { plugin, id } => live(plugin, id),
                _ => true,
            });
        }
        let placed: Vec<&BarItem> = layout.bar_items().collect();
        let snap = self.snap.borrow();
        let mut right = 0;
        for (_, m) in snap.on() {
            for e in &m.menubar {
                let item = BarItem::Plugin {
                    plugin: m.id.clone(),
                    id: e.id.clone(),
                };
                if !live(&m.id, &e.id) || placed.contains(&&item) {
                    continue;
                }
                match e.side {
                    Side::Left => slots[0].push(item),
                    Side::Centre => slots[1].push(item),
                    Side::Right => {
                        slots[2].insert(right, item);
                        right += 1;
                    }
                }
            }
        }
        slots
    }

    /// The Dock's items: the layout's, plugin entries of plugins that are on, then the
    /// `[[dock]]` entries the layout does not place, after the apps (before the first separator).
    pub fn dock(&self, layout: &Layout) -> Vec<DockItem> {
        let tiles = self.dock_tiles();
        let live = |p: &str, id: &str| tiles.iter().any(|t| t.plugin == p && t.id == id);
        let mut items: Vec<DockItem> = layout
            .dock_items
            .iter()
            .filter(|i| match i {
                DockItem::Plugin { plugin, id } => live(plugin, id),
                _ => true,
            })
            .cloned()
            .collect();
        let mut at = items
            .iter()
            .position(|i| *i == DockItem::Separator)
            .unwrap_or(items.len());
        for t in &tiles {
            let item = DockItem::Plugin {
                plugin: t.plugin.clone(),
                id: t.id.clone(),
            };
            if !layout.dock_items.contains(&item) {
                items.insert(at, item);
                at += 1;
            }
        }
        items
    }

    /// Every `[[dock]]` entry of the plugins that are on (and may run its command, audit C8-1).
    pub fn dock_tiles(&self) -> Vec<DockTile> {
        let snap = self.snap.borrow();
        snap.on()
            .flat_map(|(p, m)| {
                m.dock
                    .iter()
                    .filter(|e| snap.may_run(&m.id, &e.exec))
                    .map(|e| DockTile {
                        plugin: m.id.clone(),
                        id: e.id.clone(),
                        name: e.name.clone(),
                        icon: icon_in(&p.dir, &e.icon),
                        exec: e.exec.clone(),
                    })
            })
            .collect()
    }

    /// Every `[[widget]]` card of the plugins that are on (the desktop gallery).
    pub fn cards(&self) -> Vec<Card> {
        let snap = self.snap.borrow();
        snap.on()
            .flat_map(|(_, m)| {
                m.widget.iter().map(|e| Card {
                    key: widget_key(&m.id, &e.id),
                    plugin: m.id.clone(),
                    id: e.id.clone(),
                    name: m.name.clone(),
                    sizes: e.sizes.clone(),
                })
            })
            .collect()
    }

    /// The (plugin, entry) of a desktop widget id from [`widget_key`].
    pub fn card(&self, key: &str) -> Option<(String, String)> {
        self.cards()
            .into_iter()
            .find(|c| c.key == key)
            .map(|c| (c.plugin, c.id))
    }
}

/// Runs a script call, a panic inside it (a bug in the host or mlua) becoming a sentence for
/// that item only.
fn guarded<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .unwrap_or_else(|_| Err("the plugin host stopped this script".into()))
}

/// An icon field: a file inside the plugin folder (absolute; SVGs drawn by name without the
/// extension, as `kit::icon_file` looks them up), else an icon theme name.
fn icon_in(dir: &Path, icon: &str) -> String {
    if is_icon_file(icon) {
        let p = dir.join(icon);
        let s = p.to_string_lossy();
        s.strip_suffix(".svg")
            .map_or_else(|| s.to_string(), str::to_owned)
    } else {
        icon.to_owned()
    }
}

/// Desktop widget id (`lua:<key>`) of card `id` of `plugin`: `plugin-<plugin with dashes>-<id>`
/// (a widget id is lowercase letters, digits and dashes, at most 64), hashed when longer.
pub fn widget_key(plugin: &str, id: &str) -> String {
    let k = format!(
        "{WIDGET_PREFIX}{}-{}",
        plugin.replace('.', "-"),
        id.replace('_', "-")
    );
    if k.len() <= 64 {
        return k;
    }
    // FNV-1a: stable across runs and versions.
    let h = format!("{plugin}/{id}")
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
        });
    format!("{WIDGET_PREFIX}{h:016x}")
}

// ---- Control Center tiles (W2-B) ----

/// One `[[tile]]` of a plugin that is on, with its toggle's state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tile {
    pub plugin: String,
    pub id: String,
    pub label: String,
    /// Icon name, or a file inside the plugin folder.
    pub icon: String,
    pub toggle: String,
    pub on: bool,
}

/// The tiles of `s`, their state from the toggle flag files under `state_home`.
pub fn tiles_of(s: &Snapshot, state_home: &Path) -> Vec<Tile> {
    s.on()
        .flat_map(|(p, m)| {
            m.tile.iter().map(|t| Tile {
                plugin: m.id.clone(),
                id: t.id.clone(),
                label: t.label.clone(),
                icon: icon_in(&p.dir, &t.icon),
                toggle: t.toggle.clone(),
                on: cp::toggle_get(state_home, &m.id, &t.toggle),
            })
        })
        .collect()
}

/// Control Center: every plugin tile now. Reads the disk: call it off the GTK thread.
pub fn tiles(paths: &Paths) -> Vec<Tile> {
    tiles_of(&read_snapshot(paths, &[]), &paths.state_home)
}

/// Control Center: tile `tile` of `plugin` switched `on`. Runs the toggle's command (limited,
/// as the user) and sets its flag; blocks for up to the command's timeout, so call it off the
/// GTK thread.
pub fn set_tile(paths: &Paths, plugin: &str, tile: &str, on: bool) -> Result<(), String> {
    let s = read_snapshot(paths, &[]);
    let toggle = s
        .manifest(plugin)
        .and_then(|(_, m)| m.tile.iter().find(|t| t.id == tile))
        .map(|t| t.toggle.clone())
        .ok_or_else(|| format!("{plugin}: no tile {tile}"))?;
    let job = toggle_job(paths, &s, plugin, &toggle, on)?;
    job().map(|_| ())
}

/// Control Center's [`TileSource`](crate::ui::control_center::tiles::TileSource) over the plugin
/// `[[tile]]`s (ADR 0031, audit5 S7-3): answers from a cache that [`TileCache::refresh`] fills
/// off the GTK thread; a click runs [`set_tile`] off the GTK thread, then reads the tiles again,
/// so a failed toggle shows its real state.
pub struct TileCache {
    paths: Paths,
    cache: RefCell<Vec<Tile>>,
    changed: RefCell<Option<Rc<dyn Fn()>>>,
    me: std::rc::Weak<TileCache>,
    /// The host whose folder watch calls [`TileCache::refresh`]; kept alive here.
    host: RefCell<Option<Rc<RefCell<Host>>>>,
}

impl TileCache {
    pub fn new(paths: Paths) -> Rc<TileCache> {
        Rc::new_cyclic(|me| TileCache {
            paths,
            cache: RefCell::default(),
            changed: RefCell::default(),
            me: me.clone(),
            host: RefCell::default(),
        })
    }

    /// `changed` runs when a refresh found other tiles (the Control Center redraws).
    pub fn on_changed(&self, changed: Rc<dyn Fn()>) {
        self.changed.replace(Some(changed));
    }

    /// Takes `tiles` as the cached list; runs `changed` when it differs.
    pub fn fill(&self, tiles: Vec<Tile>) {
        if *self.cache.borrow() == tiles {
            return;
        }
        self.cache.replace(tiles);
        if let Some(f) = self.changed.borrow().clone() {
            f();
        }
    }

    /// Reads the tiles again off the GTK thread.
    pub fn refresh(&self) {
        let (paths, me) = (self.paths.clone(), self.me.clone());
        glib::spawn_future_local(async move {
            let Ok(t) = gio::spawn_blocking(move || tiles(&paths)).await else {
                return;
            };
            if let Some(c) = me.upgrade() {
                c.fill(t);
            }
        });
    }

    /// The plugin and tile ids behind the Control Center id `<plugin>.<tile>`.
    fn find(&self, id: &str) -> Option<(String, String)> {
        self.cache
            .borrow()
            .iter()
            .find(|t| cc_tile_id(t) == id)
            .map(|t| (t.plugin.clone(), t.id.clone()))
    }

    /// Watches the plugin folders for as long as `life`: a change refreshes the tiles.
    pub fn watch(&self, life: &Lifetime) {
        let host = Rc::new(RefCell::new(Host::new(self.paths.clone(), &[])));
        let me = self.me.clone();
        watch(
            &host,
            life,
            Rc::new(move || {
                if let Some(c) = me.upgrade() {
                    c.refresh();
                }
            }),
        );
        self.host.replace(Some(host));
    }
}

fn cc_tile_id(t: &Tile) -> String {
    format!("{}.{}", t.plugin, t.id)
}

impl crate::ui::control_center::tiles::TileSource for TileCache {
    fn tiles(&self) -> Vec<crate::ui::control_center::tiles::Tile> {
        self.cache
            .borrow()
            .iter()
            .map(|t| crate::ui::control_center::tiles::Tile {
                id: cc_tile_id(t),
                icon: t.icon.clone(),
                label: t.label.clone(),
                on: t.on,
            })
            .collect()
    }

    fn toggle(&self, id: &str, on: bool) {
        let Some((plugin, tile)) = self.find(id) else {
            return;
        };
        let (paths, me) = (self.paths.clone(), self.me.clone());
        glib::spawn_future_local(async move {
            let p = paths.clone();
            let _ = gio::spawn_blocking(move || set_tile(&p, &plugin, &tile, on)).await;
            if let Some(c) = me.upgrade() {
                c.refresh();
            }
        });
    }
}

// ---- Hotkeys ----

/// One `[[hotkey]]` of a plugin that is on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub plugin: String,
    pub id: String,
    /// Heading in Settings > Keyboard (the manifest's `group`, else the plugin's name).
    pub group: String,
    pub combo: Combo,
    pub exec: Vec<String>,
}

impl Hotkey {
    /// Settings > Keyboard id, the key of a rebinding in `shortcuts.json`.
    pub fn shortcut_id(&self) -> String {
        format!("{SHORTCUT_PREFIX}{}/{}", self.plugin, self.id)
    }
}

pub fn hotkeys_of(s: &Snapshot) -> Vec<Hotkey> {
    s.on()
        .flat_map(|(_, m)| {
            m.hotkey.iter().filter_map(|h| {
                if !s.may_run(&m.id, &h.exec) {
                    return None;
                }
                let (mods, key) = h.combo();
                Some(Hotkey {
                    plugin: m.id.clone(),
                    id: h.id.clone(),
                    group: if h.group.is_empty() {
                        m.name.clone()
                    } else {
                        h.group.clone()
                    },
                    combo: Combo::parse(&mods.join(" "), key)?,
                    exec: h.exec.clone(),
                })
            })
        })
        .collect()
}

/// Every plugin hotkey now. Reads the disk: call it off the GTK thread.
pub fn hotkeys(paths: &Paths) -> Vec<Hotkey> {
    hotkeys_of(&read_snapshot(paths, &[]))
}

/// One shell word for the compositor's `exec` (it runs `sh -c`): quoted unless plainly safe.
fn sh_word(s: &str) -> String {
    let plain = !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./=:+@%,".contains(&b));
    if plain {
        s.to_owned()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// Settings > Keyboard rows of plugin hotkeys (group Custom, rebindable; `changed` in
/// `shortcuts.json` under [`Hotkey::shortcut_id`] moves or turns one off).
pub fn shortcuts(list: &[Hotkey], o: &Overrides) -> Vec<Shortcut> {
    list.iter()
        .map(|h| {
            let id = h.shortcut_id();
            let mut cmd: Vec<String> = Vec::new();
            if h.exec.first().is_some_and(|a| a == "wol") {
                cmd.extend(["env".to_owned(), format!("WOL_HELPER={}", h.plugin)]);
            }
            cmd.extend(h.exec.iter().cloned());
            Shortcut {
                combo: match o.changed.get(&id) {
                    Some(c) => c.clone(),
                    None => Some(h.combo.clone()),
                },
                id,
                name: h.group.clone(),
                group: Group::Custom,
                default: Some(h.combo.clone()),
                bind: Bind {
                    flags: String::new(),
                    combo: h.combo.clone(),
                    dispatcher: "exec".into(),
                    args: cmd.iter().map(|a| sh_word(a)).collect::<Vec<_>>().join(" "),
                },
                editable: true,
            }
        })
        .collect()
}

/// The binds to register: each plugin hotkey on its combo, unless a shipped or user shortcut
/// (`taken`) or an earlier plugin already has that combo (the system wins, then plugin order).
pub fn binds(list: &[Hotkey], o: &Overrides, taken: &[Shortcut]) -> Vec<(Combo, Bind)> {
    let mut out: Vec<(Combo, Bind)> = Vec::new();
    for s in shortcuts(list, o) {
        let Some(k) = s.combo.clone() else { continue };
        let busy =
            shortcuts::conflict(taken, &k, &s.id).is_some() || out.iter().any(|(c, _)| c.same(&k));
        if !busy {
            out.push((k, s.bind));
        }
    }
    out
}

/// What registering plugin hotkeys needs from disk: the binds, by the rules of [`binds`].
fn read_binds(paths: &Paths) -> Vec<(Combo, Bind)> {
    let read = |p: &str| std::fs::read_to_string(p).ok();
    let o = Overrides::parse(
        std::fs::read_to_string(paths.config_home.join(shortcuts::PREFS_FILE))
            .ok()
            .as_deref(),
    );
    let taken = shortcuts::effective(&shortcuts::load_defaults(read), &o);
    binds(&hotkeys(paths), &o, &taken)
}

type Registered = Rc<RefCell<Vec<(Combo, Bind)>>>;

thread_local! {
    /// The paths and binds of the last [`register_hotkeys`], for [`replay_hotkeys`].
    static HOTKEYS: RefCell<Option<(Paths, Registered)>> = const { RefCell::new(None) };
}

/// Registers the plugin hotkeys with the compositor (`hyprctl keyword bind`, off the GTK thread),
/// moving from the binds registered before (`last`). Called on every load.
pub fn register_hotkeys(paths: &Paths, last: &Registered) {
    HOTKEYS.with_borrow_mut(|h| *h = Some((paths.clone(), last.clone())));
    let (paths, last) = (paths.clone(), last.clone());
    glib::spawn_future_local(async move {
        let old = last.borrow().clone();
        let new = gio::spawn_blocking(move || read_binds(&paths))
            .await
            .unwrap_or_default();
        let cmds = shortcuts::live(&old, &new);
        last.replace(new);
        if !cmds.is_empty() {
            let _ = gio::spawn_blocking(move || {
                for c in cmds {
                    let _ = crate::hypr::request(&c);
                }
            })
            .await;
        }
    });
}

/// What to send after the compositor reloaded its config: every bind in `last`, as if none were
/// registered (a reload drops all `keyword` binds; audit5 S9-4).
pub fn replay_commands(last: &[(Combo, Bind)]) -> Vec<String> {
    shortcuts::live(&[], last)
}

/// The compositor reloaded its config (`configreloaded`): the plugin hotkeys are gone there, so the
/// next [`register_hotkeys`] starts from nothing and sends them all again.
pub fn replay_hotkeys() {
    let Some((paths, last)) = HOTKEYS.with_borrow(|h| h.clone()) else {
        return;
    };
    last.borrow_mut().clear();
    register_hotkeys(&paths, &last);
}

// ---- GTK: loading, watching, drawing ----

/// Reads the plugins off the GTK thread and applies them; `changed` runs when the host took a
/// new read (items to rebuild). Toasts the host left are posted as notifications.
pub fn load(host: &Rc<RefCell<Host>>, changed: Rc<dyn Fn()>) {
    let (paths, kinds) = {
        let h = host.borrow();
        (h.paths.clone(), h.kinds)
    };
    let host = Rc::downgrade(host);
    glib::spawn_future_local(async move {
        let Ok(snap) = gio::spawn_blocking(move || read_snapshot(&paths, kinds)).await else {
            return;
        };
        let Some(h) = host.upgrade() else { return };
        let fresh = h.borrow_mut().apply(snap);
        if fresh {
            changed();
        }
        toast(&h);
    });
}

/// Posts the host's toasts (a plugin turned off) as notifications, off the GTK thread.
pub fn toast(host: &Rc<RefCell<Host>>) {
    let t = host.borrow_mut().take_toasts();
    if t.is_empty() {
        return;
    }
    let title = TOAST_TITLE.to_owned();
    std::thread::spawn(move || {
        for body in t {
            let _ = wol_lua::Live.notify(&title, &body);
        }
    });
}

/// A shared redraw or re-arm callback.
type Callback = Rc<dyn Fn()>;

/// Watches the user plugin folder, each plugin folder in it and the folder of `plugins.json`;
/// a change (after [`SETTLE`]) reloads `host` and runs `changed`. Watching ends with `life`.
pub fn watch(host: &Rc<RefCell<Host>>, life: &Lifetime, changed: Rc<dyn Fn()>) {
    let paths = host.borrow().paths.clone();
    for d in paths.watched() {
        let _ = std::fs::create_dir_all(&d);
    }
    let monitors: Rc<RefCell<Vec<gio::FileMonitor>>> = Rc::default();
    let pending = Rc::new(Cell::new(false));
    let rewatch: Rc<RefCell<Option<Callback>>> = Rc::default();
    let on_change: Rc<dyn Fn()> = {
        let (host, pending, rewatch, changed) = (
            Rc::downgrade(host),
            pending.clone(),
            Rc::downgrade(&rewatch),
            changed.clone(),
        );
        Rc::new(move || {
            if pending.replace(true) {
                return;
            }
            let (host, pending, rewatch, changed) = (
                host.clone(),
                pending.clone(),
                rewatch.clone(),
                changed.clone(),
            );
            glib::timeout_add_local_once(SETTLE, move || {
                pending.set(false);
                let Some(h) = host.upgrade() else { return };
                let again = rewatch.upgrade().and_then(|r| r.borrow().clone());
                load(
                    &h,
                    Rc::new(move || {
                        changed();
                        if let Some(r) = &again {
                            r();
                        }
                    }),
                );
            });
        })
    };
    // (Re)watches the folders: plugin folders come and go with installs.
    let arm: Rc<dyn Fn()> = {
        let (monitors, on_change, paths) = (monitors.clone(), on_change.clone(), paths.clone());
        let host = Rc::downgrade(host);
        Rc::new(move || {
            let mut dirs = paths.watched();
            if let Some(h) = host.upgrade() {
                let snap = h.borrow().snap.borrow().clone();
                dirs.extend(
                    snap.plugins
                        .iter()
                        .filter(|p| !p.system)
                        .map(|p| p.dir.clone()),
                );
            }
            let mut ms = monitors.borrow_mut();
            for m in ms.drain(..) {
                m.cancel();
            }
            for d in dirs {
                if let Ok(m) = gio::File::for_path(&d)
                    .monitor_directory(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
                {
                    let f = on_change.clone();
                    m.connect_changed(move |_, _, _, _| f());
                    ms.push(m);
                }
            }
        })
    };
    rewatch.replace(Some(arm.clone()));
    arm();
    {
        let (monitors, rewatch) = (monitors.clone(), rewatch.clone());
        life.on_end(move || {
            rewatch.take();
            for m in monitors.borrow_mut().drain(..) {
                m.cancel();
            }
        });
    }
    load(
        host,
        Rc::new(move || {
            changed();
            arm();
        }),
    );
}

/// While commands of `host` run, looks for their answers every [`ANSWER_POLL`] and runs
/// `redraw` when one came back (the script renders again with it). Stops when none are left.
pub fn await_answers(host: &Rc<RefCell<Host>>, redraw: Rc<dyn Fn()>) {
    let runner = host.borrow().runner.clone();
    if !runner.busy() {
        return;
    }
    let weak = Rc::downgrade(host);
    glib::timeout_add_local(ANSWER_POLL, move || {
        if weak.upgrade().is_none() {
            return glib::ControlFlow::Break;
        }
        if runner.take_fresh() {
            redraw();
        }
        if runner.busy() {
            glib::ControlFlow::Continue
        } else {
            glib::ControlFlow::Break
        }
    });
}

/// A menu bar item of `plugin`'s entry `id`: its tree in a bar button; a click opens its
/// popover when it has one. Renders now, then every `refresh` seconds while `life` lasts and
/// the host has not reloaded. A failure shows the plugin's name with the sentence as tooltip.
pub fn bar_item(host: &Rc<RefCell<Host>>, plugin: &str, id: &str, life: &Lifetime) -> gtk::Button {
    let holder = w::hbox(0);
    let button = w::menubar_item(&holder);
    let (plugin, id) = (plugin.to_owned(), id.to_owned());
    let generation = host.borrow().generation;
    let draw: Rc<dyn Fn()> = {
        let (host, holder, button, plugin, id) = (
            Rc::downgrade(host),
            holder.clone(),
            button.downgrade(),
            plugin.clone(),
            id.clone(),
        );
        Rc::new(move || {
            let (Some(h), Some(button)) = (host.upgrade(), button.upgrade()) else {
                return;
            };
            let r = h.borrow_mut().render(Kind::Menubar, &plugin, &id);
            let name = h.borrow().name(&plugin);
            fill(&holder, &button, &name, r);
            toast(&h);
        })
    };
    draw();
    {
        let d = draw.clone();
        await_answers(host, Rc::new(move || d()));
    }
    let every = host
        .borrow()
        .refresh(Kind::Menubar, &plugin, &id)
        .unwrap_or(0);
    if every > 0 {
        let (host, draw) = (Rc::downgrade(host), draw.clone());
        let id = glib::timeout_add_seconds_local(every, move || {
            let Some(h) = host.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if h.borrow().generation != generation {
                return glib::ControlFlow::Break;
            }
            draw();
            let d = draw.clone();
            await_answers(&h, Rc::new(move || d()));
            glib::ControlFlow::Continue
        });
        life.own(id);
    }
    let host = Rc::downgrade(host);
    button.connect_clicked(move |b| {
        let Some(h) = host.upgrade() else { return };
        let r = h.borrow_mut().popover(&plugin, &id);
        toast(&h);
        let node = match r {
            Some(Ok(Some(n))) => n,
            Some(Ok(None)) | None => return,
            Some(Err(m)) => Node::Label {
                text: m,
                role: shared::view::Role::Secondary,
            },
        };
        popover(&h, b, &plugin, &id, &node);
    });
    button
}

/// Shows a render: the tree, or the plugin's name with the reason as its tooltip.
fn fill(holder: &gtk::Box, button: &gtk::Button, name: &str, r: Option<Shown>) {
    while let Some(c) = holder.first_child() {
        holder.remove(&c);
    }
    match r {
        Some(Ok(node)) => {
            let view = View::new(&node, Sink::new(|_| {}));
            view.widget().set_can_target(false);
            holder.append(view.widget());
            button.set_tooltip_text(Some(name));
        }
        Some(Err(m)) => {
            holder.append(&w::label(name, "secondary"));
            button.set_tooltip_text(Some(&format!("{name}: {m}")));
        }
        None => button.set_visible(false),
    }
}

/// The item's popover under `at`; its buttons run the script's handlers.
fn popover(host: &Rc<RefCell<Host>>, at: &gtk::Button, plugin: &str, id: &str, node: &Node) {
    let pop = gtk::Popover::new();
    pop.set_parent(at);
    pop.add_css_class("menu");
    let (h, p, i) = (Rc::downgrade(host), plugin.to_owned(), id.to_owned());
    let close = pop.downgrade();
    let view = View::new(
        node,
        Sink::new(move |a| {
            if let (Act::Clicked(b), Some(h)) = (a, h.upgrade()) {
                h.borrow_mut().click(Kind::Menubar, &p, &i, b);
                toast(&h);
                if let Some(pop) = close.upgrade() {
                    pop.popdown();
                }
            }
        }),
    );
    let body = view.widget().clone();
    body.set_margin_top(w::S2);
    body.set_margin_bottom(w::S2);
    body.set_margin_start(w::S2);
    body.set_margin_end(w::S2);
    pop.set_child(Some(&body));
    pop.connect_closed(|p| p.unparent());
    pop.popup();
}

/// A Dock tile of a plugin entry: launches its `exec` as the user.
pub fn dock_tile(t: &DockTile) -> w::DockTile {
    let argv = t.exec.clone();
    w::DockTile::new(&t.icon, &t.name, move || kit::spawn(&argv))
}

#[cfg(test)]
mod tests;
