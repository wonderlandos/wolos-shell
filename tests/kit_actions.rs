// SPDX-License-Identifier: GPL-2.0-or-later
//! Kit components of the "actions and selection" batch tested alone, from their specs (WolOS
//! Design System, ADR 0058 draft, DS-SPEC-1): button, icon button, segmented control, slider,
//! stepper, dropdown (pop-up menu) and menu button. The generic part (sizes per density, role and
//! name, layout rules, goldens, the KNOWN ratchet) is `support/kit_case.rs`; each component adds
//! its `build_*` (through the Kit function, never raw GTK) and its `act_*`.
//!
//! `WOL_KIT_ONLY=slider` runs the cases whose name contains it; `WOL_GOLDEN_UPDATE=1` rewrites the
//! goldens and trees; `WOL_KIT_MEASURE=1` prints what the Kit draws. Needs `gtk4-broadwayd` (the
//! golden image has it); skips without one.
#[path = "support/broadway.rs"]
mod broadway;
#[path = "support/kit_case.rs"]
mod kit_case;
#[path = "support/kit_test.rs"]
mod kit_test;

use gtk::prelude::*;
use kit_case::{at_rest, regular, Built, Component, Ctx, Known, Named};
use shell::kit::{form, widgets as w};
use std::rc::Rc;

/// Where the Kit does not meet its spec yet: a ratchet like `rules_ui.rs` BASELINE. The measured
/// value is frozen; when a fix changes it the test fails until the row goes (or is lowered).
/// (component, check, measured)
const KNOWN: Known = &[
    // GTK's min-height is a minimum of the content box: `.list-row { min-height: 40px; padding: 6px
    // 12px }` draws 52 (DS-GTK-1: min-height 28), and `.slider-row { min-height: 60px; padding: 8px
    // 12px }` draws 80 (DS-GTK-1: min-height 44).
    ("segmented", "standard.row", "52"),
    ("slider", "standard.row_stack", "80"),
    ("stepper", "standard.row", "52"),
    ("dropdown", "standard.row", "52"),
    // `scale.slider` has no height of its own: the row's min-height stretches the line to 40 (DS-GTK-1).
    ("slider", "standard.sizes.regular.height", "40"),
    // A control's `min-height: 24px` is the content box, the 1 px border comes on top: 26 (DS-GTK-1:
    // min-height 22). GtkMenuButton has no Kit rule at all and draws the toolkit's 34.
    ("dropdown", "standard.sizes.regular.height", "26"),
    ("menu-button", "standard.sizes.regular.height", "34"),
    // Accessibility the Kit does not give yet (DS-A11Y-1): a segmented group is a generic box, a stepper
    // is two buttons and a label with no spinbutton role or value, a menu button has no accessible label.
    ("segmented", "a11y.role", "generic"),
    ("segmented", "a11y.name", "nameless"),
    ("stepper", "a11y.role", "generic"),
    ("stepper", "a11y.name", "nameless"),
    ("menu-button", "a11y.name", "nameless"),
];

fn card(row: gtk::Widget) -> gtk::Widget {
    w::section("", &[row]).upcast()
}

/// Lets a debounced change (`form::SETTLE_MS`) fire.
fn wait_settle(c: &Ctx) {
    broadway::pump(form::SETTLE_MS + 100);
    c.m.settle();
}

// ---- button ----

fn build_button(state: &str) -> Built {
    let kind = match state {
        "primary" => w::Kind::Primary,
        "destructive" => w::Kind::Destructive,
        "plain" => w::Kind::Plain,
        _ => w::Kind::Default,
    };
    let b = w::button("Install", kind, || {});
    let probe = b.clone();
    let log = Rc::new(std::cell::RefCell::new(Vec::new()));
    let l2 = log.clone();
    b.connect_clicked(move |_| l2.borrow_mut().push("clicked".into()));
    match state {
        "disabled" => {
            b.set_sensitive(false);
            let mut built = Built::alone(&b);
            built.log = log;
            built
        }
        "busy" => {
            // Work in progress: the button waits and a 16 px spinner turns beside it
            // (`w::busy_spinner`, the way Card and AuthSheet show it).
            let row = w::hbox(w::S2);
            row.set_halign(gtk::Align::Start);
            let spin = w::busy_spinner();
            spin.set_visible(true);
            spin.set_spinning(true);
            row.append(&spin);
            b.set_sensitive(false);
            row.append(&b);
            let mut built = Built::new(&row, &probe);
            built.log = log;
            built
        }
        _ => {
            b.set_halign(gtk::Align::Start);
            let mut built = Built::alone(&b);
            built.log = log;
            built
        }
    }
}

