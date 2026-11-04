// SPDX-License-Identifier: GPL-2.0-or-later
//! System Settings on the HIG AppWindow: a searchable sidebar of panes (System Settings order) and
//! grouped Section cards. Search covers every pane and the rows on it (`search`): the sidebar keeps
//! the panes with a hit and the content shows the hits, row by row, until the field is cleared. Every control applies for real or is not shown: theme contract
//! (Appearance), desktop prefs applied live through one `hyprctl` batch and journaled (Desktop &
//! Dock, Effects & Looks, Keyboard, Trackpad & Mouse, Accessibility), shell prefs + `reload`,
//! `System` actions and wol-admind for root changes (the Authenticate sheet asks first). Machine
//! facts load off the UI thread ([`Machine`]); panes are built the first time they are shown.
use crate::kit::{self, form, widgets as w};
use crate::system::{Action, ActionError, System};
use gtk::{gio, glib, prelude::*};
use shared::theme::{Color, Variant};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use wol_core::prefs::{self, ShellPrefs};
use wol_core::theme::accent;

mod about;
mod accessibility;
mod accounts;
mod appearance;
mod assistants;
mod battery;
mod bluetooth;
mod control_center;
mod datetime;
mod desktop_dock;
mod displays;
mod effects;
mod general;
mod hardware;
mod keyboard;
mod language;
mod lock;
mod login_items;
mod machine;
mod malware;
mod memory_saver;
mod modules;
mod network;
mod notifications;
mod printers;
mod privacy;
mod restore;
mod search;
mod sharing;
mod shortcuts;
mod software_update;
mod sound;
mod startup_security;
mod storage;
mod trackpad;
mod wallpaper;
mod widgets;
mod wifi;

pub use accounts::{account_name, password_problem};
pub use machine::{Demo, DemoEdge, Kernel, Live, Machine, Sharing};
pub use malware::{full_section as malware_full_section, section as malware_section};
pub use search::{Hit, hits};
pub use shortcuts::KEY_LEGEND;
pub use software_update::{UpdateState, parse_check, with_flatpaks};
pub use sound::{
    Extras as SoundExtras, Request as SoundRequest, SoundState, Source as SoundSource, Touch,
    apply_scale_plan, live as sound_live, scale_plan, system_percent,
};

/// One sidebar pane: its title (also the `wol-settings <title>` name), icon and search words.
pub struct Pane {
    pub title: &'static str,
    pub icon: &'static str,
    pub words: &'static str,
}

const fn pane(title: &'static str, icon: &'static str, words: &'static str) -> Pane {
    Pane { title, icon, words }
}

/// The sidebar, top to bottom; groups are separated by a gap.
pub const SIDEBAR: [&[Pane]; 6] = [
    &[
        pane(
            "Wi-Fi",
            "network-wireless-symbolic",
            "Wi-Fi wireless network internet join password hotspot",
        ),
        pane(
            "Bluetooth",
            "bluetooth-active-symbolic",
            "devices headphones airpods keyboard mouse pair connect",
        ),
        pane(
            "Network",
            "network-wired-symbolic",
            "ethernet ip address router dns gateway mac",
        ),
    ],
    &[
        pane(
            "Notifications",
            "preferences-system-notifications-symbolic",
            "focus do not disturb banners alerts quiet",
        ),
        pane(
            "Sound",
            "audio-volume-high-symbolic",
            "volume speakers headphones output input microphone mic mute audio balance codec latency",
        ),
    ],
    &[
        pane(
            "General",
            "emblem-system-symbolic",
            concat!(
                "about version software update storage disk date time zone clock language region locale ",
                "login items startup sharing computer name remote ssh terminal shell bash fish nu pwsh"
            ),
        ),
        pane(
            "Appearance",
            "preferences-desktop-appearance-symbolic",
            "theme dark light auto accent colour color palette",
        ),
        pane(
            "Accessibility",
            "preferences-desktop-accessibility-symbolic",
            "reduce motion transparency contrast zoom cursor size",
        ),
        pane(
            "Control Center",
            "wolos-control-center-symbolic",
            "modules now playing battery keyboard brightness screenshot record",
        ),
        pane(
            "Desktop & Dock",
            "wolos-dock-symbolic",
            "dock menu bar magnification hide tiling tile windows gaps layout",
        ),
        pane(
            "Displays",
            "video-display-symbolic",
            "resolution scale monitor night shift brightness",
        ),
        pane(
            "Wallpaper",
            "preferences-desktop-wallpaper-symbolic",
            "background picture desktop dynamic photo",
        ),
        pane(
            "Effects & Looks",
            "wolos-effects-symbolic",
            "blur transparency opacity glass corners rounding radius borders gradient shadows animations speed gaps dim presets rice",
        ),
        pane(
            "Widgets",
            "wolos-widgets-symbolic",
            "desktop widgets clock weather calendar arrange",
        ),
        pane(
            "Battery",
            "battery-full-symbolic",
            "energy power low power mode health lid sleep memory saver ram app nap",
        ),
    ],
    &[
        pane(
            "Lock Screen",
            "system-lock-screen-symbolic",
            "lock idle screen saver sleep password",
        ),
        pane(
            "Privacy & Security",
            "security-high-symbolic",
            "permissions screen recording firewall consent privacy secure boot startup security tpm",
        ),
        pane(
            "Users & Groups",
            "system-users-symbolic",
            "accounts users administrator password shell zsh",
        ),
    ],
    &[
        pane(
            "Keyboard",
            "input-keyboard-symbolic",
            "key repeat input sources layout shortcuts",
        ),
        pane(
            "Trackpad & Mouse",
            "input-touchpad-symbolic",
            "pointer speed scroll natural tap click touchpad",
        ),
        pane(
            "Printers",
            "printer-symbolic",
            "print printer scanner default",
        ),
        pane(
            "Hardware",
            "wolos-chip-symbolic",
            "drivers firmware devices webcam graphics fix",
        ),
    ],
    &[
        pane(
            "Assistants & Automation",
            "wolos-assistant-symbolic",
            "ai helpers mcp scripts automation permissions",
        ),
        pane(
            "Modules",
            "view-app-grid-symbolic",
            "optional add-ons developer tools ai claude creative console install",
        ),
        pane(
            "Restore",
            "wolos-restore-symbolic",
            "undo history restore points boot environments",
        ),
    ],
];

/// Every sidebar pane title, in order (same as [`SIDEBAR`]).
pub const PAGES: [&str; 25] = [
    "Wi-Fi",
    "Bluetooth",
    "Network",
    "Notifications",
    "Sound",
    "General",
    "Appearance",
    "Accessibility",
    "Control Center",
    "Desktop & Dock",
    "Displays",
    "Wallpaper",
    "Effects & Looks",
    "Widgets",
    "Battery",
    "Lock Screen",
    "Privacy & Security",
    "Users & Groups",
    "Keyboard",
    "Trackpad & Mouse",
    "Printers",
    "Hardware",
    "Assistants & Automation",
    "Modules",
    "Restore",
];

/// Pages without a sidebar row, reached from a pane (`wol-settings "Storage"` works too).
pub const SUBPAGES: [&str; 9] = [
    shortcuts::TITLE,
    startup_security::TITLE,
    "About",
    "Software Update",
    "Storage",
    "Date & Time",
    "Language & Region",
    "Login Items",
    "Sharing",
];

/// Older page names (links, scripts) -> today's.
const ALIASES: [(&str, &str); 6] = [
    ("Menu Bar & Dock", "Desktop & Dock"),
    ("Lock & Idle", "Lock Screen"),
    ("Accounts", "Users & Groups"),
    ("Energy", "Battery"),
    ("Effects", "Effects & Looks"),
    ("Shortcuts", shortcuts::TITLE),
];

/// Today's name for `name` (case-insensitive aliases).
pub fn canonical(name: &str) -> &str {
    ALIASES
        .iter()
        .find(|a| a.0.eq_ignore_ascii_case(name))
        .map_or(name, |a| a.1)
}

