// SPDX-License-Identifier: GPL-2.0-or-later
//! Event sources on the GTK main loop (no threads, no polling): the shell socket and the compositor's event socket.
use super::{Panels, dock::Dock, panels::Osd};
use crate::{hypr, protocol};
use gtk::{gio, glib, prelude::*};
use std::rc::Rc;

/// `$XDG_RUNTIME_DIR/wol-shell.sock`: `toggle|show|hide <panel>`, `reload`, `approve <json>`.
/// Surfaces with their own command semantics (e.g. the switcher); true = handled.
pub type Handlers = Vec<Rc<dyn Fn(&protocol::Command) -> bool>>;
/// Takes an `approve` request (JSON, verbatim), the connection to answer on and the requester's
/// executable as the kernel reported it at accept (None for root wold, which names the app
/// itself) (`ui::approval`).
pub type Approver = Rc<dyn Fn(String, gio::SocketConnection, Option<String>)>;

/// Seconds a shell-socket client may stay silent before its connection is dropped.
pub const CLIENT_TIMEOUT_SECS: u64 = 10;
/// Bytes one shell-socket connection may send in total (commands are short; `approve` carries
/// one JSON request).
pub const CLIENT_QUOTA: u64 = 4 * shared::portal::MAX_LINE as u64;

/// Next bounded UTF-8 line of a shell-socket client, newline stripped. None at EOF, after
/// [`CLIENT_TIMEOUT_SECS`] of silence, or when the client broke a bound: the caller drops it.
async fn next_line(
    input: &gio::InputStream,
    split: &mut shared::bounded::LineSplitter,
) -> Option<String> {
    loop {
        match split.next_line() {
            Some(Ok(line)) => return String::from_utf8(line).ok(),
            Some(Err(_)) => return None,
            None => {}
        }
        let chunk = glib::future_with_timeout(
            std::time::Duration::from_secs(CLIENT_TIMEOUT_SECS),
            input.read_bytes_future(4096, glib::Priority::DEFAULT),
        )
        .await
        .ok()?
        .ok()?;
        if chunk.is_empty() {
            // EOF: an unterminated last command still counts.
            let rest = split.rest();
            return (!rest.is_empty())
                .then(|| String::from_utf8(rest).ok())
                .flatten();
        }
        split.push(&chunk).ok()?;
    }
}

pub fn shell_socket(
    panels: Panels,
    handlers: Handlers,
    approver: Approver,
    osd: Rc<Osd>,
    sys: Rc<dyn crate::system::System>,
) {
    // Saved config files apply on their own (the verb `reload` stays the explicit way).
    watch_prefs();
    let handlers = Rc::new(handlers);
    let path = protocol::socket_path(&crate::kit::runtime_dir());
    let _ = std::fs::remove_file(&path);
    let listener = gio::SocketListener::new();
    if let Err(e) = listener.add_address(
        &gio::UnixSocketAddress::new(&path),
        gio::SocketType::Stream,
        gio::SocketProtocol::Default,
        None::<&glib::Object>,
    ) {
        eprintln!("wol-shell: {}: {e}", path.display());
        return;
    }
    {
        // The shell's own surfaces (menu bar status icons) run commands without a process.
        let (panels, osd, sys, handlers) =
            (panels.clone(), osd.clone(), sys.clone(), handlers.clone());
        LOCAL.set(Some(Rc::new(move |cmd: &protocol::Command| {
            if !handlers.iter().any(|h| h(cmd)) {
                run(cmd, &panels, &osd, &sys);
            }
        })));
    }
    glib::spawn_future_local(async move {
        while let Ok((conn, _)) = listener.accept_future().await {
            let (panels, osd, sys, handlers, approver) = (
                panels.clone(),
                osd.clone(),
                sys.clone(),
                handlers.clone(),
                approver.clone(),
            );
            // Who connected, read now (STANDARDS 7.1 B1): `/proc/<pid>` is pinned as a
            // descriptor at accept and the executable is only ever read through it (R2C-12), so a
            // peer that exits and whose pid is reused is never named as the newcomer, and one
            // that execs into another binary before it sends `approve` is refused (audit3 R2A-8).
            let pinned = peer_of(&conn).map(|(uid, pid)| Pinned::capture(uid, pid));
            let at_accept = pinned.as_ref().map(|p| (p.uid, p.pid, p.exe.clone()));
            glib::spawn_future_local(async move {
                let input = conn.input_stream();
                // A client that connects and says nothing is dropped after a while (it would
                // otherwise hold its stream for the session); a line is at most MAX_LINE bytes
                // and a connection CLIENT_QUOTA bytes, or the client is dropped (audit A17).
                let mut split =
                    shared::bounded::LineSplitter::new(shared::portal::MAX_LINE, CLIENT_QUOTA);
                while let Some(line) = next_line(&input, &mut split).await {
                    let Some(cmd) = protocol::parse(&line) else {
                        eprintln!("wol-shell: bad command '{line}'");
                        continue;
                    };
                    // One request per connection; the answer goes back on it. Never logged (a
                    // Wi-Fi password may be in it).
                    if cmd.verb == protocol::Verb::Approve {
                        // Who asks comes from the kernel, never the message (audit A23).
                        let peer = peer_of(&conn);
                        let exe = |_: i32| pinned.as_ref().and_then(Pinned::exe_now);
                        let same = still_same(at_accept.as_ref(), peer, exe);
                        if same && may_ask(peer, shared::sys::euid(), &cmd.target, exe) {
                            let requester = at_accept
                                .as_ref()
                                .filter(|a| a.0 != 0)
                                .and_then(|a| a.2.clone());
                            approver(cmd.target, conn.clone(), requester);
                        } else {
                            // R2C-6: an answer, not a silent close, so the requester says why.
                            eprintln!("wol-shell: refused an approval from an unexpected peer");
                            let out = conn.output_stream();
                            let deny = shared::shell::Answer::Deny.line().into_bytes();
                            let _ = out.write_all_future(deny, glib::Priority::DEFAULT).await;
                        }
                        break;
                    }
                    if !handlers.iter().any(|h| h(&cmd)) {
                        run(&cmd, &panels, &osd, &sys);
                    }
                }
            });
        }
    });
}

