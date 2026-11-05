// SPDX-License-Identifier: GPL-2.0-or-later
//! App notifications through wold (ADR 0020). wold admits only the shell to the account's
//! notifications (`subscribe account`); each one goes to the session's notification server (our
//! own banners and Notification Center, Focus included) as a plain Notify. A pressed button comes
//! back as ActionInvoked and goes to wold as `notification.answer`, which tells only the app that
//! posted it. An app replacing a notification updates its banner in place (`replaces_id`), and
//! withdrawing closes it. No GTK here: a thread, two sockets.
use shared::events::Event;
use shared::portal::{Answer, Call};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
/// Banner ids remembered for an answer at most (oldest forgotten).
const KEPT: usize = 256;

/// Notify's arguments for a wold notification: app name, summary, body, flat actions.
#[derive(Debug, PartialEq)]
pub struct Notify {
    pub app: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<String>,
    /// New text for a banner already up.
    pub replace: bool,
}

/// None for every event that is not a notification for this account.
pub fn notify_of(e: &Event, uid: u32) -> Option<(u32, Notify)> {
    let Event::Notification {
        uid: u,
        app,
        title,
        body,
        id,
        actions,
        replace,
    } = e
    else {
        return None;
    };
    if *u != uid {
        return None;
    }
    let name = app
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(app);
    Some((
        *id,
        Notify {
            app: name.into(),
            summary: title.clone(),
            body: body.clone(),
            actions: actions
                .iter()
                .flat_map(|a| [a.key.clone(), a.label.clone()])
                .collect(),
            replace: *replace,
        },
    ))
}

/// One notice of the shell itself (a plugin turned off): a plain Notify to the session's
/// notification server (the shell's own banners), not through wold. Blocks: call it off the GTK
/// thread.
pub fn post_own(summary: &str, body: &str) -> zbus::Result<()> {
    let conn = zbus::blocking::Connection::session()?;
    let hints: HashMap<&str, zbus::zvariant::Value> = HashMap::new();
    conn.call_method(
        Some(NAME),
        PATH,
        Some(NAME),
        "Notify",
        &(
            "WolOS",
            0u32,
            "",
            summary,
            body,
            Vec::<String>::new(),
            hints,
            -1i32,
        ),
    )
    .map(|_| ())
}

/// Presses kept for a banner whose Notify reply has not come back yet (other apps' banners
/// land here too and fall off).
const EARLY_KEPT: usize = 8;

/// Banner id -> wold id, bounded.
#[derive(Default)]
struct Ids {
    map: Vec<(u32, u32)>,
    /// (banner, action key) pressed before [`Ids::add`] knew the banner: the server sends
    /// ActionInvoked as soon as the banner is up, which can beat Notify's reply here.
    early: Vec<(u32, String)>,
}

impl Ids {
    /// Banner `banner` shows wold's `wold`. Some(key) when it was already pressed: answer it.
    fn add(&mut self, banner: u32, wold: u32) -> Option<String> {
        if let Some(i) = self.early.iter().position(|e| e.0 == banner) {
            return Some(self.early.remove(i).1);
        }
        self.map.retain(|e| e.1 != wold);
        if self.map.len() == KEPT {
            self.map.remove(0);
        }
        self.map.push((banner, wold));
        None
    }

    fn take(&mut self, banner: u32) -> Option<u32> {
        let i = self.map.iter().position(|e| e.0 == banner)?;
        Some(self.map.remove(i).1)
    }

    /// A press on `banner`: wold's id to answer, or kept until [`Ids::add`] names the banner.
    fn press(&mut self, banner: u32, key: String) -> Option<(u32, String)> {
        if let Some(id) = self.take(banner) {
            return Some((id, key));
        }
        if self.early.len() == EARLY_KEPT {
            self.early.remove(0);
        }
        self.early.push((banner, key));
        None
    }

