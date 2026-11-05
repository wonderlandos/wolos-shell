// SPDX-License-Identifier: GPL-2.0-or-later
//! Navigation parts tested alone, from their specs (WolOS Design System, ADR 0058 draft,
//! DS-COMPONENTS): tabs, navigation bar, navigation rail, breadcrumb, disclosure, bottom app bar,
//! status bar and menu. Generic checks: `support/kit_case.rs`. `WOL_KIT_ONLY`, `WOL_GOLDEN_UPDATE`
//! and `WOL_KIT_MEASURE` as in `kit_choice.rs`.
#[path = "support/broadway.rs"]
mod broadway;
#[path = "support/kit_case.rs"]
mod kit_case;
#[path = "support/kit_test.rs"]
mod kit_test;

use gtk::prelude::*;
use kit_case::{Built, Component, Ctx, Known, Named, at_rest};
use kit_test::Density;
use shell::kit::{navigation as nav, widgets as w};
use std::cell::RefCell;
use std::rc::Rc;

const KNOWN: Known = &[];

type Log = Rc<RefCell<Vec<String>>>;

fn none(_: &str, _: Density) -> Option<&'static str> {
    None
}

fn with_log(mut b: Built, log: &Log) -> Built {
    b.log = log.clone();
    b
}

// ---- tabs ----

fn build_tabs(state: &str) -> Built {
    let log: Log = Rc::default();
    let l = log.clone();
    let titles: &[&str] = if state == "scrollable" {
        &[
            "Overview", "Details", "Reviews", "Privacy", "Versions", "Files", "Logs", "Related",
        ]
    } else {
        &["Overview", "Details", "Reviews"]
    };
    let pages = titles
        .iter()
        .map(|t| {
            (
                *t,
                w::label(&format!("{t} panel"), "").upcast::<gtk::Widget>(),
            )
        })
        .collect();
    let kind = if state == "secondary" {
        nav::TabKind::Secondary
    } else {
        nav::TabKind::Primary
    };
    let t = nav::Tabs::new(
        kind,
        pages,
        usize::from(state == "primary"),
        state == "scrollable",
        move |i| l.borrow_mut().push(format!("tab {i}")),
    );
    let root = t.root.clone();
    let bar = t.bar.clone();
    with_log(Built::new(&root, &bar), &log).keeping(t.tabs)
}

fn tab_size(_: &str, _: Density) -> Option<&'static str> {
    None
}

fn act_tabs(c: &mut Ctx) {
    let tabs: Vec<gtk::ToggleButton> = c.m.find::<gtk::ToggleButton>();
    let picked = usize::from(c.state == "primary");
    c.expect(
        tabs.iter().all(|t| kit_case::role_name(t) == "tab"),
        "every tab has the tab role",
    );
    c.same(
        tabs.iter().filter(|t| t.is_active()).count(),
        1,
        "one tab is picked",
    );
    // The tab height follows the density (the spec's row metric for the tab list).
    let dn = c.frame.density.name();
    if let (Some(t), Some(want)) = (tabs.first(), c.spec["density"][dn]["row"].as_i64()) {
        let want = if c.state == "secondary" {
            want as i32 - 8
        } else {
            want as i32
        };
        let (_, _, _, h) = c.m.rect(t);
        c.hold(&format!("{dn}.{}.tab.height", c.state), h, want);
    }
    if let Some(t) = tabs.get(picked) {
        c.expect(
            gtk::test_accessible_has_relation(t, gtk::AccessibleRelation::Controls),
            "the tab controls its panel",
        );
    }
    if let Some(t) = tabs.get(2) {
        c.m.activate(t);
        c.same(
            c.log(),
            vec!["tab 2".to_string()],
            "picking the third tab tells once",
        );
        let stack = c.m.find::<gtk::Stack>().into_iter().next();
        c.same(
            stack
                .and_then(|s| s.visible_child_name())
                .map(|n| n.to_string()),
            Some("tab-2".to_string()),
            "the third panel shows",
        );
    }
    c.m.unfocus();
}

// ---- navigation bar and rail ----

const PLACES: [(&str, &str); 4] = [
    ("folder-symbolic", "Library"),
    ("system-software-install-symbolic", "Store"),
    ("software-update-available-symbolic", "Updates"),
    ("preferences-system-symbolic", "Settings"),
];

fn build_bar(state: &str) -> Built {
    let log: Log = Rc::default();
    let l = log.clone();
    let (b, v) = nav::navigation_bar(&PLACES, if state == "last" { 3 } else { 0 }, move |i| {
        l.borrow_mut().push(format!("go {i}"))
    });
    let page = w::vbox(0);
    page.append(&b);
    with_log(Built::new(&page, &b), &log).keeping(v)
}

