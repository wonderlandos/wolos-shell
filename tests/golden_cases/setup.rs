// SPDX-License-Identifier: GPL-2.0-or-later
//! Setup Assistant by keyboard (QA-FDAY-9, QA-LIVE-2, QA-LIVE-3): one ring on the focused control,
//! a ring where Tab lands on the Welcome page, and the Account page keeps what its form says when
//! the person goes Back and Continues again. The checks run before the screenshots.
use super::{Case, find_type, pump};
use gtk::prelude::*;
use shell::kit::{self, widgets as w};
use shell::setup::{Field, Step};
use shell::ui;

fn hosted(s: &ui::setup::Setup, size: (i32, i32)) -> gtk::Window {
    let win = gtk::Window::builder()
        .default_width(size.0)
        .default_height(size.1)
        .child(&s.root)
        .build();
    win.add_css_class("lock-window");
    win.present();
    // What a Tab key press sets on the window (child_focus alone does not).
    win.set_focus_visible(true);
    pump(400);
    win
}

/// The ringed widgets of `n`'s tree, by path.
fn ringed(n: &shell::layout::Node, out: &mut Vec<String>) {
    if n.ring {
        out.push(n.path.clone());
    }
    for c in &n.children {
        ringed(c, out);
    }
}

fn rings_of(win: &gtk::Window) -> Vec<String> {
    let mut rings = Vec::new();
    let child = win.child().expect("a child");
    ringed(&kit::check::tree(&child, win.upcast_ref(), ""), &mut rings);
    rings
}

fn focused(win: &gtk::Window) -> Option<gtk::Widget> {
    gtk::prelude::GtkWindowExt::focus(win)
}

/// FDAY-9: the account card after Tab drew three nested outlines (field group, Hint row,
/// administrator line, the card) because the ring sat on boxes. Tab from the first field and
/// every field after it: exactly one ring, on that field, none around a row, a card or the page.
pub(super) fn account_tab_draws_one_ring() {
    let s = ui::setup::Setup::new(&shell::system::Fake);
    s.show(Step::Account);
    let win = hosted(&s, (1280, 800));
    let mut seen = Vec::new();
    for tab in 0..7 {
        assert!(
            win.child_focus(gtk::DirectionType::TabForward),
            "Tab {tab} moves the focus"
        );
        pump(60);
        let rings = rings_of(&win);
        assert_eq!(rings.len(), 1, "Tab {tab}: one ring, got {rings:?}");
        let f = focused(&win).expect("a focus");
        seen.push(f.type_().name().to_string());
        assert!(
            f.is::<gtk::Entry>()
                || f.is::<gtk::PasswordEntry>()
                || f.is::<gtk::Button>()
                || f.is::<gtk::Text>()
                || f.ancestor(gtk::PasswordEntry::static_type()).is_some()
                || f.ancestor(gtk::Entry::static_type()).is_some(),
            "Tab {tab} lands on a control, not {:?}",
            f.type_().name()
        );
    }
    assert!(
        seen.iter()
            .any(|n| n.contains("Text") || n.contains("Entry")),
        "Tab visits the fields: {seen:?}"
    );
    let issues: Vec<String> = kit::check::issues(&win)
        .into_iter()
        .map(|i| i.message)
        .filter(|m| m.contains("focus ring"))
        .collect();
    assert!(issues.is_empty(), "{issues:?}");
    win.close();
    eprintln!("setup_cases: account_tab_draws_one_ring ok");
}

/// LIVE-2: Tab on the Welcome page changed nothing on screen. The first Tab lands on Continue and
/// rings it; the ring stays on Continue for every further Tab (it is the page's only control).
pub(super) fn welcome_tab_rings_continue() {
    let s = ui::setup::Setup::new(&shell::system::Fake);
    let win = hosted(&s, (1280, 800));
    for tab in 0..4 {
        assert!(
            win.child_focus(gtk::DirectionType::TabForward),
            "Tab {tab} moves the focus"
        );
        pump(60);
        let f = focused(&win).expect("a focus");
        let mut labels: Vec<gtk::Label> = Vec::new();
        find_type(f.upcast_ref(), &mut labels);
        assert!(
            f.is::<gtk::Button>() && f.has_css_class("hero-action"),
            "Tab {tab} lands on Continue, not on {:?} {:?}",
            f.type_().name(),
            labels.iter().map(|l| l.label()).collect::<Vec<_>>()
        );
        let rings = rings_of(&win);
        assert_eq!(rings.len(), 1, "Tab {tab}: Continue rings: {rings:?}");
        assert!(rings[0].contains("Button"), "{rings:?}");
    }
    win.close();
    eprintln!("setup_cases: welcome_tab_rings_continue ok");
}