fn act_button(c: &mut Ctx) {
    let Some(btn) = c.find::<gtk::Button>() else {
        return;
    };
    let kind = match c.state {
        "primary" => "btn-primary",
        "destructive" => "btn-destructive",
        "plain" => "btn-plain",
        _ => "btn-default",
    };
    c.expect(
        btn.has_css_class("btn") && btn.has_css_class(kind),
        format_args!("the button wears btn and {kind}"),
    );
    match c.state {
        "disabled" | "busy" => {
            c.expect(!btn.is_sensitive(), "the button waits");
            c.tab_skips(&btn, "a button that waits");
            if c.state == "busy" {
                let spinners = c.m.find::<gtk::Spinner>();
                c.expect(
                    spinners.len() == 1 && spinners[0].is_spinning(),
                    "one spinner turns beside the waiting button",
                );
                if let Some(s) = spinners.first() {
                    let (_, _, sw, sh) = c.m.rect(s);
                    c.hold("busy.spinner.width", sw, 16);
                    c.hold("busy.spinner.height", sh, 16);
                }
            }
            c.m.unfocus();
        }
        _ => {
            c.tab_reaches(&btn, "the button");
            c.m.activate(&btn);
            let n = c.log().len();
            c.same(n, 1, "Space or Enter clicks the button once");
            c.m.unfocus();
        }
    }
}

// ---- icon button ----

fn build_icon_button(state: &str) -> Built {
    let b = w::icon_button("list-add-symbolic", "Add", || {});
    b.set_halign(gtk::Align::Start);
    let log = Rc::new(std::cell::RefCell::new(Vec::new()));
    let l2 = log.clone();
    b.connect_clicked(move |_| l2.borrow_mut().push("clicked".into()));
    if state == "disabled" {
        b.set_sensitive(false);
    }
    let mut built = Built::alone(&b);
    built.log = log;
    built
}

fn act_icon_button(c: &mut Ctx) {
    let Some(btn) = c.find::<gtk::Button>() else {
        return;
    };
    // RU-050: an icon alone has a tooltip, and the tooltip is its name.
    let tip = btn.tooltip_text().map(|t| t.to_string()).unwrap_or_default();
    c.same(tip.as_str(), "Add", "the tooltip names the button");
    match c.state {
        "disabled" => {
            c.tab_skips(&btn, "a disabled icon button");
            c.m.unfocus();
        }
        _ => {
            c.tab_reaches(&btn, "the icon button");
            c.m.activate(&btn);
            let n = c.log().len();
            c.same(n, 1, "Space clicks the icon button once");
            c.m.unfocus();
        }
    }
}

// ---- segmented ----

const SEGMENTS: [&str; 3] = ["Light", "Dark", "Auto"];

fn build_segmented(state: &str) -> Built {
    let selected = if state == "last" { 2 } else { 0 };
    let log = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let l2 = log.clone();
    let row = form::segmented_row("Appearance", "", &SEGMENTS, selected, move |i| {
        l2.borrow_mut().push(format!("picked {i}"))
    });
    let seg = {
        // The segmented box inside the row.
        let mut found = None;
        let mut stack = vec![row.clone()];
        while let Some(x) = stack.pop() {
            if x.has_css_class("segmented") {
                found = Some(x);
                break;
            }
            let mut ch = x.first_child();
            while let Some(y) = ch {
                ch = y.next_sibling();
                stack.push(y);
            }
        }
        found.expect("the segmented control in its row")
    };
    if state == "disabled" {
        seg.set_sensitive(false);
    }
    let mut built = Built::new(&card(row), &seg);
    built.log = log;
    built
}

fn act_segmented(c: &mut Ctx) {
    let buttons = c.m.find::<gtk::ToggleButton>();
    c.same(buttons.len(), 3, "one toggle button per option");
    let active: Vec<bool> = buttons.iter().map(|b| b.is_active()).collect();
    let want = if c.state == "last" {
        [false, false, true]
    } else {
        [true, false, false]
    };
    c.same(active, want.to_vec(), "exactly the selected option is pressed");
    if buttons.len() != 3 {
        return;
    }
    if c.state == "disabled" {
        c.expect(
            buttons.iter().all(|b| !b.is_sensitive()),
            "a disabled control disables every option",
        );
        c.tab_skips(&buttons[0], "a disabled segmented control");
        c.m.unfocus();
        return;
    }
    // Choosing another option reports it once and moves the pressed state (one at a time).
    let other = if c.state == "last" { 0 } else { 1 };
    c.m.activate(&buttons[other]);
    c.same(c.log(), vec![format!("picked {other}")], "a pick is reported once");
    let now: Vec<bool> = buttons.iter().map(|b| b.is_active()).collect();
    c.expect(
        now.iter().filter(|a| **a).count() == 1 && now[other],
        "only the picked option is pressed",
    );
    c.m.unfocus();
}

// ---- slider ----

