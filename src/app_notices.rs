//! App notifications through wold (ADR 0020). wold admits only the shell to the account's
//! notifications (`subscribe account`); each one goes to the session's notification server (our
//! own banners and Notification Center, Focus included) as a plain Notify. A pressed button comes
//! back as ActionInvoked and goes to wold as `notification.answer`, which tells only the app that
//! posted it. An app replacing a notification updates its banner in place (`replaces_id`), and
//! withdrawing closes it. No GTK here: a thread, two sockets.
use shared::events::Event;
use shared::portal::{Answer, Call};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
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

/// Banner id -> wold id, bounded.
#[derive(Default)]
struct Ids(Vec<(u32, u32)>);

impl Ids {
    fn add(&mut self, banner: u32, wold: u32) {
        self.0.retain(|e| e.1 != wold);
        if self.0.len() == KEPT {
            self.0.remove(0);
        }
        self.0.push((banner, wold));
    }

    fn take(&mut self, banner: u32) -> Option<u32> {
        let i = self.0.iter().position(|e| e.0 == banner)?;
        Some(self.0.remove(i).1)
    }

    /// The banner showing wold's notification `wold`; 0 when it is gone.
    fn banner(&self, wold: u32) -> u32 {
        self.0.iter().find(|e| e.1 == wold).map_or(0, |e| e.0)
    }
}

fn wold_socket() -> PathBuf {
    std::env::var_os("WOL_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| shared::jobs::SOCKET.into())
}

/// One call to wold.
fn call(wold: &Path, c: &Call) -> Answer {
    let ask = || -> std::io::Result<Answer> {
        let mut s = UnixStream::connect(wold)?;
        s.set_read_timeout(Some(Duration::from_secs(5)))?;
        s.write_all(format!("call {}\n", c.encode()).as_bytes())?;
        let mut line = String::new();
        BufReader::new(&s).take(64 * 1024).read_line(&mut line)?;
        Ok(Answer::parse(&line))
    };
    // Only logged: the person sees nothing of a lost answer.
    ask().unwrap_or_else(|_| Answer::err(shared::portal::Code::Unavailable, "wold did not answer"))
}

/// Why the bridge stopped (logged).
pub type Stop = Box<dyn std::error::Error + Send + Sync>;

/// Bridge until wold or the session bus goes away. `dbus`: bus address (None: session bus).
pub fn run(wold: &Path, dbus: Option<&str>) -> Result<(), Stop> {
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
    {
        let (ids, wold) = (ids.clone(), wold.to_path_buf());
        std::thread::spawn(move || {
            for m in signals.flatten() {
                let h = m.header();
                match h.member().map(|m| m.as_str()) {
                    Some("ActionInvoked") => {
                        let Ok((banner, key)) = m.body().deserialize::<(u32, String)>() else {
                            continue;
                        };
                        let id = ids.lock().ok().and_then(|mut i| i.take(banner));
                        if let Some(id) = id {
                            let a = call(&wold, &Call::NotificationAnswer { id, key });
                            if !a.ok {
                                eprintln!("wol-shell: notification answer: {:?}", a.message);
                            }
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
        });
    }
    let mut s = UnixStream::connect(wold)?;
    s.write_all(b"subscribe account\n")?;
    let uid = shared::sys::euid();
    let mut r = BufReader::new(s);
    while let Some(line) = crate::app_host::read_line(&mut r) {
        let Some(event) = Event::parse(&line) else {
            continue;
        };
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
                if let Ok(mut i) = ids.lock() {
                    i.add(b, id);
                }
            }
            Ok(_) => {}
            Err(e) => eprintln!("wol-shell: app notification: {e}"),
        }
    }
    Err("wold closed the connection".into())
}

/// For the session: reconnect when wold restarts (backs off to a minute while it is away).
pub fn start() {
    std::thread::spawn(|| {
        let mut wait = 2;
        loop {
            let t = std::time::Instant::now();
            if let Err(e) = run(&wold_socket(), None)
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

#[cfg(test)]
mod tests {
    use super::*;
    use shared::portal::NoteAction;

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
}
