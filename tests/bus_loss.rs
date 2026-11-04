// SPDX-License-Identifier: GPL-2.0-or-later
//! QA-CRASH-2: losing the user session bus ended wol-shell (and the lock screen) silently: GIO's
//! shared connection raises SIGTERM on close (`exit-on-close`). `kit::fence::survive_bus_loss`
//! turns that off. Each case runs a child (this binary again) on a private bus, kills the bus and
//! looks at how the child ended. Needs `dbus-daemon` (skips without it).
use gtk::prelude::*;
use gtk::{gio, glib};
use shell::kit::fence;
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::time::Duration;

const CHILD: &str = "WOL_BUS_LOSS_CHILD";

/// The child: a plain `gio::Application` on the bus, fixed (`keep`) or not. Prints `ready` once
/// registered, then lives 2 s past the bus and exits 0.
fn child(keep: bool) {
    let app = gio::Application::builder()
        .application_id("org.wolos.test.busloss")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    if keep {
        fence::survive_bus_loss(&app);
    }
    app.connect_activate(|app| {
        let hold = app.hold();
        assert!(app.dbus_connection().is_some(), "no bus connection");
        println!("ready");
        let mut hold = Some(hold);
        glib::timeout_add_local_once(Duration::from_secs(2), move || drop(hold.take()));
    });
    let code: u8 = app.run_with_args::<&str>(&[]).into();
    std::process::exit(code.into());
}

/// Runs case `name` as a child on a private bus, kills the bus once it is ready; the child's end.
fn run_child(name: &str) -> Option<(std::process::ExitStatus, String)> {
    let mut bus = match Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--print-address=1"])
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(b) => b,
        Err(_) => {
            eprintln!("skip: no dbus-daemon");
            return None;
        }
    };
    let mut addr = String::new();
    BufReader::new(bus.stdout.take().unwrap())
        .read_line(&mut addr)
        .unwrap();
    let mut kid = Command::new(std::env::current_exe().unwrap())
        .args([name, "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .env("DBUS_SESSION_BUS_ADDRESS", addr.trim())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut out = BufReader::new(kid.stdout.take().unwrap());
    let mut line = String::new();
    while out.read_line(&mut line).unwrap() > 0 && !line.contains("ready") {
        line.clear();
    }
    assert!(
        line.contains("ready"),
        "the child never registered on the bus"
    );
    bus.kill().unwrap();
    bus.wait().unwrap();
    let status = kid.wait().unwrap();
    let mut err = String::new();
    kid.stderr.take().unwrap().read_to_string(&mut err).unwrap();
    Some((status, err))
}

#[test]
fn bus_loss_ends_a_plain_app() {
    if std::env::var_os(CHILD).is_some() {
        return child(false);
    }
    // The bug, kept as a test so the case below is known to reproduce it.
    use std::os::unix::process::ExitStatusExt;
    let Some((status, _)) = run_child("bus_loss_ends_a_plain_app") else {
        return;
    };
    assert_eq!(status.signal(), Some(15), "{status:?}");
}

#[test]
fn bus_loss_keeps_the_app_running() {
    if std::env::var_os(CHILD).is_some() {
        return child(true);
    }
    let Some((status, err)) = run_child("bus_loss_keeps_the_app_running") else {
        return;
    };
    assert!(status.success(), "{status:?}\n{err}");
    assert!(err.contains("the session bus went away"), "{err}");
}

#[test]
fn bus_lost_line_names_the_process() {
    assert_eq!(
        fence::bus_lost_line("wol-shell"),
        "wol-shell: the session bus went away; keeping on without it"
    );
}