fn build_slider(state: &str) -> Built {
    let log = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let l2 = log.clone();
    let value = if state == "high" { 60.0 } else { 10.0 };
    let row = form::slider_row(
        "Key repeat rate",
        (10.0, 60.0, 5.0),
        value,
        ("Slow", "Fast"),
        move |v| l2.borrow_mut().push(format!("value {v}")),
    );
    let scale = {
        let mut found = None;
        let mut stack = vec![row.clone()];
        while let Some(x) = stack.pop() {
            if x.is::<gtk::Scale>() {
                found = Some(x);
                break;
            }
            let mut ch = x.first_child();
            while let Some(y) = ch {
                ch = y.next_sibling();
                stack.push(y);
            }
        }
        found.expect("the scale in its row")
    };
    if state == "disabled" {
        scale.set_sensitive(false);
    }
    let mut built = Built::new(&card(row), &scale);
    built.log = log;
    built
}

fn act_slider(c: &mut Ctx) {
    let Some(s) = c.find::<gtk::Scale>() else {
        return;
    };
    c.expect(s.has_css_class("slider"), "the scale wears slider");
    let adj = s.adjustment();
    c.same((adj.lower(), adj.upper()), (10.0, 60.0), "the range is the row's");
    match c.state {
        "disabled" => {
            c.tab_skips(&s, "a disabled slider");
            c.m.unfocus();
        }
        state => {
            c.tab_reaches(&s, "the slider");
            let at = s.value();
            // The arrow keys of a slider move it one step; the change is reported once, settled.
            let forward = state != "high";
            s.emit_by_name::<()>(
                "move-slider",
                &[&if forward {
                    gtk::ScrollType::StepForward
                } else {
                    gtk::ScrollType::StepBackward
                }],
            );
            wait_settle(c);
            let moved = s.value() - at;
            c.same(moved, if forward { 5.0 } else { -5.0 }, "an arrow key moves one step");
            let n = c.log().len();
            c.same(n, 1, "the change is reported once, after it settles");
            c.m.unfocus();
        }
    }
}

// ---- stepper ----

fn fmt_pt(v: i64) -> String {
    format!("{v} pt")
}

fn build_stepper(state: &str) -> Built {
    let log = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let l2 = log.clone();
    let value = match state {
        "min" => 0,
        "max" => 32,
        _ => 12,
    };
    let row = form::stepper_row("Gaps", "", value, (0, 32, 4), fmt_pt, move |v| {
        l2.borrow_mut().push(format!("value {v}"))
    });
    let step = {
        let mut found = None;
        let mut stack = vec![row.clone()];
        while let Some(x) = stack.pop() {
            if x.has_css_class("stepper") {
                found = Some(x);
                break;
            }
            let mut ch = x.first_child();
            while let Some(y) = ch {
                ch = y.next_sibling();
                stack.push(y);
            }
        }
        found.expect("the stepper in its row")
    };
    let mut built = Built::new(&card(row), &step);
    built.log = log;
    built
}

fn act_stepper(c: &mut Ctx) {
    let buttons = c.m.find::<gtk::Button>();
    c.same(buttons.len(), 2, "a Less and a More button");
    if buttons.len() != 2 {
        return;
    }
    let (less, more) = (&buttons[0], &buttons[1]);
    let shown = c.m.by_class("stepper-value").into_iter().next();
    let text = shown
        .and_then(|l| l.downcast::<gtk::Label>().ok())
        .map(|l| l.text().to_string())
        .unwrap_or_default();
    let (at, min, max) = match c.state {
        "min" => (0, true, false),
        "max" => (32, false, true),
        _ => (12, false, false),
    };
    c.same(text.as_str(), fmt_pt(at).as_str(), "the value as the format renders it");
    c.same(less.is_sensitive(), !min, "Less waits at the minimum");
    c.same(more.is_sensitive(), !max, "More waits at the maximum");
    // The keyboard: Tab reaches the first button that works; Space steps by 4.
    let first = if min { more } else { less };
    c.tab_reaches(first, "the first button that works");
    c.m.activate(first);
    let want = if min { at + 4 } else { at - 4 };
    c.same(c.log(), vec![format!("value {want}")], "a step is reported once");
    c.m.unfocus();
}

// ---- dropdown ----

const OPTIONS: [&str; 3] = ["Dwindle", "Master", "Monocle"];

