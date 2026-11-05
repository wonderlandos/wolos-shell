// SPDX-License-Identifier: GPL-2.0-or-later
//! Groups of actions and the drawer tested alone, from their specs (WolOS Design System, ADR 0058
//! draft, DS-COMPONENTS): split button, button group, floating toolbar, navigation drawer and
//! date input. Generic checks: `support/kit_case.rs`. `WOL_KIT_ONLY`, `WOL_GOLDEN_UPDATE` and
//! `WOL_KIT_MEASURE` as in `kit_choice.rs`.
#[path = "support/broadway.rs"]
mod broadway;
#[path = "support/kit_case.rs"]
mod kit_case;
#[path = "support/kit_test.rs"]
mod kit_test;

use gtk::prelude::*;
use kit_case::{Built, Component, Ctx, Known, Named, at_rest};
use kit_test::Density;
use shell::kit::{groups, widgets as w};
use std::cell::RefCell;
use std::rc::Rc;

/// Nothing is known to be off: the date input is 24 px like its spec since the field root fix of KIT-LINT.
const KNOWN: Known = &[];

type Log = Rc<RefCell<Vec<String>>>;

fn none(_: &str, _: Density) -> Option<&'static str> {
    None
}

fn regular(_: &str, _: Density) -> Option<&'static str> {
    Some("regular")
}

fn with_log(mut b: Built, log: &Log) -> Built {
    b.log = log.clone();
    b
}

// ---- split button ----

fn build_split(state: &str) -> Built {
    let log: Log = Rc::default();
    let (l1, l2) = (log.clone(), log.clone());
    let l3 = log.clone();
    let s = groups::split_button(
        "Save",
        "More Ways to Save",
        vec![
            (
                "Save As…",
                Box::new(move || l2.borrow_mut().push("as".into())) as Box<dyn Fn()>,
            ),
            (
                "Save a Copy",
                Box::new(move || l3.borrow_mut().push("copy".into())) as Box<dyn Fn()>,
            ),
        ],
        move || l1.borrow_mut().push("save".into()),
    );
    s.set_sensitive(state != "disabled");
    with_log(Built::alone(&s), &log)
}

fn act_split(c: &mut Ctx) {
    let buttons = c.m.find::<gtk::Button>();
    let menu = c.m.find::<gtk::MenuButton>();
    c.same(menu.len(), 1, "one arrow");
    let Some(main) = buttons.iter().find(|b| b.has_css_class("split-main")) else {
        c.expect(false, "the main button");
        return;
    };
    if let Some(arrow) = menu.first() {
        let (mx, _, mw, _) = c.m.rect(main);
        let (ax, _, aw, ah) = c.m.rect(arrow);
        c.hold("standard.gap", ax - (mx + mw), 2);
        c.hold("standard.arrow.width", aw, 24);
        c.hold("standard.arrow.height", ah, 24);
        c.expect(
            gtk::test_accessible_has_property(arrow, gtk::AccessibleProperty::Label),
            "the arrow is named for assistive technology",
        );
        if c.state == "disabled" {
            c.tab_skips(arrow, "the arrow");
        }
    }
    if c.state == "disabled" {
        c.expect(!main.is_sensitive(), "the main button is off");
        c.tab_skips(main, "the main button");
    } else {
        c.tab_reaches(main, "the main button");
        c.m.activate(main);
        c.same(
            c.log(),
            vec!["save".to_string()],
            "the main button runs the main action once",
        );
    }
}

// ---- button group ----

fn build_group(state: &str) -> Built {
    let log: Log = Rc::default();
    let mk = |tip: &str, glyph: &str| {
        let l = log.clone();
        let t = tip.to_string();
        w::icon_button(glyph, tip, move || l.borrow_mut().push(t.clone()))
    };
    let g = if state == "words" {
        let l1 = log.clone();
        let l2 = log.clone();
        groups::button_group(
            "Range",
            &[
                w::button("Day", w::Kind::Default, move || {
                    l1.borrow_mut().push("Day".into())
                }),
                w::button("Week", w::Kind::Default, move || {
                    l2.borrow_mut().push("Week".into())
                }),
            ],
        )
    } else {
        groups::button_group(
            "Zoom",
            &[
                mk("Zoom Out", "zoom-out-symbolic"),
                mk("Zoom In", "zoom-in-symbolic"),
                mk("Full Screen", "view-fullscreen-symbolic"),
            ],
        )
    };
    with_log(Built::alone(&g), &log)
}