/// Runs one shell command in this process.
type Local = Rc<dyn Fn(&protocol::Command)>;

thread_local! {
    /// In-process entry to [`run`], set by [`shell_socket`].
    static LOCAL: std::cell::RefCell<Option<Local>> = Default::default();
}

/// Runs `cmd` as if it came in on the shell socket (no `wol` process); nothing before the socket
/// is up (golden renders, dev windows).
pub fn local(cmd: protocol::Command) {
    if let Some(f) = LOCAL.with_borrow(|l| l.clone()) {
        f(&cmd);
    }
}

/// `control-center:<page>` -> the page (audit2 A2-14). The plain `control-center` is None.
pub fn cc_page(target: &str) -> Option<&str> {
    target.strip_prefix("control-center:")
}

/// What `<verb> control-center:<page>` does to the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CcAction {
    /// Closed: open it on the page.
    Open,
    /// Open on another page: slide to the page and stay open (it switches to the clicked
    /// item; audit3 R2A-2, a click on Wi-Fi closed the panel).
    SwitchTo,
    Close,
    Nothing,
}

/// [`CcAction`] for `verb` on `page`, with the panel `visible` on `current`.
pub fn cc_action(
    verb: protocol::Verb,
    visible: bool,
    current: Option<&str>,
    page: &str,
) -> CcAction {
    use protocol::Verb;
    match (verb, visible) {
        (Verb::Hide, true) => CcAction::Close,
        (Verb::Show | Verb::Toggle, false) => CcAction::Open,
        (Verb::Show | Verb::Toggle, true) if current != Some(page) => CcAction::SwitchTo,
        (Verb::Toggle, true) => CcAction::Close,
        _ => CcAction::Nothing,
    }
}

/// The kernel's account of a shell-socket peer: (uid, pid).
fn peer_of(conn: &gio::SocketConnection) -> Option<(u32, i32)> {
    let c = conn.socket().credentials().ok()?;
    Some((c.unix_user().ok()?, c.unix_pid().ok()?))
}

/// The peer asking now is the one that connected: same uid and pid, and the pid still runs the
/// executable it ran at accept.
fn still_same(
    at_accept: Option<&(u32, i32, Option<String>)>,
    now: Option<(u32, i32)>,
    exe: impl Fn(i32) -> Option<String>,
) -> bool {
    match (at_accept, now) {
        (Some((uid, pid, Some(path))), Some((u, p))) => {
            (*uid, *pid) == (u, p) && exe(p).as_deref() == Some(path.as_str())
        }
        _ => false,
    }
}

/// A shell-socket peer as the kernel reported it at accept, with `/proc/<pid>` opened right then
/// (the `api::peer` pattern, R2C-12): reads go through the descriptor, never through the pid
/// number again, so a reused pid reads nothing instead of describing the newcomer.
struct Pinned {
    uid: u32,
    pid: i32,
    dir: Option<std::fs::File>,
    exe: Option<String>,
}

impl Pinned {
    fn capture(uid: u32, pid: i32) -> Self {
        let dir = (pid > 0)
            .then(|| std::fs::File::open(format!("/proc/{pid}")).ok())
            .flatten();
        let exe = dir.as_ref().and_then(exe_through);
        Self { uid, pid, dir, exe }
    }

    /// The pinned process's executable now; None once it is gone.
    fn exe_now(&self) -> Option<String> {
        self.dir.as_ref().and_then(exe_through)
    }
}

/// Canonical executable of the process a `/proc/<pid>` descriptor pins, read through
/// `/proc/self/fd`; None if it is gone or not readable.
fn exe_through(dir: &std::fs::File) -> Option<String> {
    use std::os::fd::AsRawFd;
    let link = format!("/proc/self/fd/{}/exe", dir.as_raw_fd());
    let s = std::fs::read_link(link)
        .ok()?
        .into_os_string()
        .into_string()
        .ok()?;
    (s.len() < 4096).then_some(s)
}

