// SPDX-License-Identifier: GPL-2.0-or-later
//! System Settings when a change asks for the administrator's password (QA hunt 1): Cancel on
//! the Authenticate sheet flips the Wi-Fi switch back with no "Incorrect password" sentence
//! (QA-HUNT-1 E1, QA-HUNT-10 G1); the sheet is modal and Esc cancels it wherever the focus is
//! (QA-LIVE-8); a change without a status line of its own shows the shared "Checking…" while it
//! waits (QA-HUNT-9 C2); the search field caps a paste and Esc clears it (QA-LIVE-10); the Wi-Fi
//! page's watcher goes with the window (QA-SET-8). Needs the toolkit: runs on a private Broadway
//! display (the golden harness's `gtk4-broadwayd`), and skips without one. One `#[test]`: GTK
//! lives on the thread that started it; the stages are the functions below.
#[path = "support/broadway.rs"]
mod broadway;
use gtk::prelude::*;
use shell::system::{Action, ActionError, BtDevice, Done, System};
use shell::ui;
use std::rc::Rc;
use std::sync::Mutex;

/// wol-admind as an administrator meets it: the radios want the password (NeedsAuth until one
/// is typed); a Bluetooth device's connect waits until the test answers it.
#[derive(Default)]
struct Asking {
    held: Mutex<Vec<async_channel::Sender<Result<(), ActionError>>>>,
    /// The senders of every `watch()` handed out (a page's snapshot loop listens on each).
    watchers: Mutex<Vec<async_channel::Sender<()>>>,
    /// Answer nothing: the channel closes (a worker that died).
    dead: std::sync::atomic::AtomicBool,
}