fn act_group(c: &mut Ctx) {
    let b = c.m.find::<gtk::Button>();
    c.same(
        b.len(),
        if c.state == "words" { 2 } else { 3 },
        "the buttons",
    );
    c.expect(
        gtk::test_accessible_has_property(c.probe(), gtk::AccessibleProperty::Label),
        "the group is named",
    );
    for x in &b {
        let named = x.label().is_some() || x.tooltip_text().is_some_and(|t| !t.is_empty());
        c.expect(named, "every button has words or a tooltip");
    }
    for pair in b.windows(2) {
        let (ax, _, aw, _) = c.m.rect(&pair[0]);
        let (bx, _, _, _) = c.m.rect(&pair[1]);
        c.hold("standard.hairline", bx - (ax + aw), 1);
    }
    if let Some(second) = b.get(1) {
        c.m.activate(second);
        let want = if c.state == "words" {
            "Week"
        } else {
            "Zoom In"
        };
        c.same(c.log(), vec![want.to_string()], "a button acts alone, once");
    }
}

// ---- floating toolbar ----

fn build_floating(state: &str) -> Built {
    let log: Log = Rc::default();
    let mk = |tip: &str, glyph: &str| {
        let l = log.clone();
        let t = tip.to_string();
        w::icon_button(glyph, tip, move || l.borrow_mut().push(t.clone()))
    };
    let acts = if state == "primary" {
        vec![
            mk("Undo", "edit-undo-symbolic"),
            mk("Redo", "edit-redo-symbolic"),
        ]
    } else {
        vec![
            mk("Undo", "edit-undo-symbolic"),
            mk("Redo", "edit-redo-symbolic"),
            mk("Copy", "edit-copy-symbolic"),
        ]
    };
    let primary = (state == "primary").then(|| mk("Save", "document-save-symbolic"));
    let t = groups::floating_toolbar("Image Tools", &acts, primary.as_ref());
    with_log(Built::alone(&t), &log)
}

fn act_floating(c: &mut Ctx) {
    let b = c.m.find::<gtk::Button>();
    c.same(b.len(), 3, "three actions");
    for x in &b {
        c.expect(
            x.tooltip_text().is_some_and(|t| !t.is_empty()),
            "every action has a tooltip",
        );
    }
    c.same(
        b.iter()
            .filter(|x| x.has_css_class("floating-primary"))
            .count(),
        usize::from(c.state == "primary"),
        "at most one primary, at the end",
    );
    let (_, _, w_, _) = c.m.rect(c.probe());
    c.expect(
        w_ >= 24 * 3,
        format_args!("the pill is wide enough for its actions ({w_})"),
    );
    if let Some(first) = b.first() {
        c.tab_reaches(first, "the first action");
        c.m.activate(first);
        c.same(c.log(), vec!["Undo".to_string()], "an action runs once");
    }
}

// ---- navigation drawer ----

const PLACES: [(&str, &str); 4] = [
    ("folder-symbolic", "Library"),
    ("system-software-install-symbolic", "Store"),
    ("software-update-available-symbolic", "Updates"),
    ("preferences-system-symbolic", "Settings"),
];

fn build_drawer(state: &str) -> Built {
    let log: Log = Rc::default();
    let l = log.clone();
    let (d, v) = groups::navigation_drawer(
        "Atlas",
        &PLACES,
        if state == "last" { 3 } else { 0 },
        move |i| l.borrow_mut().push(format!("go {i}")),
    );
    d.set_halign(gtk::Align::Start);
    with_log(Built::alone(&d), &log).keeping(v)
}

