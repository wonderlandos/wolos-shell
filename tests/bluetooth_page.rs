// SPDX-License-Identifier: GPL-2.0-or-later
//! Settings > Bluetooth (QA-100 A7): the page has Pair and Forget (the manual promises them), a
//! failed power change flips the switch back (QA-SET-9/HUNT-10), and Forget asks first. Needs
//! the toolkit: runs on a private Broadway display (the golden harness's `gtk4-broadwayd`), and
//! skips without one.
#[path = "support/broadway.rs"]
mod broadway;
use gtk::prelude::*;
use shell::system::{Action, ActionError, BtDevice, Done, System};
use shell::ui;
use std::rc::Rc;

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

fn open_page(sys: Rc<dyn System>) -> (shell::kit::widgets::AppWindow, gtk::Window, gtk::Widget) {
    let a = ui::settings::content(sys, &ui::settings::Host::demo());
    let win = gtk::Window::builder()
        .default_width(shell::kit::widgets::APP_SIZE.0)
        .default_height(shell::kit::widgets::APP_SIZE.1)
        .child(&a.root)
        .build();
    win.present();
    broadway::pump(200);
    a.open("Bluetooth");
    broadway::pump(400);
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
    let _display = broadway::start(460);
    if gtk::init().is_err() {
        eprintln!("skipped: no display for the toolkit");
        return;
    }

    // --- pair_forget_rows_exist: every paired device has Forget; the page can add a device. ---
    let (a, win, shown) = open_page(Rc::new(Refusing(ActionError::Failed)));
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
    broadway::pump(100);
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
    broadway::pump(100);
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
    broadway::pump(100);
    assert!(!sheet.reveals_child(), "Cancel keeps the device");
    forgets[1].emit_clicked();
    broadway::pump(100);
    assert!(sheet.reveals_child(), "free again after Cancel");
    cancel.emit_clicked();
    broadway::pump(100);

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
    broadway::pump(400);
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
    let (a, win, shown) = open_page(Rc::new(Refusing(ActionError::Cancelled)));
    let device = of::<gtk::Button>(&shown)
        .into_iter()
        .find(|b| b.has_css_class("device-button"))
        .expect("a device row");
    device.emit_clicked();
    broadway::pump(300);
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
