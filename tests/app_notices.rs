// SPDX-License-Identifier: GPL-2.0-or-later
//! App notifications end to end (ADR 0020): a fake wold hands the shell a notification with a
//! button; it becomes a banner on our notification server (private dbus-daemon); pressing the
//! button sends `notification.answer` with wold's id back to wold.
use shared::events::Event;
use shared::portal::{Answer, Call, NoteAction};
use shell::ui::notify::{Bus, Event as Shown};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::sync::mpsc;
use std::time::Duration;

struct Daemon(std::process::Child, String);
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn daemon() -> Option<Daemon> {
    let mut c = std::process::Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--print-address=1"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .ok()?;
    let mut line = String::new();
    BufReader::new(c.stdout.take()?).read_line(&mut line).ok()?;
    Some(Daemon(c, line.trim().to_string()))
}

#[test]
fn a_wold_notification_becomes_a_banner_and_its_button_answers() {
    let Some(d) = daemon() else {
        eprintln!("skipped: no dbus-daemon");
        return;
    };
    let dir = std::env::temp_dir().join(format!("wol-app-notices-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let wold = dir.join("wold.sock");
    let l = UnixListener::bind(&wold).unwrap();
    let (calls_tx, calls) = mpsc::channel::<Call>();
    let uid = shared::sys::euid();
    std::thread::spawn(move || {
        for conn in l.incoming().flatten() {
            let calls_tx = calls_tx.clone();
            std::thread::spawn(move || {
                let mut line = String::new();
                BufReader::new(&conn).read_line(&mut line).unwrap();
                let mut w = &conn;
                if line == "subscribe account\n" {
                    for (u, title) in [(uid + 1, "Not yours"), (uid, "Download done")] {
                        let e = Event::Notification {
                            uid: u,
                            app: "/usr/bin/hello".into(),
                            title: title.into(),
                            body: "parts.iso".into(),
                            id: 7,
                            actions: vec![NoteAction {
                                key: "open".into(),
                                label: "Open".into(),
                            }],
                            replace: false,
                        };
                        w.write_all(e.to_line().as_bytes()).unwrap();
                    }
                    std::thread::sleep(Duration::from_secs(10));
                    return;
                }
                let json = line.trim().strip_prefix("call ").unwrap();
                calls_tx
                    .send(Call::decode(json.as_bytes()).unwrap())
                    .unwrap();
                w.write_all(Answer::ok().to_line().as_bytes()).unwrap();
            });
        }
    });

    let (tx, rx) = async_channel::unbounded();
    let addr = d.1.clone();
    let (bus_tx, bus_rx) = mpsc::channel::<Bus>();
    std::thread::spawn(move || {
        async_io::block_on(async {
            let b = Bus::serve(Some(&addr), tx).await.expect("serve");
            bus_tx.send(b).unwrap();
            futures_lite::future::pending::<()>().await;
        })
    });
    let bus = bus_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let (addr, w2) = (d.1.clone(), wold.clone());
    std::thread::spawn(move || shell::app_notices::run(&w2, Some(&addr)));

    let shown = async_io::block_on(async {
        futures_lite::future::or(async { rx.recv().await.ok() }, async {
            async_io::Timer::after(Duration::from_secs(10)).await;
            None
        })
        .await
    });
    let Some(Shown::Show(n)) = shown else {
        panic!("no banner: {shown:?}")
    };
    assert_eq!(n.app, "hello");
    assert_eq!(n.summary, "Download done", "only this account's notices");
    assert_eq!(n.body, "parts.iso");
    assert_eq!(n.actions, vec![("open".to_string(), "Open".to_string())]);
    assert!(rx.try_recv().is_err(), "one banner");

    async_io::block_on(bus.invoke(n.id, "open")).unwrap();
    let answered = calls.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(
        answered,
        Call::NotificationAnswer {
            id: 7,
            key: "open".into()
        },
        "wold's id, not the banner's"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn replacing_updates_the_banner_and_withdrawing_closes_it() {
    let Some(d) = daemon() else {
        eprintln!("skipped: no dbus-daemon");
        return;
    };
    let dir = std::env::temp_dir().join(format!("wol-app-replace-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let wold = dir.join("wold.sock");
    let l = UnixListener::bind(&wold).unwrap();
    let uid = shared::sys::euid();
    std::thread::spawn(move || {
        let conn = l.incoming().next().unwrap().unwrap();
        let mut line = String::new();
        BufReader::new(&conn).read_line(&mut line).unwrap();
        let note = |body: &str, replace| Event::Notification {
            uid,
            app: "/usr/bin/hello".into(),
            title: "Copying".into(),
            body: body.into(),
            id: 7,
            actions: Vec::new(),
            replace,
        };
        let mut w = &conn;
        for e in [
            note("1 of 5", false),
            note("2 of 5", true),
            Event::NotificationWithdrawn {
                uid: uid + 1,
                id: 7,
            },
            Event::NotificationWithdrawn { uid, id: 7 },
        ] {
            w.write_all(e.to_line().as_bytes()).unwrap();
            std::thread::sleep(Duration::from_millis(300));
        }
        std::thread::sleep(Duration::from_secs(10));
    });

    let (tx, rx) = async_channel::unbounded();
    let addr = d.1.clone();
    let (bus_tx, bus_rx) = mpsc::channel::<Bus>();
    std::thread::spawn(move || {
        async_io::block_on(async {
            let b = Bus::serve(Some(&addr), tx).await.expect("serve");
            bus_tx.send(b).unwrap();
            futures_lite::future::pending::<()>().await;
        })
    });
    let _bus = bus_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let (addr, w2) = (d.1.clone(), wold.clone());
    std::thread::spawn(move || shell::app_notices::run(&w2, Some(&addr)));

    let next = || {
        async_io::block_on(async {
            futures_lite::future::or(async { rx.recv().await.ok() }, async {
                async_io::Timer::after(Duration::from_secs(10)).await;
                None
            })
            .await
        })
    };
    let Some(Shown::Show(first)) = next() else {
        panic!("no banner")
    };
    assert_eq!(first.body, "1 of 5");
    let Some(Shown::Show(second)) = next() else {
        panic!("no replacement")
    };
    assert_eq!(second.body, "2 of 5");
    assert_eq!(second.id, first.id, "same banner");
    let Some(Shown::Close(id)) = next() else {
        panic!("not closed")
    };
    assert_eq!(id, first.id);
    let _ = std::fs::remove_dir_all(dir);
}