    /// The banner showing wold's notification `wold`; 0 when it is gone.
    fn banner(&self, wold: u32) -> u32 {
        self.map.iter().find(|e| e.1 == wold).map_or(0, |e| e.0)
    }
}

/// Tells wold the person pressed `key` on its notification `id`.
fn answer(wold: &Path, id: u32, key: String) {
    let a = call(wold, &Call::NotificationAnswer { id, key });
    if !a.ok {
        eprintln!("wol-shell: notification answer: {:?}", a.message);
    }
}

pub(crate) fn wold_socket() -> PathBuf {
    std::env::var_os("WOL_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| shared::jobs::SOCKET.into())
}

/// Longest answer line wold sends to one call.
const ANSWER_MAX: usize = 64 * 1024;

/// One call to wold.
pub(crate) fn call(wold: &Path, c: &Call) -> Answer {
    call_with(wold, c, None)
}

/// One call to wold, with an open file attached.
pub(crate) fn call_with(wold: &Path, c: &Call, file: Option<&std::fs::File>) -> Answer {
    let ask = || -> std::io::Result<Answer> {
        let mut s = shared::jobs::connect(wold)?;
        s.set_read_timeout(Some(Duration::from_secs(5)))?;
        let line = format!("call {}\n", c.encode());
        match file {
            Some(f) => shared::sys::send_with_fd(&s, line.as_bytes(), std::os::fd::AsFd::as_fd(f))?,
            None => s.write_all(line.as_bytes())?,
        }
        // STANDARDS 7.1 "Line protocols" (R2C-18): one bounded answer line.
        let mut r = shared::bounded::BoundedLineReader::new(&s, ANSWER_MAX, ANSWER_MAX as u64 + 1);
        let line = r
            .read_line_str()
            .map_err(std::io::Error::other)?
            .unwrap_or_default();
        Ok(Answer::parse(&line))
    };
    // Only logged: the person sees nothing of a lost answer.
    ask().unwrap_or_else(|_| Answer::err(shared::portal::Code::Unavailable, "wold did not answer"))
}

/// Why the bridge stopped (logged).
pub type Stop = Box<dyn std::error::Error + Send + Sync>;

/// The session-bus side of the bridge: one connection, its Notifications proxy and the thread
/// reading ActionInvoked/NotificationClosed. Made once and kept across wold reconnects: a new
/// one per retry leaked a bus connection and a thread every time (audit4 S3-2).
struct BusSide {
    proxy: zbus::blocking::Proxy<'static>,
    ids: Arc<Mutex<Ids>>,
    /// False once the signal thread ended (the bus went away): open a new side.
    alive: Arc<AtomicBool>,
    /// The bus address (None: the session bus), for the players apps publish.
    address: Option<String>,
}

impl BusSide {
    fn open(wold: &Path, dbus: Option<&str>) -> Result<Self, Stop> {
        let conn = match dbus {
            Some(a) => zbus::blocking::connection::Builder::address(a).and_then(|b| b.build()),
            None => zbus::blocking::Connection::session(),
        }?;
        let proxy = zbus::blocking::Proxy::new(&conn, NAME, PATH, NAME)?;
        let ids: Arc<Mutex<Ids>> = Default::default();
        // Listen before the first banner, so no press is missed. One stream keeps the server's
        // order: ActionInvoked comes before the NotificationClosed that follows it.
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface(NAME)
            .and_then(|r| r.path(PATH))?
            .build();
        let signals = zbus::blocking::MessageIterator::for_match_rule(rule, &conn, Some(64))?;
        let alive = Arc::new(AtomicBool::new(true));
        let (i, wold, a) = (ids.clone(), wold.to_path_buf(), alive.clone());
        std::thread::spawn(move || {
            signal_loop(signals, &i, &wold);
            a.store(false, Ordering::Relaxed);
        });
        Ok(Self {
            proxy,
            ids,
            alive,
            address: dbus.map(str::to_owned),
        })
    }

    fn alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }
}

