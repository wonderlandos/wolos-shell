// SPDX-License-Identifier: GPL-2.0-or-later
//! Containers and collections tested alone, from their specs (WolOS Design System, ADR 0058 draft,
//! DS-COMPONENTS): card, list item, divider, section header, carousel, skeleton, table, tree,
//! split view and the floating action button. Generic checks: `support/kit_case.rs`.
//! `WOL_KIT_ONLY`, `WOL_GOLDEN_UPDATE` and `WOL_KIT_MEASURE` as in `kit_choice.rs`.
#[path = "support/broadway.rs"]
mod broadway;
#[path = "support/kit_case.rs"]
mod kit_case;
#[path = "support/kit_test.rs"]
mod kit_test;

use gtk::prelude::*;
use kit_case::{Built, Component, Ctx, Known, Named, at_rest};
use kit_test::Density;
use shell::kit::{collection as col, widgets as w};
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

// ---- card ----

fn build_card(state: &str) -> Built {
    let style = match state {
        "filled" => col::CardStyle::Filled,
        "outlined" => col::CardStyle::Outlined,
        _ => col::CardStyle::Elevated,
    };
    let body = w::vbox(w::S1);
    body.append(&w::label("Atlas", "title"));
    body.append(&w::label(
        "A private browser that remembers less.",
        "secondary",
    ));
    let c = col::card(style, &body);
    c.set_halign(gtk::Align::Start);
    with_log(Built::new(&c, &c), &Rc::default())
}

fn act_card(c: &mut Ctx) {
    let dn = c.frame.density.name();
    let want = c.spec["density"][dn]["inset"].as_i64().unwrap_or(12) as i32;
    let (cx, cy, _, _) = c.m.rect(c.probe());
    if let Some(first) = c.probe().first_child() {
        let (bx, by, _, _) = c.m.rect(&first);
        let border = i32::from(c.state == "outlined");
        c.hold(&format!("{dn}.inset.left"), bx - cx - border, want);
        c.hold(&format!("{dn}.inset.top"), by - cy - border, want);
    }
}

// ---- list item ----

fn build_item(state: &str) -> Built {
    let sw = w::toggle(true, |_| {});
    let row = match state {
        "two-line" => col::list_item(
            col::Lead::Icon("network-wireless-symbolic"),
            "Home network",
            "Connected, secured",
            2,
            None,
        ),
        "three-line" => col::list_item(
            col::Lead::Image("folder-pictures-symbolic"),
            "Holiday in Porto",
            "48 photos and 3 videos from the trip in June, shared with the family album and backed up",
            3,
            None,
        ),
        "avatar" => col::list_item(
            col::Lead::Avatar("Ana Lima"),
            "Ana Lima",
            "ana@example.org",
            2,
            None,
        ),
        "trailing" => col::list_item(
            col::Lead::Icon("bluetooth-symbolic"),
            "Bluetooth",
            "Visible as Ana's laptop",
            2,
            Some(&sw.clone().upcast()),
        ),
        _ => col::list_item(col::Lead::Icon("folder-symbolic"), "Documents", "", 1, None),
    };
    let s = w::section("", &[row.clone().upcast()]);
    let probe: gtk::Widget = if state == "trailing" {
        sw.upcast()
    } else {
        row.upcast()
    };
    with_log(Built::new(&s, &probe), &Rc::default())
}

fn act_item(c: &mut Ctx) {
    if c.state == "trailing" {
        c.expect(
            gtk::test_accessible_has_relation(c.probe(), gtk::AccessibleRelation::LabelledBy),
            "the trailing switch is named by the title",
        );
    }
    if c.state == "three-line"
        && let Some(sub) =
            c.m.find::<gtk::Label>()
                .into_iter()
                .find(|l| l.has_css_class("secondary"))
    {
        c.same(sub.lines(), 2, "the supporting text stops after two lines");
    }
}

// ---- divider ----

fn build_divider(state: &str) -> Built {
    let d = col::divider(state == "inset");
    let card = w::vbox(0);
    card.add_css_class("card-list");
    card.append(&w::list_row("", "Above", "", None));
    card.append(&d);
    card.append(&w::list_row("", "Below", "", None));
    with_log(Built::new(&card, &d), &Rc::default())
}