/// Sidebar panes with a hit for every word of `query` (a pane's title or words, a row on it or on
/// one of its subpages); every pane for an empty query.
pub fn search(query: &str) -> Vec<&'static str> {
    if query.trim().is_empty() {
        return PAGES.to_vec();
    }
    search::panes(query)
}

/// The page `wol-settings [page]` asks for, under today's name; None for the window as it was.
pub fn requested_page(args: &[String]) -> Option<&str> {
    args.get(1).map(|p| canonical(p))
}

thread_local! {
    /// The one Settings window's content, once built.
    static WINDOW: RefCell<Option<Rc<w::AppWindow>>> = const { RefCell::new(None) };
    /// A page asked for while the window was still being built.
    static PENDING: RefCell<Option<String>> = const { RefCell::new(None) };
    static BUILDING: Cell<bool> = const { Cell::new(false) };
}

/// `wol-settings [page]`. One window: a launch while Settings runs reaches the running instance
/// with its own arguments (`HANDLES_COMMAND_LINE`), which presents the window on that page (live:
/// the Wi-Fi tile's "Wi-Fi Settings…" opened the pane of the first launch, or a second window).
pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("org.wolos.settings")
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    app.connect_command_line(|app, cmdline| {
        let args: Vec<String> = cmdline
            .arguments()
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        present(app, requested_page(&args).map(str::to_owned));
        glib::ExitCode::SUCCESS
    });
    app.run()
}

/// Shows the Settings window on `page` (built on the first call; host facts read off the UI
/// thread, so a slow `hostname` never blanks the launch).
fn present(app: &gtk::Application, page: Option<String>) {
    if let Some(aw) = WINDOW.with_borrow(|w| w.clone()) {
        if let Some(p) = &page {
            aw.open(p);
        }
        if let Some(win) = app.active_window() {
            win.present();
        }
        return;
    }
    if page.is_some() {
        PENDING.set(page);
    }
    if BUILDING.replace(true) {
        return;
    }
    let hold = app.hold();
    let app = app.clone();
    glib::spawn_future_local(async move {
        let host = gio::spawn_blocking(Host::read)
            .await
            .unwrap_or_else(|_| Host::read());
        kit::style::install("");
        let win = gtk::ApplicationWindow::builder()
            .application(&app)
            .title("System Settings")
            .default_width(w::APP_SIZE.0)
            .default_height(w::APP_SIZE.1)
            .decorated(false)
            .build();
        let aw = Rc::new(content(crate::system::default_system(), &host));
        if let Some(p) = PENDING.take() {
            aw.open(&p);
        }
        win.set_child(Some(&aw.root));
        kit::snapshot_on_map(&win, "settings");
        win.present();
        WINDOW.set(Some(aw));
        BUILDING.set(false);
        drop(hold);
    });
}

/// Host facts shown by the panes (injected so golden renders do not depend on the build machine).
#[derive(Clone)]
pub struct Host {
    pub hostname: String,
    pub users: Vec<String>,
    pub os: String,
    /// "Linux 6.17.1-wolos" (`ostype` + `osrelease`): what the kernel reports.
    pub base: String,
    pub processor: String,
    pub memory: String,
    /// Slower facts (storage, hardware, network...), always read off the UI thread.
    pub machine: Arc<dyn Machine>,
    /// Shells installed for this person that new terminal windows can open in (General).
    pub terminal_shells: Vec<&'static str>,
}

impl Host {
    pub fn read() -> Self {
        let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
        let shells = std::fs::read_to_string("/etc/shells").unwrap_or_default();
        let info = wol_core::system::info::SystemInfo::read(&wol_core::system::PosixHost);
        Host {
            hostname: std::fs::read_to_string("/etc/hostname")
                .ok()
                .or_else(|| {
                    wol_core::system::run_with_timeout(&["hostname"], wol_core::system::RUN_TIMEOUT)
                })
                .unwrap_or_default()
                .trim()
                .to_string(),
            users: crate::login::users(&passwd, &shells),
            os: info.kernel,
            base: info.base,
            processor: info.processor,
            memory: info.memory,
            machine: Arc::new(Live::new()),
            terminal_shells: wol_core::terminal_shell::installed(&glib::home_dir(), |p| {
                use std::os::unix::fs::PermissionsExt;
                p.metadata()
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            }),
        }
    }

    /// The fixed laptop of golden renders and screenshots.
    pub fn demo() -> Self {
        Host {
            hostname: "wonderland".into(),
            users: vec!["alice".into(), "dinah".into()],
            os: "WolKernel".into(),
            base: "Linux 6.17.1-wolos".into(),
            processor: "Intel Core i5-1240P (12 cores)".into(),
            memory: "16 GB".into(),
            machine: Arc::new(Demo),
            // The base image's shells (the Console module adds the others).
            terminal_shells: wol_core::terminal_shell::ids().take(2).collect(),
        }
    }
}

/// `$XDG_CONFIG_HOME/<rel>`.
fn config(rel: &str) -> PathBuf {
    PathBuf::from(kit::config_home()).join(rel)
}

fn read(p: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(p).ok()
}

/// Settings file change recorded in the journal (`wol undo`, Settings > Restore); the window's
/// Undo toast hears of it with its entry id ([`changed`]).
fn save_journaled(what: &str, rel: &str, text: Option<&str>) -> std::io::Result<()> {
    save_entry(what, rel, text).map(drop)
}

/// [`save_journaled`], returning the journal entry's id (None = the file already had `text`, so
/// nothing was recorded and no toast offers to undo an older change).
fn save_entry(what: &str, rel: &str, text: Option<&str>) -> std::io::Result<Option<u64>> {
    let id = wol_core::journal::Journal::for_config(&PathBuf::from(kit::config_home())).save(
        what,
        "settings",
        &[(rel, text)],
        wol_core::journal::now(),
    )?;
    if let Some(id) = id {
        changed(what, id);
    }
    Ok(id)
}

/// Called with what changed (as in the journal) and the journal entry that recorded it. The
/// toast's Undo reverts that entry: reading the newest entry instead reverted an older change
/// when a save recorded nothing (QA 0.12.0 bug 9).
type ChangeHook = Rc<dyn Fn(&str, u64)>;

thread_local! {
    /// The Settings window's hook for journaled changes (its Undo toast); unset elsewhere.
    static CHANGED: RefCell<Option<ChangeHook>> = const { RefCell::new(None) };
}

/// Tells the window that the journaled change `id` (`what`, as in the journal) was just saved.
fn changed(what: &str, id: u64) {
    if let Some(f) = CHANGED.with(|c| c.borrow().clone()) {
        f(what, id)
    }
}

/// [`changed`] for a save made in `wol_core` (desktop prefs, shortcuts, sound fragments), whose
/// entry id does not come back: Settings' newest entry about `what`, if any.
fn changed_latest(what: &str) {
    if CHANGED.with(|c| c.borrow().is_none()) {
        return;
    }
    let j = wol_core::journal::Journal::for_config(&PathBuf::from(kit::config_home()));
    if let Some(id) = latest_entry(&j.entries(), what) {
        changed(what, id)
    }
}

/// The id of the newest entry Settings recorded about `what` (oldest-first `entries`), never an
/// undo's own entry.
fn latest_entry(entries: &[wol_core::journal::Entry], what: &str) -> Option<u64> {
    entries
        .iter()
        .rev()
        .find(|e| e.what == what && e.by == "settings" && e.undoes.is_none())
        .map(|e| e.id)
}

/// Runs argv lists in order on a worker thread (2 s timeout each); never blocks the UI.
fn background(cmds: Vec<Vec<String>>) {
    std::thread::spawn(move || {
        for c in cmds {
            let argv: Vec<&str> = c.iter().map(String::as_str).collect();
            let _ = wol_core::system::run_with_timeout(&argv, wol_core::system::RUN_TIMEOUT);
        }
    });
}

/// wol-admind request `name args…` (wire name, `shared::admin`), None when this build has no such
/// verb: the control that needs it is not shown.
fn admin(name: &str, args: &[&str]) -> Option<Action> {
    let action = shared::admin::AdminAction::parse(name)?;
    Some(Action::Admin(shared::admin::AdminRequest {
        action,
        args: argv(args),
    }))
}