fn signal_loop(signals: zbus::blocking::MessageIterator, ids: &Mutex<Ids>, wold: &Path) {
    for m in signals.flatten() {
        let h = m.header();
        match h.member().map(|m| m.as_str()) {
            Some("ActionInvoked") => {
                let Ok((banner, key)) = m.body().deserialize::<(u32, String)>() else {
                    continue;
                };
                let pressed = ids.lock().ok().and_then(|mut i| i.press(banner, key));
                if let Some((id, key)) = pressed {
                    answer(wold, id, key);
                }
            }
            Some("NotificationClosed") => {
                if let Ok((banner, _)) = m.body().deserialize::<(u32, u32)>()
                    && let Ok(mut i) = ids.lock()
                {
                    i.take(banner);
                }
            }
            _ => {}
        }
    }
}

/// wold's account subscription; nothing on the session bus is made before it answers.
fn subscribe(wold: &Path) -> Result<std::os::unix::net::UnixStream, Stop> {
    let mut s = shared::jobs::connect(wold)?;
    s.write_all(b"subscribe account\n")?;
    // A write that wold does not take within the client timeout ends the bridge (it restarts).
    s.set_write_timeout(Some(shared::bounded::CLIENT_TIMEOUT))?;
    Ok(s)
}

/// Bridge until wold or the session bus goes away. `dbus`: bus address (None: session bus).
/// wold first: with wold away it returns before touching the bus (audit4 S3-2).
pub fn run(wold: &Path, dbus: Option<&str>) -> Result<(), Stop> {
    let s = subscribe(wold)?;
    let bus = BusSide::open(wold, dbus)?;
    bridge(s, &bus, wold)
}

fn bridge(s: std::os::unix::net::UnixStream, bus: &BusSide, wold: &Path) -> Result<(), Stop> {
    let (proxy, ids) = (&bus.proxy, &bus.ids);
    let uid = shared::sys::euid();
    // The players apps publish (libwol 1.16): up while this subscription lasts.
    let mut players = crate::app_media::Players::new(wold.to_path_buf(), bus.address.clone());
    // STANDARDS 7.1 "Line protocols" (R2C-18): every event line is bounded. The subscription has
    // no idle timeout: wold sends nothing while nothing happens, and a dropped subscription would
    // miss the next ask; the total is unbounded for the same reason (a session lasts days).
    let mut r = shared::bounded::BoundedLineReader::new(s, crate::app_host::MAX_LINE, u64::MAX);
    loop {
        let line = match r.read_line_str() {
            Ok(Some(l)) => l,
            Ok(None) => break,
            Err(e) => return Err(e.into()),
        };
        let Some(event) = Event::parse(&line) else {
            continue;
        };
        if let Event::SessionAsk { uid: u, id, ask } = event {
            if u == uid {
                crate::app_session::answer_later(wold.to_path_buf(), id, ask);
            }
            continue;
        }
        if players.event(&event, uid) {
            continue;
        }
        if let Event::NotificationWithdrawn { uid: u, id } = event {
            let banner = ids.lock().map_or(0, |i| i.banner(id));
            if u == uid && banner != 0 {
                let closed: Result<(), _> = proxy.call("CloseNotification", &(banner,));
                if let Err(e) = closed {
                    eprintln!("wol-shell: withdrawing a notification: {e}");
                }
            }
            continue;
        }
        let Some((id, n)) = notify_of(&event, uid) else {
            continue;
        };
        // A banner the person already dismissed comes back as a new one.
        let replaces = if n.replace {
            ids.lock().map_or(0, |i| i.banner(id))
        } else {
            0
        };
        let hints: HashMap<&str, zbus::zvariant::Value> = HashMap::new();
        let banner: Result<u32, _> = proxy.call(
            "Notify",
            &(
                n.app.as_str(),
                replaces,
                "",
                n.summary.as_str(),
                n.body.as_str(),
                n.actions,
                hints,
                -1i32,
            ),
        );
        match banner {
            Ok(b) if id != 0 => {
                let early = ids.lock().ok().and_then(|mut i| i.add(b, id));
                if let Some(key) = early {
                    answer(wold, id, key);
                }
            }
            Ok(_) => {}
            Err(e) => eprintln!("wol-shell: app notification: {e}"),
        }
    }
    Err("wold closed the connection".into())
}

