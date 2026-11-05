// SPDX-License-Identifier: GPL-2.0-or-later
//! QA-MANUAL-33 (manual lock-screen.md: "press Return (or the arrow button)"): the lock screen's
//! password pill has an arrow button; it waits for a password and then sends it exactly like
//! Return. Needs the toolkit: runs on a private Broadway display (the golden harness's
//! `gtk4-broadwayd`), and skips without one.
use gtk::prelude::*;
use shell::{kit::widgets as w, ui::lock};
use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

/// Kills the private broadwayd on every exit path.
struct Kill(std::process::Child);
// Drop: kills the private broadwayd the test started.
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A free Broadway display (the golden harness's scheme, another port range), or None.
fn broadway() -> Option<Kill> {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let n = (0..200)
        .map(|i| 240 + (std::process::id() + i) % 200)
        .find(|n| {
            !Path::new(&runtime)
                .join(format!("broadway{}.socket", n + 1))
                .exists()
                && std::net::TcpListener::bind(("127.0.0.1", (18000 + n) as u16)).is_ok()
        })?;
    let display = format!(":{n}");
    let child = std::process::Command::new("gtk4-broadwayd")
        .args([display.as_str(), "--port", &(18000 + n).to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let kill = Kill(child);
    let sock = Path::new(&runtime).join(format!("broadway{}.socket", n + 1));
    for _ in 0..100 {
        if std::os::unix::net::UnixStream::connect(&sock).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    unsafe {
        std::env::set_var("GDK_BACKEND", "broadway");
        std::env::set_var("BROADWAY_DISPLAY", &display);
    }
    Some(kill)
}

fn has(root: &gtk::Widget, target: &gtk::Widget) -> bool {
    let mut c = root.first_child();
    while let Some(x) = c {
        if &x == target || has(&x, target) {
            return true;
        }
        c = x.next_sibling();
    }
    false
}

#[test]
fn the_arrow_button_unlocks_like_return() {
    let _display = broadway();
    if gtk::init().is_err() {
        eprintln!("skipped: no display for the toolkit");
        return;
    }
    let panel = w::UserPanel::new("Alice Liddell");
    let screen = lock::lock_screen(&panel);
    assert!(
        has(&screen, panel.submit.upcast_ref()),
        "the arrow is on the lock screen"
    );
    assert_eq!(panel.submit.label().as_deref(), Some("\u{2192}"));
    assert!(panel.submit.get_visible());
    let sent = Rc::new(Cell::new(0));
    let s = sent.clone();
    panel.field.connect_activate(move |_| s.set(s.get() + 1));
    assert!(
        !panel.submit.is_sensitive(),
        "no password yet: nothing to send"
    );
    panel.field.set_text("curiouser");
    assert!(panel.submit.is_sensitive());
    panel.submit.emit_clicked();
    assert_eq!(sent.get(), 1, "the arrow sends the password like Return");
    panel.set_busy(true);
    assert!(
        !panel.submit.get_visible(),
        "the spinner takes its place while checking"
    );
    panel.set_busy(false);
    assert!(panel.submit.get_visible());
}
