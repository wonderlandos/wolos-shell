// SPDX-License-Identifier: GPL-2.0-or-later
//! Kit components of the "input" batch tested alone, from their specs (WolOS Design System, ADR
//! 0058 draft, DS-SPEC-2): text field (entry row and pending entry row), password field with the
//! Caps Lock note, search field, shortcut recorder and colour well. The generic part (sizes per
//! density, role and name, layout rules, goldens, the KNOWN ratchet) is `support/kit_case.rs`; each
//! component adds its `build_*` (through the Kit function, never raw GTK) and its `act_*`.
//!
//! `WOL_KIT_ONLY=text-field` runs the cases whose name contains it; `WOL_GOLDEN_UPDATE=1` rewrites
//! the goldens and trees; `WOL_KIT_MEASURE=1` prints what the Kit draws. Needs `gtk4-broadwayd`
//! (the golden image has it); skips without one.
#[path = "support/broadway.rs"]
mod broadway;
#[path = "support/kit_case.rs"]
mod kit_case;
#[path = "support/kit_test.rs"]
mod kit_test;

use gtk::prelude::*;
use kit_case::{at_rest, regular, Built, Component, Ctx, Known, Named};
use shell::kit::{form, pickers, widgets as w};
use std::cell::RefCell;
use std::rc::Rc;

/// Where the Kit does not meet its spec yet: a ratchet like `rules_ui.rs` BASELINE. The measured
/// value is frozen; when a fix changes it the test fails until the row goes (or is lowered).
/// (component, check, measured)
const KNOWN: Known = &[
    // `.list-row` draws 52 for a 40 row, and a control's `min-height: 24px` is the content box with the
    // 1 px border on top (26) (DS-GTK-1: min-height 28 and 22). The colour well is a 22 px disc in a
    // 26 px button.
    ("text-field", "standard.row", "52"),
    ("text-field", "standard.sizes.regular.height", "26"),
    ("password-field", "standard.sizes.regular.height", "26"),
    ("search-field", "standard.sizes.regular.height", "26"),
    ("shortcut-recorder", "standard.row", "52"),
    ("colour-well", "standard.row", "52"),
    ("colour-well", "standard.sizes.regular.width", "26"),
    ("colour-well", "standard.sizes.regular.height", "26"),
    // Names the Kit does not give yet (DS-A11Y-1, RU-047): a password or search field is named by its
    // placeholder at best, a colour well by its tooltip, which GTK does not expose as a name.
    ("password-field", "a11y.name", "nameless"),
    ("search-field", "a11y.name", "nameless"),
    ("colour-well", "a11y.name", "nameless"),
];

fn card(row: gtk::Widget) -> gtk::Widget {
    w::section("", &[row]).upcast()
}

// ---- text field ----

fn build_text_field(state: &str) -> Built {
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let l2 = log.clone();
    let text = if state == "empty" { "" } else { "Atlas" };
    let done: Rc<RefCell<Option<kit_case::Done>>> = Rc::default();
    let row = if state == "pending" {
        let d = done.clone();
        form::pending_entry_row("Computer name", text, "Name", move |t, finish| {
            l2.borrow_mut().push(format!("committed {t}"));
            *d.borrow_mut() = Some(finish);
        })
    } else {
        form::entry_row("Computer name", text, "Name", move |t| {
            l2.borrow_mut().push(format!("committed {t}"))
        })
    };
    let entry = find_entry(&row);
    if state == "disabled" {
        entry.set_sensitive(false);
    }
    let mut built = Built::new(&card(row), &entry);
    built.log = log;
    built.done = done;
    built
}

fn find_entry(row: &gtk::Widget) -> gtk::Widget {
    let mut stack = vec![row.clone()];
    while let Some(x) = stack.pop() {
        if x.is::<gtk::Entry>() {
            return x;
        }
        let mut ch = x.first_child();
        while let Some(y) = ch {
            ch = y.next_sibling();
            stack.push(y);
        }
    }
    panic!("the entry in its row")
}

