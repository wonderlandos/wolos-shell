// SPDX-License-Identifier: GPL-2.0-or-later
//! Sheets and overlays tested alone, from their specs (WolOS Design System, ADR 0058 draft,
//! DS-COMPONENTS): the question sheet, bottom sheet, side sheet, search view, rich tooltip and
//! on-screen display. Generic checks: `support/kit_case.rs`. `WOL_KIT_ONLY`, `WOL_GOLDEN_UPDATE`
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
use shell::kit::{sheets, widgets as w};
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

// ---- sheet ----

fn build_sheet(state: &str) -> Built {
    let log: Log = Rc::default();
    let (l1, l2) = (log.clone(), log.clone());
    let cancel = w::button("Cancel", w::Kind::Default, move || {
        l1.borrow_mut().push("cancel".into())
    });
    let (title, text, go) = if state == "destructive" {
        (
            "Delete 3 photos for good?",
            "They are not in the Trash; this cannot be undone.",
            w::button("Delete", w::Kind::Destructive, move || {
                l2.borrow_mut().push("go".into())
            }),
        )
    } else {
        (
            "Install Atlas?",
            "It takes about 300 MB and opens when it is ready.",
            w::button("Install", w::Kind::Primary, move || {
                l2.borrow_mut().push("go".into())
            }),
        )
    };
    let r = sheets::question_sheet(title, text, &[cancel, go]);
    r.set_visible(true);
    r.set_reveal_child(true);
    let probe = r
        .child()
        .and_then(|card| card.first_child())
        .unwrap_or_else(|| r.clone().upcast());
    with_log(Built::new(&r, &probe), &log)
}

fn act_sheet(c: &mut Ctx) {
    let p = c.probe().clone();
    c.expect(
        gtk::test_accessible_has_relation(&p, gtk::AccessibleRelation::LabelledBy)
            && gtk::test_accessible_has_relation(&p, gtk::AccessibleRelation::DescribedBy),
        "the sheet is labelled by its title and described by its text",
    );
    let buttons = c.m.find::<gtk::Button>();
    let labels: Vec<String> = buttons
        .iter()
        .filter_map(|b| b.label().map(String::from))
        .collect();
    c.expect(
        labels
            .last()
            .is_some_and(|l| l != "Cancel" && l != "OK" && l != "Yes"),
        format_args!("the default action is last and a verb ({labels:?})"),
    );
    if let Some(b) = buttons.last() {
        c.m.activate(b);
        c.same(
            c.log(),
            vec!["go".to_string()],
            "the default action runs once",
        );
    }
    c.m.unfocus();
}

// ---- bottom sheet ----

fn build_bottom(_: &str) -> Built {
    let rows: Vec<gtk::Widget> = [
        ("edit-copy-symbolic", "Copy Link"),
        ("send-to-symbolic", "Share"),
        ("edit-delete-symbolic", "Move to Trash"),
    ]
    .iter()
    .map(|(i, t)| w::list_button(i, t, "").upcast())
    .collect();
    let list = w::vbox(0);
    for r in &rows {
        list.append(r);
    }
    let b = sheets::bottom_sheet(&list);
    let page = w::vbox(0);
    page.append(&w::spacer());
    page.append(&b);
    with_log(Built::new(&page, &b), &Rc::default())
}

fn act_bottom(c: &mut Ctx) {
    let h = c.m.by_class("sheet-handle").into_iter().next();
    c.expect(h.is_some(), "the bottom sheet shows its grab handle");
    if let Some(h) = h {
        let (_, _, hw, hh) = c.m.rect(&h);
        c.hold("standard.sizes.handle.width", hw, 40);
        c.hold("standard.sizes.handle.height", hh, 4);
    }
}

// ---- side sheet ----

fn build_side(_: &str) -> Built {
    let log: Log = Rc::default();
    let l = log.clone();
    let body = w::section(
        "",
        &[
            w::list_row("", "Kind", "PNG image", None).upcast(),
            w::list_row("", "Size", "2.4 MB", None).upcast(),
        ],
    );
    let s = sheets::side_sheet("Details", "Close", &body, move || {
        l.borrow_mut().push("closed".into())
    });
    let page = w::hbox(0);
    let content = w::label("Content", "");
    content.set_hexpand(true);
    page.append(&content);
    page.append(&s);
    with_log(Built::new(&page, &s), &log)
}

