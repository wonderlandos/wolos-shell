// SPDX-License-Identifier: GPL-2.0-or-later
//! Date picker, time picker and text area tested alone, from their specs (WolOS Design System,
//! ADR 0058 draft, DS-COMPONENTS). Generic checks: `support/kit_case.rs`. `WOL_KIT_ONLY`,
//! `WOL_GOLDEN_UPDATE` and `WOL_KIT_MEASURE` as in `kit_choice.rs`.
#[path = "support/broadway.rs"]
mod broadway;
#[path = "support/kit_case.rs"]
mod kit_case;
#[path = "support/kit_test.rs"]
mod kit_test;

use gtk::prelude::*;
use kit_case::{Built, Component, Ctx, Known, Named, at_rest};
use kit_test::Density;
use shell::kit::{calendar, text, widgets as w};
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

/// The golden runner's fixed day (kit::FAKE_NOW): Monday 26 October 2026.
const TODAY: (i32, u32, u8) = (2026, 10, 26);

fn build_date(state: &str) -> Built {
    let log: Log = Rc::default();
    let l = log.clone();
    let p = calendar::DatePicker::new(
        (2026, 10),
        (state == "picked").then_some(14),
        TODAY,
        ("Previous Month", "Next Month"),
        move |y, m, d| l.borrow_mut().push(format!("{y}-{m:02}-{d:02}")),
    );
    p.root.set_halign(gtk::Align::Start);
    let (root, grid) = (p.root.clone(), p.grid.clone());
    with_log(Built::new(&root, &grid), &log).keeping(p)
}

fn act_date(c: &mut Ctx) {
    let days: Vec<gtk::ToggleButton> = c.m.find::<gtk::ToggleButton>();
    c.same(days.len(), 31, "October has 31 days");
    let today: Vec<_> = days.iter().filter(|d| d.has_css_class("today")).collect();
    c.same(today.len(), 1, "today is marked once");
    if let Some(t) = today.first() {
        c.same(
            t.label().map(String::from),
            Some("26".to_string()),
            "today is the 26th",
        );
    }
    c.same(
        days.iter().filter(|d| d.is_active()).count(),
        usize::from(c.state == "picked"),
        "the picked day only",
    );
    c.expect(
        days.iter()
            .all(|d| gtk::test_accessible_has_property(d, gtk::AccessibleProperty::Label)),
        "every day is named by its full date",
    );
    if let Some(d) = days.first() {
        let (_, _, dw, dh) = c.m.rect(d);
        c.hold("standard.sizes.day.width", dw, 28);
        c.hold("standard.sizes.day.height", dh, 28);
    }
    // October 2026 starts on a Thursday: the first day sits in the fourth column (weeks from Monday).
    let grid = c.m.find::<gtk::Grid>().into_iter().next();
    if let (Some(g), Some(first)) = (grid, days.first()) {
        let (col, _, _, _) = g.query_child(first);
        c.same(col, 3, "weeks start on Monday");
    }
    if let Some(d) = days.get(19) {
        c.m.activate(d);
        c.same(
            c.log(),
            vec!["2026-10-20".to_string()],
            "picking the 20th tells the date once",
        );
    }
    c.m.unfocus();
}

fn build_range(state: &str) -> Built {
    let log: Log = Rc::default();
    let l = log.clone();
    let p = calendar::DatePicker::range(
        (2026, 10),
        (state == "picked").then_some(((2026, 10, 12), (2026, 10, 16))),
        TODAY,
        ("Previous Month", "Next Month"),
        move |a, z| {
            l.borrow_mut().push(format!(
                "{}-{:02}-{:02} to {}-{:02}-{:02}",
                a.0, a.1, a.2, z.0, z.1, z.2
            ))
        },
    );
    p.root.set_halign(gtk::Align::Start);
    let (root, grid) = (p.root.clone(), p.grid.clone());
    with_log(Built::new(&root, &grid), &log).keeping(p)
}

