// SPDX-License-Identifier: GPL-2.0-or-later
//! Settings > Displays beyond the two-display render (settings.rs): the empty state, a computer
//! without hyprsunset, and the Keep / Undo countdown a changed mode waits for (QA-DESIGN-28,
//! QA-SET-23, QA-LIVE-13).
use super::{Case, find_type, pump};
use gtk::prelude::*;
use shell::kit::widgets as w;
use shell::status::{Audio, Battery, Network};
use shell::system::{Action, ActionError, Monitor, System};
use shell::ui;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

/// [`shell::system::Fake`] with the displays and Night Shift this test chooses; every action is
/// taken and written down.
struct Rigged {
    monitors: Vec<Monitor>,
    night: Option<bool>,
    seen: Arc<Mutex<Vec<Action>>>,
}

impl Rigged {
    fn new(monitors: Vec<Monitor>, night: Option<bool>) -> Self {
        Rigged {
            monitors,
            night,
            seen: Arc::default(),
        }
    }
}

impl System for Rigged {
    fn battery(&self) -> Option<Battery> {
        shell::system::Fake.battery()
    }
    fn audio(&self) -> Audio {
        shell::system::Fake.audio()
    }
    fn network(&self) -> Network {
        shell::system::Fake.network()
    }
    fn brightness(&self) -> Option<u8> {
        shell::system::Fake.brightness()
    }
    fn night_light(&self) -> Option<bool> {
        self.night
    }
    fn monitors(&self) -> Vec<Monitor> {
        self.monitors.clone()
    }
    fn perform(&self, a: &Action) -> Result<(), ActionError> {
        self.seen.lock().unwrap().push(a.clone());
        Ok(())
    }
}

fn laptop() -> Vec<Monitor> {
    shell::system::Fake.monitors()
}

fn open(sys: Rigged) -> (w::AppWindow, gtk::Widget) {
    let a = ui::settings::content(Rc::new(sys), &ui::settings::Host::demo());
    a.open("Displays");
    pump(300);
    let p = a.content.visible_child().expect("a page shows");
    (a, p)
}

fn labels(w: &gtk::Widget) -> Vec<String> {
    let mut v = Vec::new();
    find_type::<gtk::Label>(w, &mut v);
    v.iter().map(|l| l.label().to_string()).collect()
}

fn buttons(w: &gtk::Widget) -> Vec<gtk::Button> {
    let mut v = Vec::new();
    find_type::<gtk::Button>(w, &mut v);
    v
}

fn drop_downs(w: &gtk::Widget) -> Vec<gtk::DropDown> {
    let mut v = Vec::new();
    find_type::<gtk::DropDown>(w, &mut v);
    v
}

pub fn cases() -> Vec<Case> {
    let mut v = Vec::new();
    for (name, sys) in [
        ("displays-empty-state-copy", None),
        ("displays-no-hyprsunset-no-row", Some(None)),
    ] {
        v.push(Case {
            name: name.into(),
            size: w::APP_SIZE,
            class: "preview-app",
            wallpaper: false,
            build: Box::new(move || {
                let r = match sys {
                    None => Rigged::new(Vec::new(), Some(false)),
                    Some(night) => Rigged::new(laptop(), night),
                };
                open(r).0.root.clone().upcast()
            }),
        });
    }
    v
}

pub fn checks() {
    empty_state_says_what_happened();
    no_night_shift_row_without_hyprsunset();
    a_changed_mode_waits_for_keep();
    nothing_is_written_before_keep();
    no_answer_goes_back_by_itself();
    closing_the_window_goes_back();
}

/// QA-DESIGN-28: the page is open, so the desktop runs; "once the desktop is running" blamed the
/// wrong thing. It says the list could not be read and offers Try Again.
fn empty_state_says_what_happened() {
    let (_a, p) = open(Rigged::new(Vec::new(), Some(false)));
    let l = labels(&p);
    assert!(
        l.iter().any(|t| t.contains("couldn’t be read")),
        "title: {l:?}"
    );
    assert!(
        !l.iter().any(|t| t.contains("once the desktop is running")),
        "the old sentence is gone: {l:?}"
    );
    assert!(
        buttons(&p)
            .iter()
            .any(|b| b.label().as_deref() == Some("Try Again")),
        "Try Again"
    );
}

