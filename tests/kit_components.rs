// SPDX-License-Identifier: GPL-2.0-or-later
//! Kit components tested alone, from their specs (WolOS Design System, ADR 0058 draft): no app,
//! no page. Each component's spec (`design/components/<id>.toml`, compiled into
//! `design/components/catalog.json`) lists its states, densities and variants; this test builds
//! every case with the Kit, acts on it the way a person does, and holds it to the spec: sizes per
//! density, the keyboard, the accessibility roles and relations, the layout rules, and one golden
//! picture plus one view-tree snapshot per case (`tests/golden-kit/kit-<id>-<state>-<density>-<variant>`).
//!
//! `WOL_KIT_ONLY=switch` runs the cases whose name contains it; `WOL_GOLDEN_UPDATE=1` rewrites the
//! goldens and trees. Needs `gtk4-broadwayd` (the golden image has it); skips without one.
//! First component: the switch (proof of concept). Every other component adds one `build_*`.
#[path = "support/broadway.rs"]
mod broadway;
#[path = "support/kit_test.rs"]
mod kit_test;

use gtk::prelude::*;
use kit_test::{Density, Frame, Mounted, Tester};
use shell::kit::{form, widgets as w};
use shell::theme::Variant;
use std::cell::RefCell;
use std::rc::Rc;

/// Where the Kit does not meet its spec yet: a ratchet like `rules_ui.rs` BASELINE. The measured
/// value is frozen; when a fix changes it the test fails until the row goes (or is lowered).
/// (component, check, measured px)
const KNOWN: &[(&str, &str, i32)] = &[
    // GTK's min-height is a minimum of the content box: `.list-row { min-height: 40px; padding: 6px
    // 12px }` draws 52, every Settings row of the goldens too (DS-GTK-1: min-height 28).
    ("switch", "standard.row", 52),
    // `switch.toggle { min-height: 24px; padding: 2px }` draws 28 (DS-GTK-1: min-height 20).
    ("switch", "standard.sizes.regular.height", 28),
    ("switch", "comfortable.sizes.regular.height", 28),
    // WOLKIT-1TO1: the spec is the approved 36 (GAP-TABLE M2); Kit windows draw it through
    // libWolkit by default. GtkSwitch is two thumbs wide, so this GTK fallback draws 44.
    ("switch", "standard.sizes.regular.width", 44),
    ("switch", "comfortable.sizes.regular.width", 44),
];

type Done = Box<dyn FnOnce(bool)>;

/// One built case: the widget to mount, and what the test needs to act on it.
struct Built {
    widget: gtk::Widget,
    pending: Rc<RefCell<Option<Done>>>,
    flips: Rc<RefCell<Vec<bool>>>,
}

/// The switch in its Settings row (`form::toggle_row`, `form::pending_toggle_row`) in a card list,
/// in one state of `tests.states`.
fn build_switch(state: &str) -> Built {
    let flips: Rc<RefCell<Vec<bool>>> = Rc::default();
    let pending: Rc<RefCell<Option<Done>>> = Rc::default();
    let row = match state {
        "off" | "on" | "disabled" => {
            let f = flips.clone();
            form::toggle_row("Bluetooth", "", state == "on", move |on| {
                f.borrow_mut().push(on)
            })
        }
        "pending" => {
            let p = pending.clone();
            form::pending_toggle_row("Bluetooth", "", false, move |_, done| {
                *p.borrow_mut() = Some(done)
            })
        }
        other => panic!("switch: the spec names state {other}, which this test cannot build"),
    };
    let card = w::section("", &[row]);
    Built {
        widget: card.upcast(),
        pending,
        flips,
    }
}

fn near(a: i32, b: i32) -> bool {
    (a - b).abs() <= 1
}

/// Holds `measured` to the spec's `want`, unless KNOWN freezes it; a problem sentence or none.
fn hold(
    id: &str,
    check: &str,
    measured: i32,
    want: i32,
    bad: &mut Vec<String>,
    seen: &mut Vec<String>,
) {
    let key = format!("{id}:{check}");
    if let Some(k) = KNOWN.iter().find(|k| k.0 == id && k.1 == check) {
        seen.push(key);
        if measured != k.2 {
            bad.push(format!(
                "{id} {check}: drawn {measured} px, KNOWN says {}: the Kit changed; lower or remove the KNOWN row (spec: {want})",
                k.2
            ));
        }
    } else if !near(measured, want) {
        bad.push(format!(
            "{id} {check}: drawn {measured} px, the spec says {want}"
        ));
    }
}

