// SPDX-License-Identifier: GPL-2.0-or-later
//! Low battery, full disk and updates-ready notifications, checked with the menu bar poll.
//! Battery levels come from `wol_core::power::policy` (20, 10, 5 %, sleep at 3 %), updates from
//! `wol_core::update::check`; the sentences from [`crate::copy`].
use crate::software_update::PluginNews;
use crate::status::{Battery, BatteryStatus};
use wol_core::power::policy::{self, Alert};
use wol_core::update::check::Updates;

/// Below this share of free space on the startup disk, warn once (until it is back above
/// [`DISK_CLEAR_PERCENT`]).
pub const DISK_WARN_PERCENT: u64 = 5;
pub const DISK_CLEAR_PERCENT: u64 = 10;
/// Menu bar polls between disk checks (5 s each: every 5 minutes; `df` forks).
pub const DISK_EVERY_POLLS: u32 = 60;

/// What one poll asks the session to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    /// Notification: summary, body, critical (stays until dismissed).
    Show(String, String, bool),
    /// Notify, then lock and sleep now (the battery is about to die).
    Sleep(String, String),
    /// A file was stopped: summary, body, path. Move to Trash and Details buttons.
    Malware(String, String, String),
    /// Notify with a Restart action (a driver was installed in the background).
    Restart(String, String),
    /// Notify; a click on it opens Settings > Software Update (`software_update::OPEN`).
    Updates(String, String),
    /// A sign-in page holds the network (a café, a hotel): summary, body; Open shows the page
    /// NetworkManager probes (`wol_core::netinfo::nm_connectivity_uri`).
    Portal(String, String),
    /// A removable disk was plugged in (QA-FDAY-13): summary, body; Open in Files shows it.
    Stick(String, String),
}

/// What a banner's chosen action starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Then {
    /// This argv.
    Run(Vec<String>),
    /// The page NetworkManager probes, in the browser (`wol_core::netinfo::nm_connectivity_uri`).
    PortalPage,
}

/// A banner that waits for its click (`notify-send -w`): the notify-send argv and, per action id,
/// what the click starts. [`show_banner`] runs it; no shell string carries it (RR-020).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Banner {
    pub notify: Vec<String>,
    pub then: Vec<(&'static str, Then)>,
}

/// A banner with `actions` (id, button label, what it starts) after the summary and body.
fn banner(
    summary: String,
    body: String,
    critical: bool,
    actions: Vec<(&'static str, &str, Then)>,
) -> Banner {
    let mut notify: Vec<String> = ["notify-send", "-a", "WolOS"].map(String::from).to_vec();
    if critical {
        notify.extend(["-u".into(), "critical".into()]);
    }
    notify.push("-w".into());
    for (id, label, _) in &actions {
        notify.extend(["-A".into(), format!("{id}={label}")]);
    }
    notify.extend([summary, body]);
    Banner {
        notify,
        then: actions.into_iter().map(|(id, _, t)| (id, t)).collect(),
    }
}

/// Shows `b` on its own thread (notify-send waits for the click, as long as it takes); the
/// action chosen starts what it names. Nothing runs when the banner is closed.
pub(crate) fn show_banner(b: Banner) {
    let waiter = std::thread::Builder::new().name("banner-wait".into());
    let started = waiter.spawn(move || {
        let out = std::process::Command::new(&b.notify[0])
            .args(&b.notify[1..])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output();
        let chosen = match out {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_owned(),
            Err(e) => {
                crate::diag::say!("wol-shell: notify-send: {e}");
                return;
            }
        };
        let argv = match b.then.iter().find(|(id, _)| *id == chosen) {
            Some((_, Then::Run(argv))) => argv.clone(),
            Some((_, Then::PortalPage)) => {
                match wol_core::netinfo::nm_connectivity_uri(&wol_core::system::PosixHost) {
                    Some(u) => vec!["xdg-open".into(), u],
                    None => return,
                }
            }
            None => return,
        };
        crate::kit::spawn(&argv);
    });
    if let Err(e) = started {
        crate::diag::say!("wol-shell: no thread for a banner: {e}");
    }
}

/// The notice for a change of connectivity: once per portal, when the network turns to a sign-in
/// page, never again while it stays one.
pub fn portal(
    prev: wol_core::netinfo::Connectivity,
    now: wol_core::netinfo::Connectivity,
) -> Option<Notice> {
    use wol_core::netinfo::Connectivity::Portal;
    (now == Portal && prev != Portal).then(|| {
        Notice::Portal(
            crate::copy::settings::PORTAL_TITLE.into(),
            crate::copy::settings::PORTAL_BODY.into(),
        )
    })
}

/// File (under `$XDG_STATE_HOME`) holding the `checked` time of the update check already told
/// about, so a new login does not tell about the same check again (live-1 M1).
pub const UPDATES_SHOWN_FILE: &str = "wolos/updates-shown";

/// The `checked` time of the check already told about in an earlier session (0: none). Blocking.
pub fn load_shown(state_home: &std::path::Path) -> u64 {
    std::fs::read_to_string(state_home.join(UPDATES_SHOWN_FILE))
        .ok()
        .and_then(|t| t.trim().parse().ok())
        .unwrap_or(0)
}

/// Keeps `checked` as the check told about (live-1 M1). Blocking: off the UI thread.
pub fn persist_shown(state_home: &std::path::Path, checked: u64) {
    let file = state_home.join(UPDATES_SHOWN_FILE);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, format!("{checked}\n"));
}

