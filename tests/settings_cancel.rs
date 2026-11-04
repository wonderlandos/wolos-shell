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
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

/// The config home of this test run: the settings this test saves go to a folder of its own (the
/// wallpaper stage turns it into a file to make a save fail).
fn config_dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("wol-settings-cancel-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&d);
    d
}

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
    // Esc reaches the window's key controllers first (capture), wherever the focus is.
    let stopped = press_esc(a);
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

/// QA-SET-22: a wallpaper whose save failed stayed the checked tile (the old one lost its tick
/// for a picture that was never saved). The save fails here because the config home is not a
/// folder.
fn wallpaper_failed_save_checks_the_old_tile(a: &shell::kit::widgets::AppWindow) {
    let shown = open(a, "Wallpaper");
    let tiles = of::<gtk::ToggleButton>(&shown);
    assert!(tiles.len() >= 2, "the shipped wallpapers show as tiles");
    let was = tiles
        .iter()
        .find(|t| t.is_active())
        .expect("one tile is checked")
        .clone();
    let other = tiles
        .iter()
        .find(|t| !t.is_active())
        .expect("another tile")
        .clone();
    // The config home becomes a file: no save can be made under it.
    let home = config_dir();
    let kept = home.with_extension("kept");
    std::fs::rename(&home, &kept).unwrap();
    std::fs::write(&home, "").unwrap();
    other.set_active(true);
    broadway::pump(300);
    std::fs::remove_file(&home).unwrap();
    std::fs::rename(&kept, &home).unwrap();
    assert!(
        was.is_active(),
        "the saved wallpaper is the checked tile again"
    );
    assert!(!other.is_active(), "the one that could not be saved is not");
}

/// QA-SET-64: Down in the search field walked the sidebar (Wi-Fi, Bluetooth, ...) and Return
/// opened the wrong pane; the hits were reachable by mouse only. Down now puts the focus on the
/// first hit, and the hit is a button Return activates.
fn search_down_walks_the_hits(a: &shell::kit::widgets::AppWindow) {
    use gtk::gdk::{Key, ModifierType};
    let field = a
        .sidebar
        .row_at_index(0)
        .and_then(|r| r.child())
        .and_downcast::<gtk::SearchEntry>()
        .expect("the search field");
    field.set_text("password");
    broadway::pump(300);
    let results = a
        .content
        .child_by_name("Search Results")
        .expect("the hits page");
    let body = of::<gtk::Box>(&results)
        .into_iter()
        .next()
        .expect("the hits box");
    assert!(body.is_mapped(), "the hits are on screen");
    // Other keys, and Down with a modifier, stay the field's.
    assert_eq!(
        ui::settings::search_key(&field, &body, Key::a, ModifierType::empty()),
        gtk::glib::Propagation::Proceed
    );
    assert_eq!(
        ui::settings::search_key(&field, &body, Key::Down, ModifierType::CONTROL_MASK),
        gtk::glib::Propagation::Proceed
    );
    assert_eq!(
        ui::settings::search_key(&field, &body, Key::Down, ModifierType::empty()),
        gtk::glib::Propagation::Stop,
        "Down is taken"
    );
    broadway::pump(100);
    let win = a.root.root().and_downcast::<gtk::Window>().expect("window");
    let focus = gtk::prelude::GtkWindowExt::focus(&win).expect("something has focus");
    assert!(
        focus.is::<gtk::Button>() && focus.is_ancestor(&results),
        "the first hit has the focus, not a sidebar row: {focus:?}"
    );
    // Nothing typed: nothing to walk.
    field.set_text("");
    broadway::pump(300);
    assert_eq!(
        ui::settings::search_key(&field, &body, Key::Down, ModifierType::empty()),
        gtk::glib::Propagation::Proceed
    );
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

/// Esc as the window's key controllers see it, in the order they were added: true when one took it.
fn press_esc(a: &shell::kit::widgets::AppWindow) -> bool {
    let mut keys: Vec<gtk::EventControllerKey> = a
        .root
        .observe_controllers()
        .iter::<gtk::glib::Object>()
        .filter_map(Result::ok)
        .filter_map(|c| c.downcast::<gtk::EventControllerKey>().ok())
        .collect();
    // As GTK delivers a key: the capture phase first, then the bubble phase.
    keys.sort_by_key(|k| k.propagation_phase() != gtk::PropagationPhase::Capture);
    let esc = gtk::glib::translate::IntoGlib::into_glib(gtk::gdk::Key::Escape);
    let stopped = keys.iter().any(|k| {
        k.emit_by_name::<bool>(
            "key-pressed",
            &[&esc, &0u32, &gtk::gdk::ModifierType::empty()],
        )
    });
    broadway::pump(100);
    stopped
}

/// QA-VIS2-37: Esc closes a sheet (New account, Join, Change Password) and a pane change closes
/// it too, or cancels the Authenticate sheet; they used to pile up behind each other. Esc with no
/// sheet up goes on to whoever wanted it.
fn sheets_close_on_esc_and_on_a_pane_change(
    a: &shell::kit::widgets::AppWindow,
    root: &gtk::Widget,
    auth: &gtk::Revealer,
) {
    open(a, "Wi-Fi");
    let plain = of::<gtk::Revealer>(root)
        .into_iter()
        .find(|r| r.has_css_class("toolbar-sheet") && r != auth)
        .expect("a plain toolbar sheet");
    assert!(!press_esc(a), "no sheet up: Esc is nobody's here");
    plain.set_reveal_child(true);
    broadway::pump(100);
    assert!(press_esc(a), "Esc is the sheet's");
    assert!(!plain.reveals_child(), "Esc closes the sheet");
    plain.set_reveal_child(true);
    broadway::pump(100);
    open(a, "Keyboard");
    assert!(!plain.reveals_child(), "a pane change closes the sheet");
    // The Authenticate sheet is cancelled by a pane change, and the switch flips back.
    let power = flip_wifi(a, auth);
    open(a, "Keyboard");
    assert!(!auth.reveals_child(), "a pane change cancels the sheet");
    assert!(power.is_active(), "the switch flipped back");
    assert_eq!(blames(root), Vec::<String>::new());
}

#[test]
fn cancel_has_no_incorrect_password() {
    let _display = broadway::start_with_config(460, Some(&config_dir()));
    if gtk::init().is_err() {
        eprintln!("skipped: no display for the toolkit");
        return;
    }
    let sys = Arc::new(Asking::default());
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
    sheets_close_on_esc_and_on_a_pane_change(&a, &root, &sheet);
    act_without_status_shows_checking(&a, &root, &sys);
    search_paste_capped_esc_clears(&a);
    search_down_walks_the_hits(&a);
    wallpaper_failed_save_checks_the_old_tile(&a);
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