fn act_divider(c: &mut Ctx) {
    let (dx, _, dw, dh) = c.m.rect(c.probe());
    c.hold("standard.sizes.line.height", dh, 1);
    let card = c.m.by_class("card-list").into_iter().next();
    if let Some(card) = card {
        let (cx, _, cw, _) = c.m.rect(&card);
        let inset = if c.state == "inset" { 12 } else { 0 };
        c.hold(&format!("{}.start", c.state), dx - cx - 1, inset);
        c.hold(&format!("{}.end", c.state), cx + cw - (dx + dw) - 1, inset);
    }
}

// ---- section header ----

fn build_header(state: &str) -> Built {
    let a = (state == "action").then(|| w::button("See All", w::Kind::Plain, || {}));
    let h = col::section_header("Recently Added", a.as_ref());
    let title = h.first_child().unwrap_or_else(|| h.clone().upcast());
    with_log(Built::new(&h, &title), &Rc::default())
}

fn act_header(c: &mut Ctx) {
    c.same(
        c.m.find::<gtk::Button>().len(),
        usize::from(c.state == "action"),
        "at most one action",
    );
}

// ---- carousel ----

fn build_carousel(_: &str) -> Built {
    let items: Vec<gtk::Widget> = [
        ("wolos-files", "Files"),
        ("wolos-mail", "Mail"),
        ("wolos-calendar", "Calendar"),
        ("wolos-clock", "Clock"),
        ("wolos-browser", "Atlas"),
        ("wolos-calculator", "Calculator"),
    ]
    .iter()
    .map(|(i, t)| {
        let b = w::vbox(w::S1);
        b.append(&w::app_icon(i, w::ICON_L));
        b.append(&w::centered(t, ""));
        col::card(col::CardStyle::Outlined, &b).upcast()
    })
    .collect();
    let c = col::carousel(&items, ("Previous", "Next"));
    with_log(Built::new(&c, &c), &Rc::default())
}

fn act_carousel(c: &mut Ctx) {
    let steps = c.m.by_class("carousel-step");
    c.same(steps.len(), 2, "previous and next buttons");
    let named = steps
        .iter()
        .all(|s| gtk::test_accessible_has_property(s, gtk::AccessibleProperty::Label));
    c.expect(named, "the step buttons are named");
    let sc = c.m.find::<gtk::ScrolledWindow>().into_iter().next();
    if let (Some(sc), Some(next)) = (
        sc,
        steps
            .get(1)
            .and_then(|s| s.clone().downcast::<gtk::Button>().ok()),
    ) {
        let before = sc.hadjustment().value();
        c.m.activate(&next);
        c.m.settle();
        c.expect(sc.hadjustment().value() > before, "Next moves the strip");
        sc.hadjustment().set_value(before);
        c.m.settle();
    }
    c.m.unfocus();
}

// ---- skeleton ----

fn build_skeleton(state: &str) -> Built {
    let s = col::skeleton(
        if state == "avatar" { 2 } else { 3 },
        state == "avatar",
        "Loading apps",
    );
    let card = w::vbox(0);
    card.add_css_class("card-list");
    let row = w::hbox(0);
    row.add_css_class("list-row");
    s.set_hexpand(true);
    row.append(&s);
    card.append(&row);
    with_log(Built::new(&card, &s), &Rc::default())
}

fn act_skeleton(c: &mut Ctx) {
    c.expect(
        gtk::test_accessible_has_state(c.probe(), gtk::AccessibleState::Busy),
        "a skeleton is busy for assistive technology",
    );
    if let Some(l) = c.m.by_class("skeleton-line").first() {
        let (_, _, _, h) = c.m.rect(l);
        c.hold("standard.sizes.line.height", h, 12);
    }
}

// ---- table ----

fn build_table(_: &str) -> Built {
    let rows = vec![
        vec!["wol-shell".to_string(), "412".into(), "3.1".into()],
        vec!["atlas".into(), "1 204".into(), "12.4".into()],
        vec!["wold".into(), "38".into(), "0.2".into()],
        vec!["pipewire".into(), "21".into(), "0.8".into()],
    ];
    let t = col::table(&["Process", "Memory, MB", "CPU, %"], &[1, 2], rows);
    with_log(Built::new(&t, &t), &Rc::default())
}