#[derive(Debug, Default)]
pub struct Alerts {
    /// Battery level already warned about while discharging.
    battery: Option<u8>,
    disk_warned: bool,
    polls: u32,
    /// The update check already notified about (its `checked` time).
    updates_shown: u64,
    /// `updates_shown` changed and is not yet on disk: [`persist_shown`] writes it off the UI thread.
    to_persist: Option<u64>,
    /// Plugin versions already notified about: (id, version).
    plugins_shown: Vec<(String, String)>,
    /// The online install result already shown (its `at` time).
    online_shown: Option<u64>,
    /// Newest stopped file already shown (its `when`).
    malware_shown: u64,
    /// The connectivity of the last poll ([`portal`] tells on a change to a sign-in page).
    connectivity: wol_core::netinfo::Connectivity,
    /// The removable disks of the last poll (None before the first: a stick that is already in at
    /// login is not news).
    sticks: Option<Vec<String>>,
}

impl Alerts {
    /// One connectivity reading per poll: the sign-in notice once per portal.
    pub fn connectivity(&mut self, now: wol_core::netinfo::Connectivity) -> Option<Notice> {
        portal(std::mem::replace(&mut self.connectivity, now), now)
    }

    /// One battery reading: a notice at each level once per discharge.
    pub fn battery(&mut self, b: Option<Battery>) -> Option<Notice> {
        let b = b?;
        let discharging = b.status == BatteryStatus::Discharging;
        let (alert, level) = policy::battery_alert(self.battery, b.percent, discharging);
        self.battery = level;
        Some(match alert? {
            Alert::Low(_) => {
                let (s, body) = crate::copy::battery_low(b.percent);
                Notice::Show(s, body, b.percent <= 5)
            }
            Alert::Sleep => Notice::Sleep(
                crate::copy::BATTERY_SLEEP.0.into(),
                crate::copy::BATTERY_SLEEP.1.into(),
            ),
        })
    }

    /// Free and total bytes of the startup disk: a notice when it gets under 5 % free.
    pub fn disk(&mut self, free: u64, total: u64) -> Option<Notice> {
        if total == 0 {
            return None;
        }
        let pct = free.saturating_mul(100) / total;
        if pct >= DISK_CLEAR_PERCENT {
            self.disk_warned = false;
        }
        if pct >= DISK_WARN_PERCENT || self.disk_warned {
            return None;
        }
        self.disk_warned = true;
        let (s, body) = crate::copy::disk_full(&wol_core::storage::human(free));
        Some(Notice::Show(s, body, false))
    }

    /// The `checked` time to write to [`UPDATES_SHOWN_FILE`], once: the poll hands it to a thread
    /// that may block ([`persist_shown`]).
    pub fn take_to_persist(&mut self) -> Option<u64> {
        self.to_persist.take()
    }

    /// The daily update check's findings: a notice once per check that found something, and
    /// never for the release this computer already runs (`running`: its `wolos-release.json`,
    /// None when it cannot be read; the same filter `wol update status` and Software Update use).
    pub fn updates(&mut self, u: Option<Updates>, running: Option<&str>) -> Option<Notice> {
        let u = u?.installed(running);
        if !u.notify_after(self.updates_shown) {
            return None;
        }
        self.updates_shown = u.checked;
        self.to_persist = Some(u.checked);
        let (s, body) = crate::copy::updates_ready(u.count(), !u.security.is_empty());
        Some(Notice::Updates(s, body))
    }

    /// Plugin updates the plugin index has for this account (audit6 ST6-4): one notice for the
    /// versions not yet told about, none when every one was. A plugin that updates again is told
    /// about again; one that was updated is forgotten.
    pub fn plugin_updates(&mut self, news: Vec<PluginNews>) -> Option<Notice> {
        self.plugins_shown
            .retain(|(id, v)| news.iter().any(|n| n.id == *id && n.to == *v));
        let fresh: Vec<&PluginNews> = news
            .iter()
            .filter(|n| !self.plugins_shown.contains(&(n.id.clone(), n.to.clone())))
            .collect();
        if fresh.is_empty() {
            return None;
        }
        self.plugins_shown
            .extend(fresh.iter().map(|n| (n.id.clone(), n.to.clone())));
        let names: Vec<&str> = fresh.iter().map(|n| n.name.as_str()).collect();
        let (s, body) = crate::copy::plugin_updates_ready(&names);
        Some(Notice::Show(s, body, false))
    }

