// SPDX-License-Identifier: GPL-2.0-or-later
//! Choice controls tested alone, from their specs (WolOS Design System, ADR 0058 draft,
//! DS-COMPONENTS): checkbox, radio group, chips, the discrete slider and the range slider. The
//! generic part (sizes per density, role and name, layout rules, goldens, the KNOWN ratchet) is
//! `support/kit_case.rs`; each component adds its `build_*` (through the Kit function) and `act_*`.
//!
//! `WOL_KIT_ONLY=chip` runs the cases whose name contains it; `WOL_GOLDEN_UPDATE=1` rewrites the
//! goldens and trees; `WOL_KIT_MEASURE=1` prints what the Kit draws. Needs `gtk4-broadwayd` (the
//! golden image has it); skips without one.
#[path = "support/broadway.rs"]
mod broadway;
#[path = "support/kit_case.rs"]
mod kit_case;
#[path = "support/kit_test.rs"]
mod kit_test;

use gtk::prelude::*;
use kit_case::{Built, Component, Ctx, Known, Named, at_rest};
use kit_test::Density;
use shell::kit::{choice, widgets as w};
use std::cell::RefCell;
use std::rc::Rc;

/// Where the Kit does not meet its spec yet (component, check, measured).
const KNOWN: Known = &[];

fn none(_: &str, _: Density) -> Option<&'static str> {
    None
}

/// `sizes.regular` is the standard density's; the other densities hold `density.<d>.control`.
fn regular(_: &str, d: Density) -> Option<&'static str> {
    (d == Density::Standard).then_some("regular")
}

fn logged(b: Built, log: &Rc<RefCell<Vec<String>>>) -> Built {
    let mut b = b;
    b.log = log.clone();
    b
}

// ---- checkbox ----

fn build_checkbox(state: &str) -> Built {
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let l = log.clone();
    let c = choice::checkbox("Show hidden files", state == "on", move |on| {
        l.borrow_mut().push(format!("changed {on}"))
    });
    if state == "mixed" {
        choice::set_mixed(&c, true);
    }
    if state == "disabled" {
        c.set_sensitive(false);
    }
    c.set_halign(gtk::Align::Start);
    logged(Built::alone(&c), &log)
}

fn act_checkbox(c: &mut Ctx) {
    let Some(b) = c.probe().clone().downcast::<gtk::CheckButton>().ok() else {
        c.expect(false, "the probe is a CheckButton");
        return;
    };
    c.expect(b.has_css_class("checkbox"), "the box wears .checkbox");
    if c.state == "disabled" {
        c.tab_skips(&b, "a checkbox that cannot change");
        return;
    }
    c.same(
        b.is_inconsistent(),
        c.state == "mixed",
        "mixed shows only when mixed",
    );
    c.tab_reaches(&b, "the checkbox");
    let was = b.is_active();
    c.m.activate(&b);
    c.same(b.is_active(), !was, "Space flips the box");
    c.same(
        c.log(),
        vec![format!("changed {}", !was)],
        "the change is heard once",
    );
    c.m.unfocus();
}

// ---- radio ----

fn build_radio(state: &str) -> Built {
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let l = log.clone();
    let title = w::label("Appearance", "heading");
    let g = choice::radio_group(
        Some(&title),
        &["Light", "Dark", "Automatic"],
        usize::from(state == "second"),
        move |i| l.borrow_mut().push(format!("picked {i}")),
    );
    if state == "disabled" {
        g.set_sensitive(false);
    }
    let b = w::vbox(w::S2);
    b.append(&title);
    b.append(&g);
    logged(Built::new(&b, &g), &log)
}

fn act_radio(c: &mut Ctx) {
    let radios = c.m.find::<gtk::CheckButton>();
    c.same(radios.len(), 3, "three options");
    let on: Vec<usize> = radios
        .iter()
        .enumerate()
        .filter(|(_, r)| r.is_active())
        .map(|(i, _)| i)
        .collect();
    c.same(
        on,
        vec![usize::from(c.state == "second")],
        "exactly one option is picked",
    );
    c.expect(
        radios
            .iter()
            .all(|r| r.has_css_class("radio") && super_role(r) == "radio"),
        "every option is a radio",
    );
    if c.state == "disabled" {
        if let Some(r) = radios.first() {
            c.tab_skips(r, "a disabled group");
        }
        return;
    }
    if let Some(r) = radios.get(2) {
        c.m.activate(r);
        c.same(
            c.log(),
            vec!["picked 2".to_string()],
            "picking the third tells once",
        );
        let still: usize = radios.iter().filter(|r| r.is_active()).count();
        c.same(still, 1, "one option stays picked");
    }
    c.m.unfocus();
}