/// Text of every label under `s.root` that is on screen.
fn shown_text(s: &ui::setup::Setup) -> Vec<String> {
    let mut labels: Vec<gtk::Label> = Vec::new();
    find_type(&s.root, &mut labels);
    labels
        .into_iter()
        .filter(|l| l.is_mapped() && l.is_child_visible())
        .map(|l| l.label().to_string())
        .collect()
}

fn problem_fields(s: &ui::setup::Setup) -> Vec<String> {
    let mut all: Vec<gtk::Widget> = Vec::new();
    find_type(&s.root, &mut all);
    all.iter()
        .filter(|e| {
            e.has_css_class("problem") && (e.is::<gtk::Entry>() || e.is::<gtk::PasswordEntry>())
        })
        .map(|e| format!("{} {:?}", e.type_().name(), e.css_classes()))
        .collect()
}

fn continue_button(s: &ui::setup::Setup) -> gtk::Button {
    let mut b: Vec<gtk::Button> = Vec::new();
    find_type(&s.root, &mut b);
    b.into_iter()
        .find(|b| b.is_mapped() && b.label().as_deref() == Some("Continue"))
        .expect("a Continue button on the page")
}

/// LIVE-3: an invalid name shows its sentence and outlines the field; Back and Continue again
/// must show the same sentence and outline, with Continue still off for that reason.
pub(super) fn validation_survives_back_forward() {
    let s = ui::setup::Setup::new(&shell::system::Fake);
    s.show(Step::Account);
    let win = hosted(&s, (1280, 800));
    s.type_into(Field::FullName, "Alice Liddell");
    s.type_into(Field::Name, "Alice!");
    pump(100);
    let why = "Use lowercase letters and digits, starting with a letter.";
    let before = shown_text(&s);
    assert!(
        before.iter().any(|t| t == why),
        "typing shows it: {before:?}"
    );
    assert_eq!(problem_fields(&s).len(), 1, "{:?}", problem_fields(&s));
    assert!(!continue_button(&s).is_sensitive());
    s.back();
    pump(400);
    assert_ne!(s.step(), Step::Account);
    let mut guard = 0;
    while s.step() != Step::Account {
        s.next();
        pump(400);
        guard += 1;
        assert!(guard < 4, "Continue reaches the Account page again");
    }
    let after = shown_text(&s);
    assert!(
        after.iter().any(|t| t == why),
        "the sentence is back after Back and Continue: {after:?}"
    );
    assert_eq!(
        problem_fields(&s).len(),
        1,
        "the field is outlined again: {:?}",
        problem_fields(&s)
    );
    assert!(
        !continue_button(&s).is_sensitive(),
        "Continue stays off, and now the page says why"
    );
    win.close();
    eprintln!("setup_cases: validation_survives_back_forward ok");
}

/// HUNT-13: a button that is a whole list row (Wi-Fi networks, outputs, nav rows, choice rows) rings
/// inside its own rounded edge (`outline-offset: -2px`), not 2 px outside it over the neighbours
/// (a focused row read as the chosen one). The three classes the rule names exist in the kit, and
/// the rule is the stylesheet's, which a live focused row then draws (the golden below).
pub(super) fn row_buttons_ring_inside_the_row() {
    let css = include_str!("../../src/kit/kit.css");
    let rule = css
        .lines()
        .find(|l| l.contains("button.row-button:focus:focus-visible"))
        .expect("kit.css has the row-button focus rule");
    for class in ["device-button", "row-button", "nav-row"] {
        assert!(
            rule.contains(&format!("button.{class}:focus:focus-visible")),
            "{rule}"
        );
    }
    assert!(rule.contains("outline-offset: -2px"), "{rule}");
    // The general rule keeps 2 px outside for every other control (buttons, fields).
    assert!(css.contains("*:focus:focus-visible, entry:focus-visible, spinbutton:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }"));
    let b = w::choice_row("network-wireless-symbolic", "Home", "Secured", false);
    assert!(b.has_css_class("row-button"), "choice rows are row buttons");
    eprintln!("setup_cases: row_buttons_ring_inside_the_row ok");
}