    /// Each file the malware check stopped, once. Nothing appears when nothing is wrong.
    pub fn malware(&mut self, blocked: &[wol_core::malware::Blocked]) -> Vec<Notice> {
        let fresh: Vec<Notice> = blocked
            .iter()
            .filter(|b| b.when > self.malware_shown)
            .map(|b| {
                let file = wol_core::malware::file_name(&b.path);
                let (s, body) = crate::copy::malware_found(file, &b.name);
                Notice::Malware(s, body, b.path.clone())
            })
            .collect();
        self.malware_shown = blocked
            .iter()
            .map(|b| b.when)
            .fold(self.malware_shown, u64::max);
        fresh
    }

    /// A driver installed (or not) when the network came up: one notice per attempt, with
    /// Restart when it worked, a pointer to Settings when it did not.
    pub fn online(&mut self, r: Option<shared::hardware::OnlineInstall>) -> Option<Notice> {
        let r = r?;
        let ((s, body), restart) = wol_core::hardware::online::notice(&r, self.online_shown)?;
        self.online_shown = Some(r.at);
        let (s, body) = (s.to_string(), body.to_string());
        Some(if restart {
            Notice::Restart(s, body)
        } else {
            Notice::Show(s, body, false)
        })
    }

    /// The removable disks now: a notice for each one that was not there at the last poll. The
    /// first poll only remembers (QA-FDAY-13: plugging a stick said nothing).
    pub fn sticks(&mut self, now: Vec<Stick>) -> Vec<Notice> {
        let ids: Vec<String> = now.iter().map(|s| s.dev.clone()).collect();
        let Some(before) = self.sticks.replace(ids) else {
            return Vec::new();
        };
        now.into_iter()
            .filter(|s| !before.contains(&s.dev))
            .map(|s| {
                let (summary, body) = crate::copy::stick_plugged(&s.name);
                Notice::Stick(summary, body)
            })
            .collect()
    }

    /// Disk is checked on every [`DISK_EVERY_POLLS`]th poll, starting with the first.
    pub fn disk_due(&mut self) -> bool {
        let due = self.polls.is_multiple_of(DISK_EVERY_POLLS);
        self.polls = self.polls.wrapping_add(1);
        due
    }
}

/// Startup disk (`/`, or `/home` when it is fuller) free and total bytes from `df`.
fn startup_disk() -> Option<(u64, u64)> {
    let disks = wol_core::system::stats::DiskUsage::read(&wol_core::system::PosixHost);
    disks
        .iter()
        .filter(|d| d.mount == "/" || d.mount == "/home")
        .map(|d| {
            (
                d.available.max(0) as u64,
                (d.used + d.available).max(0) as u64,
            )
        })
        .filter(|(_, t)| *t > 0)
        .min_by_key(|(f, t)| f.saturating_mul(100) / t)
}

thread_local! {
    static STATE: std::cell::RefCell<Alerts> = std::cell::RefCell::new(Alerts::default());
}

/// What one poll read off the UI thread: the due checks (every [`DISK_EVERY_POLLS`]th poll)
/// and the malware lists when they changed.
#[derive(Debug, Default)]
pub struct Reads {
    /// None between the due polls.
    pub due: Option<DueReads>,
    /// The account's malware lists, only when a file changed since the last read.
    pub malware: Option<Vec<String>>,
    /// The removable disks plugged in now.
    pub sticks: Vec<Stick>,
}

#[derive(Debug, Default)]
pub struct DueReads {
    pub updates: Option<Updates>,
    /// The check already told about in an earlier session ([`load_shown`]).
    pub shown: u64,
    /// This slot's `wolos-release.json`: a check's release equal to it is installed, not an update.
    pub running: Option<String>,
    /// Plugin updates, read about once an hour ([`PLUGINS_EVERY_DUE`]); None between the checks
    /// (an empty list is a check that found nothing).
    pub plugins: Option<Vec<PluginNews>>,
    pub online: Option<shared::hardware::OnlineInstall>,
    /// Free and total bytes of the startup disk.
    pub disk: Option<(u64, u64)>,
}

/// Due reads between plugin index checks (a due read is every 5 minutes: once an hour). The check
/// reads the verified cache and downloads only when wopkg's own rule asks for it, so this bounds
/// how often an offline computer tries.
pub const PLUGINS_EVERY_DUE: u32 = 12;
static DUE_READS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Whether this due read also checks the plugin index: the first one, then every
/// [`PLUGINS_EVERY_DUE`]th.
fn plugins_due() -> bool {
    DUE_READS
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        .is_multiple_of(PLUGINS_EVERY_DUE)
}

/// Blocking: `df`, the update check's files, the online-install record and the malware lists.
fn read(due: bool) -> Reads {
    let due = due.then(|| DueReads {
        updates: Updates::read(
            std::path::Path::new(wol_core::update::check::DIR),
            crate::focus::unix_now(),
        ),
        running: std::fs::read_to_string(wol_core::update::check::RELEASE_FILE).ok(),
        shown: load_shown(&wol_core::journal::state_home()),
        plugins: plugins_due().then(crate::software_update::plugin_news),
        online: std::fs::read_to_string(wol_core::hardware::online::ONLINE_PATH)
            .ok()
            .and_then(|t| wol_core::hardware::online::parse(&t)),
        disk: startup_disk(),
    });
    Reads {
        due,
        malware: malware_read(),
        sticks: wol_core::storage::removable_disks(std::path::Path::new("/sys/block")),
    }
}

