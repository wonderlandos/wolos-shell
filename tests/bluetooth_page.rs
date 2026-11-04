// SPDX-License-Identifier: GPL-2.0-or-later
//! Settings > Bluetooth (QA-100 A7): the page has Pair and Forget (the manual promises them), a
//! failed power change flips the switch back (QA-SET-9/HUNT-10), and Forget asks first. Needs
//! the toolkit: runs on a private Broadway display (the golden harness's `gtk4-broadwayd`), and
//! skips without one.
use gtk::prelude::*;
use shell::system::{Action, ActionError, BtDevice, Done, System};
use shell::ui;
use std::path::Path;
use std::sync::Arc;

/// Kills the private broadwayd on every exit path.
struct Kill(std::process::Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A free Broadway display (the golden harness's scheme, its own port range), or None.
fn broadway() -> Option<Kill> {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let n = (0..200)
        .map(|i| 460 + (std::process::id() + i) % 200)
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

/// The radio refuses every change (BlueZ down, rfkill): `Bluetooth` ends in `Failed`.
struct Refusing(ActionError);

impl System for Refusing {
    fn battery(&self) -> Option<shell::status::Battery> {
        None
    }
    fn audio(&self) -> shell::status::Audio {
        Default::default()
    }
    fn network(&self) -> shell::status::Network {
        Default::default()
    }
    fn brightness(&self) -> Option<u8> {
        None
    }
    fn wifi_enabled(&self) -> Option<bool> {
        Some(true)
    }
    fn bluetooth(&self) -> Option<bool> {
        Some(true)
    }
    fn bt_devices(&self) -> Vec<BtDevice> {
        Vec::new()
    }
    fn spawn_with(&self, _a: Action, _pw: Option<&str>) -> Done {
        let (tx, rx) = async_channel::bounded(1);
        let _ = tx.try_send(Err(self.0));
        rx
    }
}

fn pump(ms: u64) {
    let ctx = gtk::glib::MainContext::default();
    let end = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    while std::time::Instant::now() < end {
        while ctx.iteration(false) {}
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn all<T: IsA<gtk::Widget>>(root: &gtk::Widget, out: &mut Vec<T>) {
    let mut c = root.first_child();
    while let Some(w) = c {
        if let Some(t) = w.downcast_ref::<T>() {
            out.push(t.clone());
        }
        all(&w, out);
        c = w.next_sibling();
    }
}

fn of<T: IsA<gtk::Widget>>(root: &impl IsA<gtk::Widget>) -> Vec<T> {
    let mut v = Vec::new();
    all(root.upcast_ref(), &mut v);
    v
}

fn open_page(sys: Arc<dyn System>) -> (shell::kit::widgets::AppWindow, gtk::Window, gtk::Widget) {
    let a = ui::settings::content(sys, &ui::settings::Host::demo());
    let win = gtk::Window::builder()
        .default_width(shell::kit::widgets::APP_SIZE.0)
        .default_height(shell::kit::widgets::APP_SIZE.1)
        .child(&a.root)
        .build();
    win.present();
    pump(200);
    a.open("Bluetooth");
    pump(400);
    let shown = a.content.visible_child().expect("Bluetooth shows");
    (a, win, shown)
}

fn label_text(root: &impl IsA<gtk::Widget>) -> Vec<String> {
    of::<gtk::Label>(root)
        .into_iter()
        .filter(|l| l.is_visible())
        .map(|l| l.label().to_string())
        .collect()
}

/// One test: GTK lives on the thread that started it.
#[test]
fn pair_forget_rows_exist_and_set_bt_err_restores_switch() {
    let _display = broadway();
    if gtk::init().is_err() {
        eprintln!("skipped: no display for the toolkit");
        return;
    }

    // --- pair_forget_rows_exist: every paired device has Forget; the page can add a device. ---
    let (a, win, shown) = open_page(Arc::new(Refusing(ActionError::Failed)));
    let buttons = of::<gtk::Button>(&shown);
    let forgets: Vec<_> = buttons
        .iter()
        .filter(|b| b.label().as_deref() == Some("Forget") && b.has_css_class("btn-plain"))
        .collect();
    let devices = buttons
        .iter()
        .filter(|b| b.has_css_class("device-button"))
        .count();
    assert!(devices >= 3, "the demo's paired devices: {devices}");
    assert_eq!(forgets.len(), devices, "one Forget per paired device");
    assert!(
        buttons
            .iter()
            .any(|b| b.label().as_deref() == Some("Add Device…")),
        "a way to pair a new device"
    );

    // Forget asks first: the sheet names the device and keeps it until confirmed.
    let sheet = of::<gtk::Revealer>(&shown)
        .into_iter()
        .find(|r| !of::<gtk::Entry>(r).is_empty())
        .expect("the pairing sheet");
    assert!(!sheet.reveals_child());
    forgets[0].emit_clicked();
    pump(100);
    assert!(sheet.reveals_child(), "Forget opens the sheet");
    let said = label_text(&sheet);
    assert!(
        said.iter()
            .any(|t| t.starts_with("Forget “") && t.ends_with("”?")),
        "{said:?}"
    );
    let cancel = of::<gtk::Button>(&sheet)
        .into_iter()
        .find(|b| b.label().as_deref() == Some("Cancel"))
        .expect("Cancel on the sheet");
    // One sheet at a time: a second Forget (or a pairing) does nothing while the first is open.
    let first_title = label_text(&sheet);
    forgets[1].emit_clicked();
    pump(100);
    assert_eq!(
        label_text(&sheet),
        first_title,
        "the open sheet is not replaced"
    );
    assert!(
        label_text(&shown)
            .iter()
            .any(|t| t == shell::copy::settings::BLUETOOTH_BUSY),
        "the second click says why"
    );
    cancel.emit_clicked();
    pump(100);
    assert!(!sheet.reveals_child(), "Cancel keeps the device");
    forgets[1].emit_clicked();
    pump(100);
    assert!(sheet.reveals_child(), "free again after Cancel");
    cancel.emit_clicked();
    pump(100);

    // --- set_bt_err_restores_switch: the change fails, the switch and the list go back. ---
    let power = of::<gtk::Switch>(&shown)
        .into_iter()
        .next()
        .expect("the power switch");
    assert!(power.is_active() && power.state(), "on at start");
    let list_before = of::<gtk::Button>(&shown)
        .iter()
        .filter(|b| b.has_css_class("device-button"))
        .count();
    power.set_active(false);
    pump(400);
    assert!(
        power.is_active() && power.state() && power.is_sensitive(),
        "a refused change flips the switch back"
    );
    let still = of::<gtk::Button>(&shown)
        .iter()
        .filter(|b| b.is_visible() && b.has_css_class("device-button"))
        .count();
    assert_eq!(still, list_before, "the list stays with the radio");
    let said = label_text(&shown);
    assert!(
        said.iter()
            .any(|t| t.contains("didn’t work") || t.contains("change")),
        "an error sentence under the switch: {said:?}"
    );
    assert!(!said.iter().any(|t| t.to_lowercase().contains("incorrect")));
    win.close();
    drop(a);

    // --- QA-HUNT-1 (bluetooth.rs:75): Cancel on the sheet says so, never "Incorrect password". ---
    let (a, win, shown) = open_page(Arc::new(Refusing(ActionError::Cancelled)));
    let device = of::<gtk::Button>(&shown)
        .into_iter()
        .find(|b| b.has_css_class("device-button"))
        .expect("a device row");
    device.emit_clicked();
    pump(300);
    let said = label_text(&shown);
    assert!(
        !said
            .iter()
            .any(|t| t.to_lowercase().contains("incorrect") || t.contains("Denied")),
        "no blame for the password: {said:?}"
    );
    assert!(
        !said
            .iter()
            .any(|t| t == shell::copy::action(ActionError::Failed)),
        "Cancel is the person's choice, not a failure: {said:?}"
    );
    assert!(
        said.iter().filter(|t| *t == "Connected").count() >= 1
            && !said.iter().any(|t| t == "Disconnecting…"),
        "the row is back to its state: {said:?}"
    );
    win.close();
    drop(a);
}