/// An `approve` line is taken only when `super::approval::may_approve` allows its kernel-reported
/// peer (uid, pid -> executable) to ask for that request; an unparsable request is refused too.
fn may_ask(
    peer: Option<(u32, i32)>,
    own_uid: u32,
    json: &str,
    exe: impl Fn(i32) -> Option<String>,
) -> bool {
    let Some((uid, pid)) = peer else {
        return false;
    };
    let (Some(path), Some(a)) = (exe(pid), shared::shell::Approval::parse(json)) else {
        return false;
    };
    super::approval::may_approve(uid, own_uid, &path, &a.request)
}

thread_local! {
    /// Hooks with the lifetime they end with (None: the session).
    static RELOAD: std::cell::RefCell<Vec<(Option<crate::kit::lifetime::Lifetime>, OnReload)>> =
        Default::default();
    /// `shell.json`'s prefs as last loaded: a save that does not load keeps them.
    static GOOD: std::cell::Cell<Option<ShellPrefs>> = const { std::cell::Cell::new(None) };
    /// The notice text for each config file that does not load now: one bad save, one notice.
    static PROBLEMS: std::cell::RefCell<std::collections::BTreeMap<&'static str, String>> =
        Default::default();
    /// The live watch on the config folder (kept for the process).
    static WATCH: std::cell::RefCell<Option<gio::FileMonitor>> = const { std::cell::RefCell::new(None) };
}
use wol_core::prefs::{ConfigError, ShellFile, ShellPrefs};
type OnReload = Rc<dyn Fn(&ShellPrefs)>;

fn config_text(rel: &str) -> Option<String> {
    std::fs::read_to_string(std::path::Path::new(&crate::kit::config_home()).join(rel)).ok()
}

/// `shell.json` now; a missing file is the defaults.
fn load_shell() -> Result<ShellPrefs, ConfigError> {
    config_text(wol_core::prefs::SHELL_FILE).map_or(Ok(ShellPrefs::default()), |t| {
        ShellFile::try_parse(&t).map(|f| f.prefs)
    })
}

/// `$XDG_CONFIG_HOME/wolos/shell.json` (written by Settings > Menu Bar & Dock). A file that does
/// not load gives the last prefs that did (the defaults before any).
pub fn shell_prefs() -> ShellPrefs {
    match load_shell() {
        Ok(p) => {
            GOOD.set(Some(p));
            p
        }
        Err(_) => GOOD.get().unwrap_or_default(),
    }
}

/// Calls `f` with the current prefs now and after every `reload`.
pub fn on_reload(f: impl Fn(&ShellPrefs) + 'static) {
    f(&shell_prefs());
    RELOAD.with(|r| r.borrow_mut().push((None, Rc::new(f))));
}

/// [`on_reload`] until `life` ends (a surface that goes with its display).
pub fn on_reload_while(life: &crate::kit::lifetime::Lifetime, f: impl Fn(&ShellPrefs) + 'static) {
    if !life.alive() {
        return;
    }
    f(&shell_prefs());
    RELOAD.with(|r| r.borrow_mut().push((Some(life.clone()), Rc::new(f))));
    life.on_end(|| {
        RELOAD.with(|r| {
            r.borrow_mut()
                .retain(|h| h.0.as_ref().is_none_or(|l| l.alive()))
        });
    });
}

/// Reload hooks registered now (the leak test counts them).
pub fn reload_hooks() -> usize {
    RELOAD.with(|r| r.borrow().len())
}

/// `reload`, and a save [`watch_prefs`] saw: every hook gets `shell.json`'s prefs. When
/// `shell.json` or `widgets.json` does not load, no hook runs (what shows stays) and the error
/// names the file and its first bad key.
pub fn reload() -> Result<(), ConfigError> {
    if let Some(e) = config_text(shared::widgets::DESKTOP_FILE)
        .as_deref()
        .and_then(wol_core::prefs::widgets_error)
    {
        return Err(e);
    }
    let p = load_shell()?;
    GOOD.set(Some(p));
    // A hook may add or end others while it runs: call a snapshot.
    let hooks: Vec<OnReload> = RELOAD.with(|r| r.borrow().iter().map(|h| h.1.clone()).collect());
    hooks.iter().for_each(|f| f(&p));
    Ok(())
}

/// Files the live watch applies, under `$XDG_CONFIG_HOME/wolos/` (frozen names).
pub const WATCHED: [&str; 3] = [
    wol_core::prefs::SHELL_NAME,
    wol_core::prefs::WIDGETS_NAME,
    crate::kit::style::USER_CSS_NAME,
];
/// Quiet time after the last change before a save applies (an editor writes in bursts).
pub const DEBOUNCE_MS: u64 = 100;
/// Summary of the notice for a config file that does not load.
pub const NOT_APPLIED: &str = "Changes Not Applied";

/// The notice body for `e`: which setting, never the reader's words.
pub fn notice_text(e: &ConfigError) -> String {
    let what = match e.file {
        wol_core::prefs::SHELL_NAME => "menu bar and Dock settings",
        wol_core::prefs::WIDGETS_NAME => "desktop widgets",
        _ => "style sheet",
    };
    match (&e.key, e.line) {
        (Some(k), Some(l)) => {
            format!(
                "“{k}” on line {l} of your {what} doesn’t fit, so nothing changed. Check it and save again."
            )
        }
        (Some(k), None) => {
            format!(
                "“{k}” in your {what} doesn’t fit, so nothing changed. Check it and save again."
            )
        }
        (None, l) => format!(
            "{} can’t be read near line {}, so nothing changed. Check it and save again.",
            format_args!("Your {what}"),
            l.unwrap_or(1)
        ),
    }
}