fn build_rail(state: &str) -> Built {
    let log: Log = Rc::default();
    let l = log.clone();
    let (b, v) = nav::navigation_rail(&PLACES, if state == "last" { 3 } else { 0 }, move |i| {
        l.borrow_mut().push(format!("go {i}"))
    });
    b.set_halign(gtk::Align::Start);
    let page = w::hbox(0);
    page.append(&b);
    with_log(Built::new(&page, &b), &log).keeping(v)
}

fn bar_size(_: &str, _: Density) -> Option<&'static str> {
    Some("bar")
}

fn rail_size(_: &str, _: Density) -> Option<&'static str> {
    Some("rail")
}

fn act_destinations(c: &mut Ctx) {
    let d: Vec<gtk::ToggleButton> = c.m.find::<gtk::ToggleButton>();
    c.same(d.len(), 4, "four destinations");
    let picked: Vec<usize> = d
        .iter()
        .enumerate()
        .filter(|(_, b)| b.is_active())
        .map(|(i, _)| i)
        .collect();
    c.same(
        picked,
        vec![if c.state == "last" { 3 } else { 0 }],
        "the picked destination is pressed",
    );
    let labelled = d.iter().all(|b| {
        let mut x = b.first_child();
        let mut words = false;
        while let Some(n) = x {
            let mut y = n.first_child();
            while let Some(m) = y {
                words |= m
                    .downcast_ref::<gtk::Label>()
                    .is_some_and(|l| !l.text().is_empty());
                y = m.next_sibling();
            }
            x = n.next_sibling();
        }
        words
    });
    c.expect(labelled, "every destination has a label under its glyph");
    if let Some(b) = d.get(1) {
        c.m.activate(b);
        c.same(
            c.log(),
            vec!["go 1".to_string()],
            "a destination tells once",
        );
    }
    c.m.unfocus();
}

// ---- breadcrumb ----

fn build_crumb(state: &str) -> Built {
    let log: Log = Rc::default();
    let l = log.clone();
    let trail: &[&str] = if state == "deep" {
        &[
            "Settings",
            "Network",
            "Wi-Fi",
            "Home network with a very long name that does not fit",
        ]
    } else {
        &["Settings", "Network"]
    };
    let b = nav::breadcrumb(trail, move |i| l.borrow_mut().push(format!("up {i}")));
    with_log(Built::alone(&b), &log)
}

fn crumb_size(_: &str, _: Density) -> Option<&'static str> {
    None
}

fn act_crumb(c: &mut Ctx) {
    let links = c.m.find::<gtk::Button>();
    let want = if c.state == "deep" { 3 } else { 1 };
    c.same(
        links.len(),
        want,
        "every parent is a link and the current page is not",
    );
    if let Some(b) = links.first() {
        let (_, _, _, h) = c.m.rect(b);
        c.hold("standard.sizes.crumb.height", h, 24);
        c.m.activate(b);
        c.same(
            c.log(),
            vec!["up 0".to_string()],
            "the first parent tells once",
        );
    }
    let current = c.m.by_class("crumb-current").into_iter().next();
    c.expect(
        current.is_some_and(|l| l.is::<gtk::Label>()),
        "the current page is plain text",
    );
    c.m.unfocus();
}

// ---- disclosure ----

fn build_disclosure(state: &str) -> Built {
    let body = w::section("", &[w::list_row("", "Proxy", "", None).upcast()]);
    let e = nav::disclosure("Advanced Options", state == "open", &body);
    with_log(Built::alone(&e), &Rc::default())
}

fn act_disclosure(c: &mut Ctx) {
    let Ok(e) = c.probe().clone().downcast::<gtk::Expander>() else {
        return;
    };
    c.same(
        e.is_expanded(),
        c.state == "open",
        "the body shows only when open",
    );
    let dn = c.frame.density.name();
    if let (Some(head), Some(want)) = (
        e.first_child().and_then(|b| b.first_child()),
        c.spec["density"][dn]["row"].as_i64(),
    ) {
        let (_, _, _, h) = c.m.rect(&head);
        c.hold(&format!("{dn}.header.height"), h, want as i32);
    }
    let was = e.is_expanded();
    e.set_expanded(!was);
    c.m.settle();
    c.same(e.is_expanded(), !was, "the header flips the body");
    e.set_expanded(was);
    c.m.settle();
}

// ---- bottom app bar ----

fn build_bottom(state: &str) -> Built {
    let acts: Vec<gtk::Widget> = [
        ("edit-copy-symbolic", "Copy"),
        ("edit-delete-symbolic", "Move to Trash"),
        ("send-to-symbolic", "Share"),
    ]
    .iter()
    .take(if state == "primary" { 2 } else { 3 })
    .map(|(i, t)| w::icon_button(i, t, || {}).upcast())
    .collect();
    let primary = (state == "primary")
        .then(|| w::button("New Note", w::Kind::Primary, || {}).upcast::<gtk::Widget>());
    let b = nav::bottom_app_bar(&acts, primary.as_ref());
    let page = w::vbox(0);
    page.append(&b);
    with_log(Built::new(&page, &b), &Rc::default())
}