fn build_dropdown(state: &str) -> Built {
    let log = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let l2 = log.clone();
    let pick = move |i: usize| l2.borrow_mut().push(format!("picked {i}"));
    let row = if state == "search" {
        form::search_picker_row("Layout", "", &OPTIONS, 0, pick)
    } else {
        form::picker_row("Layout", "", &OPTIONS, if state == "last" { 2 } else { 0 }, pick)
    };
    let dd = {
        let mut found = None;
        let mut stack = vec![row.clone()];
        while let Some(x) = stack.pop() {
            if x.is::<gtk::DropDown>() {
                found = Some(x);
                break;
            }
            let mut ch = x.first_child();
            while let Some(y) = ch {
                ch = y.next_sibling();
                stack.push(y);
            }
        }
        found.expect("the drop-down in its row")
    };
    if state == "disabled" {
        dd.set_sensitive(false);
    }
    let mut built = Built::new(&card(row), &dd);
    built.log = log;
    built
}

fn act_dropdown(c: &mut Ctx) {
    let Some(dd) = c.find::<gtk::DropDown>() else {
        return;
    };
    c.expect(dd.has_css_class("dropdown"), "the drop-down wears dropdown");
    let at = if c.state == "last" { 2 } else { 0 };
    c.same(dd.selected(), at, "the selected option");
    if c.state == "search" {
        c.expect(dd.enables_search(), "a search picker searches its options");
    } else {
        c.expect(!dd.enables_search(), "a plain picker has no search field");
    }
    if c.state == "disabled" {
        c.tab_skips(&dd, "a disabled drop-down");
        c.m.unfocus();
        return;
    }
    c.tab_reaches(&dd, "the drop-down");
    // Choosing an option reports its index once.
    let to = if at == 0 { 1 } else { 0 };
    dd.set_selected(to);
    c.m.settle();
    c.same(c.log(), vec![format!("picked {to}")], "a pick is reported once");
    c.m.unfocus();
}

// ---- menu button ----

fn build_menu_button(state: &str) -> Built {
    let b = form::actions_button(vec![("Remove Printer", Box::new(|| {}) as Box<dyn Fn()>)])
        .expect("a menu button for one action");
    b.set_halign(gtk::Align::Start);
    if state == "disabled" {
        b.set_sensitive(false);
    }
    Built::alone(&b)
}

fn act_menu_button(c: &mut Ctx) {
    let Some(b) = c.find::<gtk::MenuButton>() else {
        return;
    };
    let tip = b.tooltip_text().map(|t| t.to_string()).unwrap_or_default();
    c.same(tip.as_str(), "More", "the tooltip names the menu button");
    c.expect(b.popover().is_some(), "the button owns its menu");
    c.expect(
        form::actions_button(Vec::new()).is_none(),
        "no action, no button",
    );
    if c.state == "disabled" {
        c.tab_skips(&b, "a disabled menu button");
    } else {
        c.tab_reaches(&b, "the menu button");
        // Space opens the menu, Escape closes it.
        b.popup();
        c.m.settle();
        let open = b.popover().is_some_and(|p| p.is_visible());
        c.expect(open, "Space opens the menu");
        b.popdown();
        c.m.settle();
    }
    c.m.unfocus();
}

#[test]
fn every_actions_component_case_meets_its_spec() {
    let all = [
        Component {
            id: "button",
            build: build_button,
            act: act_button,
            size: regular,
            named: Named::Words,
            width: w::APP_NARROW.0,
            picture: at_rest,
            row: None,
            control: true,
            check_role: true,
        },
        Component {
            id: "icon-button",
            build: build_icon_button,
            act: act_icon_button,
            size: regular,
            named: Named::Label,
            width: w::APP_NARROW.0,
            picture: at_rest,
            row: None,
            control: true,
            check_role: true,
        },
        Component {
            id: "segmented",
            build: build_segmented,
            act: act_segmented,
            size: regular,
            named: Named::Label,
            width: w::APP_NARROW.0,
            picture: at_rest,
            row: Some("list-row"),
            control: false,
            check_role: true,
        },
        Component {
            id: "slider",
            build: build_slider,
            act: act_slider,
            size: regular,
            named: Named::Label,
            width: w::APP_NARROW.0,
            picture: at_rest,
            row: Some("slider-row:row_stack"),
            control: false,
            check_role: true,
        },
        Component {
            id: "stepper",
            build: build_stepper,
            act: act_stepper,
            size: regular,
            named: Named::Label,
            width: w::APP_NARROW.0,
            picture: at_rest,
            row: Some("list-row"),
            control: false,
            check_role: true,
        },
        Component {
            id: "dropdown",
            build: build_dropdown,
            act: act_dropdown,
            size: regular,
            named: Named::Label,
            width: w::APP_NARROW.0,
            picture: at_rest,
            row: Some("list-row"),
            control: true,
            check_role: true,
        },
        Component {
            id: "menu-button",
            build: build_menu_button,
            act: act_menu_button,
            size: regular,
            named: Named::Label,
            width: w::APP_NARROW.0,
            picture: at_rest,
            row: None,
            control: false,
            check_role: true,
        },
    ];
    kit_case::run(&all, KNOWN);
}
