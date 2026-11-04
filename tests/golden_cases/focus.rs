// SPDX-License-Identifier: GPL-2.0-or-later
//! Keyboard focus in System Settings (QA 0.12.0 bugs 1 and 2): the ring is on the focused control
//! only, never on the rows and cards around it, and the sidebar arrows never stop between groups.
use super::{Case, find_type, pump};
use gtk::prelude::*;
use shell::kit::{self, widgets as w};
use shell::ui;
use std::rc::Rc;

fn settings() -> w::AppWindow {
    ui::settings::content(Rc::new(shell::system::Fake), &ui::settings::Host::demo())
}

fn row(a: &w::AppWindow, title: &str) -> gtk::ListBoxRow {
    let mut i = 0;
    while let Some(r) = a.sidebar.row_at_index(i) {
        if r.widget_name() == title {
            return r;
        }
        i += 1;
    }
    panic!("no sidebar row {title:?}")
}

/// The first "Install" button on the visible page.
fn install(a: &w::AppWindow) -> gtk::Button {
    let page = a.content.visible_child().expect("a page shows");
    let mut all: Vec<gtk::Button> = Vec::new();
    find_type(&page, &mut all);
    all.into_iter()
        .find(|b| {
            let mut l: Vec<gtk::Label> = Vec::new();
            find_type(b.upcast_ref(), &mut l);
            b.label().as_deref() == Some("Install") || l.iter().any(|l| l.label() == "Install")
        })
        .expect("an Install button on Modules")
}

fn focused(win: &gtk::Window) -> Option<gtk::Widget> {
    gtk::prelude::GtkWindowExt::focus(win)
}

/// Bug 2: Down from "Users & Groups" went to an empty outlined gap row before "Keyboard". Group
/// space is a list header now: every sidebar row is a page, and Down and Up (the arrow keys'
/// `move-cursor` binding) go from the last row of a group to the first of the next and back.
/// Searching drops the space.
pub(super) fn sidebar_arrows_go_from_group_to_group() {
    let a = settings();
    let win = gtk::Window::builder()
        .default_width(w::APP_SIZE.0)
        .default_height(w::APP_SIZE.1)
        .child(&a.root)
        .build();
    win.present();
    pump(300);
    let mut i = 0;
    while let Some(r) = a.sidebar.row_at_index(i) {
        assert!(
            r.has_css_class("sidebar-search")
                || a.content.child_by_name(&r.widget_name()).is_some(),
            "sidebar row {i} is not a page (a gap row)"
        );
        i += 1;
    }
    for (last, first) in [("Users & Groups", "Keyboard"), ("Network", "Notifications")] {
        let (from, to) = (row(&a, last), row(&a, first));
        assert!(
            to.header().is_some(),
            "{first} starts a group: space above it"
        );
        assert!(from.grab_focus(), "{last} takes the focus");
        win.set_focus_visible(true);
        a.sidebar
            .emit_move_cursor(gtk::MovementStep::DisplayLines, 1, false, false);
        assert_eq!(
            focused(&win).as_ref(),
            Some(to.upcast_ref::<gtk::Widget>()),
            "Down from {last} lands on {first}"
        );
        a.sidebar
            .emit_move_cursor(gtk::MovementStep::DisplayLines, -1, false, false);
        assert_eq!(
            focused(&win).as_ref(),
            Some(from.upcast_ref::<gtk::Widget>()),
            "Up from {first} lands on {last}"
        );
    }
    // Hunt 1 B1: Up from the first page row stopped on the row around the search field (ringed,
    // typing did nothing until Tab). The cursor passes the row; the field takes the focus.
    let search = a.sidebar.row_at_index(0).expect("the search row");
    assert!(search.has_css_class("sidebar-search"));
    let entry = search.child().expect("the search field");
    let first = a.sidebar.row_at_index(1).expect("a first page row");
    assert!(
        first.grab_focus(),
        "{} takes the focus",
        first.widget_name()
    );
    a.sidebar
        .emit_move_cursor(gtk::MovementStep::DisplayLines, -1, false, false);
    pump(50);
    let f = focused(&win);
    assert!(
        f.as_ref()
            .is_some_and(|f| f == &entry || f.is_ancestor(&entry)),
        "Up from {} lands in the search field, not on its row: {:?}",
        first.widget_name(),
        f.map(|f| f.type_().name())
    );
    win.close();
    let s = ui::settings::content_searching(
        Rc::new(shell::system::Fake),
        &ui::settings::Host::demo(),
        "o",
    );
    let mut i = 0;
    while let Some(r) = s.sidebar.row_at_index(i) {
        assert!(
            r.header().is_none(),
            "{:?}: no group space while searching",
            r.widget_name()
        );
        i += 1;
    }
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

/// Bug 1: with the keyboard on a Modules row's Install button, GTK marks the row, the card list
/// and the page `:focus-visible` as well (that is GTK, checked here); only the button draws the
/// ring, and the layout check finds no ring around a container.
pub(super) fn only_the_focused_control_has_the_ring() {
    let a = settings();
    a.open("Modules");
    pump(50);
    let win = gtk::Window::builder()
        .default_width(w::APP_SIZE.0)
        .default_height(w::APP_SIZE.1)
        .child(&a.root)
        .build();
    win.present();
    pump(300);
    let b = install(&a);
    assert!(b.grab_focus(), "Install takes the focus");
    win.set_focus_visible(true);
    pump(50);
    let mut up = b.parent();
    let row = loop {
        let p = up.expect("Install sits in a list row");
        if p.has_css_class("list-row") {
            break p;
        }
        up = p.parent();
    };
    let f = row.state_flags();
    assert!(
        f.contains(gtk::StateFlags::FOCUS_VISIBLE) && !f.contains(gtk::StateFlags::FOCUSED),
        "GTK marks the focus's row :focus-visible but not :focus ({f:?})"
    );
    let tree = kit::check::tree(&a.root.clone().upcast(), win.upcast_ref(), "");
    let mut rings = Vec::new();
    ringed(&tree, &mut rings);
    assert_eq!(rings.len(), 1, "one ring, on Install: {rings:?}");
    assert!(
        rings[0]
            .rsplit(" › ")
            .next()
            .is_some_and(|l| l.starts_with("Button")),
        "{rings:?}"
    );
    let issues: Vec<String> = kit::check::issues(&win)
        .into_iter()
        .map(|i| i.message)
        .filter(|m| m.contains("focus ring"))
        .collect();
    assert!(issues.is_empty(), "{issues:?}");
    win.close();
}

/// Golden: Modules with the keyboard ring on the first Install button (one ring, nothing around
/// the row or the list).
pub fn cases() -> Vec<Case> {
    vec![Case {
        name: "settings-modules-focus-ring-wide".into(),
        size: w::APP_SIZE,
        class: "preview-app",
        wallpaper: false,
        build: Box::new(|| {
            let a = settings();
            a.open("Modules");
            pump(50);
            let b = install(&a);
            b.connect_map(|b| {
                let b = b.clone();
                gtk::glib::idle_add_local_once(move || {
                    b.grab_focus();
                    if let Some(win) = b.root().and_downcast::<gtk::Window>() {
                        win.set_focus_visible(true);
                    }
                });
            });
            a.root.clone().upcast()
        }),
    }]
}