fn act_text_field(c: &mut Ctx) {
    let Some(e) = c.find::<gtk::Entry>() else {
        return;
    };
    c.expect(e.has_css_class("text-field"), "the entry wears text-field");
    c.same(e.placeholder_text().map(|p| p.to_string()), Some("Name".to_string()), "the placeholder says what to type");
    match c.state {
        "empty" => {
            c.same(e.text().to_string(), String::new(), "an empty field shows its placeholder");
            c.tab_reaches(&e, "the field");
            e.set_text("Orion");
            e.emit_activate();
            c.m.settle();
            c.same(c.log(), vec!["committed Orion".to_string()], "Enter commits the text once");
            // The same text again is not a change.
            e.emit_activate();
            c.same(c.log().len(), 1, "Enter on an unchanged text commits nothing");
        }
        "filled" => {
            c.same(e.text().to_string(), "Atlas".to_string(), "the text shown");
            c.tab_reaches(&e, "the field");
            e.emit_activate();
            c.same(c.log().len(), 0, "Enter on an unchanged text commits nothing");
        }
        "disabled" => {
            c.tab_skips(&e, "a disabled field");
        }
        "pending" => {
            e.set_text("Orion");
            e.emit_activate();
            c.m.settle();
            c.expect(!e.is_sensitive(), "the field waits while the commit runs");
            c.expect(c.b.done.borrow().is_some(), "the commit started");
            // A cancelled commit puts the field back, and Enter commits the same text again.
            if let Some(done) = c.b.done.borrow_mut().take() {
                done(false);
            }
            c.m.settle();
            c.expect(e.is_sensitive(), "a failed commit ends the wait");
            e.emit_activate();
            c.m.settle();
            c.same(c.log().len(), 2, "after a cancel, Enter commits the same text again");
            if let Some(done) = c.b.done.borrow_mut().take() {
                done(true);
            }
            c.m.settle();
        }
        _ => {}
    }
    c.m.unfocus();
}

// ---- password field ----

fn build_password_field(state: &str) -> Built {
    let col = w::vbox(w::S1);
    let field = w::password_field("Password");
    let caps = w::caps_lock_note();
    caps.set_halign(gtk::Align::Start);
    col.append(&field);
    col.append(&caps);
    col.set_halign(gtk::Align::Start);
    if state == "filled" {
        field.set_text("correct horse");
    }
    if state == "caps" {
        caps.set_visible(true);
    }
    if state == "disabled" {
        field.set_sensitive(false);
    }
    Built::new(&col, &field)
}

fn act_password_field(c: &mut Ctx) {
    let Some(p) = c.find::<gtk::PasswordEntry>() else {
        return;
    };
    c.expect(p.has_css_class("text-field"), "the field wears text-field");
    c.expect(p.shows_peek_icon(), "the field can show what was typed (peek)");
    let note = c.m.by_text(w::CAPS_LOCK);
    c.expect(note.is_some(), "the Caps Lock note exists under the field");
    c.same(
        note.is_some_and(|n| n.is_visible()),
        c.state == "caps",
        "the note shows only while Caps Lock is on",
    );
    match c.state {
        "disabled" => {
            c.tab_skips(&p, "a disabled field");
        }
        _ => c.tab_reaches(&p, "the field"),
    }
    c.m.unfocus();
}

// ---- search field ----

fn build_search_field(state: &str) -> Built {
    let s = w::search_field("Search");
    s.set_halign(gtk::Align::Start);
    s.set_width_chars(24);
    if state == "filled" {
        s.set_text("wolf");
    }
    if state == "disabled" {
        s.set_sensitive(false);
    }
    Built::alone(&s)
}

fn act_search_field(c: &mut Ctx) {
    let Some(s) = c.find::<gtk::SearchEntry>() else {
        return;
    };
    c.expect(s.has_css_class("search-field"), "the field wears search-field");
    c.same(
        s.placeholder_text().map(|p| p.to_string()),
        Some("Search".to_string()),
        "the placeholder names the field",
    );
    if c.state == "disabled" {
        c.tab_skips(&s, "a disabled field");
    } else {
        c.tab_reaches(&s, "the field");
        if c.state == "filled" {
            c.same(s.text().to_string(), "wolf".to_string(), "the typed text");
        }
    }
    c.m.unfocus();
}

// ---- shortcut recorder ----

fn combo() -> wol_core::shortcuts::Combo {
    wol_core::shortcuts::Combo::parse("SUPER", "space").expect("a combo")
}

fn build_shortcut_recorder(state: &str) -> Built {
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let (l1, l2) = (log.clone(), log.clone());
    let c = combo();
    let r = form::ShortcutRecorder::new(
        if state == "off" { None } else { Some(&c) },
        move |on| l1.borrow_mut().push(format!("recording {on}")),
        move |c| l2.borrow_mut().push(format!("recorded {}", c.map_or("nothing".into(), |c| c.label()))),
    );
    match state {
        "conflict" => r.set_conflict(true),
        "locked" => r.set_editable(false),
        "recording" => r.button.emit_clicked(),
        _ => {}
    }
    let row = form::row("Show Search", "", &r.button);
    let mut built = Built::new(&card(row), &r.button);
    built.log = log;
    // The recorder owns the key controller: keep it as long as the case.
    built.keeping(r)
}