fn act_range(c: &mut Ctx) {
    let days: Vec<gtk::ToggleButton> = c.m.find::<gtk::ToggleButton>();
    c.same(days.len(), 31, "October has 31 days");
    let lit = days.iter().filter(|d| d.is_active()).count();
    let between = days.iter().filter(|d| d.has_css_class("in-range")).count();
    c.expect(
        days.iter()
            .all(|d| gtk::test_accessible_has_property(d, gtk::AccessibleProperty::Label)),
        "every day is named by its full date",
    );
    c.same(
        lit,
        if c.state == "picked" { 2 } else { 0 },
        "the two ends are lit",
    );
    c.same(
        between,
        if c.state == "picked" { 3 } else { 0 },
        "the days between are tinted",
    );
    // Two clicks make a range, in either order; a third starts again.
    if let (Some(a), Some(z)) = (days.get(19), days.get(9)) {
        c.m.activate(a);
        c.same(c.log().len(), 0, "the first click only starts the range");
        c.m.activate(z);
        c.same(
            c.log(),
            vec!["2026-10-10 to 2026-10-20".to_string()],
            "the second click ends the range, whichever side it is on",
        );
        c.same(
            days.iter().filter(|d| d.has_css_class("in-range")).count(),
            9,
            "the nine days between the 10th and the 20th are tinted",
        );
        // A third click starts a new range: one end lit, nothing between, nothing told yet.
        if let Some(third) = days.get(3) {
            c.m.activate(third);
            c.same(c.log().len(), 1, "the third click only starts a new range");
            c.same(
                days.iter().filter(|d| d.is_active()).count(),
                1,
                "a new range has its first end only",
            );
            c.same(
                days.iter().filter(|d| d.has_css_class("in-range")).count(),
                0,
                "no day between yet",
            );
        }
    }
    c.m.unfocus();
}

fn build_time(state: &str) -> Built {
    let log: Log = Rc::default();
    let l = log.clone();
    let t = calendar::time_picker(
        if state == "evening" { (18, 30) } else { (9, 5) },
        ("Hour", "Minute"),
        move |h, m| l.borrow_mut().push(format!("{h:02}:{m:02}")),
    );
    let first = t.first_child().unwrap_or_else(|| t.clone().upcast());
    with_log(Built::new(&t, &first), &log)
}

fn time_size(_: &str, _: Density) -> Option<&'static str> {
    Some("part")
}

fn act_time(c: &mut Ctx) {
    let parts = c.m.find::<gtk::SpinButton>();
    c.same(parts.len(), 2, "an hour and a minute");
    let shown: Vec<String> = parts.iter().map(|p| p.text().to_string()).collect();
    let want = if c.state == "evening" {
        vec!["18", "30"]
    } else {
        vec!["09", "05"]
    };
    c.same(
        shown,
        want.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        "two digits on each side",
    );
    c.expect(
        parts
            .iter()
            .all(|p| gtk::test_accessible_has_property(p, gtk::AccessibleProperty::Label)),
        "the hour and the minute are named",
    );
    if let Some(m) = parts.get(1) {
        m.set_value(59.0);
        m.spin(gtk::SpinType::StepForward, 1.0);
        c.same(m.value(), 0.0, "the minutes wrap from 59 to 00");
    }
}

thread_local! {
    static AREA: RefCell<Option<Rc<text::TextArea>>> = const { RefCell::new(None) };
}

fn build_area(state: &str) -> Built {
    let a = Rc::new(text::TextArea::new(
        if state == "empty" {
            ""
        } else {
            "Pack the charger.\nWater the plants on Friday.\nCall Ana about the trip to Porto."
        },
        "A few words about this note",
    ));
    if state == "disabled" {
        a.set_editable(false);
    }
    let title = w::label("Note", "");
    a.view
        .update_relation(&[gtk::accessible::Relation::LabelledBy(&[title.upcast_ref()])]);
    let b = w::vbox(w::S2);
    b.append(&title);
    b.append(&a.root);
    AREA.with(|x| *x.borrow_mut() = Some(a.clone()));
    let probe = a.view.clone();
    with_log(Built::new(&b, &probe), &Rc::default()).keeping(a)
}

fn act_area(c: &mut Ctx) {
    let Some(a) = AREA.with(|x| x.borrow().clone()) else {
        return;
    };
    let (_, _, _, h) = c.m.rect(&a.root);
    c.expect(h >= 84, format_args!("at least three lines tall ({h})"));
    let ph = c.m.by_class("text-area-placeholder").into_iter().next();
    c.same(
        ph.is_some_and(|p| p.is_visible()),
        c.state == "empty",
        "the placeholder shows only while empty",
    );
    if c.state == "disabled" {
        c.expect(!a.view.is_editable(), "a disabled area cannot be changed");
        c.tab_skips(&a.view, "a disabled area");
    } else {
        c.tab_reaches(&a.view, "the text area");
        c.m.unfocus();
    }
}

fn comp(id: &'static str, build: fn(&str) -> Built, act: fn(&mut Ctx)) -> Component {
    Component {
        id,
        build,
        act,
        size: none,
        named: Named::Label,
        width: w::APP_NARROW.0,
        picture: at_rest,
        row: None,
        control: false,
        check_role: true,
    }
}

#[test]
fn pickers_and_text_area_meet_their_specs() {
    let v = vec![
        comp("date-picker", build_date, act_date),
        comp("date-range-picker", build_range, act_range),
        Component {
            size: time_size,
            ..comp("time-picker", build_time, act_time)
        },
        comp("text-area", build_area, act_area),
    ];
    kit_case::run(&v, KNOWN);
}