fn argv(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// Fills `into` with `render(load())`. `load` runs off the UI thread; a spinner shows only when it
/// takes longer than 150 ms (HIG: never a blank pane, no flash for fast loads).
fn later<T: Send + 'static>(
    into: &gtk::Box,
    load: impl FnOnce() -> T + Send + 'static,
    render: impl FnOnce(&gtk::Box, T) + 'static,
) {
    let spinner = w::spinner();
    spinner.set_visible(false);
    into.append(&spinner);
    let done = Rc::new(Cell::new(false));
    {
        let (s, done) = (spinner.downgrade(), done.clone());
        glib::timeout_add_local_once(std::time::Duration::from_millis(150), move || {
            if let (false, Some(s)) = (done.get(), s.upgrade()) {
                s.set_visible(true);
            }
        });
    }
    let into = into.clone();
    glib::spawn_future_local(async move {
        let r = gio::spawn_blocking(load).await;
        done.set(true);
        into.remove(&spinner);
        if let Ok(v) = r {
            render(&into, v);
        }
    });
}

/// `then(load())`, `load` off the UI thread.
fn off_thread<T: Send + 'static>(
    load: impl FnOnce() -> T + Send + 'static,
    then: impl FnOnce(T) + 'static,
) {
    glib::spawn_future_local(async move {
        if let Ok(v) = gio::spawn_blocking(load).await {
            then(v)
        }
    });
}

fn clear(b: &gtk::Box) {
    while let Some(c) = b.first_child() {
        b.remove(&c);
    }
}

/// The control of type `T` inside a row (`form::slider_row`'s slider, `form::picker_row`'s menu),
/// to put it back after a failed save.
fn control_in<T: IsA<gtk::Widget>>(row: &gtk::Widget) -> Option<T> {
    if let Ok(t) = row.clone().downcast::<T>() {
        return Some(t);
    }
    let mut c = row.first_child();
    while let Some(w) = c {
        if let Some(t) = control_in(&w) {
            return Some(t);
        }
        c = w.next_sibling();
    }
    None
}

/// Completion callback for one action.
pub(crate) type Done = Rc<dyn Fn(Result<(), ActionError>)>;

/// A change waiting on the Authenticate sheet, with the status line that tells how it went.
pub(crate) struct Waiting {
    a: Action,
    done: Done,
    status: Option<gtk::Label>,
}

/// After this many wrong passwords in a row the sheet stops asking: every waiting change fails
/// and its control flips back (QA 0.12.0 bug 4: the sheet looped forever).
const MAX_REFUSALS: u32 = 3;
/// A change with no answer after this long says "Checking…" on its status line (QA 0.12.0 bug 7).
const CHECKING_AFTER: std::time::Duration = std::time::Duration::from_millis(300);
/// The sheet stays up (busy) at most this long after Allow: a wrong password answers within it,
/// a long change (an install) goes on under its status line.
const AUTH_HOLD: std::time::Duration = std::time::Duration::from_secs(2);