/// The notices standing now (one per config file that does not load), for tests and logs.
pub fn problems() -> Vec<String> {
    PROBLEMS.with_borrow(|p| p.values().cloned().collect())
}

/// Records how applying `files` went: a new error shows one notice (the same error again does
/// not), a file that loads again clears its notice.
fn report(files: &[&'static str], r: Result<(), ConfigError>) {
    let shown = match r {
        Ok(()) => {
            PROBLEMS.with_borrow_mut(|p| {
                files.iter().for_each(|f| {
                    p.remove(f);
                })
            });
            return;
        }
        Err(e) => {
            eprintln!("wol-shell: not applied: {e}");
            let text = notice_text(&e);
            PROBLEMS.with_borrow_mut(|p| {
                files.iter().filter(|f| **f != e.file).for_each(|f| {
                    p.remove(f);
                });
                (p.insert(e.file, text.clone()).as_ref() != Some(&text)).then_some(text)
            })
        }
    };
    if let Some(body) = shown {
        crate::kit::spawn(&[
            "notify-send".into(),
            "-a".into(),
            "WolOS".into(),
            NOT_APPLIED.into(),
            body,
        ]);
    }
}

/// [`WATCHED`] index of the file an event of the folder watch is about. None for other files,
/// and for this process's own atomic saves (`<name>.<pid>-<nonce>.tmp` renamed over the file):
/// what the shell wrote it already shows.
pub fn touched(name: &str, renamed_to: Option<&str>, pid: u32) -> Option<usize> {
    if let Some(to) = renamed_to
        && let Some(i) = WATCHED.iter().position(|w| *w == to)
    {
        let own = name
            .strip_prefix(to)
            .and_then(|r| r.strip_prefix('.'))
            .is_some_and(|r| r.starts_with(&format!("{pid}-")));
        return (!own).then_some(i);
    }
    WATCHED.iter().position(|w| *w == name)
}

/// Applies [`WATCHED`] saves on their own, [`DEBOUNCE_MS`] after the last change: `shell.json`
/// and `widgets.json` through [`reload`], `kit.css` through `kit::style::user_css`. One
/// directory watch (it sees editors' rename saves and a file created later); no polling. Again
/// = the watch starts over.
pub fn watch_prefs() {
    let dir = std::path::Path::new(&crate::kit::config_home()).join("wolos");
    let _ = std::fs::create_dir_all(&dir);
    let m = match gio::File::for_path(&dir)
        .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
    {
        Ok(m) => m,
        Err(e) => return eprintln!("wol-shell: {}: no live reload: {e}", dir.display()),
    };
    m.set_rate_limit(i32::try_from(DEBOUNCE_MS).unwrap_or(i32::MAX));
    let due: Rc<std::cell::RefCell<(Option<glib::SourceId>, [bool; WATCHED.len()])>> =
        Rc::default();
    let pid = std::process::id();
    m.connect_changed(move |_, file, other, ev| {
        use gio::FileMonitorEvent as E;
        if !matches!(
            ev,
            E::Changed
                | E::ChangesDoneHint
                | E::Created
                | E::Deleted
                | E::MovedIn
                | E::MovedOut
                | E::Renamed
        ) {
            return;
        }
        let name = |f: &gio::File| f.basename().and_then(|b| b.to_str().map(String::from));
        let to = other.filter(|_| ev == E::Renamed).and_then(name);
        let Some(i) = name(file).and_then(|n| touched(&n, to.as_deref(), pid)) else {
            return;
        };
        let mut d = due.borrow_mut();
        d.1[i] = true;
        if let Some(s) = d.0.take() {
            s.remove();
        }
        let due = due.clone();
        d.0 = Some(glib::timeout_add_local_once(
            std::time::Duration::from_millis(DEBOUNCE_MS),
            move || {
                let which = {
                    let mut d = due.borrow_mut();
                    d.0 = None;
                    std::mem::take(&mut d.1)
                };
                if which[2] {
                    let css = config_text(crate::kit::style::USER_CSS_FILE).unwrap_or_default();
                    report(&[WATCHED[2]], crate::kit::style::user_css(&css));
                }
                if which[0] || which[1] {
                    report(&WATCHED[..2], reload());
                }
            },
        ));
    });
    WATCH.with(|w| w.replace(Some(m)));
}

fn run(
    cmd: &protocol::Command,
    panels: &Panels,
    osd: &Rc<Osd>,
    sys: &Rc<dyn crate::system::System>,
) {
    if cmd.verb == protocol::Verb::Reload {
        // Theme reloads through its file monitor; shell prefs here.
        return report(&WATCHED[..2], reload());
    }
    if super::transient::closes_all(cmd) {
        return super::transient::close_on_escape(panels);
    }
    // `show session-logout|session-shutdown` (WolOS menu): the session panel on its question.
    if let Some(action) = super::panels::session_action(&cmd.target) {
        if let Some(w) = panels.borrow().get("session") {
            w.set_visible(true);
        }
        return super::panels::ask_session(action);
    }
    if matches!(cmd.target.as_str(), "osd" | "osd-mic") && cmd.verb == protocol::Verb::Show {
        // Sent by wol-volume after a change: read the sink and the source now, then show the new
        // level (`osd-mic` after the mic-mute key: the microphone's state; audit5 S9-3).
        let (done, osd, sys) = (sys.refresh_audio(), osd.clone(), sys.clone());
        let mic = cmd.target == "osd-mic";
        glib::spawn_future_local(async move {
            let _ = done.recv().await;
            let (icon, level) = osd_view(mic, sys.audio());
            osd.show(icon, level);
        });
        return;
    }
    if let Some(page) = cc_page(&cmd.target) {
        let Some(w) = panels.borrow().get("control-center").cloned() else {
            return;
        };
        let current = super::control_center::current_page(&w);
        match cc_action(cmd.verb, w.is_visible(), current.as_deref(), page) {
            CcAction::Open => {
                // Taken by the panel's show handler: it opens on `page`, not the main page.
                super::control_center::request_page(page);
                w.set_visible(true);
            }
            CcAction::SwitchTo => super::control_center::switch_page(&w, page),
            CcAction::Close => w.set_visible(false),
            CcAction::Nothing => {}
        }
        return;
    }
    match panels.borrow().get(cmd.target.as_str()) {
        Some(w) => w.set_visible(protocol::apply(cmd.verb, w.is_visible())),
        None => eprintln!("wol-shell: no panel '{}'", cmd.target),
    }
}

/// Reconnect delay after `failures` consecutive drops of the compositor event socket: 1 s, 2 s,
/// 4 s … capped at 30 s (a `hyprctl reload` or a compositor restart must not cost the dock its
/// running dots for the session).
pub fn backoff(failures: u32) -> std::time::Duration {
    std::time::Duration::from_secs(1u64 << failures.min(5)).min(std::time::Duration::from_secs(30))
}

/// One line of an event socket, lossily decoded (a window title need not be UTF-8); None at EOF
/// or on an error.
async fn read_line(input: &gio::DataInputStream) -> Option<String> {
    let bytes = input
        .read_line_future(glib::Priority::DEFAULT_IDLE)
        .await
        .ok()??;
    if bytes.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// How long the shared list of installed apps is used before it is read again (in the background,
/// the old list answering meanwhile): an app installed a moment ago gets its name on the next look.
pub const INSTALLED_TTL: std::time::Duration = std::time::Duration::from_secs(10);

/// What [`installed`] does with the cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fetch {
    /// Never read: read it now and wait.
    Wait,
    /// Never read, and the first read is under way: wait for it (audit3 R2A-7: two first
    /// callers each read every file).
    Join,
    /// Old: answer with it and read it again in the background.
    Refresh,
    Use,
}

fn fetch(loaded_at: Option<std::time::Instant>, now: std::time::Instant, loading: bool) -> Fetch {
    match loaded_at {
        None if loading => Fetch::Join,
        None => Fetch::Wait,
        Some(t) if !loading && now.saturating_duration_since(t) >= INSTALLED_TTL => Fetch::Refresh,
        Some(_) => Fetch::Use,
    }
}

#[derive(Default)]
struct Installed {
    list: Rc<Vec<crate::desktop::Entry>>,
    loaded_at: Option<std::time::Instant>,
    loading: bool,
}

thread_local! {
    /// The .desktop entries, shared by the menu bar, the switcher, Force Quit and About (each
    /// read every file on the UI thread per open, audit2 A2-10).
    static INSTALLED: std::cell::RefCell<Installed> = Default::default();
}

async fn load_installed() -> Rc<Vec<crate::desktop::Entry>> {
    INSTALLED.with_borrow_mut(|i| i.loading = true);
    let list = Rc::new(
        gio::spawn_blocking(crate::desktop::load_all)
            .await
            .unwrap_or_default(),
    );
    INSTALLED.with_borrow_mut(|i| {
        *i = Installed {
            list: list.clone(),
            loaded_at: Some(std::time::Instant::now()),
            loading: false,
        }
    });
    list
}

/// The installed apps, read off the UI thread: the first call waits for the read, later ones get
/// the shared list at once (re-read in the background after [`INSTALLED_TTL`]).
pub async fn installed() -> Rc<Vec<crate::desktop::Entry>> {
    let (at, loading, list) = INSTALLED.with_borrow(|i| (i.loaded_at, i.loading, i.list.clone()));
    match fetch(at, std::time::Instant::now(), loading) {
        Fetch::Wait => load_installed().await,
        Fetch::Join => {
            while INSTALLED.with_borrow(|i| i.loading && i.loaded_at.is_none()) {
                glib::timeout_future(std::time::Duration::from_millis(20)).await;
            }
            INSTALLED.with_borrow(|i| i.list.clone())
        }
        Fetch::Refresh => {
            glib::spawn_future_local(async {
                load_installed().await;
            });
            list
        }
        Fetch::Use => list,
    }
}

/// The menu bar reads the shared list directly (no await per focus change): an old one is read
/// again in the background, so an app installed after login gets its name (audit3 R2A-7).
fn refresh_installed() {
    let (at, loading) = INSTALLED.with_borrow(|i| (i.loaded_at, i.loading));
    if fetch(at, std::time::Instant::now(), loading) == Fetch::Refresh {
        glib::spawn_future_local(async {
            load_installed().await;
        });
    }
}

/// Frontmost app -> every menu bar.
type Show = Rc<dyn Fn(&str, &str)>;

/// After `activewindow>>,` (a layer took the keyboard, or the last window closed): what the menu
/// bar shows, from `j/activewindow` and whether a WolOS layer holds the keys (`j/layers`,
/// `appmenu::panel_has_keys`). None keeps the name: the switcher, Overview, About, Search and
/// Force Quit blanked the name of the app they were about (audit2 A2-7).
fn after_blank(active: Option<(String, String)>, panel_has_keys: bool) -> Option<(String, String)> {
    match active {
        Some((c, t)) if !c.is_empty() => Some((c, t)),
        _ if panel_has_keys => None,
        _ => Some((String::new(), String::new())),
    }
}

/// Compositor `.socket2.sock`: active window -> menu bar name; open/close -> dock running dots;
/// another app in front or a workspace switch closes the transient panels. Reconnects with
/// [`backoff`] and reseeds from `j/activewindow` + `j/clients` (off the UI thread) each time.
pub fn hypr_events(dock: Rc<Dock>, panels: Panels) {
    let Some(path) = hypr::socket(".socket2.sock") else {
        return;
    };
    glib::spawn_future_local(async move {
        installed().await;
        let show: Show = Rc::new(move |class: &str, title: &str| {
            refresh_installed();
            let installed = INSTALLED.with_borrow(|i| i.list.clone());
            let name = crate::apps::frontmost_name(class, title, crate::apps::here(), &installed);
            super::menubar::set_app_all(name, class.to_string());
        });
        let mut failures = 0u32;
        loop {
            let addr = gio::UnixSocketAddress::new(&path);
            match gio::SocketClient::new().connect_future(&addr).await {
                Ok(conn) => {
                    // Events only report changes: start from what is focused and open now.
                    reseed(&dock, &show).await;
                    let input = gio::DataInputStream::new(&conn.input_stream());
                    while let Some(line) = read_line(&input).await {
                        failures = 0;
                        handle(&line, &dock, &panels, &show).await;
                    }
                    eprintln!("wol-shell: compositor event socket closed, reconnecting");
                }
                Err(e) => eprintln!("wol-shell: compositor event socket: {e}"),
            }
            glib::timeout_future(backoff(failures)).await;
            failures = failures.saturating_add(1);
        }
    });
}

/// The focused window and the running classes now (`.socket.sock`, off the UI thread).
async fn reseed(dock: &Dock, show: &Show) {
    let active = gio::spawn_blocking(|| hypr::request("j/activewindow"))
        .await
        .ok()
        .flatten();
    if let Some(j) = &active {
        super::transient::seed_active(hypr::parse_active_client(j).map(|c| c.address).as_deref());
        if let Some((class, title)) = hypr::parse_active(j) {
            show(&class, &title);
        }
    }
    let clients = gio::spawn_blocking(hypr::clients).await.unwrap_or_default();
    dock.set_running(&hypr::running_classes(&clients));
    refresh_fullscreen().await;
}

/// `hypr::fullscreen_state` from the compositor now (off the UI thread): banners hold while the active
/// workspace is in full screen (audit3 R2A-9); the volume OSD is not held, it shows over it
/// (brief, like a HUD; decision audit5 S6-3). Each display's mode too, for its desktop
/// widgets (audit5 S6-2).
async fn refresh_fullscreen() {
    let (mode, per) =
        gio::spawn_blocking(|| (hypr::read_fullscreen(), hypr::read_monitor_fullscreen()))
            .await
            .unwrap_or_default();
    hypr::set_monitor_fullscreen(per);
    hypr::set_fullscreen_state(mode);
}

/// The OSD's icon and level for `show osd` (the speaker) or `show osd-mic` (`mic`: the
/// microphone, at 0 while muted, so the mute key is seen to have worked).
pub fn osd_view(mic: bool, a: crate::status::Audio) -> (&'static str, u8) {
    if !mic {
        return (crate::status::volume(a), a.volume.unwrap_or(0));
    }
    let icon = crate::status::mic(a).unwrap_or("microphone-sensitivity-muted-symbolic");
    let level = if a.mic_muted { 0 } else { a.mic.unwrap_or(0) };
    (icon, level)
}

/// One event line.
async fn handle(line: &str, dock: &Rc<Dock>, panels: &Panels, show: &Show) {
    if super::transient::feed(line) {
        super::transient::close_all(panels);
    }
    if hypr::fullscreen_may_change(line) {
        refresh_fullscreen().await;
    }
    match hypr::parse_event(line) {
        Some(hypr::Event::ActiveWindow { class, title }) => {
            if !class.is_empty() {
                dock.focused(&class);
                return show(&class, &title);
            }
            // `activewindow>>,`: a layer surface took the keyboard (a shell panel, Search) or the
            // last window closed. The bar keeps its app unless no window has the focus any more
            // (live: the name blanked every time the Control Center opened).
            if super::transient::any_open(panels) {
                return;
            }
            let (active, layers) = gio::spawn_blocking(|| {
                let active = hypr::request("j/activewindow");
                let layers = hypr::request("j/layers");
                (active, layers)
            })
            .await
            .unwrap_or_default();
            let keys =
                layers.is_some_and(|j| crate::appmenu::panel_has_keys(&hypr::parse_layers(&j)));
            if let Some((c, t)) = after_blank(active.as_deref().and_then(hypr::parse_active), keys)
            {
                show(&c, &t);
            }
        }
        Some(hypr::Event::Urgent { address }) => {
            let clients = gio::spawn_blocking(hypr::clients).await.unwrap_or_default();
            let focused = |c: &hypr::Client| c.focus == 0;
            if let Some(c) = clients.iter().find(|c| c.address == address && !focused(c)) {
                dock.attention(&c.class);
            }
        }
        Some(hypr::Event::FocusedMonitor(m)) => super::menubar::set_focused(&m),
        // A config reload drops every `keyword` bind (audit5 S9-4): the fallback Esc and Super
        // binds (when the key plugin is not loaded) and all plugin hotkeys go in again.
        Some(hypr::Event::ConfigReloaded) => {
            let _ = gio::spawn_blocking(hypr::ensure_key_forwarding).await;
            crate::hypr::config_reloaded();
        }
        Some(hypr::Event::OpenWindow { .. } | hypr::Event::CloseWindow { .. }) => {
            let clients = gio::spawn_blocking(hypr::clients).await.unwrap_or_default();
            dock.set_running(&hypr::running_classes(&clients));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::shell::{Approval, ApprovalRequest};

    /// Audit5 S9-3: the mic-mute key shows the microphone, muted, not the speaker level.
    #[test]
    fn the_mic_osd_shows_the_microphone_state() {
        use crate::status::Audio;
        let a = Audio {
            volume: Some(80),
            muted: false,
            mic: Some(50),
            mic_muted: true,
        };
        assert_eq!(
            osd_view(true, a),
            ("microphone-sensitivity-muted-symbolic", 0)
        );
        let live = Audio {
            mic_muted: false,
            ..a
        };
        assert_eq!(
            osd_view(true, live),
            ("microphone-sensitivity-high-symbolic", 50)
        );
        assert_eq!(osd_view(false, a), (crate::status::volume(a), 80));
        assert!(
            shared::shell::PANELS.contains(&"osd-mic"),
            "the socket accepts it"
        );
    }

    fn json(request: ApprovalRequest) -> String {
        serde_json::to_string(&Approval {
            helper: "x".into(),
            tool: "t".into(),
            request,
        })
        .unwrap()
    }

    /// Audit A23: an approval is taken by the kernel's account of the peer, not the message.
    #[test]
    fn approvals_come_only_from_wold_or_the_own_account_for_user_changes() {
        let admin = json(ApprovalRequest::Packages {
            verb: "install".into(),
            names: vec!["vim".into()],
        });
        let user = json(ApprovalRequest::Change {
            capability: "appearance".into(),
            what: "switch to Dark appearance".into(),
        });
        let wold = |_| Some("/usr/bin/wold".to_owned());
        let other = |_| Some("/tmp/evil".to_owned());
        assert!(may_ask(Some((0, 1)), 1000, &admin, wold));
        assert!(
            !may_ask(Some((0, 1)), 1000, &admin, other),
            "root, not wold"
        );
        assert!(
            !may_ask(Some((1000, 1)), 1000, &admin, wold),
            "user asks admin"
        );
        assert!(may_ask(Some((1000, 1)), 1000, &user, other));
        assert!(
            !may_ask(Some((1001, 1)), 1000, &user, other),
            "another account"
        );
        assert!(!may_ask(None, 1000, &user, wold), "no credentials");
        assert!(!may_ask(Some((0, 1)), 1000, &admin, |_| None), "peer gone");
        assert!(!may_ask(Some((0, 1)), 1000, "{bad", wold));
    }

    /// Audit3 R2A-8: the peer is captured at accept; one that execs into another binary (or
    /// whose pid changed) before `approve` is refused.
    #[test]
    fn an_approval_peer_that_changed_since_accept_is_refused() {
        let at = (1000, 7, Some("/usr/bin/wol-settings".to_owned()));
        let same = |_| Some("/usr/bin/wol-settings".to_owned());
        let execd = |_| Some("/usr/bin/wold".to_owned());
        assert!(still_same(Some(&at), Some((1000, 7)), same));
        assert!(
            !still_same(Some(&at), Some((1000, 7)), execd),
            "exec'd since"
        );
        assert!(!still_same(Some(&at), Some((1000, 8)), same), "another pid");
        assert!(!still_same(Some(&at), None, same), "no credentials now");
        assert!(!still_same(None, Some((1000, 7)), same), "none at accept");
        let gone = (1000, 7, None);
        assert!(
            !still_same(Some(&gone), Some((1000, 7)), same),
            "exe unreadable at accept"
        );
    }

    /// R2C-12: the executable is read through the descriptor pinned at accept; once the
    /// process is gone it reads nothing, whatever now runs under that pid number.
    #[test]
    fn the_requester_is_read_through_the_pinned_process() {
        let me = Pinned::capture(1000, i32::try_from(std::process::id()).unwrap());
        let exe = std::env::current_exe().unwrap();
        assert_eq!(me.exe.as_deref(), exe.to_str());
        assert_eq!(me.exe_now(), me.exe);
        let mut child = std::process::Command::new("sleep")
            .arg("5")
            .spawn()
            .unwrap();
        let p = Pinned::capture(1000, i32::try_from(child.id()).unwrap());
        assert!(
            p.exe.as_deref().is_some_and(|e| e.ends_with("sleep")),
            "{:?}",
            p.exe
        );
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(
            p.exe_now(),
            None,
            "gone: never the next process with that pid"
        );
        assert!(Pinned::capture(1000, 0).exe.is_none());
    }

    /// Audit2 A2-7: ⌘Tab, Overview or About took the keyboard (`activewindow>>,`) and the bold
    /// app name blanked. A WolOS layer holding the keys keeps the name; no window and no such
    /// layer (the last window closed) blanks it.
    #[test]
    fn a_shell_layer_taking_the_keyboard_keeps_the_app_name() {
        let layers = |ns: &[&str]| {
            crate::appmenu::panel_has_keys(&ns.iter().map(|s| s.to_string()).collect::<Vec<_>>())
        };
        for ns in [
            "wol-panel-switcher",
            "wol-panel-overview",
            "wol-panel-about",
            "wol-panel-force-quit",
        ] {
            assert_eq!(
                after_blank(None, layers(&["wol-menubar", "wol-dock", ns])),
                None,
                "{ns}"
            );
        }
        let blank = Some((String::new(), String::new()));
        assert_eq!(
            after_blank(None, layers(&["wol-menubar", "wol-dock"])),
            blank
        );
        assert_eq!(after_blank(Some(("".into(), "".into())), false), blank);
        let foot = Some(("foot".to_string(), "~".to_string()));
        assert_eq!(
            after_blank(foot.clone(), true),
            foot,
            "a window in front wins"
        );
    }

    /// Audit2 A2-10: the switcher, Force Quit and About share one list, read off the UI thread
    /// once and again in the background when old; a read in flight is not started twice.
    #[test]
    fn the_installed_list_is_read_once_then_refreshed_in_the_background() {
        let now = std::time::Instant::now();
        assert_eq!(fetch(None, now, false), Fetch::Wait);
        assert_eq!(
            fetch(None, now, true),
            Fetch::Join,
            "a first read is under way"
        );
        assert_eq!(fetch(Some(now), now, false), Fetch::Use);
        let later = now + INSTALLED_TTL;
        assert_eq!(fetch(Some(now), later, false), Fetch::Refresh);
        assert_eq!(fetch(Some(now), later, true), Fetch::Use, "already reading");
    }

    /// Audit2 A2-14: the status icons name a Control Center page; the plain target opens main.
    #[test]
    fn control_center_targets_name_their_page() {
        assert_eq!(cc_page("control-center:wifi"), Some("wifi"));
        assert_eq!(cc_page("control-center"), None);
        assert_eq!(cc_page("notifications"), None);
        assert_eq!(
            crate::protocol::parse("toggle control-center:sound").map(|c| c.target),
            Some("control-center:sound".into())
        );
    }

    /// Audit3 R2A-2: a status icon clicked while the Control Center is open on another page
    /// switches to its page; the same icon again closes it.
    #[test]
    fn a_status_icon_switches_the_open_control_center_to_its_page() {
        use crate::protocol::Verb::*;
        for (verb, visible, current, want) in [
            (Toggle, false, None, CcAction::Open),
            (Show, false, Some("main"), CcAction::Open),
            (Toggle, true, Some("main"), CcAction::SwitchTo),
            (Show, true, Some("sound"), CcAction::SwitchTo),
            (Toggle, true, Some("wifi"), CcAction::Close),
            (Show, true, Some("wifi"), CcAction::Nothing),
            (Hide, true, Some("main"), CcAction::Close),
            (Hide, false, None, CcAction::Nothing),
        ] {
            assert_eq!(
                cc_action(verb, visible, current, "wifi"),
                want,
                "{verb:?} {visible} {current:?}"
            );
        }
    }

    /// A dropped event socket (compositor reload) is retried, slower each time, never faster
    /// than once a second and never slower than every 30 s.
    #[test]
    fn reconnects_back_off_and_cap() {
        let secs: Vec<u64> = (0..8).map(|n| backoff(n).as_secs()).collect();
        assert_eq!(secs, [1, 2, 4, 8, 16, 30, 30, 30]);
        assert_eq!(backoff(u32::MAX).as_secs(), 30, "no shift overflow");
    }
}
