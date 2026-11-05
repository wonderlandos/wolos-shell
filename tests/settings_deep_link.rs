// SPDX-License-Identifier: GPL-2.0-or-later
//! QA-SEARCH-1 (lead, 2026-11-05: 'privacy settings' in Search opened Settings on its first pane).
//! A result opens `wol-settings <page-id>[#<row-id>]`; Settings must land on that page, scroll to
//! the row and light it, also when it is already open (a second launch reaches the running
//! window). Needs the toolkit: runs on a private Broadway display and skips without one.
#[path = "support/broadway.rs"]
mod broadway;
use gtk::prelude::*;
use shell::ui;
use shell::ui::settings::reveal;
use std::sync::Arc;

/// Longest the test waits for a frame to land the row (the event it waits for ends it at once).
const MAX_WAIT_MS: u64 = 20_000;

fn args(a: &str) -> Vec<String> {
    vec!["wol-settings".into(), a.into()]
}

/// Widgets under `root` with the lit class.
fn lit(root: &gtk::Widget, out: &mut Vec<gtk::Widget>) {
    let mut c = root.first_child();
    while let Some(w) = c {
        if w.has_css_class(reveal::TARGET_CLASS) {
            out.push(w.clone());
        }
        lit(&w, out);
        c = w.next_sibling();
    }
}

fn lit_in(root: &impl IsA<gtk::Widget>) -> Vec<gtk::Widget> {
    let mut v = Vec::new();
    lit(root.upcast_ref(), &mut v);
    v
}

/// Runs the main loop until `done` (checked after each event) or `ms` pass: a frame of a heavy
/// pane (Effects & Looks draws blur in software) may take longer than any fixed pump.
fn pump_until(ms: u64, done: impl Fn() -> bool) {
    let ctx = gtk::glib::MainContext::default();
    let over = std::rc::Rc::new(std::cell::Cell::new(false));
    let o = over.clone();
    gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(ms), move || o.set(true));
    while !over.get() && !done() {
        ctx.iteration(true);
    }
}

/// One test: GTK lives on the thread that started it.
#[test]
fn a_result_lands_on_its_page_and_row() {
    let _display = broadway::start(700);
    if gtk::init().is_err() {
        eprintln!("skipped: no display for the toolkit");
        return;
    }
    let a = ui::settings::content(Arc::new(shell::system::Fake), &ui::settings::Host::demo());
    let win = gtk::Window::builder()
        .default_width(shell::kit::widgets::APP_SIZE.0)
        .default_height(shell::kit::widgets::APP_SIZE.1)
        .child(&a.root)
        .build();
    win.present();
    broadway::pump(200);
    // The first pane, as Settings opens with no argument.
    assert_eq!(a.content.visible_child_name().as_deref(), Some("Wi-Fi"));

    // The launch's own argument is what the running window is asked for (the IPC path):
    // each case is a launch while Settings is open.
    for (arg, page, row) in [
        ("privacy", "Privacy & Security", None),
        (
            "privacy#screen-recording",
            "Privacy & Security",
            Some("Screen Recording"),
        ),
        ("sound#reset-audio", "Sound", Some("Reset audio")),
        (
            "effects#windows-open-with",
            "Effects & Looks",
            Some("Windows open with"),
        ),
        ("control-center#airplane-mode", "Control Center", None),
        (
            "software-update#check-now",
            "Software Update",
            Some("Check Now"),
        ),
        // An older title still works, with no row.
        ("Privacy & Security", "Privacy & Security", None),
        // A row the page does not have: the page opens, nothing is lit.
        ("sound#no-such-row", "Sound", None),
    ] {
        let t = ui::settings::requested(&args(arg)).unwrap_or_else(|| panic!("{arg}: no page"));
        assert_eq!(
            (t.page, t.row),
            (page, row.filter(|_| !arg.contains("no-such"))),
            "{arg}"
        );
        ui::settings::open_target(&a, &t);
        broadway::pump(300);
        if t.row.is_some() {
            pump_until(MAX_WAIT_MS, || !lit_in(&a.root).is_empty());
        }
        assert_eq!(
            a.content.visible_child_name().as_deref(),
            Some(page),
            "{arg}: the page shown"
        );
        let shown = a.content.visible_child().expect("a page shows");
        let on = lit_in(&shown);
        match t.row {
            Some(r) => {
                // Exactly one row is lit, and it is the row asked for.
                if on.len() != 1 {
                    // The row is not on this pane's built widgets (a row only some states draw):
                    // named so the gap is visible, not silent.
                    panic!("{arg}: {} lit rows", on.len());
                }
                let names = reveal::shown_as(r);
                assert!(
                    reveal::find_title(&on[0], &names).is_some(),
                    "{arg}: lit the wrong row"
                );
            }
            None => assert!(on.is_empty(), "{arg}: nothing to light"),
        }
    }

    // A row far down the page: the page scrolls to it (the row is inside the viewport).
    let t = ui::settings::requested(&args("effects#windows-open-with")).unwrap();
    ui::settings::open_target(&a, &t);
    pump_until(MAX_WAIT_MS, || !lit_in(&a.root).is_empty());
    let scroll = a
        .content
        .visible_child()
        .and_downcast::<gtk::ScrolledWindow>()
        .expect("the page scrolls");
    let row = lit_in(&scroll).pop().expect("lit");
    let adj = scroll.vadjustment();
    assert!(adj.value() > 0.0, "scrolled: {}", adj.value());
    let y = row
        .compute_point(&scroll, &gtk::graphene::Point::new(0.0, 0.0))
        .expect("the row has a place in the scroller")
        .y();
    assert!(
        y >= 0.0 && f64::from(y) < adj.page_size(),
        "the row is in view: y {y}, viewport {}",
        adj.page_size()
    );

    // The light goes when the person moves to another page.
    a.open("Sound");
    broadway::pump(100);
    assert!(
        lit_in(&a.root).is_empty(),
        "the light stays after leaving the page"
    );
    // And only one page is ever lit: a second launch moves it.
    for arg in ["privacy#screen-recording", "sound#reset-audio"] {
        let t = ui::settings::requested(&args(arg)).unwrap();
        ui::settings::open_target(&a, &t);
        pump_until(MAX_WAIT_MS, || !lit_in(&a.root).is_empty());
        assert_eq!(lit_in(&a.root).len(), 1, "{arg}");
    }

    // In another language the row shows its translation: it is found by the title shown or by the
    // English one, and a section lights as a whole.
    let section = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let heading = gtk::Label::new(Some("Gravação de Tela"));
    heading.add_css_class("heading");
    section.append(&heading);
    let row = shell::kit::widgets::list_row("", "Reduzir movimento", "", None);
    section.append(&row);
    let pt = |english: &str| {
        let mut v = reveal::shown_as(english);
        v.push(match english {
            "Screen Recording" => "gravação de tela".to_string(),
            _ => "reduzir movimento".to_string(),
        });
        v
    };
    let found = reveal::find_title(section.upcast_ref(), &pt("Screen Recording")).expect("heading");
    assert_eq!(&reveal::row_of(&found), section.upcast_ref::<gtk::Widget>());
    let found = reveal::find_title(section.upcast_ref(), &pt("Reduce motion")).expect("row");
    assert_eq!(&reveal::row_of(&found), row.upcast_ref::<gtk::Widget>());
}