/// What the Authenticate sheet does with one answer.
pub(crate) enum Next {
    /// The change ended: its `done` gets this.
    Finish(Waiting, Result<(), ActionError>),
    /// It waits for the password: the sheet shows, with this sentence (may be empty).
    Ask(&'static str),
    /// Too many wrong passwords: every waiting change fails with `Denied`.
    GiveUp(Vec<Waiting>),
}

/// The Authenticate sheet's bookkeeping, GTK-free (QA 0.12.0 bugs 4, 6). Every change that
/// answered `NeedsAuth` waits here in order; one password re-runs all of them, each with its own
/// copy (a one-shot slot went to whichever ran first); Cancel fails all of them. A single slot
/// lost the first change and left its switch grey.
#[derive(Default)]
pub(crate) struct AuthQueue {
    waiting: std::collections::VecDeque<Waiting>,
    /// Wrong passwords in a row (one per Allow, however many changes it re-ran).
    refusals: u32,
    /// The Allow that was last counted as a wrong password.
    counted: u32,
    /// Allows so far: numbers the retries.
    attempt: u32,
    /// Retries sent with a typed password and not answered yet.
    checking: usize,
    /// The Allow whose wrong password ended the sheet: its later answers fail at once too.
    gave_up: u32,
}

impl AuthQueue {
    /// A change was sent; `retry` = the Allow it carries the password of.
    pub(crate) fn sent(&mut self, retry: Option<u32>) {
        if retry.is_some() {
            self.checking += 1;
        }
    }
    /// The answer `r` to `w` (sent with the password of Allow `retry`, if any).
    pub(crate) fn answered(
        &mut self,
        w: Waiting,
        r: Result<(), ActionError>,
        retry: Option<u32>,
    ) -> Next {
        if retry.is_some() {
            self.checking = self.checking.saturating_sub(1);
        }
        match r {
            Err(ActionError::NeedsAuth | ActionError::Denied)
                if retry.is_some() && retry == Some(self.gave_up) =>
            {
                Next::Finish(w, Err(ActionError::Denied))
            }
            Err(e @ (ActionError::NeedsAuth | ActionError::Denied)) => {
                if let Some(n) = retry
                    && n != self.counted
                {
                    self.counted = n;
                    self.refusals += 1;
                }
                self.waiting.push_back(w);
                if self.refusals >= MAX_REFUSALS {
                    self.refusals = 0;
                    self.gave_up = retry.unwrap_or_default();
                    return Next::GiveUp(self.waiting.drain(..).collect());
                }
                Next::Ask(if retry.is_some() || e == ActionError::Denied {
                    ActionError::Denied.message()
                } else {
                    ""
                })
            }
            r => {
                if retry.is_some() && r.is_ok() {
                    self.refusals = 0;
                }
                Next::Finish(w, r)
            }
        }
    }
    /// Allow: every waiting change, in order, and the number of this Allow.
    pub(crate) fn allow(&mut self) -> (Vec<Waiting>, u32) {
        self.attempt = self.attempt.wrapping_add(1).max(1);
        (self.waiting.drain(..).collect(), self.attempt)
    }
    /// Cancel: every waiting change (each fails with `Denied`).
    pub(crate) fn cancel(&mut self) -> Vec<Waiting> {
        self.refusals = 0;
        self.waiting.drain(..).collect()
    }
    pub(crate) fn len(&self) -> usize {
        self.waiting.len()
    }
    pub(crate) fn checking(&self) -> usize {
        self.checking
    }
}

/// Shared page state: the System, the Authenticate sheet and an inline status line per action.
pub(crate) struct Ctx {
    pub(crate) sys: Rc<dyn System>,
    auth: w::AuthSheet,
    queue: RefCell<AuthQueue>,
}

impl Ctx {
    /// Wires the Authenticate sheet; place it on `aw` after any other sheets.
    pub(crate) fn new(sys: Rc<dyn System>, who: &str) -> Rc<Self> {
        let ctx = Rc::new(Ctx {
            sys,
            auth: w::authenticate_sheet(who),
            queue: Default::default(),
        });
        let c = ctx.clone();
        ctx.auth.allow.connect_clicked(move |_| c.allow());
        let c = ctx.clone();
        ctx.auth.field.connect_activate(move |_| c.allow());
        let c = ctx.clone();
        ctx.auth.cancel.connect_clicked(move |_| c.cancel());
        ctx
    }
    pub(crate) fn place(&self, aw: &w::AppWindow) {
        w::toolbar_place(aw, &self.auth.revealer);
    }
    /// Says why the password is wanted; returns what the sheet said before.
    pub(crate) fn set_why(&self, why: &str) -> String {
        let was = self.auth.why.text().to_string();
        self.auth.why.set_label(why);
        was
    }
    /// `a` off the UI thread; root actions ask for the password, then retry; `done` gets the final result.
    pub(crate) fn act(self: &Rc<Self>, a: Action, done: Rc<dyn Fn(Result<(), ActionError>)>) {
        self.run(
            Waiting {
                a,
                done,
                status: None,
            },
            None,
        );
    }
    /// `a`, then its one-sentence error (if any) on `status`.
    pub(crate) fn act_say(self: &Rc<Self>, a: Action, status: &gtk::Label) {
        let st = status.clone();
        self.run(
            Waiting {
                a,
                done: Rc::new(move |r| say(&st, r.err().map_or("", |e| e.message()))),
                status: Some(status.clone()),
            },
            None,
        );
    }
    /// [`Self::act_say`] for a switch: `done(ok)` lets it settle or flip back.
    pub(crate) fn act_switch(
        self: &Rc<Self>,
        a: Action,
        status: &gtk::Label,
        done: Box<dyn FnOnce(bool)>,
    ) {
        let (st, done) = (status.clone(), std::cell::RefCell::new(Some(done)));
        self.run(
            Waiting {
                a,
                done: Rc::new(move |r| {
                    say(&st, r.as_ref().err().map_or("", |e| e.message()));
                    if let Some(d) = done.borrow_mut().take() {
                        d(r.is_ok())
                    }
                }),
                status: Some(status.clone()),
            },
            None,
        );
    }
    /// Sends `w` (with the password of Allow number `pw.1`, if any) and handles its answer.
    fn run(self: &Rc<Self>, w: Waiting, pw: Option<(Rc<str>, u32)>) {
        let rx = self
            .sys
            .spawn_with(w.a.clone(), pw.as_ref().map(|(p, _)| &**p));
        let retry = pw.map(|(_, n)| n);
        self.queue.borrow_mut().sent(retry);
        let answered = Rc::new(Cell::new(false));
        if let Some(st) = w.status.clone() {
            let answered = answered.clone();
            glib::timeout_add_local_once(CHECKING_AFTER, move || {
                if !answered.get() {
                    say(&st, crate::copy::CHECKING);
                }
            });
        }
        let c = self.clone();
        glib::spawn_future_local(async move {
            let Ok(r) = rx.recv().await else { return };
            answered.set(true);
            if let Some(st) = &w.status {
                if st.text() == crate::copy::CHECKING {
                    say(st, "");
                }
                // The sheet says it too; the status line keeps it once the sheet is gone.
                if retry.is_some() && matches!(r, Err(ActionError::Denied | ActionError::NeedsAuth))
                {
                    say(st, ActionError::Denied.message());
                }
            }
            let next = c.queue.borrow_mut().answered(w, r, retry);
            match next {
                Next::Finish(w, r) => (w.done)(r),
                Next::Ask(message) => c.ask(message),
                Next::GiveUp(all) => {
                    c.auth.revealer.set_reveal_child(false);
                    for w in all {
                        (w.done)(Err(ActionError::Denied));
                    }
                }
            }
            c.settle();
        });
    }
    /// Shows the sheet for the waiting changes.
    fn ask(&self, message: &str) {
        let n = self.queue.borrow().len();
        let text = if message.is_empty() && n > 1 {
            crate::copy::auth_waiting(n)
        } else {
            message.to_string()
        };
        self.auth.message.set_label(&text);
        if !self.auth.revealer.reveals_child() {
            self.auth.revealer.set_reveal_child(true);
        }
        if self.queue.borrow().checking() == 0 {
            self.auth.field.grab_focus();
        }
    }
    /// Once no retry is out: the sheet takes input again, or goes when nothing waits.
    fn settle(&self) {
        let q = self.queue.borrow();
        if q.checking() > 0 {
            return;
        }
        self.auth.set_busy(false);
        self.auth.allow.set_sensitive(true);
        self.auth.field.set_sensitive(true);
        if q.len() == 0 {
            self.auth.revealer.set_reveal_child(false);
        } else {
            self.auth.field.grab_focus();
        }
    }
    fn allow(self: &Rc<Self>) {
        if self.queue.borrow().checking() > 0 {
            return;
        }
        let pw: Rc<str> = self.auth.field.text().as_str().into();
        self.auth.field.set_text("");
        let (all, n) = self.queue.borrow_mut().allow();
        if all.is_empty() {
            return self.auth.revealer.set_reveal_child(false);
        }
        self.auth.message.set_label("");
        self.auth.set_busy(true);
        self.auth.allow.set_sensitive(false);
        self.auth.field.set_sensitive(false);
        for w in all {
            self.run(w, Some((pw.clone(), n)));
        }
        drop(pw);
        // A long change (an install) does not hold the sheet: its status line says "Checking…".
        let c = Rc::downgrade(self);
        glib::timeout_add_local_once(AUTH_HOLD, move || {
            let Some(c) = c.upgrade() else { return };
            if c.queue.borrow().len() == 0 && c.queue.borrow().checking() > 0 {
                c.auth.set_busy(false);
                c.auth.allow.set_sensitive(true);
                c.auth.field.set_sensitive(true);
                c.auth.revealer.set_reveal_child(false);
            }
        });
    }
    fn cancel(&self) {
        let all = self.queue.borrow_mut().cancel();
        self.auth.revealer.set_reveal_child(false);
        for w in all {
            (w.done)(Err(ActionError::Denied));
        }
    }
}

/// Inline one-sentence status under a section (HIG §3 errors).
fn status_label() -> gtk::Label {
    let l = w::label("", "secondary");
    l.set_wrap(true);
    l.set_visible(false);
    l.add_css_class("status-line");
    l
}

fn say(l: &gtk::Label, text: &str) {
    l.set_label(text);
    l.set_visible(!text.is_empty());
}

/// Everything a pane needs: actions, shared prefs, host facts, sheets and navigation.
pub(crate) struct Env {
    pub(crate) ctx: Rc<Ctx>,
    pub(crate) desk: Rc<Desk>,
    pub(crate) keys: Rc<Keys>,
    pub(crate) host: Host,
    sheets: Sheets,
    open: Rc<dyn Fn(&str)>,
    /// Keyboard Shortcuts opens on this group (Desktop & Dock > Tiling Shortcuts…).
    pub(crate) shortcut_group: Cell<Option<wol_core::shortcuts::Group>>,
    /// Builds the open pages again from the saved files (after an undo or an outside change).
    refresh: RefCell<Option<Rc<dyn Fn()>>>,
}

impl Env {
    pub(crate) fn sys(&self) -> &Rc<dyn System> {
        &self.ctx.sys
    }
    pub(crate) fn machine(&self) -> Arc<dyn Machine> {
        self.host.machine.clone()
    }
    /// Shows page `name` (subpage links, back buttons).
    pub(crate) fn open(&self, name: &str) {
        (self.open)(name)
    }
    /// Shows every built page again from the saved files.
    pub(crate) fn refresh(&self) {
        let f = self.refresh.borrow().clone();
        if let Some(f) = f {
            f()
        }
    }
    /// A closure that opens `name` (for button callbacks).
    pub(crate) fn opener(self: &Rc<Self>, name: &'static str) -> impl Fn() + 'static {
        let o = self.open.clone();
        move || o(name)
    }
}

type Build = Box<dyn FnOnce() -> gtk::Widget>;

/// Builds `name`'s page into its holder the first time it shows.
fn build_page(env: &Rc<Env>, name: &str) -> Option<Build> {
    let e = env.clone();
    let b: Build = match name {
        "Wi-Fi" => Box::new(move || wifi::page(&e).upcast()),
        "Bluetooth" => Box::new(move || bluetooth::page(&e).upcast()),
        "Network" => Box::new(move || network::page(&e).upcast()),
        "Notifications" => Box::new(move || notifications::page(&e).upcast()),
        "Sound" => Box::new(move || sound::page(&e).upcast()),
        "General" => Box::new(move || general::page(&e).upcast()),
        "Appearance" => Box::new(move || appearance::page(&e).upcast()),
        "Accessibility" => Box::new(move || accessibility::page(&e).upcast()),
        "Control Center" => Box::new(move || control_center::page(&e).upcast()),
        "Desktop & Dock" => Box::new(move || desktop_dock::page(&e).upcast()),
        "Displays" => Box::new(move || displays::page(&e.ctx).upcast()),
        "Wallpaper" => Box::new(move || wallpaper::page(&e).upcast()),
        "Effects & Looks" => Box::new(move || effects::page(&e).upcast()),
        "Widgets" => Box::new(move || widgets::page(&e).upcast()),
        "Battery" => Box::new(move || battery::page(&e).upcast()),
        "Lock Screen" => Box::new(move || lock::page(&e).upcast()),
        "Privacy & Security" => Box::new(move || privacy::page(&e).upcast()),
        "Users & Groups" => Box::new(move || accounts::page(&e).upcast()),
        "Keyboard" => Box::new(move || keyboard::page(&e).upcast()),
        "Trackpad & Mouse" => Box::new(move || trackpad::page(&e).upcast()),
        "Printers" => Box::new(move || printers::page(&e).upcast()),
        "Hardware" => Box::new(move || hardware::page(&e).upcast()),
        "Assistants & Automation" => Box::new(move || assistants::page(&e).upcast()),
        "Modules" => Box::new(move || modules::page(&e).upcast()),
        "Restore" => Box::new(move || restore::page(&e).upcast()),
        shortcuts::TITLE => Box::new(move || shortcuts::page(&e).upcast()),
        startup_security::TITLE => Box::new(move || startup_security::page(&e).upcast()),
        "About" => Box::new(move || about::page(&e).upcast()),
        "Software Update" => Box::new(move || software_update::page(&e).upcast()),
        "Storage" => Box::new(move || storage::page(&e).upcast()),
        "Date & Time" => Box::new(move || datetime::page(&e).upcast()),
        "Language & Region" => Box::new(move || language::page(&e).upcast()),
        "Login Items" => Box::new(move || login_items::page(&e).upcast()),
        "Sharing" => Box::new(move || sharing::page(&e).upcast()),
        _ => return None,
    };
    Some(b)
}

pub fn content(sys: Rc<dyn System>, host: &Host) -> w::AppWindow {
    build(sys, host).0
}

/// [`content`] with `query` typed into the search field: the hits page (golden renders).
pub fn content_searching(sys: Rc<dyn System>, host: &Host, query: &str) -> w::AppWindow {
    let (aw, field, on_query) = build(sys, host);
    field.set_text(query);
    on_query(query);
    aw
}

/// What a typed query does: filter the sidebar, show the hits (or go back when cleared).
type OnQuery = Rc<dyn Fn(&str)>;

fn build(sys: Rc<dyn System>, host: &Host) -> (w::AppWindow, gtk::SearchEntry, OnQuery) {
    let aw = w::AppWindow::new(true, false);
    aw.set_column(crate::kit::column::PANE_COLUMN);
    let ctx = Ctx::new(sys.clone(), "System Settings");
    let sheets = Sheets {
        join: w::toolbar_sheet(&aw, &w::vbox(0)),
        user: w::toolbar_sheet(&aw, &w::vbox(0)),
    };
    // Added last: the Authenticate sheet sits above the others.
    ctx.place(&aw);
    let open: Rc<dyn Fn(&str)> = {
        let (side, stack) = (aw.sidebar.downgrade(), aw.content.downgrade());
        Rc::new(move |name: &str| {
            let (Some(sidebar), Some(content)) = (side.upgrade(), stack.upgrade()) else {
                return;
            };
            let name = canonical(name);
            let mut i = 0;
            while let Some(r) = sidebar.row_at_index(i) {
                if r.widget_name() == name {
                    sidebar.select_row(Some(&r));
                    content.set_visible_child_name(name);
                    return;
                }
                i += 1;
            }
            if content.child_by_name(name).is_some() {
                sidebar.unselect_all();
                content.set_visible_child_name(name);
            }
        })
    };
    let env = Rc::new(Env {
        desk: Desk::load(&ctx),
        keys: Keys::load(&ctx),
        ctx,
        host: host.clone(),
        sheets,
        open,
        shortcut_group: Cell::new(None),
        refresh: RefCell::default(),
    });

    let field = aw.sidebar_search("Search");
    let pending: Rc<RefCell<HashMap<String, (gtk::Box, Build)>>> = Default::default();
    let holders: Rc<RefCell<Vec<(&'static str, gtk::Box)>>> = Default::default();
    let laptop = sys.battery().is_some();
    for (i, group) in SIDEBAR.iter().enumerate() {
        if i > 0 {
            aw.source_gap();
        }
        for p in group.iter() {
            let holder = w::vbox(0);
            aw.source_item(p.icon, p.title, &holder);
            if p.title == "Battery" && !laptop {
                // Desktops: the same pane, named like the Mac's.
                retitle(&aw, p.title, "Energy", "power-profile-balanced-symbolic");
            }
            holders.borrow_mut().push((p.title, holder.clone()));
            if let Some(b) = build_page(&env, p.title) {
                pending.borrow_mut().insert(p.title.into(), (holder, b));
            }
        }
    }
    for name in SUBPAGES {
        let holder = w::vbox(0);
        aw.hidden_page(name, &holder);
        holders.borrow_mut().push((name, holder.clone()));
        if let Some(b) = build_page(&env, name) {
            pending.borrow_mut().insert(name.into(), (holder, b));
        }
    }
    let ensure = {
        let pending = pending.clone();
        move |stack: &gtk::Stack| {
            let Some(name) = stack.visible_child_name() else {
                return;
            };
            let next = pending.borrow_mut().remove(name.as_str());
            if let Some((holder, build)) = next {
                holder.append(&build());
            }
        }
    };
    ensure(&aw.content);
    // After an undo every built page is built again from the restored files.
    let rebuild = {
        let (env, pending, holders, stack, ensure) = (
            Rc::downgrade(&env),
            pending.clone(),
            holders.clone(),
            aw.content.downgrade(),
            ensure.clone(),
        );
        move || {
            let (Some(stack), Some(env)) = (stack.upgrade(), env.upgrade()) else {
                return;
            };
            env.desk.reload();
            env.keys.reload();
            for (name, holder) in holders.borrow().iter() {
                if pending.borrow().contains_key(*name) {
                    continue;
                }
                if let Some(b) = build_page(&env, name) {
                    clear(holder);
                    pending
                        .borrow_mut()
                        .insert((*name).into(), (holder.clone(), b));
                }
            }
            ensure(&stack);
        }
    };
    let rebuild: Rc<dyn Fn()> = Rc::new(rebuild);
    *env.refresh.borrow_mut() = Some(rebuild);
    undo_toast(&aw, &env);
    aw.content
        .connect_visible_child_name_notify(move |s| ensure(s));

    // Search: rows whose title or words match; gaps hide while searching; Enter opens the first.
    // The content shows every hit (pane and row) until the field is cleared.
    let results = w::page("Search");
    let results_body = w::vbox(w::S6);
    results.append(&results_body);
    aw.hidden_page(RESULTS, &results);
    let before: Rc<RefCell<Option<String>>> = Default::default();
    let query: Rc<RefCell<String>> = Default::default();
    {
        let q = query.clone();
        aw.sidebar.set_filter_func(move |row| {
            let q = q.borrow();
            if q.trim().is_empty() || row.has_css_class("sidebar-search") {
                return true;
            }
            let name = row.widget_name();
            !name.is_empty() && search(&q).contains(&name.as_str())
        });
    }
    let on_query: OnQuery = {
        let (q, side, stack) = (
            query.clone(),
            aw.sidebar.downgrade(),
            aw.content.downgrade(),
        );
        let (env, body, before) = (
            Rc::downgrade(&env),
            results_body.downgrade(),
            before.clone(),
        );
        Rc::new(move |text: &str| {
            let text = text.to_string();
            *q.borrow_mut() = text.clone();
            if let Some(side) = side.upgrade() {
                side.invalidate_filter();
            }
            let (Some(stack), Some(env), Some(body)) =
                (stack.upgrade(), env.upgrade(), body.upgrade())
            else {
                return;
            };
            if text.trim().is_empty() {
                if let Some(back) = before.borrow_mut().take() {
                    env.open(&back);
                }
                return;
            }
            let showing = stack.visible_child_name().map(|n| n.to_string());
            if showing.as_deref() != Some(RESULTS) {
                *before.borrow_mut() = showing;
            }
            fill_results(&env, &body, &text);
            // No sidebar row stays selected under the hits (a selected row would show its page).
            if let Some(side) = side.upgrade() {
                side.unselect_all();
            }
            stack.set_visible_child_name(RESULTS);
        })
    };
    {
        let f = on_query.clone();
        field.connect_search_changed(move |s| f(&s.text()));
    }
    {
        let env = env.clone();
        field.connect_activate(move |s| {
            if let Some(page) = search::enter_target(&s.text()) {
                env.open(page);
            }
        });
    }
    (aw, field, on_query)
}

/// The hidden page that lists search hits.
const RESULTS: &str = "Search Results";

/// Search hits for `query`, grouped by page: each opens its page (the row's page for a row hit).
fn fill_results(env: &Rc<Env>, body: &gtk::Box, query: &str) {
    clear(body);
    let found = hits(query);
    if found.is_empty() {
        body.append(&w::empty_state(
            "system-search-symbolic",
            crate::copy::settings::SEARCH_NONE_TITLE,
            crate::copy::settings::SEARCH_NONE,
            None,
        ));
        return;
    }
    let mut pages: Vec<&'static str> = Vec::new();
    for h in &found {
        if !pages.contains(&h.page) {
            pages.push(h.page);
        }
    }
    for page in pages {
        let rows: Vec<gtk::Widget> = found
            .iter()
            .filter(|h| h.page == page)
            .map(|h| {
                // A row on a subpage names the subpage; the section already names the pane.
                let (title, sub) = match h.row {
                    Some(r) if search::parent(page) != page => (r, page),
                    Some(r) => (r, ""),
                    None => (page, ""),
                };
                let b = form::nav_row(icon_of(page), title, sub);
                let open = env.opener(page);
                b.connect_clicked(move |_| open());
                b.upcast()
            })
            .collect();
        body.append(&w::section(search::parent(page), &rows));
    }
}

/// The sidebar icon of `page`, or of the pane it is reached from.
fn icon_of(page: &str) -> &'static str {
    let pane = search::parent(page);
    SIDEBAR
        .iter()
        .flat_map(|g| g.iter())
        .find(|p| p.title == pane)
        .map_or("emblem-system-symbolic", |p| p.icon)
}

/// After each journaled change: "Wallpaper changed · Undo". Undo reverts every change made while
/// the toast was up (a slider drag is many saves), newest first, then refreshes the pages.
fn undo_toast(aw: &w::AppWindow, env: &Rc<Env>) {
    let toast = kit::toast::Toast::new(aw);
    let burst: Rc<RefCell<Vec<u64>>> = Rc::default();
    let rebuild = Rc::downgrade(env);
    let sys = env.sys().clone();
    let hook: ChangeHook = {
        let toast = Rc::downgrade(&toast);
        Rc::new(move |what: &str, id: u64| {
            let Some(t) = toast.upgrade() else { return };
            if !t.revealer.reveals_child() {
                burst.borrow_mut().clear();
            }
            if !burst.borrow().contains(&id) {
                burst.borrow_mut().push(id);
            }
            let (sys, burst, rebuild, toast) = (
                sys.clone(),
                burst.clone(),
                rebuild.clone(),
                Rc::downgrade(&t),
            );
            t.show(&changed_line(what), "Undo", move || {
                let ids: Vec<u64> = burst.borrow_mut().drain(..).rev().collect();
                let failed = ids.into_iter().find_map(|id| sys.undo(Some(id)).err());
                if let Some(env) = rebuild.upgrade() {
                    env.refresh();
                }
                if let Some(t) = toast.upgrade() {
                    t.show(failed.as_deref().unwrap_or("Change undone"), "", || {});
                }
            });
        })
    };
    CHANGED.with(|c| *c.borrow_mut() = Some(hook));
    // The toast lives as long as the window's widgets.
    let keep = toast.clone();
    aw.root.connect_destroy(move |_| {
        let _ = &keep;
        CHANGED.with(|c| c.borrow_mut().take());
    });
}

/// "Wallpaper changed", "Keyboard shortcuts changed".
fn changed_line(what: &str) -> String {
    format!("{} changed", restore::topic(what))
}

/// Sidebar row `title` shows `label` with `icon` (Battery on desktops).
fn retitle(aw: &w::AppWindow, title: &str, label: &str, icon: &str) {
    let mut i = 0;
    while let Some(r) = aw.sidebar.row_at_index(i) {
        i += 1;
        if r.widget_name() != title {
            continue;
        }
        let Some(row) = r.child().and_downcast::<gtk::Box>() else {
            return;
        };
        if let Some(img) = row.first_child().and_downcast::<gtk::Image>() {
            img.set_icon_name(Some(icon));
        }
        if let Some(l) = row.last_child().and_downcast::<gtk::Label>() {
            l.set_label(label);
        }
    }
}

/// Desktop & Dock > Windows on its own with `prefs` (golden renders of the tiling section).
pub fn windows_section(
    sys: Rc<dyn System>,
    prefs: wol_core::hyprprefs::DesktopPrefs,
) -> gtk::Widget {
    let aw = w::AppWindow::new(false, false);
    let ctx = Ctx::new(sys, "System Settings");
    let desk = Rc::new(Desk {
        ctx: ctx.clone(),
        cur: RefCell::new(prefs),
    });
    let env = Rc::new(Env {
        keys: Keys::load(&ctx),
        desk,
        ctx,
        host: Host::demo(),
        sheets: Sheets {
            join: w::toolbar_sheet(&aw, &w::vbox(0)),
            user: w::toolbar_sheet(&aw, &w::vbox(0)),
        },
        open: Rc::new(|_| {}),
        shortcut_group: Cell::new(None),
        refresh: RefCell::default(),
    });
    desktop_dock::windows(&env)
}

/// Desktop prefs shared by every pane that edits them: one saved file, one apply path.
pub(crate) struct Desk {
    ctx: Rc<Ctx>,
    cur: RefCell<wol_core::hyprprefs::DesktopPrefs>,
}

impl Desk {
    fn load(ctx: &Rc<Ctx>) -> Rc<Self> {
        Rc::new(Desk {
            ctx: ctx.clone(),
            cur: RefCell::new(wol_core::hyprprefs::load_desktop(&config(""))),
        })
    }
    pub(crate) fn get(&self) -> wol_core::hyprprefs::DesktopPrefs {
        self.cur.borrow().clone()
    }
    /// Changes the prefs, saves them and applies the difference live; a failure shows on `status`
    /// and reads the saved prefs back (the next change must not save this one too), then calls
    /// `failed()`: the pane puts its controls back to the saved prefs. A change that changes
    /// nothing is not saved.
    pub(crate) fn set_or(
        self: &Rc<Self>,
        status: &gtk::Label,
        f: impl FnOnce(&mut wol_core::hyprprefs::DesktopPrefs),
        failed: impl FnOnce() + 'static,
    ) {
        let before = self.get();
        f(&mut self.cur.borrow_mut());
        if *self.cur.borrow() == before {
            return;
        }
        let (st, me, failed) = (status.clone(), self.clone(), RefCell::new(Some(failed)));
        self.ctx.act(
            Action::Desktop(Box::new(self.get())),
            Rc::new(move |r| {
                say(&st, if r.is_ok() { "" } else { SAVE_FAILED });
                if r.is_ok() {
                    changed_latest("desktop");
                } else {
                    me.reload();
                    if let Some(f) = failed.borrow_mut().take() {
                        f()
                    }
                }
            }),
        );
    }
    /// [`Desk::set_or`] for a switch: `done(ok)` after the save; a failure also reads the saved
    /// prefs back, so the switch and the file agree again.
    pub(crate) fn set_switch(
        self: &Rc<Self>,
        status: &gtk::Label,
        f: impl FnOnce(&mut wol_core::hyprprefs::DesktopPrefs),
        done: Box<dyn FnOnce(bool)>,
    ) {
        f(&mut self.cur.borrow_mut());
        let (st, me, done) = (status.clone(), self.clone(), RefCell::new(Some(done)));
        self.ctx.act(
            Action::Desktop(Box::new(self.get())),
            Rc::new(move |r| {
                say(&st, if r.is_ok() { "" } else { SAVE_FAILED });
                if r.is_ok() {
                    changed_latest("desktop");
                } else {
                    me.reload();
                }
                if let Some(d) = done.borrow_mut().take() {
                    d(r.is_ok())
                }
            }),
        );
    }
    /// Reads the saved prefs again (after an undo).
    fn reload(&self) {
        *self.cur.borrow_mut() = wol_core::hyprprefs::load_desktop(&config(""));
    }
}

const SAVE_FAILED: &str =
    "This change couldn’t be saved. Check that your home folder has free space.";

/// Keyboard shortcuts: shipped defaults (single source: the shipped hyprland.conf) and the user's changes.
pub(crate) struct Keys {
    ctx: Rc<Ctx>,
    pub(crate) defaults: Vec<wol_core::shortcuts::Shortcut>,
    over: RefCell<wol_core::shortcuts::Overrides>,
    /// The Keyboard Shortcuts page's re-sync, run after a failed save read the file back.
    resync: RefCell<std::rc::Weak<dyn Fn()>>,
}

impl Keys {
    fn load(ctx: &Rc<Ctx>) -> Rc<Self> {
        Rc::new(Keys {
            ctx: ctx.clone(),
            defaults: wol_core::shortcuts::load_defaults(|p| std::fs::read_to_string(p).ok()),
            over: RefCell::new(wol_core::hyprprefs::load_overrides(&config(""))),
            resync: RefCell::new(std::rc::Weak::<fn()>::new() as std::rc::Weak<dyn Fn()>),
        })
    }
    /// Every shortcut as it is now (defaults with changes, then custom ones).
    pub(crate) fn list(&self) -> Vec<wol_core::shortcuts::Shortcut> {
        wol_core::shortcuts::effective(&self.defaults, &self.over.borrow())
    }
    pub(crate) fn overrides(&self) -> wol_core::shortcuts::Overrides {
        self.over.borrow().clone()
    }
    /// Applies `f` to a copy of the overrides; on success saves and rebinds live. A failed save
    /// reads the saved changes back and re-syncs the page ([`Keys::on_resync`]).
    pub(crate) fn edit(
        self: &Rc<Self>,
        status: &gtk::Label,
        f: impl FnOnce(&mut wol_core::shortcuts::Overrides) -> Result<(), wol_core::shortcuts::Refused>,
    ) -> Result<(), wol_core::shortcuts::Refused> {
        let mut o = self.overrides();
        f(&mut o)?;
        *self.over.borrow_mut() = o.clone();
        let (st, me) = (status.clone(), self.clone());
        self.ctx.act(
            Action::Shortcuts(Box::new(o)),
            Rc::new(move |r| {
                say(&st, if r.is_ok() { "" } else { SAVE_FAILED });
                if r.is_ok() {
                    changed_latest("shortcuts");
                } else {
                    me.reload();
                    let resync = me.resync.borrow().upgrade();
                    if let Some(f) = resync {
                        f()
                    }
                }
            }),
        );
        Ok(())
    }
    /// What puts the page's controls back to [`Keys::list`] after a failed save (held weakly:
    /// the page keeps it alive).
    pub(crate) fn on_resync(&self, f: &Rc<dyn Fn()>) {
        *self.resync.borrow_mut() = Rc::downgrade(f);
    }
    /// Reads the saved changes again (after an undo).
    fn reload(&self) {
        *self.over.borrow_mut() = wol_core::hyprprefs::load_overrides(&config(""));
    }
    /// WolOS shortcuts pause while one is being recorded.
    pub(crate) fn capture(&self, on: bool) {
        self.ctx.act(Action::ShortcutCapture(on), Rc::new(|_| {}));
    }
}

struct Sheets {
    join: gtk::Revealer,
    user: gtk::Revealer,
}

/// Replace a toolbar sheet's body.
fn sheet_body(r: &gtk::Revealer, body: &impl IsA<gtk::Widget>) {
    if let Some(card) = r.child().and_downcast::<gtk::Box>() {
        clear(&card);
        card.append(body);
    }
}

fn row(title: &str, control: &impl IsA<gtk::Widget>) -> gtk::Widget {
    form::row(title, "", control)
}

fn row_sub(title: &str, subtitle: &str, control: &impl IsA<gtk::Widget>) -> gtk::Widget {
    form::row(title, subtitle, control)
}

fn info(title: &str, value: &str) -> gtk::Widget {
    form::info_row(title, value)
}

fn dropdown(options: &[&str], selected: usize, changed: impl Fn(usize) + 'static) -> gtk::DropDown {
    let d = w::dropdown(options, selected as u32);
    d.connect_selected_notify(move |d| changed(d.selected() as usize));
    d
}

/// Shell prefs file (shell.json), written then announced to the running shell (`reload`).
fn save_shell(p: ShellPrefs) -> bool {
    // The bar and Dock layout (W2-C) is kept: only the prefs change.
    let text = prefs::keep_layout(read(&config(prefs::SHELL_FILE)).as_deref(), p);
    let ok = save_journaled("shell", prefs::SHELL_FILE, Some(&text)).is_ok();
    if ok {
        reload_shell();
    }
    ok
}

/// Tells the running shell to re-read its files (`reload` on wol-shell.sock).
fn reload_shell() {
    let sock = crate::protocol::socket_path(&kit::runtime_dir());
    std::thread::spawn(move || {
        use std::io::Write;
        if let Ok(mut s) = std::os::unix::net::UnixStream::connect(sock) {
            let _ = s.write_all(b"reload\n");
        }
    });
}

fn load_shell() -> ShellPrefs {
    ShellPrefs::parse(read(&config(prefs::SHELL_FILE)).as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// wol-admind as the sheet meets it: NeedsAuth without a password, Ok with "right",
    /// Denied with any other; `pkg` actions are wold's refusal (no password helps).
    #[derive(Default)]
    struct Asking {
        sent: std::sync::Mutex<Vec<(Action, Option<String>)>>,
    }

    impl System for Asking {
        fn battery(&self) -> Option<crate::status::Battery> {
            None
        }
        fn audio(&self) -> crate::status::Audio {
            Default::default()
        }
        fn network(&self) -> crate::status::Network {
            Default::default()
        }
        fn brightness(&self) -> Option<u8> {
            None
        }
        fn spawn_with(&self, a: Action, pw: Option<&str>) -> crate::system::Done {
            self.sent
                .lock()
                .unwrap()
                .push((a.clone(), pw.map(String::from)));
            let r = match (&a, pw) {
                (Action::PkgInstall(_), _) => Err(ActionError::Refused),
                (_, None) => Err(ActionError::NeedsAuth),
                (_, Some("right")) => Ok(()),
                (_, Some(_)) => Err(ActionError::Denied),
            };
            let (tx, rx) = async_channel::bounded(1);
            let _ = tx.try_send(r);
            rx
        }
    }

    type Log = Rc<RefCell<Vec<(&'static str, Result<(), ActionError>)>>>;

    fn waiting(a: Action, name: &'static str, log: &Log) -> Waiting {
        let log = log.clone();
        Waiting {
            a,
            done: Rc::new(move |r| log.borrow_mut().push((name, r))),
            status: None,
        }
    }

    /// One send and its answer through the queue, as `Ctx::run` does (blocking here).
    fn send(
        q: &mut AuthQueue,
        sys: &Asking,
        w: Waiting,
        pw: Option<(&str, u32)>,
    ) -> Option<&'static str> {
        let retry = pw.map(|(_, n)| n);
        q.sent(retry);
        let r = sys
            .spawn_with(w.a.clone(), pw.map(|(p, _)| p))
            .recv_blocking()
            .unwrap();
        match q.answered(w, r, retry) {
            Next::Finish(w, r) => {
                (w.done)(r);
                None
            }
            Next::Ask(m) => Some(m),
            Next::GiveUp(all) => {
                for w in all {
                    (w.done)(Err(ActionError::Denied));
                }
                Some("gave up")
            }
        }
    }

    fn toggles() -> [Action; 2] {
        use shared::admin::{AdminAction as A, AdminRequest};
        let req = |action, on: &str| {
            Action::Admin(AdminRequest {
                action,
                args: vec![on.into()],
            })
        };
        [req(A::RemoteLogin, "on"), req(A::AirPlayReceiver, "on")]
    }

    /// QA 0.12.0 bug 6: two Sharing switches asked at once; the second overwrote the first,
    /// whose switch stayed grey. Both wait now: one Allow runs both with the password, in order.
    #[test]
    fn two_queued_toggles_both_run_after_one_allow() {
        let (sys, log, mut q) = (Asking::default(), Log::default(), AuthQueue::default());
        let [a, b] = toggles();
        assert_eq!(
            send(&mut q, &sys, waiting(a.clone(), "remote", &log), None),
            Some("")
        );
        assert_eq!(
            send(&mut q, &sys, waiting(b.clone(), "airplay", &log), None),
            Some("")
        );
        assert_eq!(q.len(), 2, "both wait");
        assert!(log.borrow().is_empty());
        let (all, n) = q.allow();
        assert_eq!(q.len(), 0);
        for w in all {
            assert_eq!(send(&mut q, &sys, w, Some(("right", n))), None);
        }
        assert_eq!(q.checking(), 0);
        assert_eq!(*log.borrow(), [("remote", Ok(())), ("airplay", Ok(()))]);
        let sent = sys.sent.lock().unwrap();
        assert_eq!(
            sent[2..],
            [(a, Some("right".into())), (b, Some("right".into()))]
        );
    }

    /// Cancel flips every waiting switch back, not just the last one.
    #[test]
    fn cancel_fails_every_waiting_change() {
        let (sys, log, mut q) = (Asking::default(), Log::default(), AuthQueue::default());
        let [a, b] = toggles();
        send(&mut q, &sys, waiting(a, "remote", &log), None);
        send(&mut q, &sys, waiting(b, "airplay", &log), None);
        for w in q.cancel() {
            (w.done)(Err(ActionError::Denied));
        }
        assert_eq!(q.len(), 0);
        assert_eq!(
            *log.borrow(),
            [
                ("remote", Err(ActionError::Denied)),
                ("airplay", Err(ActionError::Denied))
            ]
        );
    }

    /// QA 0.12.0 bug 4: the sheet asked forever. A wrong password asks again with its sentence
    /// (once per Allow, however many changes it re-ran); the third gives up and fails them all.
    #[test]
    fn three_wrong_passwords_end_the_sheet() {
        let (sys, log, mut q) = (Asking::default(), Log::default(), AuthQueue::default());
        let [a, b] = toggles();
        send(&mut q, &sys, waiting(a, "remote", &log), None);
        send(&mut q, &sys, waiting(b, "airplay", &log), None);
        for round in 1..=3 {
            let (all, n) = q.allow();
            let said: Vec<_> = all
                .into_iter()
                .map(|w| send(&mut q, &sys, w, Some(("wrong", n))))
                .collect();
            if round < 3 {
                assert_eq!(said, [Some(ActionError::Denied.message()); 2], "{round}");
                assert_eq!(q.len(), 2);
            } else {
                // The second retry of that Allow was still out: it fails too, no sheet.
                assert_eq!(said, [Some("gave up"), None]);
            }
        }
        assert_eq!(q.len(), 0);
        assert_eq!(log.borrow().len(), 2, "{:?}", log.borrow());
        assert!(
            log.borrow()
                .iter()
                .all(|(_, r)| *r == Err(ActionError::Denied))
        );
    }

    /// wold's refusal never opens the sheet: the change ends with its own sentence.
    #[test]
    fn a_refusal_ends_the_change_without_the_sheet() {
        let (sys, log, mut q) = (Asking::default(), Log::default(), AuthQueue::default());
        assert_eq!(
            send(
                &mut q,
                &sys,
                waiting(Action::PkgInstall("vim".into()), "vim", &log),
                None
            ),
            None
        );
        assert_eq!(q.len(), 0);
        assert_eq!(*log.borrow(), [("vim", Err(ActionError::Refused))]);
        assert!(!ActionError::Refused.message().contains("password."));
    }

    /// QA 0.12.0 bug 9: the toast's Undo reverted the newest entry of any topic. It reverts the
    /// entry the change recorded; a save made in `wol_core` resolves to the newest of its topic.
    #[test]
    fn the_undo_toast_targets_the_changes_own_entry() {
        use wol_core::journal::Entry;
        let e = |id, what: &str, by: &str, undoes| Entry {
            id,
            t: 0,
            by: by.into(),
            what: what.into(),
            changes: vec![],
            undoes,
        };
        let all = [
            e(1, "desktop", "settings", None),
            e(2, "sound", "settings", None),
            e(3, "desktop", "cli", None),
            e(4, "desktop", "settings", Some(1)),
        ];
        assert_eq!(latest_entry(&all, "desktop"), Some(1));
        assert_eq!(latest_entry(&all, "sound"), Some(2));
        assert_eq!(latest_entry(&all, "theme"), None);
    }

    #[test]
    fn pages_list_the_sidebar_once_each() {
        let side: Vec<&str> = SIDEBAR
            .iter()
            .flat_map(|g| g.iter().map(|p| p.title))
            .collect();
        assert_eq!(side, PAGES, "PAGES follows the sidebar");
        let mut uniq = PAGES.to_vec();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), PAGES.len(), "each pane once");
        for s in SUBPAGES {
            assert!(!PAGES.contains(&s), "{s} is a subpage");
        }
    }

