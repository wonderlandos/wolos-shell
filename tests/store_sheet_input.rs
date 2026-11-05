// SPDX-License-Identifier: GPL-2.0-or-later
//! The Authenticate sheet keeps the input that answers it (QA-STORE-60): a held Return that
//! answered the sheet kept repeating into the page beneath (a sidebar row was activated twenty
//! times, the Store reloaded its page), and the click right after Allow clicked what the sheet
//! had covered. Real key and button events on a private Xvfb display (`support/xinput.rs`), not a
//! controller's signal emitted by hand: the bug is in what follows the first press. Skipped
//! without Xvfb, python3 or the toolkit. One `#[test]`: GTK lives on the thread that started it.
#[path = "support/broadway.rs"]
mod broadway;
#[path = "support/xinput.rs"]
mod xinput;
use gtk::prelude::*;
use shell::system::{Action, ActionError, Done, System};
use shell::ui;
use std::cell::Cell;
use std::rc::Rc;

/// wol-admind for an account that has to type its password; the change then ends at once with
/// `then` (the worst case: the sheet is gone before the finger is up).
struct Asks {
    then: Result<(), ActionError>,
}

impl System for Asks {
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
    fn perform(&self, _a: &Action) -> Result<(), ActionError> {
        Err(ActionError::NeedsAuth)
    }
    fn spawn_with(&self, _a: Action, pw: Option<&str>) -> Done {
        let (tx, rx) = async_channel::bounded(1);
        let _ = tx.try_send(pw.map_or(Err(ActionError::NeedsAuth), |_| self.then));
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

fn of<T: IsA<gtk::Widget>>(root: &gtk::Widget) -> Vec<T> {
    let mut v = Vec::new();
    all(root, &mut v);
    v
}

/// A Store in a window on `Discover`, its first Get clicked: the Authenticate sheet is up and the
/// password typed. Returns the window, the Store and the Allow button.
fn asked(then: Result<(), ActionError>) -> (gtk::Window, ui::store::Store, gtk::Button) {
    let s = ui::store::content(
        std::sync::Arc::new(Asks { then }),
        ui::store::Env {
            data_dirs: vec![],
            pkg_search: false,
            flatpaks: vec![],
            installed: Some(Default::default()),
        },
    );
    let root: gtk::Widget = s.aw.root.clone().upcast();
    let win = gtk::Window::builder()
        .child(&root)
        .default_width(900)
        .default_height(640)
        .build();
    win.present();
    broadway::pump(500);
    s.aw.select("Discover");
    broadway::pump(300);
    let get = of::<gtk::Button>(&root)
        .into_iter()
        .find(|b| b.label().as_deref() == Some("Get") && b.is_visible() && b.is_sensitive())
        .expect("a tile offers Get");
    get.emit_clicked();
    broadway::pump(500);
    let field = of::<gtk::PasswordEntry>(&root)
        .into_iter()
        .find(|f| {
            f.ancestor(gtk::Revealer::static_type())
                .and_downcast::<gtk::Revealer>()
                .is_some_and(|r| r.reveals_child())
        })
        .expect("the sheet is up with its password field");
    field.grab_focus();
    field.set_text("secret");
    broadway::pump(200);
    let allow = of::<gtk::Button>(&root)
        .into_iter()
        .find(|b| b.label().as_deref() == Some("Allow"))
        .expect("Allow");
    (win, s, allow)
}

/// Where the middle of `w` is in `win`.
fn centre(win: &gtk::Window, w: &gtk::Button) -> (i32, i32) {
    let p = gtk::graphene::Point::new(w.width() as f32 / 2.0, w.height() as f32 / 2.0);
    let p = w.compute_point(win, &p).expect("in the window");
    (p.x() as i32, p.y() as i32)
}

#[test]
fn the_input_that_answers_the_sheet_never_reaches_the_page_beneath() {
    let Some(mut input) = xinput::start(700) else {
        eprintln!("skipped: no Xvfb, python3 or libXtst");
        return;
    };
    if gtk::init().is_err() {
        eprintln!("skipped: no display for the toolkit");
        return;
    }

    // 1. Return held for a second and a half (the server repeats it): the first press answers
    // the sheet, the repeats are not the sidebar's.
    let (win, s, _allow) = asked(Err(ActionError::Failed));
    let activated = Rc::new(Cell::new(0));
    let a = activated.clone();
    s.aw.sidebar
        .connect_row_activated(move |_, _| a.set(a.get() + 1));
    let page = s.aw.content.visible_child_name();
    input.move_to(300, 400);
    broadway::pump(100);
    input.key_down("Return");
    broadway::pump(1500);
    input.key_up("Return");
    broadway::pump(100);
    assert_eq!(
        activated.get(),
        0,
        "the repeats of the Return that answered the sheet activated the selected sidebar row"
    );
    assert_eq!(s.aw.content.visible_child_name(), page, "the page stays");
    // The page is usable again once the finger is up.
    broadway::pump(800);
    assert!(s.aw.content.is_sensitive(), "the page takes input again");
    assert!(
        gtk::prelude::GtkWindowExt::focus(&win).is_some(),
        "the keyboard has a place to go"
    );
    win.destroy();
    broadway::pump(100);

    // 2. The second click of a double click on Allow lands on what the sheet covered: a
    // button standing there.
    let (win, s, allow) = asked(Ok(()));
    let beneath = gtk::Button::with_label("beneath");
    beneath.set_hexpand(true);
    beneath.set_vexpand(true);
    s.aw.content.add_named(&beneath, Some("beneath"));
    s.aw.content.set_visible_child_name("beneath");
    let clicks = Rc::new(Cell::new(0));
    let c = clicks.clone();
    beneath.connect_clicked(move |_| c.set(c.get() + 1));
    broadway::pump(200);
    let (x, y) = centre(&win, &allow);
    input.move_to(x, y);
    broadway::pump(100);
    for i in 0..2 {
        // A hand is never perfectly still: the second click starts one pixel on.
        input.move_to(x + i, y);
        broadway::pump(30);
        input.button_down();
        broadway::pump(40);
        input.button_up();
        broadway::pump(250);
    }
    broadway::pump(1500);
    assert_eq!(
        clicks.get(),
        0,
        "the click after Allow went through to the page the sheet covered"
    );
}