/// One reconnect attempt: wold first, then the kept bus side (made on first need, again only
/// when its signal thread ended).
fn attempt(wold: &Path, dbus: Option<&str>, bus: &mut Option<BusSide>) -> Result<(), Stop> {
    let s = subscribe(wold)?;
    let side = match bus.take().filter(BusSide::alive) {
        Some(b) => b,
        None => BusSide::open(wold, dbus)?,
    };
    let side = bus.insert(side);
    bridge(s, side, wold)
}

/// For the session: reconnect when wold restarts (backs off to a minute while it is away).
pub fn start() {
    std::thread::spawn(|| {
        let (wold, mut bus) = (wold_socket(), None);
        let mut wait = 2;
        loop {
            let t = std::time::Instant::now();
            if let Err(e) = attempt(&wold, None, &mut bus)
                && wait >= 60
            {
                eprintln!("wol-shell: app notifications: {e}");
            }
            // A connection that lasted resets the back-off.
            wait = if t.elapsed() > Duration::from_secs(60) {
                2
            } else {
                (wait * 2).min(60)
            };
            std::thread::sleep(Duration::from_secs(wait));
        }
    });
}

/// The banner title of the startup check's message (QA-BOOT-16).
pub const BOOT_NOTICE_TITLE: &str = "WolOS started a different version";

/// The banner for the startup check's message (`wol_core::seal::NOTICE`): a refused or damaged
/// version and which one started instead (QA-BOOT-16: it was written and nothing showed it, so a
/// refused slot ran as if all was well). None when there is no message. The file goes once its
/// cause is gone (`wol boot bless`), so a session shows it while it holds.
pub fn boot_notice(text: Option<&str>) -> Option<(&'static str, String)> {
    let body = text?.split_whitespace().collect::<Vec<_>>().join(" ");
    (!body.is_empty()).then_some((BOOT_NOTICE_TITLE, body))
}

/// Shows [`boot_notice`] once per session, a few seconds after the shell's notification server is
/// up (a GTK timer; the post blocks, so it runs on its own thread); a shell restarted within the
/// session does not show it again (a mark in the runtime dir). Call on the GTK thread.
pub fn start_boot_notice() {
    let Some((title, body)) = boot_notice(
        std::fs::read_to_string(wol_core::seal::NOTICE)
            .ok()
            .as_deref(),
    ) else {
        return;
    };
    let mark = Some(gtk::glib::user_runtime_dir().join("boot-notice.shown"));
    if mark.as_ref().is_some_and(|m| m.exists()) {
        return;
    }
    gtk::glib::timeout_add_seconds_local_once(BOOT_NOTICE_AFTER, move || {
        let posted = std::thread::Builder::new()
            .name("wol-boot-notice".into())
            .spawn(move || match post_own(title, &body) {
                Ok(()) => {
                    if let Some(m) = &mark {
                        let _ = std::fs::write(m, b"");
                    }
                }
                Err(e) => {
                    crate::diag::say!(
                        "wol-shell: the startup notice could not be shown ({e}): {body}"
                    )
                }
            });
        if let Err(e) = posted {
            crate::diag::say!("wol-shell: no thread for the startup notice: {e}");
        }
    });
}

/// Seconds after the shell starts before [`start_boot_notice`] posts (its banners are up by then).
const BOOT_NOTICE_AFTER: u32 = 4;

#[cfg(test)]
mod tests {
    use super::*;
    use shared::portal::NoteAction;