fn super_role(w: &impl IsA<gtk::Widget>) -> String {
    kit_case::role_name(w)
}

// ---- chip ----

fn build_chip(state: &str) -> Built {
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let l = log.clone();
    let say = move |s: String| l.borrow_mut().push(s);
    let set_of = |c: &gtk::Widget| choice::chip_set(std::slice::from_ref(c));
    let (probe, root): (gtk::Widget, gtk::Box) = match state {
        "filter-off" | "filter-on" => {
            let c = choice::filter_chip("Installed", state == "filter-on", move |on| {
                say(format!("filter {on}"))
            });
            let p: gtk::Widget = c.upcast();
            (p.clone(), set_of(&p))
        }
        "input" => {
            let c =
                choice::input_chip("Ana Lima", "Remove Ana Lima", move || say("removed".into()));
            let p: gtk::Widget = c.upcast();
            (p.clone(), set_of(&p))
        }
        "suggestion" => {
            let c = choice::chip(
                choice::ChipKind::Suggestion,
                "",
                "Tomorrow morning",
                move || say("clicked".into()),
            );
            let p: gtk::Widget = c.upcast();
            (p.clone(), set_of(&p))
        }
        _ => {
            let c = choice::chip(
                choice::ChipKind::Assist,
                "x-office-calendar-symbolic",
                "Add to Calendar",
                move || say("clicked".into()),
            );
            if state == "disabled" {
                c.set_sensitive(false);
            }
            let p: gtk::Widget = c.upcast();
            (p.clone(), set_of(&p))
        }
    };
    logged(Built::new(&root, &probe), &log)
}

fn act_chip(c: &mut Ctx) {
    let p = c.probe().clone();
    c.expect(p.has_css_class("chip"), "the chip wears .chip");
    match c.state {
        "disabled" => c.tab_skips(&p, "a chip that cannot act"),
        "filter-off" | "filter-on" => {
            let Ok(t) = p.downcast::<gtk::ToggleButton>() else {
                return;
            };
            let check = c.m.find::<gtk::Image>().into_iter().next();
            c.same(
                check.as_ref().is_some_and(|i| i.is_visible()),
                c.state == "filter-on",
                "a check shows only while on",
            );
            c.tab_reaches(&t, "the filter chip");
            c.m.activate(&t);
            c.same(
                c.log(),
                vec![format!("filter {}", c.state == "filter-off")],
                "Space flips the filter",
            );
            c.same(
                check.is_some_and(|i| i.is_visible()),
                c.state == "filter-off",
                "the check follows the state",
            );
        }
        "input" => {
            let x = c.m.by_class("chip-remove").into_iter().next();
            let named = x.as_ref().is_some_and(|x| {
                gtk::test_accessible_has_property(x, gtk::AccessibleProperty::Label)
                    && x.tooltip_text().as_deref() == Some("Remove Ana Lima")
            });
            c.expect(named, "the remove button is named Remove Ana Lima");
            if let Some(x) = x {
                c.m.activate(&x);
                c.same(c.log(), vec!["removed".to_string()], "remove is heard once");
                c.expect(p.parent().is_none(), "the chip left its set");
            }
        }
        _ => {
            let Ok(b) = p.downcast::<gtk::Button>() else {
                return;
            };
            c.tab_reaches(&b, "the chip");
            c.m.activate(&b);
            c.same(
                c.log(),
                vec!["clicked".to_string()],
                "Space activates the chip once",
            );
        }
    }
    c.m.unfocus();
}

// ---- discrete slider ----

