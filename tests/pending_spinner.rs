// SPDX-License-Identifier: GPL-2.0-or-later
//! A switch that waits for its change shows a small spinner while it waits, and the Authenticate
//! sheet shows one while the change is checked (QA 0.12.0 bug 7: a greyed switch and nothing
//! else read as a frozen window). Needs the toolkit: runs on a private Broadway display (the
//! golden harness's `gtk4-broadwayd`), and skips without one.
use gtk::prelude::*;
use shell::kit::{form, widgets as w};
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

type Done = Box<dyn FnOnce(bool)>;

/// Kills the private broadwayd on every exit path.
struct Kill(std::process::Child);
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

fn spinners(root: &gtk::Widget) -> Vec<gtk::Spinner> {
    let mut out = Vec::new();
    let mut c = root.first_child();
    while let Some(w) = c {
        if let Some(s) = w.downcast_ref::<gtk::Spinner>() {
            out.push(s.clone());
        }
        out.extend(spinners(&w));
        c = w.next_sibling();
    }
    out
}

fn switches(root: &gtk::Widget) -> Vec<gtk::Switch> {
    let mut out = Vec::new();
    let mut c = root.first_child();
    while let Some(w) = c {
        if let Some(s) = w.downcast_ref::<gtk::Switch>() {
            out.push(s.clone());
        }
        out.extend(switches(&w));
        c = w.next_sibling();
    }
    out
}

/// One test: GTK lives on the thread that started it. `get_visible` is the widget's own flag
/// (`is_visible` also asks its parents, and the sheet is collapsed here).
#[test]
fn a_waiting_switch_and_the_authenticate_sheet_show_a_spinner() {
    let _display = broadway();
    if gtk::init().is_err() {
        eprintln!("skipped: no display for the toolkit");
        return;
    }

    // The switch alone: spinner hidden until a flip, shown while `run` has not answered.
    let pending: Rc<RefCell<Option<Done>>> = Rc::default();
    let p = pending.clone();
    let (s, spin) = w::pending_toggle_spinner(false, move |_, done| *p.borrow_mut() = Some(done));
    assert!(!spin.get_visible() && !spin.is_spinning(), "hidden at rest");
    s.set_active(true);
    assert!(!s.is_sensitive(), "the switch waits");
    assert!(spin.get_visible() && spin.is_spinning(), "the wait shows");
    (pending.borrow_mut().take().expect("change started"))(false);
    assert!(
        s.is_sensitive() && !s.is_active() && !s.state(),
        "flipped back"
    );
    assert!(!spin.get_visible() && !spin.is_spinning(), "hidden again");
    s.set_active(true);
    assert!(spin.get_visible());
    (pending.borrow_mut().take().expect("change started"))(true);
    assert!(s.is_active() && s.state() && !spin.get_visible());

    // The row form panes use carries the same spinner beside its switch.
    let p = pending.clone();
    let row = form::pending_toggle_row("Remote Login", "", false, move |_, done| {
        *p.borrow_mut() = Some(done)
    });
    let (sw, sp) = (switches(&row), spinners(&row));
    assert_eq!((sw.len(), sp.len()), (1, 1), "one switch, one spinner");
    assert!(!sp[0].get_visible());
    sw[0].set_active(true);
    assert!(sp[0].get_visible() && sp[0].is_spinning() && !sw[0].is_sensitive());
    (pending.borrow_mut().take().expect("change started"))(true);
    assert!(!sp[0].get_visible() && sw[0].is_active() && sw[0].is_sensitive());

    // The plain pending switch is unchanged: no spinner of its own.
    let plain = w::pending_toggle(false, |_, _| {});
    assert!(spinners(plain.upcast_ref()).is_empty());

    // The Authenticate sheet: hidden spinner beside the title, shown while busy.
    let sheet = w::authenticate_sheet("Settings");
    assert!(!sheet.busy.get_visible() && !sheet.busy.is_spinning());
    assert!(
        spinners(sheet.revealer.upcast_ref()).contains(&sheet.busy),
        "the spinner is in the sheet"
    );
    sheet.set_busy(true);
    assert!(sheet.busy.get_visible() && sheet.busy.is_spinning());
    sheet.set_busy(false);
    assert!(!sheet.busy.get_visible() && !sheet.busy.is_spinning());
}