/// The notices for what a poll read, in the order they are shown: a driver install first, then
/// the disk, updates and each stopped file.
pub fn notices(a: &mut Alerts, r: Reads) -> Vec<Notice> {
    let mut out = Vec::new();
    if let Some(d) = r.due {
        // What an earlier session told stays told.
        a.updates_shown = a.updates_shown.max(d.shown);
        out.extend(a.online(d.online));
        out.extend(d.disk.and_then(|(f, t)| a.disk(f, t)));
        out.extend(a.updates(d.updates, d.running.as_deref()));
        // Between the hourly checks nothing is read, and what was told stays told.
        if let Some(news) = d.plugins {
            out.extend(a.plugin_updates(news));
        }
    }
    if let Some(texts) = r.malware {
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        out.extend(a.malware(&wol_core::malware::merge(&refs)));
    }
    out.extend(a.sticks(r.sticks));
    out
}

/// A removable disk with a medium in it ([`wol_core::storage::removable_disks`]).
pub type Stick = wol_core::storage::Removable;

/// Menu bar poll hook: battery every time (a reading already in hand), the rest read in
/// `gio::spawn_blocking`: `df` hung the menu bar, dock and panels for its 2 s timeout on a
/// stale network mount (audit4 S3-6). The GTK thread only applies the result.
pub fn poll(b: Option<Battery>, connectivity: wol_core::netinfo::Connectivity) {
    let (battery, portal, due) =
        STATE.with_borrow_mut(|a| (a.battery(b), a.connectivity(connectivity), a.disk_due()));
    for n in battery.into_iter().chain(portal) {
        act(n);
    }
    gtk::glib::spawn_future_local(async move {
        let Ok(r) = gtk::gio::spawn_blocking(move || read(due)).await else {
            return;
        };
        let (notes, told) = STATE.with_borrow_mut(|a| (notices(a, r), a.take_to_persist()));
        for n in notes {
            act(n);
        }
        if let Some(checked) = told {
            let _ = gtk::gio::spawn_blocking(move || {
                persist_shown(&wol_core::journal::state_home(), checked)
            })
            .await;
        }
    });
}

/// The newest change seen on the malware lists (read off the UI thread).
static MALWARE_SEEN: std::sync::Mutex<Option<std::time::SystemTime>> = std::sync::Mutex::new(None);

/// The account's lists, read only when they changed: a stat every poll, no read at rest.
fn malware_read() -> Option<Vec<String>> {
    let uid = wol_core::malware::my_uid();
    malware_read_in(
        [
            wol_core::malware::findings_path(uid).into(),
            wol_core::malware::sweep_path(uid).into(),
        ],
        &MALWARE_SEEN,
    )
}

/// [`malware_read`] of `files`: the guard's list and the background pass's (`sweep-<uid>.json`,
/// the account's own 0600 file; audit R7-1), each in its own folder (audit F10).
fn malware_read_in(
    files: [std::path::PathBuf; 2],
    seen: &std::sync::Mutex<Option<std::time::SystemTime>>,
) -> Option<Vec<String>> {
    let newest = files
        .iter()
        .filter_map(|f| std::fs::metadata(f).and_then(|m| m.modified()).ok())
        .max();
    let mut seen = seen.lock().unwrap_or_else(|e| e.into_inner());
    if newest.is_none() || std::mem::replace(&mut *seen, newest) == newest {
        return None;
    }
    drop(seen);
    Some(
        files
            .iter()
            .filter_map(|f| std::fs::read_to_string(f).ok())
            .collect(),
    )
}

/// The updates banner: a click on it (its `default` action) opens `software_update::OPEN`.
pub(crate) fn updates_banner(summary: String, body: String) -> Banner {
    let open = crate::software_update::OPEN
        .iter()
        .map(|a| (*a).to_string())
        .collect();
    banner(
        summary,
        body,
        false,
        vec![("default", "Open", Then::Run(open))],
    )
}

/// The sign-in banner: Open shows the page NetworkManager probes.
pub(crate) fn portal_banner(summary: String, body: String) -> Banner {
    banner(
        summary,
        body,
        false,
        vec![("open", "Open", Then::PortalPage)],
    )
}

/// The plugged-in banner: Open in Files shows the disk in the sidebar (QA-FDAY-13).
pub(crate) fn stick_banner(summary: String, body: String) -> Banner {
    banner(
        summary,
        body,
        false,
        vec![("open", "Open in Files", Then::Run(vec!["nautilus".into()]))],
    )
}