    #[test]
    fn search_matches_titles_and_words() {
        for (q, want) in [
            ("blur", "Effects & Looks"),
            ("wifi", "Wi-Fi"),
            ("zsh", "Users & Groups"),
            ("time zone", "General"),
            ("pwsh", "General"),
            ("tile", "Desktop & Dock"),
            ("firewall", "Privacy & Security"),
            ("UNDO", "Restore"),
            ("driver", "Hardware"),
        ] {
            assert!(search(q).contains(&want), "{q}: {:?}", search(q));
        }
        assert!(search("no such thing").is_empty());
        assert_eq!(
            search("").len(),
            PAGES.len(),
            "empty query lists every pane"
        );
        assert!(search("balance").contains(&"Sound"), "a row finds its pane");
        assert!(
            search("secure boot").contains(&"Privacy & Security"),
            "a subpage row finds the pane it is reached from"
        );
    }

    #[test]
    fn old_names_still_open() {
        assert_eq!(canonical("lock & idle"), "Lock Screen");
        assert_eq!(canonical("Accounts"), "Users & Groups");
        assert_eq!(canonical("Wi-Fi"), "Wi-Fi");
    }

    /// Live: `wol-settings Wi-Fi` while Settings ran opened the first launch's page (the primary
    /// instance read its own argv). The page comes from the launch's own arguments, under
    /// today's name, and every alias lands on a page Settings has.
    #[test]
    fn a_launch_names_its_page_under_todays_name() {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            requested_page(&v(&["wol-settings", "Lock & Idle"])),
            Some("Lock Screen")
        );
        assert_eq!(
            requested_page(&v(&["wol-settings", "Wi-Fi"])),
            Some("Wi-Fi")
        );
        assert_eq!(
            requested_page(&v(&["wol-settings", "About"])),
            Some("About")
        );
        assert_eq!(requested_page(&v(&["wol-settings"])), None);
        assert_eq!(requested_page(&[]), None);
        for (old, new) in ALIASES {
            assert!(
                PAGES.contains(&new) || SUBPAGES.contains(&new),
                "{old} -> {new}: not a page"
            );
        }
    }
}