fn act_shortcut_recorder(c: &mut Ctx) {
    let Some(b) = c.find::<gtk::Button>() else {
        return;
    };
    c.expect(b.has_css_class("shortcut-recorder"), "the button wears shortcut-recorder");
    let shown = c
        .m
        .find::<gtk::Label>()
        .into_iter()
        .find(|l| l.is_ancestor(&b))
        .map(|l| l.text().to_string())
        .unwrap_or_default();
    let want = match c.state {
        "off" => "Off".to_string(),
        "recording" => "Type Shortcut".to_string(),
        _ => combo().label(),
    };
    c.same(shown, want, "what the recorder shows");
    c.same(b.has_css_class("conflict"), c.state == "conflict", "the warning outline marks a taken combo");
    c.same(b.has_css_class("recording"), c.state == "recording", "recording is marked");
    match c.state {
        "locked" => {
            c.tab_skips(&b, "a fixed shortcut");
        }
        "recording" => {
            c.same(c.log(), vec!["recording true".to_string()], "recording starts once");
            // Losing the keyboard ends the recording without a change.
            c.m.unfocus();
            b.grab_focus();
            c.m.unfocus();
        }
        _ => {
            c.tab_reaches(&b, "the recorder");
            c.m.activate(&b);
            c.same(c.log(), vec!["recording true".to_string()], "Space starts recording once");
            c.expect(b.has_css_class("recording"), "the recorder listens");
        }
    }
    c.m.unfocus();
}

// ---- colour well ----

fn build_colour_well(state: &str) -> Built {
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let l2 = log.clone();
    let swatches = vec![
        ("Blue".to_string(), "#0a6fe0".to_string()),
        ("Green".to_string(), "#1f8f3c".to_string()),
    ];
    let well = pickers::ColorWell::new("#7452f5", &swatches, pickers::Tokens::current(), move |hex| {
        l2.borrow_mut().push(format!("changed {hex}"))
    });
    if state == "disabled" {
        well.button.set_sensitive(false);
    }
    let row = form::row("Border colour", "", &well.button);
    let mut built = Built::new(&card(row), &well.button);
    built.log = log;
    built.keeping(well)
}

fn act_colour_well(c: &mut Ctx) {
    let Some(b) = c.find::<gtk::MenuButton>() else {
        return;
    };
    c.expect(b.has_css_class("color-well"), "the button wears color-well");
    c.same(b.tooltip_text().map(|t| t.to_string()), Some("#7452f5".to_string()), "the tooltip tells the colour");
    c.expect(b.popover().is_some(), "the well owns its swatches");
    if c.state == "disabled" {
        c.tab_skips(&b, "a disabled well");
    } else {
        c.tab_reaches(&b, "the well");
        b.popup();
        c.m.settle();
        c.expect(b.popover().is_some_and(|p| p.is_visible()), "Space opens the swatches");
        b.popdown();
        c.m.settle();
    }
    c.m.unfocus();
}

#[test]
fn every_input_component_case_meets_its_spec() {
    let nw = w::APP_NARROW.0;
    let all = [
        Component {
            id: "text-field",
            build: build_text_field,
            act: act_text_field,
            size: regular,
            named: Named::Label,
            width: nw,
            picture: at_rest,
            row: Some("list-row"),
            control: true,
            check_role: true,
        },
        Component {
            id: "password-field",
            build: build_password_field,
            act: act_password_field,
            size: regular,
            named: Named::Label,
            width: nw,
            picture: at_rest,
            row: None,
            control: true,
            check_role: true,
        },
        Component {
            id: "search-field",
            build: build_search_field,
            act: act_search_field,
            size: regular,
            named: Named::Label,
            width: nw,
            picture: at_rest,
            row: None,
            control: true,
            check_role: true,
        },
        Component {
            id: "shortcut-recorder",
            build: build_shortcut_recorder,
            act: act_shortcut_recorder,
            size: regular,
            named: Named::Words,
            width: nw,
            picture: at_rest,
            row: Some("list-row"),
            control: true,
            check_role: true,
        },
        Component {
            id: "colour-well",
            build: build_colour_well,
            act: act_colour_well,
            size: regular,
            named: Named::Label,
            width: nw,
            picture: at_rest,
            row: Some("list-row"),
            control: false,
            check_role: true,
        },
    ];
    kit_case::run(&all, KNOWN);
}