fn act(n: Notice) {
    let notify = |s: &str, body: &str, critical: bool| {
        let mut argv = vec!["notify-send".to_string(), "-a".into(), "WolOS".into()];
        if critical {
            argv.extend(["-u".into(), "critical".into()]);
        }
        argv.extend([s.to_string(), body.to_string()]);
        crate::kit::spawn(&argv);
    };
    match n {
        Notice::Show(s, body, critical) => notify(&s, &body, critical),
        Notice::Restart(s, body) => {
            // The action button asks first like every Restart (`wol power restart`).
            let restart = ["wol", "power", "restart"].map(String::from).to_vec();
            let label = wol_core::hardware::online::RESTART;
            show_banner(banner(
                s,
                body,
                false,
                vec![("restart", label, Then::Run(restart))],
            ));
        }
        Notice::Updates(s, body) => show_banner(updates_banner(s, body)),
        Notice::Portal(s, body) => show_banner(portal_banner(s, body)),
        Notice::Stick(s, body) => show_banner(stick_banner(s, body)),
        Notice::Malware(s, body, path) => {
            // Open Anyway is a deliberate step in Settings; a notification only offers the safe one.
            let trash = vec!["gio".into(), "trash".into(), "--".into(), path];
            let details = vec!["wol-settings".to_string()];
            show_banner(banner(
                s,
                body,
                true,
                vec![
                    ("trash", "Move to Trash", Then::Run(trash)),
                    ("details", "Details", Then::Run(details)),
                ],
            ));
        }
        Notice::Sleep(s, body) => {
            notify(&s, &body, true);
            crate::kit::spawn(&["wol".into(), "power".into(), "sleep".into()]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The notice comes once per plug: not for what was in at login, not again while it stays, and
    /// again after it was out.
    #[test]
    fn a_plugged_stick_is_told_once() {
        let stick = |d: &str, n: &str| Stick {
            dev: d.into(),
            name: n.into(),
        };
        let mut a = Alerts::default();
        assert!(
            a.sticks(vec![stick("sda", "Old")]).is_empty(),
            "in at login: not news"
        );
        assert!(a.sticks(vec![stick("sda", "Old")]).is_empty());
        let got = a.sticks(vec![
            stick("sda", "Old"),
            stick("sdb", "Kingston DataTraveler"),
        ]);
        assert_eq!(got.len(), 1, "{got:?}");
        let Notice::Stick(s, body) = &got[0] else {
            panic!("{got:?}")
        };
        assert_eq!(s, "Kingston DataTraveler Was Plugged In");
        assert_eq!(body, "Open Files to see what is on it.");
        assert!(
            a.sticks(vec![
                stick("sda", "Old"),
                stick("sdb", "Kingston DataTraveler")
            ])
            .is_empty()
        );
        assert!(
            a.sticks(vec![stick("sda", "Old")]).is_empty(),
            "unplugged: quiet"
        );
        assert_eq!(
            a.sticks(vec![stick("sda", "Old"), stick("sdb", "K")]).len(),
            1,
            "plugged again"
        );
        // A disk that does not say what it is.
        assert_eq!(crate::copy::stick_plugged("").0, "A Disk Was Plugged In");
        // The banner waits for the click on Open in Files, which opens Files (no shell string).
        let b = stick_banner("S".into(), "B".into());
        assert_eq!(
            b.notify,
            [
                "notify-send",
                "-a",
                "WolOS",
                "-w",
                "-A",
                "open=Open in Files",
                "S",
                "B"
            ]
        );
        assert_eq!(b.then, [("open", Then::Run(vec!["nautilus".to_string()]))]);
    }

    /// QA-NET2-3: the sign-in notice comes once, when the network turns to a portal.
    #[test]
    fn portal_notice_once() {
        use wol_core::netinfo::Connectivity::{Full, Limited, Portal};
        assert!(matches!(portal(Full, Portal), Some(Notice::Portal(..))));
        assert_eq!(portal(Portal, Portal), None);
        assert_eq!(portal(Portal, Full), None);
        assert_eq!(portal(Full, Limited), None);
        // Through the poll's state: told on the change, quiet while it stays, told again at the
        // next portal.
        let mut a = Alerts::default();
        assert!(a.connectivity(Portal).is_some());
        assert!(a.connectivity(Portal).is_none());
        assert!(a.connectivity(Full).is_none());
        assert!(a.connectivity(Portal).is_some());
        let b = portal_banner("T".into(), "B".into());
        assert_eq!(
            b.notify,
            [
                "notify-send",
                "-a",
                "WolOS",
                "-w",
                "-A",
                "open=Open",
                "T",
                "B"
            ]
        );
        // The page is the one NetworkManager probes (core netinfo the_portal_page_is_*).
        assert_eq!(b.then, [("open", Then::PortalPage)]);
    }

    #[test]
    fn each_stopped_file_is_told_once_and_nothing_otherwise() {
        let b = |p: &str, when| wol_core::malware::Blocked {
            path: p.into(),
            name: "Win.Test".into(),
            sha256: String::new(),
            when,
            uid: 1000,
        };
        let mut a = Alerts::default();
        assert!(a.malware(&[]).is_empty(), "nothing wrong, nothing shown");
        let first = a.malware(&[b("/home/a/x.exe", 5)]);
        assert_eq!(first.len(), 1);
        let Notice::Malware(s, body, path) = &first[0] else {
            panic!()
        };
        assert_eq!(s, "Malware Found");
        assert!(body.contains("x.exe") && body.contains("Trash"), "{body}");
        assert_eq!(path, "/home/a/x.exe");
        assert!(
            crate::copy::lint(body).is_empty(),
            "{:?}",
            crate::copy::lint(body)
        );
        assert!(a.malware(&[b("/home/a/x.exe", 5)]).is_empty(), "once");
        assert_eq!(
            a.malware(&[b("/home/a/y", 9), b("/home/a/x.exe", 5)]).len(),
            1
        );
    }

    /// Audit R7-1: what the background pass publishes for this account (owned by it, 0600, the
    /// way `wol-xguard sweep` writes it) is read and told once; unchanged, it is not read again.
    #[test]
    fn the_background_pass_list_is_read_and_told() {
        let d = std::env::temp_dir().join(format!("wol-alerts-sweep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let uid = wol_core::malware::my_uid();
        let hit = wol_core::malware::Blocked {
            path: "/home/a/evil.exe".into(),
            name: "Win.Test".into(),
            sha256: String::new(),
            when: 7,
            uid,
        };
        shared::atomic::write_as(
            &d.join(wol_core::malware::sweep_file(uid)),
            0o600,
            Some(uid),
            serde_json::to_string(&[&hit]).unwrap().as_bytes(),
        )
        .unwrap();
        let seen = std::sync::Mutex::new(None);
        let files = |u: u32| {
            [
                d.join(wol_core::malware::findings_file(u)),
                d.join(wol_core::malware::sweep_file(u)),
            ]
        };
        let texts = malware_read_in(files(uid), &seen).expect("a new list is read");
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        let shown = Alerts::default().malware(&wol_core::malware::merge(&refs));
        assert!(
            matches!(&shown[..], [Notice::Malware(_, _, p)] if p == "/home/a/evil.exe"),
            "{shown:?}"
        );
        assert!(
            malware_read_in(files(uid), &seen).is_none(),
            "unchanged: no read"
        );
        assert!(
            malware_read_in(files(uid + 1), &std::sync::Mutex::new(None)).is_none(),
            "another account's list is not this one's"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    fn bat(percent: u8, status: BatteryStatus) -> Option<Battery> {
        Some(Battery { percent, status })
    }

    #[test]
    fn battery_notices_once_per_level_then_sleep() {
        use BatteryStatus::*;
        let mut a = Alerts::default();
        let mut seen = Vec::new();
        for (p, s) in [
            (25, Discharging),
            (20, Discharging),
            (19, Discharging),
            (10, Discharging),
            (9, Discharging),
            (5, Discharging),
            (3, Discharging),
            (3, Discharging),
            (4, Charging),
            (20, Discharging),
        ] {
            seen.push(match a.battery(bat(p, s)) {
                Some(Notice::Show(_, body, critical)) => format!("{p}:show:{critical}:{body}"),
                Some(Notice::Sleep(..)) => format!("{p}:sleep"),
                Some(
                    Notice::Restart(..)
                    | Notice::Malware(..)
                    | Notice::Updates(..)
                    | Notice::Portal(..)
                    | Notice::Stick(..),
                ) => {
                    unreachable!()
                }
                None => format!("{p}:-"),
            });
        }
        assert_eq!(
            seen,
            [
                "25:-",
                "20:show:false:Your battery has 20% left. Connect to power, or turn on Low Power Mode.",
                "19:-",
                "10:show:false:Your battery has 10% left. Connect to power, or turn on Low Power Mode.",
                "9:-",
                "5:show:true:Your battery has 5% left. Connect to power, or turn on Low Power Mode.",
                "3:sleep",
                "3:-",
                "4:-",
                "20:show:false:Your battery has 20% left. Connect to power, or turn on Low Power Mode.",
            ]
        );
        assert_eq!(a.battery(None), None, "desktops have no battery");
    }

    #[test]
    fn full_disk_warns_once_until_space_is_back() {
        let mut a = Alerts::default();
        const G: u64 = 1_000_000_000;
        assert_eq!(a.disk(20 * G, 100 * G), None);
        let Some(Notice::Show(s, body, false)) = a.disk(4 * G, 100 * G) else {
            panic!()
        };
        assert_eq!(s, crate::copy::disk_full("4 GB").0);
        assert!(body.contains("4 GB"), "{body}");
        // QA-ACC-23: the Trash is rarely the big one; the alert points at what shows the real ones.
        assert!(body.contains("Settings > Storage"), "{body}");
        assert_eq!(a.disk(3 * G, 100 * G), None, "once");
        assert_eq!(a.disk(7 * G, 100 * G), None, "still low: no repeat");
        assert_eq!(a.disk(12 * G, 100 * G), None);
        assert!(a.disk(2 * G, 100 * G).is_some(), "full again: warn again");
        assert_eq!(a.disk(0, 0), None);
        let due: Vec<bool> = (0..=DISK_EVERY_POLLS).map(|_| a.disk_due()).collect();
        assert!(due[0] && due[DISK_EVERY_POLLS as usize] && !due[1]);
    }

    #[test]
    fn a_background_driver_install_notifies_once() {
        use shared::hardware::OnlineInstall;
        let r = |ok, graphics| OnlineInstall {
            at: 7,
            packages: vec!["nvidia-drm-kmod".into()],
            ok,
            graphics,
        };
        let mut a = Alerts::default();
        assert_eq!(a.online(None), None);
        let Some(Notice::Restart(s, body)) = a.online(Some(r(true, true))) else {
            panic!()
        };
        assert_eq!(s, "Graphics driver installed");
        assert!(body.starts_with("Restart"), "{body}");
        assert_eq!(a.online(Some(r(true, true))), None, "once");
        let mut b = Alerts::default();
        let Some(Notice::Show(_, body, false)) = b.online(Some(r(false, true))) else {
            panic!()
        };
        assert!(body.contains("Settings"), "{body}");
    }

    #[test]
    fn updates_notify_once_per_check() {
        let mut a = Alerts::default();
        let found = |t: &str, audit: &str| {
            Updates::parse(t, "curl 8.1-1 -> 8.2-1\nfirefox 1-1 -> 2-1\n", audit, "")
        };
        assert_eq!(a.updates(None, None), None, "never checked");
        let Some(Notice::Updates(s, body)) = a.updates(found("100", ""), None) else {
            panic!()
        };
        assert_eq!(s, "Updates Available");
        assert_eq!(
            body,
            "WolOS has 2 updates ready. Open Software Update to install them."
        );
        assert_eq!(a.updates(found("100", ""), None), None, "same check: once");
        let Some(Notice::Updates(_, body)) = a.updates(found("200", "curl"), None) else {
            panic!()
        };
        assert!(body.contains("including security fixes"), "{body}");
        assert_eq!(
            a.updates(Updates::parse("300", "", "", ""), None),
            None,
            "up to date"
        );
    }

    /// The check's `base` file (a signed release manifest).
    const BASE: &str = r#"{"version":"2026.11","snapshot":"2026/11/02","wolos_repo":"r42","kernel":"6.17.3-1","image":"wolos-2026.11.iso",
"sha256":"0000000000000000000000000000000000000000000000000000000000000000"}"#;

    /// QA-STORE-57: the release this computer runs is not an update, whatever the image's own
    /// `wolos-release.json` looks like, and a newer one still is.
    #[test]
    fn running_version_is_not_an_update() {
        let found = || Updates::parse("100", "", "", BASE);
        let running = r#"{"version":"2026.11","snapshot":"2026/11/02"}"#;
        let mut a = Alerts::default();
        assert_eq!(
            a.updates(found(), Some(running)),
            None,
            "first login: no banner"
        );
        assert_eq!(
            a.updates_shown, 0,
            "nothing was told, so nothing is remembered"
        );
        let older = running.replace("2026.11", "2026.10");
        let mut a = Alerts::default();
        let Some(Notice::Updates(_, body)) = a.updates(found(), Some(&older)) else {
            panic!("a newer release is an update")
        };
        assert!(body.contains("1 update"), "{body}");
    }

    /// Live-1 M1: a click on the banner opens Software Update (the notification server runs the
    /// `default` action when the banner is clicked, `ui/notify.rs::click_action`).
    #[test]
    fn update_notice_has_default_action() {
        let b = updates_banner(
            "Updates Available".into(),
            "WolOS has 1 update ready.".into(),
        );
        assert_eq!(
            b.notify,
            [
                "notify-send",
                "-a",
                "WolOS",
                "-w",
                "-A",
                "default=Open",
                "Updates Available",
                "WolOS has 1 update ready."
            ]
        );
        let open = ["wol-settings", "software-update"]
            .map(String::from)
            .to_vec();
        assert_eq!(b.then, [("default", Then::Run(open))]);
        // The banner side: a note with a `default` action is clicked through to it.
        let note = crate::ui::notify::Note {
            id: 1,
            app: "WolOS".into(),
            icon: String::new(),
            summary: "Updates Available".into(),
            body: String::new(),
            critical: false,
            timeout_ms: -1,
            image: String::new(),
            actions: vec![("default".into(), "Open".into())],
        };
        assert_eq!(crate::ui::notify::click_action(&note), Some("default"));
    }

    /// Live-1 M1: what was told stays told in the next session, and the banner opens the page.
    #[test]
    fn a_told_check_is_not_told_again_after_a_new_login() {
        let d = std::env::temp_dir().join(format!("wol-alerts-shown-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let found = || Updates::parse("100", "curl 8.1-1 -> 8.2-1\n", "", "");
        let mut first = Alerts::default();
        assert!(matches!(
            first.updates(found(), None),
            Some(Notice::Updates(..))
        ));
        persist_shown(&d, first.take_to_persist().expect("told, so to be kept"));
        assert_eq!(first.take_to_persist(), None, "written once");
        // The next session knows nothing until its first due read brings the file's value.
        let mut next_login = Alerts::default();
        let r = Reads {
            due: Some(DueReads {
                updates: found(),
                shown: load_shown(&d),
                ..Default::default()
            }),
            malware: None,
            ..Default::default()
        };
        assert!(
            notices(&mut next_login, r).is_empty(),
            "same check, new session"
        );
        let newer = Updates::parse("200", "curl 8.1-1 -> 8.2-1\n", "", "");
        assert!(
            next_login.updates(newer, None).is_some(),
            "a newer check is told"
        );
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(
            crate::software_update::OPEN,
            ["wol-settings", "software-update"],
            "the banner's click goes to the page"
        );
    }

    /// ST6-4: a plugin update is told once per version, again for a newer one, and an empty or
    /// absent read does not make the same version new again.
    #[test]
    fn plugin_updates_notify_once_per_version() {
        let n = |id: &str, name: &str, to: &str| PluginNews {
            id: id.into(),
            name: name.into(),
            to: to.into(),
        };
        let mut a = Alerts::default();
        assert_eq!(a.plugin_updates(vec![]), None);
        let Some(Notice::Show(s, body, false)) =
            a.plugin_updates(vec![n("alice.weather", "Weather", "1.3")])
        else {
            panic!()
        };
        assert_eq!(s, "Plugin Update Available");
        assert_eq!(
            body,
            "Weather has an update. Open the App Store, then Plugins, to install it."
        );
        assert_eq!(
            a.plugin_updates(vec![n("alice.weather", "Weather", "1.3")]),
            None,
            "same version: once"
        );
        // Another plugin: only it is named.
        let Some(Notice::Show(_, body, _)) = a.plugin_updates(vec![
            n("alice.weather", "Weather", "1.3"),
            n("bob.clock", "Clock", "2.0"),
        ]) else {
            panic!()
        };
        assert!(body.starts_with("Clock has"), "{body}");
        // A newer version of the first is news again.
        let Some(Notice::Show(_, body, _)) =
            a.plugin_updates(vec![n("alice.weather", "Weather", "1.4")])
        else {
            panic!()
        };
        assert!(body.contains("Weather"), "{body}");
        // Two at once: counted, not listed.
        let mut b = Alerts::default();
        let Some(Notice::Show(_, body, _)) = b.plugin_updates(vec![
            n("alice.weather", "Weather", "1.3"),
            n("bob.clock", "Clock", "2.0"),
        ]) else {
            panic!()
        };
        assert!(
            body.starts_with("Updates are ready for 2 plugins"),
            "{body}"
        );
        // Updated (gone from the list), then a new version: told again.
        assert_eq!(b.plugin_updates(vec![]), None);
        assert!(
            b.plugin_updates(vec![n("bob.clock", "Clock", "2.0")])
                .is_some()
        );
    }

    /// Audit4 S3-6: what the blocking read found becomes notices on the UI thread, in order;
    /// a poll between the due ones reads no disk.
    #[test]
    fn a_polls_reads_become_notices_in_order() {
        use shared::hardware::OnlineInstall;
        const G: u64 = 1_000_000_000;
        let mut a = Alerts::default();
        assert!(notices(&mut a, Reads::default()).is_empty());
        let r = Reads {
            due: Some(DueReads {
                updates: Updates::parse("100", "curl 8.1-1 -> 8.2-1\n", "", ""),
                running: None,
                shown: 0,
                plugins: Some(vec![PluginNews {
                    id: "alice.weather".into(),
                    name: "Weather".into(),
                    to: "1.3".into(),
                }]),
                online: Some(OnlineInstall {
                    at: 7,
                    packages: vec!["nvidia-drm-kmod".into()],
                    ok: true,
                    graphics: true,
                }),
                disk: Some((2 * G, 100 * G)),
            }),
            malware: Some(vec![]),
            ..Default::default()
        };
        let got = notices(&mut a, r);
        assert_eq!(got.len(), 4, "{got:?}");
        assert!(matches!(got[0], Notice::Restart(..)), "driver first");
        let Notice::Show(disk, ..) = &got[1] else {
            panic!("{got:?}")
        };
        assert_eq!(disk, &crate::copy::disk_full("2 GB").0);
        assert!(matches!(&got[2], Notice::Updates(s, _) if s == "Updates Available"));
        let Notice::Show(s, body, false) = &got[3] else {
            panic!("{got:?}")
        };
        assert_eq!(
            (s.as_str(), body.contains("Weather")),
            ("Plugin Update Available", true)
        );
        assert!(read(false).due.is_none(), "no df between the due polls");
    }

    #[test]
    fn alert_sentences_pass_the_copy_lint() {
        // Bodies are sentences; summaries are titles.
        let (_, b) = crate::copy::battery_low(10);
        let (_, db) = crate::copy::disk_full("2 GB");
        let (_, u1) = crate::copy::updates_ready(1, false);
        let (_, u2) = crate::copy::updates_ready(4, true);
        let (_, p1) = crate::copy::plugin_updates_ready(&["Weather"]);
        let (_, p2) = crate::copy::plugin_updates_ready(&["Weather", "Clock"]);
        for t in [
            b.as_str(),
            crate::copy::BATTERY_SLEEP.1,
            &db,
            &u1,
            &u2,
            &p1,
            &p2,
        ] {
            assert!(
                crate::copy::lint(t).is_empty(),
                "{t}: {:?}",
                crate::copy::lint(t)
            );
        }
    }
}