/// H17-01: the desktop board with no widget still renders something. GTK attaches no buffer to a
/// window whose render node is empty, and the compositor then redraws the whole output on every
/// empty commit; the board's size probe paints one 1/255 pixel so its first frame is a real one.
pub(super) fn empty_desktop_renders_a_node() {
    use shared::widgets::Desktop;
    let board = ui::widgets::Board::new(Desktop::default(), None, Vec::new());
    assert!(board.widgets().is_empty());
    let win = gtk::Window::builder()
        .default_width(800)
        .default_height(600)
        .child(&board.root)
        .build();
    win.present();
    pump(300);
    let paintable = gtk::WidgetPaintable::new(Some(&board.root));
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(&snapshot, 800.0, 600.0);
    let tree = snapshot
        .to_node()
        .map(|n| String::from_utf8_lossy(n.serialize().as_ref()).to_string())
        .unwrap_or_default();
    assert!(
        tree.contains("cairo"),
        "the empty desktop board draws nothing: GTK would never attach a buffer to its layer ({tree:?})"
    );
    win.close();
    eprintln!("setup_cases: empty_desktop_renders_a_node ok");
}

/// DESIGN-30: the Drop Shelf insets its content evenly (12 px all round), empty or full: its
/// scroller was measured 8 px narrower than it is laid out, so the empty-state sentence wrapped to
/// a second line only in the measure and the panel kept 16 px of nothing under it.
pub(super) fn drop_shelf_insets_are_even() {
    let full: Vec<std::path::PathBuf> = [
        "/home/ana/Documents/Quarterly Report.pdf",
        "/home/ana/notes.txt",
    ]
    .iter()
    .map(Into::into)
    .collect();
    for (what, items) in [("empty", Vec::new()), ("two files", full)] {
        let c = ui::drop_shelf::content(&items, true);
        c.set_halign(gtk::Align::Start);
        c.set_valign(gtk::Align::Start);
        let win = gtk::Window::builder().child(&c).build();
        win.present();
        pump(300);
        let uneven: Vec<String> = kit::check::issues(&win)
            .into_iter()
            .map(|i| i.message)
            .filter(|m| m.contains("insets"))
            .collect();
        assert!(uneven.is_empty(), "the {what} shelf: {uneven:?}");
        win.close();
    }
    eprintln!("setup_cases: drop_shelf_insets_are_even ok");
}

/// Goldens: the Welcome page with the ring on Continue, the Account card with the ring on a field.
pub fn cases() -> Vec<Case> {
    let focus_after_map = |s: &ui::setup::Setup, tabs: usize| {
        let root = s.root.clone();
        root.connect_map(move |r| {
            let r = r.clone();
            gtk::glib::idle_add_local_once(move || {
                if let Some(win) = r.root().and_downcast::<gtk::Window>() {
                    win.set_focus_visible(true);
                    for _ in 0..tabs {
                        win.child_focus(gtk::DirectionType::TabForward);
                    }
                }
            });
        });
    };
    vec![
        Case {
            name: "row-button-focus-offset".into(),
            size: (520, 240),
            class: "preview-app",
            wallpaper: false,
            build: Box::new(|| {
                let list = w::vbox(w::S2);
                list.add_css_class("card-list");
                let rows: Vec<gtk::Button> = ["Home", "Office", "Guest"]
                    .into_iter()
                    .map(|n| {
                        w::choice_row("network-wireless-symbolic", n, "Secured", n == "Office")
                    })
                    .collect();
                for r in &rows {
                    list.append(r);
                }
                let first = rows[0].clone();
                first.connect_map(|b| {
                    let b = b.clone();
                    gtk::glib::idle_add_local_once(move || {
                        b.grab_focus();
                        if let Some(win) = b.root().and_downcast::<gtk::Window>() {
                            win.set_focus_visible(true);
                        }
                    });
                });
                list.upcast()
            }),
        },
        Case {
            name: "setup-welcome-focus".into(),
            size: (1280, 800),
            class: "lock-window",
            wallpaper: false,
            build: Box::new(move || {
                let s = ui::setup::Setup::new(&shell::system::Fake);
                focus_after_map(&s, 1);
                s.root.clone()
            }),
        },
        Case {
            name: "setup-account-tab-one-ring".into(),
            size: (1280, 800),
            class: "lock-window",
            wallpaper: false,
            build: Box::new(move || {
                let s = ui::setup::Setup::new(&shell::system::Fake);
                s.show(Step::Account);
                focus_after_map(&s, 2);
                s.root.clone()
            }),
        },
    ]
}
