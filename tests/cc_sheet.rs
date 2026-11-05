// SPDX-License-Identifier: GPL-2.0-or-later
//! Control Center and the Authenticate sheet (QA-SHEET-1): Esc took the whole panel away while
//! the sheet and the change behind it stayed, and the next open showed a stale sheet whose Allow
//! ran the action of the visit before. Now Esc takes the sheet down first (the next Esc hides the
//! panel), and hiding the panel drops the sheet and what waited behind it. Needs the toolkit: a
//! private Broadway display, skipped without one. One `#[test]`: GTK lives on the thread that
//! started it.
#[path = "support/broadway.rs"]
mod broadway;
use gtk::prelude::*;
use shell::system::{Action, ActionError, Done, System};
use shell::ui;
use std::sync::Mutex;

/// wol-admind for an account that has to type its password: NeedsAuth, every time; what was sent
/// is kept.
#[derive(Default)]
struct Asking {
    sent: Mutex<Vec<(Action, Option<String>)>>,
    /// Retries with a typed password wait here for the test to refuse them, the way wol-admind
    /// answers a wrong password after its fixed delay (QA-A9-SHELL-02).
    retries: Mutex<Vec<async_channel::Sender<Result<(), ActionError>>>>,
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
    fn night_light(&self) -> Option<bool> {
        Some(false)
    }
    fn spawn_with(&self, a: Action, pw: Option<&str>) -> Done {
        self.sent.lock().unwrap().push((a, pw.map(String::from)));
        let (tx, rx) = async_channel::bounded(1);
        if pw.is_some() {
            self.retries.lock().unwrap().push(tx);
        } else {
            let _ = tx.try_send(Err(ActionError::NeedsAuth));
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

fn auth_sheet(win: &gtk::Window) -> gtk::Revealer {
    of::<gtk::Revealer>(win)
        .into_iter()
        .find(|r| {
            of::<gtk::Label>(r)
                .iter()
                .any(|l| l.label() == "Authenticate")
        })
        .expect("the Authenticate sheet")
}

/// The Night Shift tile's button: it asks for the password.
fn night_shift(win: &gtk::Window) -> gtk::Button {
    of::<gtk::Button>(win)
        .into_iter()
        .find(|b| {
            of::<gtk::Label>(b)
                .iter()
                .any(|l| l.label() == "Night Shift")
        })
        .expect("the Night Shift tile")
}

/// Esc as the window's key controllers see it, in the order they were added: true when one of
/// them took the key.
fn press_esc(win: &gtk::Window) -> bool {
    let mut keys: Vec<gtk::EventControllerKey> = win
        .observe_controllers()
        .iter::<gtk::glib::Object>()
        .filter_map(Result::ok)
        .filter_map(|c| c.downcast::<gtk::EventControllerKey>().ok())
        .collect();
    // As GTK delivers a key: the capture phase first, then the bubble phase.
    keys.sort_by_key(|k| k.propagation_phase() != gtk::PropagationPhase::Capture);
    let esc = gtk::glib::translate::IntoGlib::into_glib(gtk::gdk::Key::Escape);
    keys.iter().any(|k| {
        k.emit_by_name::<bool>(
            "key-pressed",
            &[&esc, &0u32, &gtk::gdk::ModifierType::empty()],
        )
    })
}

#[test]
fn escape_and_hide_drop_the_authenticate_sheet() {
    let _display = broadway::start(660);
    if gtk::init().is_err() {
        eprintln!("skipped: no display for the toolkit");
        return;
    }
    let sys = std::sync::Arc::new(Asking::default());
    let win = gtk::Window::new();
    ui::control_center::dress(&win, sys.clone());
    win.present();
    broadway::pump(300);
    let sheet = auth_sheet(&win);

    // Esc with the sheet up takes the sheet down and leaves the panel.
    night_shift(&win).emit_clicked();
    broadway::pump(200);
    assert!(sheet.reveals_child(), "the tile asks for the password");
    assert!(press_esc(&win), "Esc is taken");
    broadway::pump(100);
    assert!(!sheet.reveals_child(), "Esc cancels the sheet");
    assert!(win.is_visible(), "the panel stays for the first Esc");

    // Hiding the panel with the sheet up drops the sheet and the change behind it.
    night_shift(&win).emit_clicked();
    broadway::pump(200);
    assert!(sheet.reveals_child());
    sheet.set_reveal_child(true);
    let field = of::<gtk::PasswordEntry>(&sheet)
        .into_iter()
        .next()
        .expect("the password field");
    field.set_text("typed for the last visit");
    win.set_visible(false);
    broadway::pump(100);
    assert!(!sheet.reveals_child(), "the sheet goes with the panel");
    assert_eq!(field.text(), "", "and what was typed in it");
    // The next visit: nothing waits, so Allow has nothing to run.
    let before = sys.sent.lock().unwrap().len();
    win.present();
    broadway::pump(200);
    assert!(!sheet.reveals_child(), "no stale sheet on the next open");
    let allow = of::<gtk::Button>(&sheet)
        .into_iter()
        .find(|b| of::<gtk::Label>(b).iter().any(|l| l.label() == "Allow"))
        .expect("Allow");
    allow.emit_clicked();
    broadway::pump(200);
    assert_eq!(
        sys.sent.lock().unwrap().len(),
        before,
        "Allow ran nothing from the visit before"
    );
    refusal_after_the_panel_hid(&win, &sys, &sheet);
    win.close();
}

/// QA-A9-SHELL-02: a wrong password is out (wol-admind refuses after its delay) and the panel
/// hides. The late refusal must not ask again inside the hidden panel: the next open shows no
/// sheet and Allow sends nothing.
fn refusal_after_the_panel_hid(win: &gtk::Window, sys: &Asking, sheet: &gtk::Revealer) {
    night_shift(win).emit_clicked();
    broadway::pump(200);
    assert!(sheet.reveals_child(), "the tile asks for the password");
    let allow = || {
        of::<gtk::Button>(sheet)
            .into_iter()
            .find(|b| of::<gtk::Label>(b).iter().any(|l| l.label() == "Allow"))
            .expect("Allow")
    };
    allow().emit_clicked();
    broadway::pump(100);
    assert_eq!(
        sys.retries.lock().unwrap().len(),
        1,
        "the wrong password is out"
    );
    win.set_visible(false);
    broadway::pump(100);
    let tx = sys.retries.lock().unwrap().pop().expect("the retry");
    tx.try_send(Err(ActionError::Denied)).unwrap();
    broadway::pump(100);
    assert!(
        !sheet.reveals_child(),
        "the refusal does not reopen the sheet"
    );
    win.present();
    broadway::pump(200);
    assert!(!sheet.reveals_child(), "no stale sheet on the next open");
    let before = sys.sent.lock().unwrap().len();
    allow().emit_clicked();
    broadway::pump(200);
    assert_eq!(
        sys.sent.lock().unwrap().len(),
        before,
        "Allow ran nothing from the visit before"
    );
}