fn side_size(_: &str, _: Density) -> Option<&'static str> {
    Some("pane")
}

fn act_side(c: &mut Ctx) {
    c.expect(
        gtk::test_accessible_has_relation(c.probe(), gtk::AccessibleRelation::LabelledBy),
        "the side sheet is labelled by its title",
    );
    let close = c.m.find::<gtk::Button>().into_iter().next();
    c.expect(
        close
            .as_ref()
            .is_some_and(|b| b.tooltip_text().as_deref() == Some("Close")),
        "a close button named Close",
    );
    if let Some(b) = close {
        c.m.activate(&b);
        c.same(c.log(), vec!["closed".to_string()], "close is heard once");
    }
    c.m.unfocus();
}

// ---- search view ----

fn build_search(state: &str) -> Built {
    let f = w::search_field("Search apps");
    let rows: Vec<gtk::Widget> = if state == "none" {
        f.set_text("qwerty");
        vec![]
    } else {
        f.set_text("ph");
        [
            ("Photos", "Your pictures and videos"),
            ("Phone", "Calls from this computer"),
            ("Photo Booth", "Take a picture"),
        ]
        .iter()
        .map(|(t, s)| w::list_button("", t, s).upcast())
        .collect()
    };
    let v = sheets::search_view(&f, &rows, "No apps match");
    with_log(Built::new(&v, &v), &Rc::default())
}

fn act_search(c: &mut Ctx) {
    let none = c.m.by_class("search-none");
    if c.state == "none" {
        c.same(none.len(), 1, "one quiet line when nothing matches");
        c.expect(
            c.m.by_text("No apps match").is_some(),
            "the line names what was searched",
        );
    } else {
        c.same(none.len(), 0, "no empty line while there are results");
        c.same(c.m.by_class("row-button").len(), 3, "three results as rows");
    }
}

// ---- rich tooltip ----

fn build_rich(state: &str) -> Built {
    let a = (state == "action").then(|| w::button("Learn More", w::Kind::Plain, || {}));
    let b = sheets::rich_tooltip_body(
        "Memory Saver",
        "Apps you have not used for a while sleep and wake when you open them.",
        a.as_ref(),
    );
    b.set_halign(gtk::Align::Start);
    with_log(Built::alone(&b), &Rc::default())
}

fn act_rich(c: &mut Ctx) {
    let n = c.m.find::<gtk::Button>().len();
    c.same(n, usize::from(c.state == "action"), "at most one action");
}

// ---- OSD ----

fn build_osd(state: &str) -> Built {
    let (pill, _, bar) = sheets::osd_pill(if state == "full" {
        "audio-volume-high-symbolic"
    } else {
        "audio-volume-low-symbolic"
    });
    bar.set_fraction(if state == "full" { 1.0 } else { 0.2 });
    bar.update_property(&[gtk::accessible::Property::Label("Volume")]);
    pill.set_halign(gtk::Align::Center);
    with_log(Built::new(&pill, &bar), &Rc::default())
}

fn act_osd(c: &mut Ctx) {
    let Ok(bar) = c.probe().clone().downcast::<gtk::ProgressBar>() else {
        return;
    };
    c.same(
        bar.fraction(),
        if c.state == "full" { 1.0 } else { 0.2 },
        "the bar shows the level",
    );
    c.same(
        c.m.find::<gtk::Image>().len(),
        1,
        "a glyph beside the bar, never colour alone",
    );
    let (_, _, pw, _) = c.m.rect(&c.m.by_class("osd-pill")[0]);
    c.expect(
        pw >= 216,
        format_args!("the pill is at least 216 wide ({pw})"),
    );
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
fn sheets_and_overlays_meet_their_specs() {
    let v = vec![
        comp("sheet", build_sheet, act_sheet),
        comp("bottom-sheet", build_bottom, act_bottom),
        Component {
            size: side_size,
            width: w::APP_SIZE.0,
            ..comp("side-sheet", build_side, act_side)
        },
        Component {
            check_role: false,
            ..comp("search-view", build_search, act_search)
        },
        comp("rich-tooltip", build_rich, act_rich),
        Component {
            named: Named::Label,
            ..comp("osd", build_osd, act_osd)
        },
    ];
    kit_case::run(&v, KNOWN);
}