/// Every case of the switch spec; the problems found.
fn switch_cases(t: &Tester, only: Option<&str>, report: &mut Vec<String>) -> Vec<String> {
    let spec = kit_test::spec("switch");
    let mut bad = Vec::new();
    let mut seen = Vec::new();
    let states = kit_test::strings(&spec["tests"]["states"]);
    let densities = kit_test::strings(&spec["tests"]["densities"]);
    let variants = kit_test::strings(&spec["tests"]["variants"]);
    assert!(
        !states.is_empty() && !densities.is_empty() && !variants.is_empty(),
        "switch: the spec lists its cases"
    );
    for state in &states {
        for d in &densities {
            let density = Density::parse(d).unwrap_or_else(|| panic!("switch: density {d}"));
            for v in &variants {
                let variant = if v == "light" {
                    Variant::Light
                } else {
                    Variant::Dark
                };
                let frame = Frame {
                    width: w::APP_NARROW.0,
                    density,
                    variant,
                };
                let name = format!("switch-{state}-{}", frame.tag());
                if only.is_some_and(|o| !name.contains(o)) {
                    continue;
                }
                let b = build_switch(state);
                let m = t.mount(&b.widget, frame);
                let before = bad.len();
                check_switch(
                    t, &spec, state, &frame, &b, &m, &name, &mut bad, &mut seen, report,
                );
                bad.extend(m.golden(t, &name));
                println!(
                    "{name}: {}",
                    if bad.len() == before { "ok" } else { "FAIL" }
                );
            }
        }
    }
    // A KNOWN row that no case measured is stale.
    if only.is_none() {
        for k in KNOWN.iter().filter(|k| k.0 == "switch") {
            let key = format!("{}:{}", k.0, k.1);
            if !seen.contains(&key) {
                bad.push(format!("KNOWN row {key} is measured by no case: remove it"));
            }
        }
    }
    bad
}

#[allow(clippy::too_many_arguments)] // one call per check family; a struct would only rename the arguments
fn check_switch(
    t: &Tester,
    spec: &serde_json::Value,
    state: &str,
    f: &Frame,
    b: &Built,
    m: &Mounted,
    name: &str,
    bad: &mut Vec<String>,
    seen: &mut Vec<String>,
    report: &mut Vec<String>,
) {
    let _ = t;
    let switches = m.find::<gtk::Switch>();
    assert_eq!(switches.len(), 1, "{name}: one switch");
    let s = &switches[0];
    let title = m.by_text("Bluetooth").expect("the row title");
    let row = m
        .by_class("list-row")
        .into_iter()
        .next()
        .expect("a list row");

    // Anatomy: the switch carries the Kit class and sits at the trailing end of its row.
    assert!(s.has_css_class("toggle"), "{name}: switch.toggle");
    let (sx, _, sw, sh) = m.rect(s);
    let (tx, _, tw, _) = m.rect(&title);
    assert!(sx > tx + tw, "{name}: the switch follows the title");
    let (rx, _, rw, rh) = m.rect(&row);
    let row_pad_right = rx + rw - (sx + sw);

    // Sizes per density, from the spec.
    let id = "switch";
    let dn = f.density.name();
    let want_row = spec["density"][dn]["row"].as_i64().expect("density row") as i32;
    hold(id, &format!("{dn}.row"), rh, want_row, bad, seen);
    let size = if f.density == Density::Compact {
        "small"
    } else {
        "regular"
    };
    let want_w = spec["sizes"][size]["width"].as_i64().expect("width") as i32;
    let want_h = spec["sizes"][size]["height"].as_i64().expect("height") as i32;
    hold(
        id,
        &format!("{dn}.sizes.{size}.width"),
        sw,
        want_w,
        bad,
        seen,
    );
    hold(
        id,
        &format!("{dn}.sizes.{size}.height"),
        sh,
        want_h,
        bad,
        seen,
    );
    if f.variant == Variant::Light && state == "off" {
        report.push(format!(
            "switch {dn}: row {rh} (spec {want_row}), switch {sw}x{sh} (spec {size} {want_w}x{want_h}), trailing inset {row_pad_right}"
        ));
    }

    // The layout rules of HIG section 6.
    for i in m.issues() {
        bad.push(format!("{name}: layout: {i}"));
    }

    // Accessibility: a switch, named by the row title (the row's only label), checked when on.
    assert!(
        gtk::test_accessible_has_role(s, gtk::AccessibleRole::Switch),
        "{name}: role switch"
    );
    if !gtk::test_accessible_has_relation(s, gtk::AccessibleRelation::LabelledBy) {
        bad.push(format!(
            "{name}: the switch has no name (labelled-by the row title)"
        ));
    }
    let labels: Vec<String> = m
        .find::<gtk::Label>()
        .iter()
        .filter(|l| l.is_drawable())
        .map(|l| l.text().to_string())
        .collect();
    assert_eq!(
        labels,
        ["Bluetooth"],
        "{name}: the row title is its only label"
    );

    // State, keyboard and behaviour.
    match state {
        "off" | "on" => {
            assert_eq!(s.is_active(), state == "on", "{name}: drawn {state}");
            let focused = m.tab();
            assert_eq!(
                focused.as_ref(),
                Some(s.upcast_ref()),
                "{name}: Tab reaches the switch first"
            );
            m.activate(s);
            assert_eq!(s.is_active(), state != "on", "{name}: Space flips it");
            assert_eq!(
                *b.flips.borrow(),
                [state != "on"],
                "{name}: the change is reported once"
            );
            m.activate(s);
            assert_eq!(s.is_active(), state == "on", "{name}: and back");
            // The picture is of the state at rest, without the focus ring.
            m.unfocus();
        }
        "disabled" => {
            s.set_sensitive(false);
            m.settle();
            let focused = m.tab();
            assert_ne!(
                focused.as_ref(),
                Some(s.upcast_ref()),
                "{name}: Tab skips a disabled switch"
            );
            m.unfocus();
        }
        "pending" => {
            m.activate(s);
            let spinners = m.find::<gtk::Spinner>();
            assert_eq!(spinners.len(), 1, "{name}: one spinner beside the switch");
            assert!(
                !s.is_sensitive() && spinners[0].get_visible() && spinners[0].is_spinning(),
                "{name}: the switch waits and the spinner turns"
            );
            assert!(b.pending.borrow().is_some(), "{name}: the change started");
            m.unfocus();
        }
        _ => unreachable!(),
    }
}