/// QA-SET-23: no hyprsunset, no Night Shift row (it flipped back when toggled).
fn no_night_shift_row_without_hyprsunset() {
    let (_a, p) = open(Rigged::new(laptop(), None));
    assert!(!labels(&p).iter().any(|t| t == "Night Shift"));
    assert!(
        labels(&p).iter().any(|t| t == "Resolution"),
        "the displays stay"
    );
    let (_a, p) = open(Rigged::new(laptop(), Some(false)));
    assert!(labels(&p).iter().any(|t| t == "Night Shift"));
}

/// The sheet's revealer when it reveals, found by its title.
fn keep_sheet(root: &gtk::Widget) -> Option<gtk::Revealer> {
    let mut v = Vec::new();
    find_type::<gtk::Label>(root, &mut v);
    v.iter()
        .find(|l| l.label() == "Keep these display settings?")
        .and_then(|l| l.ancestor(gtk::Revealer::static_type()))
        .and_downcast::<gtk::Revealer>()
}

/// QA-LIVE-13: a new resolution asks "Keep these display settings?"; Undo puts the display back
/// with the rule it ran with and Keep leaves it. A new arrangement alone asks nothing.
fn a_changed_mode_waits_for_keep() {
    let sys = Rigged::new(laptop(), Some(false));
    let seen = sys.seen.clone();
    let (a, p) = open(sys);
    let root: gtk::Widget = a.root.clone().upcast();
    assert!(
        keep_sheet(&root).is_none_or(|r| !r.reveals_child()),
        "nothing to keep before a change"
    );
    // Resolution is the first menu of the first display.
    let res = drop_downs(&p).into_iter().next().expect("resolution menu");
    res.set_selected(1);
    pump(200);
    let sheet = keep_sheet(&root).expect("the Keep sheet exists");
    assert!(sheet.reveals_child(), "asks to keep the new resolution");
    {
        let s = seen.lock().unwrap();
        assert!(
            matches!(s.last(), Some(Action::Monitor(r)) if r.starts_with("eDP-1,1920x1200")),
            "the new mode was asked for: {s:?}"
        );
    }
    let go = |label: &str| {
        buttons(&root)
            .into_iter()
            .find(|b| b.label().as_deref() == Some(label))
            .unwrap_or_else(|| panic!("button {label}"))
    };
    go("Undo").emit_clicked();
    pump(200);
    assert!(!sheet.reveals_child(), "the sheet goes");
    {
        let s = seen.lock().unwrap();
        assert!(
            matches!(s.last(), Some(Action::Monitor(r)) if r == "eDP-1,2560x1600@60.00,0x0,2"),
            "the display went back to what it ran with: {s:?}"
        );
    }
    // Keep: nothing more is sent.
    let p = a.content.visible_child().expect("page");
    drop_downs(&p).into_iter().next().unwrap().set_selected(2);
    pump(200);
    assert!(sheet.reveals_child());
    let before = seen.lock().unwrap().len();
    go("Keep").emit_clicked();
    pump(200);
    assert!(!sheet.reveals_child());
    assert_eq!(seen.lock().unwrap().len(), before, "Keep sends nothing");
}

fn two() -> Vec<Monitor> {
    let mut v = laptop();
    v.push(Monitor {
        name: "DP-1".into(),
        description: "External Display".into(),
        width: 1920,
        height: 1080,
        refresh: 60.0,
        scale: 1.0,
        modes: vec!["1920x1080@60.00Hz".into(), "1280x720@60.00Hz".into()],
        mirror_of: None,
    });
    v
}

/// displays.lua in the golden config home, removed first when `fresh`.
fn lua(fresh: bool) -> std::path::PathBuf {
    let f = std::path::PathBuf::from(std::env::var("XDG_CONFIG_HOME").expect("golden config home"))
        .join("hypr/wolos/displays.lua");
    if fresh {
        let _ = std::fs::remove_file(&f);
    }
    f
}