fn act_bottom(c: &mut Ctx) {
    let dn = c.frame.density.name();
    let (_, _, _, h) = c.m.rect(c.probe());
    if let Some(want) = c.spec["density"][dn]["row"].as_i64() {
        c.hold(&format!("{dn}.bar.height"), h, want as i32);
    }
    let buttons = c.m.find::<gtk::Button>();
    c.expect(buttons.len() <= 4, "at most four actions");
    let primaries = buttons
        .iter()
        .filter(|b| b.has_css_class("btn-primary"))
        .count();
    c.expect(primaries <= 1, "at most one primary action");
    if let Some(b) = buttons.first() {
        c.tab_reaches(b, "the first action");
    }
    c.m.unfocus();
}

// ---- status bar ----

fn build_status(_: &str) -> Built {
    let l = nav::status_bar("12 items, 3 selected");
    let page = w::vbox(0);
    page.append(&w::spacer());
    page.append(&l);
    with_log(Built::new(&page, &l), &Rc::default())
}

fn act_status(c: &mut Ctx) {
    let (_, _, _, h) = c.m.rect(c.probe());
    c.hold("standard.sizes.line.height", h, 24);
    c.same(
        c.m.find::<gtk::Button>().len(),
        0,
        "no buttons in a status bar",
    );
}

// ---- menu ----

fn build_menu(state: &str) -> Built {
    let log: Log = Rc::default();
    let row = |t: &str, k: Option<&'static str>, l: &Log| -> w::MenuRow {
        let (l, t2) = (l.clone(), t.to_string());
        (
            t.to_string(),
            k,
            Box::new(move || l.borrow_mut().push(format!("ran {t2}"))),
        )
    };
    let keys = state == "keys";
    let rows = vec![
        row("Open", keys.then_some("Ctrl+O"), &log),
        row("Rename…", keys.then_some("F2"), &log),
        (String::new(), None, Box::new(|| {}) as Box<dyn Fn()>),
        row("Move to Trash", keys.then_some("Delete"), &log),
    ];
    let p = w::menu_keys(rows);
    // A popover is a surface of its own: its list is mounted alone, the way it draws in the menu.
    let Some(list) = p.child() else {
        panic!("the menu has a list");
    };
    p.set_child(None::<&gtk::Widget>);
    let glass = w::glass_panel(0);
    glass.set_halign(gtk::Align::Start);
    glass.append(&list);
    with_log(Built::new(&glass, &list), &log).keeping(p)
}

fn act_menu(c: &mut Ctx) {
    let items = c.m.by_class("menu-item");
    c.same(items.len(), 3, "three items, one separator");
    c.same(
        c.m.find::<gtk::Separator>().len(),
        1,
        "one separator between groups",
    );
    let keys = c.m.by_class("menu-key").len();
    c.same(
        keys,
        if c.state == "keys" { 3 } else { 0 },
        "each item shows its shortcut when it has one",
    );
    // The item height follows the density (design/tokens/density.json menu-item: 24, 28, 32).
    let dn = c.frame.density.name();
    if let (Some(i), Some(want)) = (items.first(), c.spec["density"][dn]["item"].as_i64()) {
        let (_, _, _, h) = c.m.rect(i);
        c.hold(&format!("{dn}.item"), h, want as i32);
    }
    if let Some(b) = items
        .first()
        .and_then(|i| i.clone().downcast::<gtk::Button>().ok())
    {
        c.m.activate(&b);
        c.same(c.log(), vec!["ran Open".to_string()], "an item runs once");
    }
}

fn comp(id: &'static str, build: fn(&str) -> Built, act: fn(&mut Ctx)) -> Component {
    Component {
        id,
        build,
        act,
        size: none,
        named: Named::Nothing,
        width: w::APP_NARROW.0,
        picture: at_rest,
        row: None,
        control: false,
        check_role: true,
    }
}

#[test]
fn navigation_parts_meet_their_specs() {
    let mut v = vec![
        Component {
            size: tab_size,
            ..comp("tabs", build_tabs, act_tabs)
        },
        Component {
            size: bar_size,
            ..comp("navigation-bar", build_bar, act_destinations)
        },
        Component {
            size: rail_size,
            ..comp("navigation-rail", build_rail, act_destinations)
        },
        Component {
            size: crumb_size,
            ..comp("breadcrumb", build_crumb, act_crumb)
        },
        Component {
            named: Named::Words,
            ..comp("disclosure", build_disclosure, act_disclosure)
        },
        comp("bottom-app-bar", build_bottom, act_bottom),
        Component {
            named: Named::Words,
            ..comp("status-bar", build_status, act_status)
        },
        Component {
            check_role: false,
            named: Named::Words,
            ..comp("menu", build_menu, act_menu)
        },
    ];
    // The navigation bar is the narrow window's; the rail sits beside content at 640.
    v[1].width = w::APP_NARROW.0;
    v[2].width = w::NARROW;
    kit_case::run(&v, KNOWN);
}