fn act_table(c: &mut Ctx) {
    let dn = c.frame.density.name();
    let rows =
        c.m.all()
            .into_iter()
            .filter(|x| x.css_name() == "row")
            .collect::<Vec<_>>();
    c.same(rows.len(), 4, "four rows");
    if let (Some(r), Some(want)) = (rows.first(), c.spec["density"][dn]["item"].as_i64()) {
        let (_, _, _, h) = c.m.rect(r);
        c.hold(&format!("{dn}.row"), h, want as i32);
    }
    let right =
        c.m.by_class("tabular")
            .into_iter()
            .filter_map(|l| l.downcast::<gtk::Label>().ok())
            .all(|l| l.xalign() == 1.0);
    c.expect(right, "number columns are right-aligned");
}

// ---- tree ----

fn build_tree(state: &str) -> Built {
    let n = |l: &str, c: Vec<col::TreeNode>| col::TreeNode {
        label: l.into(),
        children: c,
    };
    let t = col::tree(
        vec![
            n(
                "Documents",
                vec![
                    n("Taxes", vec![n("2026.pdf", vec![])]),
                    n("Notes.txt", vec![]),
                ],
            ),
            n("Pictures", vec![n("Porto", vec![])]),
        ],
        state == "open",
    );
    with_log(Built::new(&t, &t), &Rc::default())
}

fn act_tree(c: &mut Ctx) {
    let dn = c.frame.density.name();
    let rows =
        c.m.all()
            .into_iter()
            .filter(|x| x.css_name() == "row")
            .collect::<Vec<_>>();
    c.same(
        rows.len(),
        if c.state == "open" { 6 } else { 2 },
        "the open tree shows every level",
    );
    if let (Some(r), Some(want)) = (rows.first(), c.spec["density"][dn]["item"].as_i64()) {
        let (_, _, _, h) = c.m.rect(r);
        c.hold(&format!("{dn}.row"), h, want as i32);
    }
}

// ---- split view ----

fn build_split(_: &str) -> Built {
    let list = w::section(
        "",
        &[
            w::list_row("", "Inbox", "", None).upcast(),
            w::list_row("", "Sent", "", None).upcast(),
        ],
    );
    let detail = w::label("Pick a mailbox", "secondary");
    detail.set_hexpand(true);
    detail.set_halign(gtk::Align::Center);
    detail.set_valign(gtk::Align::Center);
    let p = col::split_view(&list, &detail, w::SIDEBAR_WIDTH);
    with_log(Built::new(&p, &p), &Rc::default())
}

fn act_split(c: &mut Ctx) {
    let Ok(p) = c.probe().clone().downcast::<gtk::Paned>() else {
        return;
    };
    c.same(
        p.position(),
        w::SIDEBAR_WIDTH,
        "the start pane keeps its width",
    );
}

// ---- FAB ----

fn build_fab(state: &str) -> Built {
    let log: Log = Rc::default();
    let l = log.clone();
    let f = col::fab(
        "list-add-symbolic",
        if state == "extended" { "New Note" } else { "" },
        "New Note",
        move || l.borrow_mut().push("new".into()),
    );
    with_log(Built::alone(&f), &log)
}

fn fab_size(state: &str, _: Density) -> Option<&'static str> {
    Some(if state == "extended" {
        "extended"
    } else {
        "regular"
    })
}

fn act_fab(c: &mut Ctx) {
    let Ok(b) = c.probe().clone().downcast::<gtk::Button>() else {
        return;
    };
    if c.state == "regular" {
        c.expect(
            b.tooltip_text().as_deref() == Some("New Note"),
            "a glyph-only button has its tooltip",
        );
    }
    c.tab_reaches(&b, "the floating action button");
    c.m.activate(&b);
    c.same(c.log(), vec!["new".to_string()], "the action runs once");
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
fn collections_meet_their_specs() {
    let v = vec![
        comp("card", build_card, act_card),
        Component {
            check_role: false,
            ..comp("list-item", build_item, act_item)
        },
        comp("divider", build_divider, act_divider),
        Component {
            named: Named::Words,
            ..comp("section-header", build_header, act_header)
        },
        Component {
            check_role: false,
            ..comp("carousel", build_carousel, act_carousel)
        },
        Component {
            named: Named::Label,
            ..comp("skeleton", build_skeleton, act_skeleton)
        },
        Component {
            width: 640,
            ..comp("table", build_table, act_table)
        },
        comp("tree", build_tree, act_tree),
        Component {
            width: w::APP_SIZE.0,
            check_role: false,
            ..comp("split-view", build_split, act_split)
        },
        Component {
            size: fab_size,
            named: Named::Label,
            ..comp("fab", build_fab, act_fab)
        },
    ];
    kit_case::run(&v, KNOWN);
}