    /// QA-BOOT-16: the startup check's message reaches the person as a banner, in one line.
    #[test]
    fn the_startup_check_s_message_is_a_banner() {
        assert_eq!(boot_notice(None), None);
        assert_eq!(boot_notice(Some(" \n")), None);
        let (title, body) = boot_notice(Some(
            "This version of WolOS (wolos-b) is older than the version last used here.\nThe \
             other version starts instead.\n",
        ))
        .expect("a banner");
        assert_eq!(title, BOOT_NOTICE_TITLE);
        assert!(
            body.contains("older than the version last used here"),
            "{body}"
        );
        assert!(!body.contains('\n'), "{body}");
        // The shell reads the file the boot checks write.
        assert_eq!(wol_core::seal::NOTICE, "/var/lib/wolos/verity-notice");
        let ui = include_str!("ui/mod.rs");
        assert!(
            ui.contains("crate::app_notices::start_boot_notice"),
            "started by the shell"
        );
    }

    #[test]
    fn notifications_become_notify_calls() {
        let e = Event::Notification {
            uid: 1000,
            app: "/usr/bin/hello".into(),
            title: "Done".into(),
            body: "ok".into(),
            id: 7,
            actions: vec![NoteAction {
                key: "open".into(),
                label: "Open".into(),
            }],
            replace: false,
        };
        assert_eq!(
            notify_of(&e, 1000),
            Some((
                7,
                Notify {
                    app: "hello".into(),
                    summary: "Done".into(),
                    body: "ok".into(),
                    actions: vec!["open".into(), "Open".into()],
                    replace: false,
                }
            ))
        );
        assert_eq!(notify_of(&e, 1001), None, "another account's");
        assert_eq!(notify_of(&Event::Theme { id: "x".into() }, 1000), None);
    }

    #[test]
    fn ids_are_bounded() {
        let mut i = Ids::default();
        i.add(1, 10);
        for b in 2..=KEPT as u32 + 1 {
            i.add(b, b);
        }
        assert_eq!(i.take(1), None, "oldest forgotten");
        assert_eq!(i.take(2), Some(2));
        assert_eq!(i.take(2), None, "answered once");
        i.add(500, 3);
        assert_eq!(i.banner(3), 500, "the newest banner of a replaced note");
        assert_eq!(i.banner(9999), 0);
    }

    /// A press can reach the bridge before Notify's reply named the banner (seen on a loaded
    /// forge): it is answered when the banner is known, once.
    #[test]
    fn a_press_before_the_reply_is_answered() {
        let mut i = Ids::default();
        assert_eq!(i.press(5, "open".into()), None, "banner 5 not known yet");
        assert_eq!(i.add(5, 70), Some("open".to_string()), "answered now");
        assert_eq!(i.banner(70), 0, "and not kept for a second answer");
        assert_eq!(i.add(6, 71), None);
        assert_eq!(i.press(6, "x".into()), Some((71, "x".to_string())));
        for b in 100..100 + EARLY_KEPT as u32 + 4 {
            i.press(b, "other app".into());
        }
        assert_eq!(i.early.len(), EARLY_KEPT, "bounded");
    }

    #[test]
    fn wold_away_touches_no_session_bus() {
        // audit4 S3-2: the bus side was made before wold answered, and leaked on every retry.
        // A bus address that cannot work proves the order: the error is wold's, not the bus's.
        let gone = Path::new("/nonexistent/wol-fs3/wold.sock");
        let bad_bus = Some("unix:path=/nonexistent/wol-fs3/bus");
        let e = run(gone, bad_bus).unwrap_err();
        assert!(
            e.downcast_ref::<std::io::Error>().is_some(),
            "wold's error first: {e}"
        );
        let mut bus = None;
        for _ in 0..3 {
            assert!(attempt(gone, bad_bus, &mut bus).is_err());
        }
        assert!(bus.is_none(), "no bus connection while wold is away");
    }
}