fn click(root: &gtk::Widget, label: &str) {
    buttons(root)
        .into_iter()
        .find(|b| b.label().as_deref() == Some(label))
        .unwrap_or_else(|| panic!("button {label}"))
        .emit_clicked();
}

/// The first resolution menu, moved to its second entry (a smaller mode).
fn smaller(p: &gtk::Widget) {
    drop_downs(p)
        .into_iter()
        .next()
        .expect("resolution")
        .set_selected(1);
}

/// QA-LIVE-13 (review D1, D2): the mode is live at once but displays.lua is written only on Keep,
/// in one entry with every change made meanwhile (here an arrangement); Undo writes nothing.
fn nothing_is_written_before_keep() {
    let sys = Rigged::new(two(), Some(false));
    let seen = sys.seen.clone();
    let (a, p) = open(sys);
    let root: gtk::Widget = a.root.clone().upcast();
    let f = lua(true);
    smaller(&p);
    pump(200);
    assert!(
        !f.exists(),
        "a shell that dies now leaves the old mode saved"
    );
    // Arrangement of the external display (Below Main Display) while the sheet waits.
    let menus = drop_downs(&p);
    menus.get(6).expect("arrangement menu").set_selected(4);
    pump(200);
    assert!(!f.exists(), "an arrangement made meanwhile waits too");
    assert!(keep_sheet(&root).expect("sheet").reveals_child());
    click(&root, "Keep");
    pump(200);
    let text = std::fs::read_to_string(&f).expect("Keep writes displays.lua");
    assert!(text.contains("1920x1200"), "{text}");
    assert!(text.contains("DP-1") && text.contains("0x800"), "{text}");
    assert!(text.contains("position = \"0x0\""), "main pinned: {text}");
    // Undo: both displays go back, the file stays as it was.
    let _ = std::fs::remove_file(&f);
    let (a, p) = open(Rigged::new(two(), Some(false)));
    let root: gtk::Widget = a.root.clone().upcast();
    smaller(&p);
    pump(100);
    drop_downs(&p)[6].set_selected(4);
    pump(100);
    click(&root, "Undo");
    pump(200);
    assert!(!f.exists(), "Revert writes nothing");
    let _ = seen;
}

/// QA-LIVE-13: nobody answers, the display goes back by itself (the countdown's second is made
/// 20 ms long here; the 15 counts are real).
fn no_answer_goes_back_by_itself() {
    shell::displays::set_tick_millis(20);
    let sys = Rigged::new(laptop(), Some(false));
    let seen = sys.seen.clone();
    let (a, p) = open(sys);
    let root: gtk::Widget = a.root.clone().upcast();
    let f = lua(true);
    smaller(&p);
    pump(60);
    let sheet = keep_sheet(&root).expect("sheet");
    assert!(sheet.reveals_child(), "still waiting after three ticks");
    pump(700);
    assert!(!sheet.reveals_child(), "the countdown ended");
    {
        let s = seen.lock().unwrap();
        assert!(
            matches!(s.last(), Some(Action::Monitor(r)) if r == "eDP-1,2560x1600@60.00,0x0,2"),
            "the display went back: {s:?}"
        );
    }
    assert!(!f.exists(), "and nothing was saved");
    shell::displays::set_tick_millis(1000);
}

/// QA-LIVE-13: the window closing without an answer is no answer either.
fn closing_the_window_goes_back() {
    let sys = Rigged::new(laptop(), Some(false));
    let seen = sys.seen.clone();
    let (a, p) = open(sys);
    let win = gtk::Window::new();
    let f = lua(true);
    // `open` leaves the window's content unparented; put it in a window to close.
    let root = a.root.clone();
    win.set_child(Some(&root));
    win.present();
    pump(100);
    smaller(&p);
    pump(100);
    assert!(
        matches!(seen.lock().unwrap().last(), Some(Action::Monitor(r)) if r.contains("1920x1200"))
    );
    win.close();
    pump(100);
    {
        let s = seen.lock().unwrap();
        assert!(
            matches!(s.last(), Some(Action::Monitor(r)) if r == "eDP-1,2560x1600@60.00,0x0,2"),
            "closed without an answer: back to the old mode: {s:?}"
        );
    }
    assert!(!f.exists());
}