impl System for Asking {
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
        vec![BtDevice {
            addr: "aa:bb:cc:dd:ee:01".into(),
            name: "MX Keys".into(),
            connected: false,
        }]
    }
    fn watch(&self) -> Option<async_channel::Receiver<()>> {
        let (tx, rx) = async_channel::unbounded();
        self.watchers.lock().unwrap().push(tx);
        Some(rx)
    }
    fn spawn_with(&self, a: Action, pw: Option<&str>) -> Done {
        let (tx, rx) = async_channel::bounded(1);
        if self.dead.load(std::sync::atomic::Ordering::Relaxed) {
            drop(tx);
            return rx;
        }
        match (&a, pw) {
            (Action::BtConnect(..), _) => self.held.lock().unwrap().push(tx),
            (_, None) => {
                let _ = tx.try_send(Err(ActionError::NeedsAuth));
            }
            (_, Some(_)) => {
                let _ = tx.try_send(Ok(()));
            }
        }
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

/// The Authenticate sheet: the revealer titled "Authenticate" (other sheets have password
/// fields too: Join, Add User, Change Password).
fn auth_sheet(root: &gtk::Widget) -> gtk::Revealer {
    of::<gtk::Revealer>(root)
        .into_iter()
        .find(|r| {
            of::<gtk::Label>(r)
                .iter()
                .any(|l| l.label() == "Authenticate")
        })
        .expect("the Authenticate sheet")
}

/// Every label on screen that blames the password.
fn blames(root: &gtk::Widget) -> Vec<String> {
    of::<gtk::Label>(root)
        .into_iter()
        .filter(|l| l.is_visible() && l.label().to_lowercase().contains("incorrect"))
        .map(|l| l.label().to_string())
        .collect()
}

/// The page on screen after opening `page`.
fn open(a: &shell::kit::widgets::AppWindow, page: &str) -> gtk::Widget {
    a.open(page);
    broadway::pump(200);
    a.content.visible_child().expect("a page shows")
}

/// Flips the Wi-Fi power switch off: the sheet asks and the switch waits.
fn flip_wifi(a: &shell::kit::widgets::AppWindow, sheet: &gtk::Revealer) -> gtk::Switch {
    let shown = open(a, "Wi-Fi");
    let power = of::<gtk::Switch>(&shown)
        .into_iter()
        .next()
        .expect("the power switch");
    assert!(power.is_active() && power.state(), "Wi-Fi: on at start");
    power.set_active(false);
    broadway::pump(100);
    assert!(sheet.reveals_child(), "Wi-Fi: the sheet asks");
    assert!(!power.is_sensitive(), "Wi-Fi: the switch waits");
    power
}

/// QA-HUNT-1: Cancel said "Incorrect password. Try again." with no password typed; QA-HUNT-10:
/// the radio switch stayed off after Cancel.
fn cancel_says_nothing_wrong(
    a: &shell::kit::widgets::AppWindow,
    root: &gtk::Widget,
    sheet: &gtk::Revealer,
) {
    let power = flip_wifi(a, sheet);
    let cancel = of::<gtk::Button>(sheet)
        .into_iter()
        .find(|b| b.label().as_deref() == Some("Cancel"))
        .expect("Cancel on the sheet");
    cancel.emit_clicked();
    broadway::pump(100);
    assert!(!sheet.reveals_child(), "Cancel closes the sheet");
    assert!(
        power.is_active() && power.state() && power.is_sensitive(),
        "the switch flips back on"
    );
    assert_eq!(blames(root), Vec::<String>::new());
}

/// QA-HUNT-1, the reported repro: Sharing > Remote Login > Cancel printed "Incorrect password.
/// Try again." under the section.
fn cancel_on_sharing_says_nothing_wrong(
    a: &shell::kit::widgets::AppWindow,
    root: &gtk::Widget,
    sheet: &gtk::Revealer,
) {
    let shown = open(a, "Sharing");
    broadway::pump(300);
    let remote = of::<gtk::Switch>(&shown)
        .into_iter()
        .next()
        .expect("the Remote Login switch");
    let was = remote.is_active();
    remote.set_active(!was);
    broadway::pump(100);
    assert!(sheet.reveals_child(), "Sharing: the sheet asks");
    of::<gtk::Button>(sheet)
        .into_iter()
        .find(|b| b.label().as_deref() == Some("Cancel"))
        .expect("Cancel on the sheet")
        .emit_clicked();
    broadway::pump(100);
    assert_eq!(remote.is_active(), was, "Remote Login flips back");
    assert_eq!(
        blames(root),
        Vec::<String>::new(),
        "Sharing: no password was typed"
    );
}

/// QA-LIVE-8: a click behind the sheet navigated while it stayed up, and Esc did nothing once
/// the focus was elsewhere.
fn sheet_is_modal_esc_cancels(
    a: &shell::kit::widgets::AppWindow,
    root: &gtk::Widget,
    sheet: &gtk::Revealer,
) {
    let power = flip_wifi(a, sheet);
    assert!(
        !a.sidebar.is_sensitive(),
        "the sidebar takes no click behind the sheet"
    );
    assert!(!a.content.is_sensitive(), "nor the page");
    // Esc reaches the window's key controller first (capture), wherever the focus is.
    let esc = a
        .root
        .observe_controllers()
        .iter::<gtk::glib::Object>()
        .filter_map(Result::ok)
        .find_map(|c| c.downcast::<gtk::EventControllerKey>().ok())
        .expect("the window's Esc");
    let stopped: bool = esc.emit_by_name(
        "key-pressed",
        &[
            &gtk::glib::translate::IntoGlib::into_glib(gtk::gdk::Key::Escape),
            &0u32,
            &gtk::gdk::ModifierType::empty(),
        ],
    );
    broadway::pump(100);
    assert!(stopped, "Esc is the sheet's");
    assert!(!sheet.reveals_child(), "Esc cancels");
    assert!(
        a.sidebar.is_sensitive() && a.content.is_sensitive(),
        "input is back"
    );
    assert!(
        power.is_active() && power.is_sensitive(),
        "the switch flipped back"
    );
    assert_eq!(blames(root), Vec::<String>::new());
}

/// QA-HUNT-9: a plain `ctx.act` with no status line (a Bluetooth device row) showed nothing
/// while wol-admind worked.
fn act_without_status_shows_checking(
    a: &shell::kit::widgets::AppWindow,
    root: &gtk::Widget,
    sys: &Asking,
) {
    let shown = open(a, "Bluetooth");
    let checking = of::<gtk::Revealer>(root)
        .into_iter()
        .find(|r| {
            of::<gtk::Label>(r)
                .iter()
                .any(|l| l.label() == shell::copy::CHECKING)
        })
        .expect("the shared Checking… line");
    let device = of::<gtk::Button>(&shown)
        .into_iter()
        .find(|b| b.has_css_class("device-button"))
        .expect("a device row");
    device.emit_clicked();
    broadway::pump(100);
    assert!(!checking.reveals_child(), "not before a moment passed");
    broadway::pump(400);
    assert!(checking.reveals_child(), "a late change says Checking…");
    let tx = sys
        .held
        .lock()
        .unwrap()
        .pop()
        .expect("the connect was sent");
    tx.try_send(Ok(())).unwrap();
    broadway::pump(100);
    assert!(!checking.reveals_child(), "gone once it answered");
}

/// Review: a closed answer channel left the switch grey for good (the future returned early).
fn dead_worker_puts_the_switch_back(a: &shell::kit::widgets::AppWindow, sys: &Asking) {
    let shown = open(a, "Wi-Fi");
    let power = of::<gtk::Switch>(&shown)
        .into_iter()
        .next()
        .expect("the power switch");
    sys.dead.store(true, std::sync::atomic::Ordering::Relaxed);
    let was = power.is_active();
    power.set_active(!was);
    broadway::pump(200);
    sys.dead.store(false, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(power.is_active(), was, "no answer: the switch goes back");
    assert!(power.is_sensitive(), "and takes input again");
}

/// QA-LIVE-10: a 5000-character paste stayed in the field and Esc did not clear it.
fn search_paste_capped_esc_clears(a: &shell::kit::widgets::AppWindow) {
    let field = a
        .sidebar
        .row_at_index(0)
        .and_then(|r| r.child())
        .and_downcast::<gtk::SearchEntry>()
        .expect("the search field");
    field.set_text(&"wallpaper ".repeat(500));
    broadway::pump(300);
    assert!(field.text().chars().count() <= 64, "{}", field.text().len());
    field.emit_by_name::<()>("stop-search", &[]);
    broadway::pump(300);
    assert_eq!(field.text(), "", "Esc clears the field");
}

#[test]
fn cancel_has_no_incorrect_password() {
    let _display = broadway::start(460);
    if gtk::init().is_err() {
        eprintln!("skipped: no display for the toolkit");
        return;
    }
    let sys = Rc::new(Asking::default());
    let a = ui::settings::content(sys.clone(), &ui::settings::Host::demo());
    let win = gtk::Window::builder()
        .default_width(shell::kit::widgets::APP_SIZE.0)
        .default_height(shell::kit::widgets::APP_SIZE.1)
        .child(&a.root)
        .build();
    win.present();
    broadway::pump(200);
    let root: gtk::Widget = a.root.clone().upcast();
    let sheet = auth_sheet(&root);
    cancel_on_sharing_says_nothing_wrong(&a, &root, &sheet);
    cancel_says_nothing_wrong(&a, &root, &sheet);
    sheet_is_modal_esc_cancels(&a, &root, &sheet);
    act_without_status_shows_checking(&a, &root, &sys);
    search_paste_capped_esc_clears(&a);
    dead_worker_puts_the_switch_back(&a, &sys);
    wifi_watcher_goes_with_its_page(&a, &sys);
    win.close();
}

/// QA-SET-8: an `Env::refresh` builds the Wi-Fi page again; the old page's snapshot loop kept
/// running for good. Removing the page (what the rebuild does to its holder) ends its loop.
fn wifi_watcher_goes_with_its_page(a: &shell::kit::widgets::AppWindow, sys: &Asking) {
    // The stack shows a scroller; the holder is the box the page is built into.
    let mut holder = open(a, "Wi-Fi");
    while holder.downcast_ref::<gtk::Box>().is_none() {
        holder = holder.first_child().expect("the Wi-Fi holder");
    }
    let holder = holder.downcast::<gtk::Box>().unwrap();
    let open_now = || {
        sys.watchers
            .lock()
            .unwrap()
            .iter()
            .filter(|w| !w.is_closed())
            .count()
    };
    assert_eq!(open_now(), 1, "the Wi-Fi page listens");
    while let Some(c) = holder.first_child() {
        holder.remove(&c);
    }
    broadway::pump(300);
    assert_eq!(open_now(), 0, "its loop ended with the page");
}