fn drawer_size(_: &str, _: Density) -> Option<&'static str> {
    Some("drawer")
}

fn act_drawer(c: &mut Ctx) {
    let items = c.m.find::<gtk::ToggleButton>();
    c.same(items.len(), 4, "four destinations");
    let picked = if c.state == "last" { 3 } else { 0 };
    c.same(
        items.iter().position(|b| b.is_active()),
        Some(picked),
        "the picked destination is pressed",
    );
    let dn = c.frame.density.name();
    if let (Some(first), Some(want)) = (items.first(), c.spec["density"][dn]["item"].as_i64()) {
        let (_, _, _, h) = c.m.rect(first);
        c.hold(&format!("{dn}.item"), h, want as i32);
    }
    let with_glyph = items
        .iter()
        .filter(|b| {
            b.child()
                .is_some_and(|f| f.first_child().is_some_and(|g| g.is::<gtk::Image>()))
        })
        .count();
    c.same(with_glyph, 4, "every destination has a glyph");
    c.expect(
        c.m.by_text("Library").is_some(),
        "every destination has a label",
    );
    if let Some(other) = items.get(2) {
        c.m.activate(other);
        c.same(
            c.log(),
            vec!["go 2".to_string()],
            "picking a destination tells once",
        );
    }
    c.m.unfocus();
}

// ---- date input ----

fn build_date(state: &str) -> Built {
    let log: Log = Rc::default();
    let (l1, l2) = (log.clone(), log.clone());
    let d = groups::DateInput::new(
        "Start Date",
        "Choose Date",
        move || l1.borrow_mut().push("picker".into()),
        move |y, m, d| l2.borrow_mut().push(format!("typed {y}-{m:02}-{d:02}")),
    );
    match state {
        "filled" => d.entry.set_text("2026-10-26"),
        "invalid" => d.entry.set_text("2026-02-30"),
        _ => {}
    }
    log.borrow_mut().clear();
    let (root, entry) = (d.root.clone(), d.entry.clone());
    with_log(Built::new(&root, &entry), &log).keeping(d)
}

fn act_date(c: &mut Ctx) {
    let Some(entry) = c.find::<gtk::Entry>() else {
        return;
    };
    let invalid = entry.has_css_class("invalid");
    c.same(
        invalid,
        c.state == "invalid",
        "only a day that does not exist is marked",
    );
    c.same(
        gtk::test_accessible_has_state(&entry, gtk::AccessibleState::Invalid),
        true,
        "the field carries its validity for assistive technology",
    );
    c.same(
        entry.placeholder_text().map(|t| t.to_string()),
        Some("YYYY-MM-DD".to_string()),
        "the field says its shape",
    );
    let button = c.m.find::<gtk::Button>();
    c.same(button.len(), 1, "the calendar button");
    if let Some(b) = button.first() {
        c.expect(
            b.tooltip_text().is_some_and(|t| t == "Choose Date"),
            "the calendar button is named Choose Date",
        );
        c.m.activate(b);
        c.same(
            c.log(),
            vec!["picker".to_string()],
            "the calendar button opens the picker once",
        );
    }
    if c.state == "empty" {
        entry.set_text("2026-12-24");
        c.same(
            c.b.log.borrow().last().cloned(),
            Some("typed 2026-12-24".to_string()),
            "a real typed day is heard",
        );
    }
    c.m.unfocus();
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
fn groups_and_drawer_meet_their_specs() {
    let v = vec![
        Component {
            size: regular,
            ..comp("split-button", build_split, act_split)
        },
        Component {
            size: regular,
            ..comp("button-group", build_group, act_group)
        },
        Component {
            size: regular,
            ..comp("floating-toolbar", build_floating, act_floating)
        },
        Component {
            size: drawer_size,
            ..comp("navigation-drawer", build_drawer, act_drawer)
        },
        Component {
            size: regular,
            ..comp("date-input", build_date, act_date)
        },
    ];
    kit_case::run(&v, KNOWN);
}