fn build_discrete(state: &str) -> Built {
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let l = log.clone();
    let v = if state == "high" { 5.0 } else { 1.0 };
    let s = choice::discrete_slider(v, 1.0, 5.0, 1.0, move |v| {
        l.borrow_mut().push(format!("value {v}"))
    });
    if state == "disabled" {
        s.set_sensitive(false);
    }
    let title = w::label("Text size", "");
    let row = w::list_row("", "Text size", "", Some(&s.clone().upcast()));
    let _ = title;
    logged(Built::new(&w::section("", &[row.upcast()]), &s), &log)
}

fn act_discrete(c: &mut Ctx) {
    let Ok(s) = c.probe().clone().downcast::<gtk::Scale>() else {
        return;
    };
    c.expect(s.has_css_class("discrete"), "the slider wears .discrete");
    if c.state == "disabled" {
        c.tab_skips(&s, "a disabled slider");
        return;
    }
    s.set_value(2.4);
    c.same(s.value(), 2.0, "a value between stops snaps to the nearest");
    s.set_value(3.6);
    c.same(s.value(), 4.0, "and up");
    c.expect(
        c.log().iter().all(|l| l == "value 2" || l == "value 4"),
        "only stops are heard",
    );
}

// ---- range slider ----

thread_local! {
    static RANGE: RefCell<Option<Rc<choice::RangeSlider>>> = const { RefCell::new(None) };
}

fn build_range(state: &str) -> Built {
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let l = log.clone();
    let (lo, hi) = if state == "wide" {
        (10.0, 90.0)
    } else {
        (40.0, 60.0)
    };
    let r = Rc::new(choice::RangeSlider::new(
        (0.0, 100.0),
        (lo, hi),
        ("Lowest price", "Highest price"),
        move |a, b| l.borrow_mut().push(format!("range {a} {b}")),
    ));
    if state == "disabled" {
        r.root.set_sensitive(false);
    }
    RANGE.with(|x| *x.borrow_mut() = Some(r.clone()));
    let b = w::vbox(0);
    b.append(&r.root);
    logged(Built::new(&b, &r.low), &log).keeping(r)
}

fn act_range(c: &mut Ctx) {
    let Some(r) = RANGE.with(|x| x.borrow().clone()) else {
        return;
    };
    let named =
        |s: &gtk::Scale| gtk::test_accessible_has_property(s, gtk::AccessibleProperty::Label);
    c.expect(
        named(&r.low) && named(&r.high),
        "each knob has its own name",
    );
    if c.state == "disabled" {
        c.tab_skips(&r.low, "a disabled range");
        return;
    }
    let (_, hi) = r.values();
    r.low.set_value(hi + 20.0);
    c.same(r.low.value(), hi, "the low knob stops at the high one");
    let (lo, _) = r.values();
    r.high.set_value(lo - 30.0);
    c.same(r.high.value(), lo, "the high knob stops at the low one");
}

fn main_components() -> Vec<Component> {
    vec![
        Component {
            id: "checkbox",
            build: build_checkbox,
            act: act_checkbox,
            size: regular,
            named: Named::Words,
            width: w::APP_NARROW.0,
            picture: at_rest,
            row: None,
            control: true,
            check_role: true,
        },
        Component {
            id: "radio",
            build: build_radio,
            act: act_radio,
            size: none,
            named: Named::Label,
            width: w::APP_NARROW.0,
            picture: at_rest,
            row: None,
            control: false,
            check_role: true,
        },
        Component {
            id: "chip",
            build: build_chip,
            act: act_chip,
            size: regular,
            named: Named::Words,
            width: w::APP_NARROW.0,
            picture: at_rest,
            row: None,
            control: true,
            check_role: false,
        },
        Component {
            id: "discrete-slider",
            build: build_discrete,
            act: act_discrete,
            size: none,
            named: Named::Label,
            width: w::APP_NARROW.0,
            picture: at_rest,
            row: None,
            control: false,
            check_role: true,
        },
        Component {
            id: "range-slider",
            build: build_range,
            act: act_range,
            size: none,
            named: Named::Label,
            width: w::APP_NARROW.0,
            picture: at_rest,
            row: None,
            control: false,
            check_role: true,
        },
    ]
}

#[test]
fn choice_controls_meet_their_specs() {
    kit_case::run(&main_components(), KNOWN);
}