/// Settles a pending switch after its picture: a failure flips it back (RC-01), a success keeps it.
fn pending_outcomes(t: &Tester) {
    let frame = Frame {
        width: w::APP_NARROW.0,
        density: Density::Standard,
        variant: Variant::Light,
    };
    for ok in [false, true] {
        let b = build_switch("pending");
        let m = t.mount(&b.widget, frame);
        let s = m.find::<gtk::Switch>().remove(0);
        m.activate(&s);
        let done = b.pending.borrow_mut().take().expect("the change started");
        done(ok);
        m.settle();
        assert_eq!(
            s.is_active(),
            ok,
            "pending: {} shows the confirmed state",
            if ok { "success" } else { "failure" }
        );
        assert!(
            s.is_sensitive() && !m.find::<gtk::Spinner>()[0].get_visible(),
            "pending: done, the wait ends"
        );
    }
}

/// One test: GTK lives on the thread that started it.
#[test]
fn every_kit_component_case_meets_its_spec() {
    let Some(t) = Tester::start() else {
        // CI and the golden container set WOL_KIT_REQUIRE_DISPLAY=1: a lane that could not start
        // its display has checked nothing and must not pass.
        assert!(
            std::env::var_os("WOL_KIT_REQUIRE_DISPLAY").is_none_or(|v| v.is_empty() || v == "0"),
            "WOL_KIT_REQUIRE_DISPLAY is set and the toolkit display (gtk4-broadwayd) did not start"
        );
        eprintln!("skipped: no display for the toolkit (gtk4-broadwayd)");
        return;
    };
    if let Some(s) = gtk::Settings::default() {
        // Pictures of a state, not of a frame of its animation (motion has its own tests).
        s.set_gtk_enable_animations(false);
    }
    let only = std::env::var("WOL_KIT_ONLY").ok();
    let mut report = Vec::new();
    let mut bad = switch_cases(&t, only.as_deref(), &mut report);
    if only
        .as_deref()
        .is_none_or(|o| "switch-pending".contains(o) || o == "switch")
    {
        pending_outcomes(&t);
    }
    for r in &report {
        println!("measured: {r}");
    }
    println!("renders and trees: {}", t.out.display());
    assert!(
        bad.is_empty(),
        "kit components off their spec:\n{}",
        bad.join("\n")
    );
    bad.clear();
}
