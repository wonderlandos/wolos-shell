// SPDX-License-Identifier: GPL-2.0-or-later
//! A private Broadway display for the toolkit tests that are not the golden harness (one copy,
//! included with `#[path]`; three tests had their own): a free display in the test's own port
//! range, started and waited for by its "Listening on" line (no polling), killed on every exit
//! path; and [`pump`], which runs the main loop for a while without sleeping.
#![allow(dead_code)] // not every including test calls every helper
use std::io::BufRead;
use std::path::Path;

/// Displays tried before giving up (see [`start`]).
const ATTEMPTS: usize = 5;

/// The display; the broadwayd behind it is killed when this is dropped.
pub struct Display(std::process::Child);

// Killed and reaped on every exit path, panics included: no broadwayd outlives its test.
impl Drop for Display {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Starts broadwayd on a free display in `base..base + 200` (the port is 18000 + the display) and
/// points GTK at it; None when there is none or broadwayd is not installed (the test skips). Call
/// it in the test's only `#[test]`, before `gtk::init`.
pub fn start(base: u32) -> Option<Display> {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    // A candidate looked free when it was checked and may not be when broadwayd binds it (another
    // test binary or container picked it in between): try the next free ones, a bounded number of times.
    let (d, display) = (0..200)
        .map(|i| base + (std::process::id() + i) % 200)
        .filter(|n| {
            let free = !Path::new(&runtime)
                .join(format!("broadway{}.socket", n + 1))
                .exists();
            free && u16::try_from(18000 + n)
                .is_ok_and(|p| std::net::TcpListener::bind(("127.0.0.1", p)).is_ok())
        })
        .take(ATTEMPTS)
        .find_map(|n| {
            let display = format!(":{n}");
            let mut child = std::process::Command::new("gtk4-broadwayd")
                .args([display.as_str(), "--port", &(18000 + n).to_string()])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()
                .ok()?;
            let out = child.stdout.take()?;
            let d = Display(child); // killed and reaped when this attempt is dropped
            // It says "Listening on <socket>" once the socket accepts; EOF means it gave up.
            let ready = std::io::BufReader::new(out)
                .lines()
                .map_while(Result::ok)
                .any(|l| l.starts_with("Listening on"));
            ready.then_some((d, display))
        })?;
    // SAFETY: called by the including binary's only test, before gtk::init: no other thread of
    // this process reads the environment yet (Rust 2024 makes set_var unsafe for that reason).
    unsafe {
        std::env::set_var("GDK_BACKEND", "broadway");
        std::env::set_var("BROADWAY_DISPLAY", &display);
    }
    Some(d)
}

/// Runs the default main context for `ms`: it blocks between events and wakes for its own timer,
/// so nothing sleeps (RR-049) and every idle and timeout due meanwhile runs.
pub fn pump(ms: u64) {
    let ctx = gtk::glib::MainContext::default();
    let done = std::rc::Rc::new(std::cell::Cell::new(false));
    let d = done.clone();
    gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(ms), move || d.set(true));
    while !done.get() {
        ctx.iteration(true);
    }
}
