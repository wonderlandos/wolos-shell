// SPDX-License-Identifier: GPL-2.0-or-later
//! Kit components of the "communication and data" batch tested alone, from their specs (WolOS
//! Design System, ADR 0058 draft, DS-SPEC-4): toast, notice, badge, spinner, progress bar,
//! progress ring, empty state, error line, tooltip, avatar, tile, meter and gauge. The generic part
//! (sizes, role and name, layout rules, goldens, the KNOWN ratchet) is `support/kit_case.rs`; each
//! component adds its `build_*` (through the Kit function, never raw GTK) and its `act_*`.
//!
//! `WOL_KIT_ONLY=toast` runs the cases whose name contains it; `WOL_GOLDEN_UPDATE=1` rewrites the
//! goldens and trees; `WOL_KIT_MEASURE=1` prints what the Kit draws. Needs `gtk4-broadwayd` (the
//! golden image has it); skips without one.
#[path = "support/broadway.rs"]
mod broadway;
#[path = "support/kit_case.rs"]
mod kit_case;
#[path = "support/kit_test.rs"]
mod kit_test;

use gtk::prelude::*;
use kit_case::{Built, Component, Ctx, Known, Named, at_rest, regular};
use kit_test::Density;
use shell::kit::{assistant, data, form, toast::Toast, widgets as w};
use shell::theme::Variant;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Where the Kit does not meet its spec yet: a ratchet like `rules_ui.rs` BASELINE. The measured
/// value is frozen; when a fix changes it the test fails until the row goes (or is lowered).
/// (component, check, measured)
const KNOWN: Known = &[
    // Drawn sizes: `label.badge` takes the 20 px line height of the window font (16 + padding 0:
    // DS-GTK-1 sets line-height 16); `.notice` stacks its 8 px padding on `.list-row`'s 40 content
    // box (56); the progress bar trough draws 6 for `min-height: 4px` plus its border.
    ("toast", "standard.sizes.regular.height", "38"),
    ("toast", "standard.sizes.text.height", "30"),
    ("badge", "standard.sizes.regular.height", "20"),
    ("notice", "standard.sizes.regular.height", "56"),
    ("progress-bar", "standard.sizes.regular.height", "6"),
    // Accessibility the Kit does not give yet (DS-A11Y-1, DS-A11Y-3): custom drawn and plain box
    // widgets keep the generic role, a meter is a progress bar, an avatar a label.
    ("notice", "a11y.role", "generic"),
    ("progress-ring", "a11y.role", "generic"),
    ("empty-state", "a11y.role", "generic"),
    ("error-line", "a11y.role", "label"),
    ("avatar", "a11y.role", "label"),
    ("toast", "a11y.role", "generic"),
    ("meter", "a11y.role", "progressbar"),
    ("gauge", "a11y.role", "generic"),
    ("progress-bar", "a11y.name", "nameless"),
    ("meter", "a11y.name", "nameless"),
    ("gauge", "a11y.name", "nameless"),
];

fn card(row: gtk::Widget) -> gtk::Widget {
    w::section("", &[row]).upcast()
}

fn find_class(root: &gtk::Widget, class: &str) -> Option<gtk::Widget> {
    let mut stack = vec![root.clone()];
    while let Some(x) = stack.pop() {
        if x.has_css_class(class) {
            return Some(x);
        }
        let mut ch = x.first_child();
        while let Some(y) = ch {
            ch = y.next_sibling();
            stack.push(y);
        }
    }
    None
}

fn find_type<T: IsA<gtk::Widget>>(root: &gtk::Widget) -> Option<T> {
    let mut stack = vec![root.clone()];
    while let Some(x) = stack.pop() {
        if x.is::<T>() {
            return x.downcast::<T>().ok();
        }
        let mut ch = x.first_child();
        while let Some(y) = ch {
            ch = y.next_sibling();
            stack.push(y);
        }
    }
    None
}

/// The Kit's colour token of the variant, as the toolkit reads it.
fn token(variant: Variant, name: &str) -> gtk::gdk::RGBA {
    let id = if variant == Variant::Light {
        "wol-light"
    } else {
        "wol-dark"
    };
    let t = shell::theme::resolve(Some(id), false);
    let hex = t.tokens.get(name).cloned().unwrap_or_default();
    gtk::gdk::RGBA::parse(hex.as_str()).unwrap_or_else(|_| panic!("token {name}: {hex}"))
}

fn same_colour(a: &gtk::gdk::RGBA, b: &gtk::gdk::RGBA) -> bool {
    (a.red() - b.red()).abs() < 0.02
        && (a.green() - b.green()).abs() < 0.02
        && (a.blue() - b.blue()).abs() < 0.02
}

// ---- toast ----

fn build_toast(state: &str) -> Built {
    // A toast floats over the content of an AppWindow.
    let aw = w::AppWindow::new(false, false);
    aw.content.add_named(&w::page("Appearance"), Some("page"));
    aw.root.set_size_request(-1, 200);
    let toast = Toast::new(&aw);
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let l2 = log.clone();
    match state {
        "text" => toast.show("Theme changed", "", || {}),
        "action" => toast.show("Theme changed", "Undo", move || {
            l2.borrow_mut().push("undo".into())
        }),
        _ => {}
    }
    let pill = find_class(aw.root.upcast_ref(), "toast").expect("the toast pill");
    let mut built = Built::new(&aw.root, &pill);
    built.log = log;
    built.keeping(toast)
}

fn act_toast(c: &mut Ctx) {
    let Some(pill) = Some(c.probe().clone()) else {
        return;
    };
    let shown = c.state != "hidden";
    let revealer = pill.parent().and_downcast::<gtk::Revealer>();
    c.same(
        revealer.as_ref().map(|r| r.reveals_child()),
        Some(shown),
        "the toast is shown only after show()",
    );
    if !shown {
        return;
    }
    c.same(
        c.m.by_text("Theme changed").is_some(),
        true,
        "the toast says its line",
    );
    let button =
        c.m.by_class("btn")
            .into_iter()
            .next()
            .and_then(|b| b.downcast::<gtk::Button>().ok());
    match (c.state, button) {
        ("text", Some(b)) => c.expect(!b.is_visible(), "a toast with no action has no button"),
        ("action", Some(b)) => {
            c.same(
                b.label().map(|l| l.to_string()),
                Some("Undo".to_string()),
                "the one action",
            );
            c.m.activate(&b);
            c.same(c.log(), vec!["undo".to_string()], "the action runs once");
            c.same(
                revealer.as_ref().map(|r| r.reveals_child()),
                Some(false),
                "the toast leaves when its action is used",
            );
        }
        _ => c.expect(false, "the toast has its action button"),
    }
    c.m.unfocus();
}

// ---- notice ----

fn build_notice(state: &str) -> Built {
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let actions: Vec<gtk::Button> = if state == "actions" {
        ["Swap", "Cancel"]
            .iter()
            .map(|t| {
                let l = log.clone();
                let name = t.to_string();
                w::button(t, w::Kind::Default, move || {
                    l.borrow_mut().push(name.clone())
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    let n = form::notice("Super+Space is used by Show Search.", &actions);
    let mut built = Built::new(&card(n.clone().upcast()), &n);
    built.log = log;
    built
}

fn act_notice(c: &mut Ctx) {
    c.expect(c.probe().has_css_class("notice"), "the notice wears notice");
    c.expect(
        c.m.by_text("Super+Space is used by Show Search.").is_some(),
        "the notice says what is wrong",
    );
    let buttons = c.m.find::<gtk::Button>();
    if c.state == "actions" {
        c.same(buttons.len(), 2, "two ways out");
        if let Some(b) = buttons.first() {
            c.tab_reaches(b, "the first way out");
            c.m.activate(b);
            c.same(c.log(), vec!["Swap".to_string()], "the way out runs once");
        }
    } else {
        c.same(buttons.len(), 0, "no actions, no buttons");
    }
    c.m.unfocus();
}

// ---- badge ----

fn tone_of(state: &str) -> data::Tone {
    match state {
        "accent" => data::Tone::Accent,
        "success" => data::Tone::Success,
        "warning" => data::Tone::Warning,
        "danger" => data::Tone::Danger,
        _ => data::Tone::Text,
    }
}

fn build_badge(state: &str) -> Built {
    let b = data::badge(
        if state == "neutral" { "3" } else { "Online" },
        tone_of(state),
    );
    Built::alone(&b)
}

fn act_badge(c: &mut Ctx) {
    let class = data::tone_class(tone_of(c.state));
    c.expect(
        c.probe().has_css_class("badge") && c.probe().has_css_class(class),
        format_args!("the badge wears badge and {class}"),
    );
    let text = c
        .probe()
        .clone()
        .downcast::<gtk::Label>()
        .map(|l| l.text().to_string())
        .unwrap_or_default();
    c.expect(!text.is_empty(), "a badge always has words or a count");
    c.expect(
        text.chars().count() <= 12,
        "a badge is a word or a count, not a sentence",
    );
}

// ---- spinner ----

fn build_spinner(state: &str) -> Built {
    let s = if state == "large" {
        let s = w::spinner();
        s.set_halign(gtk::Align::Start);
        s.set_vexpand(false);
        s
    } else {
        let s = w::busy_spinner();
        s.set_visible(true);
        s.set_spinning(true);
        s.set_halign(gtk::Align::Start);
        s
    };
    Built::alone(&s)
}

fn act_spinner(c: &mut Ctx) {
    let Some(s) = c.find::<gtk::Spinner>() else {
        return;
    };
    c.expect(s.is_spinning(), "a spinner shown turns");
    // A hidden busy spinner does not turn: the Kit stops it with spin_while.
    let hidden = w::busy_spinner();
    c.expect(
        !hidden.is_visible() && !hidden.is_spinning(),
        "the busy spinner starts hidden and still",
    );
}

// ---- progress bar ----

fn build_progress_bar(state: &str) -> Built {
    let p = w::progress();
    match state {
        "half" => p.set_fraction(0.5),
        "full" => p.set_fraction(1.0),
        "indeterminate" => p.pulse(),
        _ => p.set_fraction(0.0),
    }
    Built::alone(&p)
}

fn act_progress_bar(c: &mut Ctx) {
    let Some(p) = c.find::<gtk::ProgressBar>() else {
        return;
    };
    let want = match c.state {
        "half" => 0.5,
        "full" => 1.0,
        _ => 0.0,
    };
    c.same(p.fraction(), want, "the filled share");
    let (_, _, pw, _) = c.m.rect(&p);
    c.expect(pw > 100, "a bar takes the width it is given");
}

// ---- progress ring ----

fn build_progress_ring(state: &str) -> Built {
    let value: Rc<Cell<Option<u8>>> = Rc::new(Cell::new(match state {
        "empty" => Some(0),
        "half" => Some(50),
        "full" => Some(100),
        _ => None,
    }));
    let ring = w::ring_with(value.clone());
    ring.set_halign(gtk::Align::Start);
    Built::alone(&ring).keeping(value)
}

fn act_progress_ring(c: &mut Ctx) {
    c.expect(
        c.probe().has_css_class("progress-ring"),
        "the ring wears progress-ring",
    );
    c.same(c.probe().halign(), gtk::Align::Start, "built to start");
    c.expect(
        w::progress_ring().has_css_class("progress-ring"),
        "the unmeasured ring is the same widget",
    );
}

// ---- empty state ----

fn build_empty_state(state: &str) -> Built {
    let action = (state == "action").then(|| w::button("Add Folder", w::Kind::Primary, || {}));
    let e = w::empty_state(
        "folder-open-symbolic",
        "Nothing here yet",
        "Add a folder and its files show up here.",
        action,
    );
    Built::alone(&e)
}

fn act_empty_state(c: &mut Ctx) {
    c.expect(
        c.probe().has_css_class("empty-state"),
        "the empty state wears empty-state",
    );
    let title = c.m.by_text("Nothing here yet");
    c.expect(
        title
            .as_ref()
            .is_some_and(|t| t.has_css_class("large-title")),
        "the title is the large title",
    );
    c.expect(
        c.m.by_text("Add a folder and its files show up here.")
            .is_some(),
        "one line says what to do",
    );
    let icon = c.m.find::<gtk::Image>().into_iter().next();
    c.same(
        icon.map(|i| i.pixel_size()),
        Some(w::ICON_XL),
        "a 64 px icon on top",
    );
    let buttons = c.m.find::<gtk::Button>();
    c.same(
        buttons.len(),
        usize::from(c.state == "action"),
        "at most one action",
    );
    if let Some(b) = buttons.first() {
        c.tab_reaches(b, "the action");
    }
    c.m.unfocus();
}

// ---- error line ----

fn build_error_line(state: &str) -> Built {
    let p = assistant::problem_line();
    if state == "shown" {
        p.set_text("That name is already taken. Try another.");
        p.set_visible(true);
    }
    p.set_halign(gtk::Align::Start);
    Built::alone(&p)
}

fn act_error_line(c: &mut Ctx) {
    let Some(p) = c.find::<gtk::Label>() else {
        return;
    };
    c.same(
        p.is_visible(),
        c.state == "shown",
        "the line shows only with a problem",
    );
    if c.state != "shown" {
        return;
    }
    c.expect(
        p.wraps(),
        "a long sentence wraps instead of widening the form",
    );
    let danger = token(c.frame.variant, "danger");
    let got = p.color();
    let tone = if same_colour(&got, &danger) {
        "danger"
    } else {
        "text"
    };
    c.run.judge(
        c.id,
        "colour",
        tone,
        "the error line is drawn in the text colour, not the danger colour",
        tone == "danger",
    );
}

// ---- tooltip ----

fn build_tooltip(_state: &str) -> Built {
    // A tooltip is a toolkit window that cannot be mounted headless: the case holds the contract on
    // its owner, an icon button.
    let b = w::icon_button("list-add-symbolic", "Add", || {});
    b.set_halign(gtk::Align::Start);
    Built::alone(&b)
}

fn act_tooltip(c: &mut Ctx) {
    let text = c
        .probe()
        .tooltip_text()
        .map(|t| t.to_string())
        .unwrap_or_default();
    c.expect(!text.is_empty(), "the owner has a tooltip");
    c.expect(
        text.split_whitespace().count() <= 3,
        "a tooltip is a name, three words at most",
    );
    c.expect(!text.ends_with('.'), "a tooltip has no full stop");
    c.expect(
        text.split_whitespace()
            .all(|w| w.chars().next().is_some_and(char::is_uppercase)),
        "a tooltip is in Title Case",
    );
}

// ---- avatar ----

fn avatar_px(state: &str) -> i32 {
    match state {
        "large" => 64,
        "xl" => w::AVATAR_XL,
        _ => 32,
    }
}

fn avatar_size(state: &str, _: Density) -> Option<&'static str> {
    Some(match state {
        "large" => "large",
        "xl" => "xl",
        _ => "small",
    })
}

fn build_avatar(state: &str) -> Built {
    let a = w::avatar("Ada Lovelace", avatar_px(state));
    Built::alone(&a)
}

fn act_avatar(c: &mut Ctx) {
    let Some(a) = c.find::<gtk::Label>() else {
        return;
    };
    c.same(
        a.text().to_string(),
        "AL".to_string(),
        "the initials of the name",
    );
    c.expect(a.has_css_class("avatar"), "the avatar wears avatar");
    c.expect(
        a.css_classes()
            .iter()
            .any(|k| k.starts_with("avatar-tone-")),
        "a name nobody coloured gets a tone",
    );
    let (_, _, aw, ah) = c.m.rect(&a);
    c.same(aw, ah, "an avatar is square, so it draws a circle");
}

// ---- tile ----

fn build_tile(state: &str) -> Built {
    let icon = w::icon("folder-open-symbolic", w::ICON_M);
    let t = w::AppTile::new(icon.upcast_ref(), "Text Editor", "Write notes and code.");
    t.root.set_halign(gtk::Align::Start);
    t.set_state(match state {
        "open" => w::TileState::Open,
        "progress" => w::TileState::Progress(50),
        _ => w::TileState::Get,
    });
    t.set_source("Flathub");
    let probe = t.root.clone();
    Built::new(&probe, &probe).keeping(t)
}

fn act_tile(c: &mut Ctx) {
    c.expect(
        c.probe().has_css_class("app-tile"),
        "the tile wears app-tile",
    );
    c.expect(
        c.m.by_text("Text Editor").is_some(),
        "the tile names the app",
    );
    c.expect(
        c.m.by_text("Flathub").is_some(),
        "the tile says where it comes from",
    );
    let get = c.m.by_class("get-button").into_iter().next();
    match c.state {
        "progress" => c.expect(
            get.as_ref().is_some_and(|g| !g.is_visible()),
            "while it installs the Get button gives way to the ring",
        ),
        s => {
            let want = if s == "open" { "Open" } else { "Get" };
            let label = get
                .and_then(|g| g.downcast::<gtk::Button>().ok())
                .and_then(|b| b.label());
            c.same(
                label.map(|l| l.to_string()),
                Some(want.to_string()),
                "the one action",
            );
        }
    }
}

// ---- meter ----

fn build_meter(state: &str) -> Built {
    let (f, tone) = match state {
        "success" => (0.5, data::Tone::Success),
        "warning" => (0.5, data::Tone::Warning),
        "danger" => (0.9, data::Tone::Danger),
        "empty" => (0.0, data::Tone::Accent),
        "full" => (1.0, data::Tone::Accent),
        _ => (0.5, data::Tone::Accent),
    };
    let m = data::meter(f, tone);
    Built::alone(&m)
}

fn act_meter(c: &mut Ctx) {
    let Some(p) = c.find::<gtk::ProgressBar>() else {
        return;
    };
    let want = match c.state {
        "danger" => 0.9,
        "empty" => 0.0,
        "full" => 1.0,
        _ => 0.5,
    };
    c.same(p.fraction(), want, "the share drawn");
    c.expect(p.has_css_class("meter"), "the bar wears meter");
    c.same(
        data::meter(2.0, data::Tone::Accent).fraction(),
        1.0,
        "a share above the whole is clamped",
    );
}

// ---- gauge ----

fn gauge_of(state: &str) -> (data::Gauge, data::IconSize) {
    let (size, f, tone, value) = match state {
        "small" => (data::IconSize::Small, 0.6, data::Tone::Accent, "60%"),
        "large" => (data::IconSize::Large, 0.6, data::Tone::Accent, "60%"),
        "danger" => (data::IconSize::Medium, 0.9, data::Tone::Danger, "90%"),
        _ => (data::IconSize::Medium, 0.6, data::Tone::Accent, "60%"),
    };
    (
        data::Gauge::new(f, value, None, Some("Memory"), tone, size),
        size,
    )
}

fn gauge_size(state: &str, _: Density) -> Option<&'static str> {
    Some(match state {
        "small" => "small",
        "large" => "large",
        _ => "medium",
    })
}

fn build_gauge(state: &str) -> Built {
    let (g, _) = gauge_of(state);
    let ring = find_class(g.root.upcast_ref(), "gauge-ring").expect("the ring");
    ring.set_halign(gtk::Align::Center);
    g.root.set_halign(gtk::Align::Start);
    let root = g.root.clone();
    Built::new(&root, &ring).keeping(g)
}

fn act_gauge(c: &mut Ctx) {
    c.expect(
        c.probe().has_css_class("gauge-ring"),
        "the ring wears gauge-ring",
    );
    c.expect(
        c.m.by_text("Memory").is_some(),
        "the caption names what is measured",
    );
    let value = if c.state == "danger" { "90%" } else { "60%" };
    c.expect(
        c.m.by_text(value).is_some(),
        "the value is in words as well as in the ring",
    );
    let _ = find_type::<gtk::Label>;
}

#[test]
fn every_comms_component_case_meets_its_spec() {
    let nw = w::APP_NARROW.0;
    let mk = |id, build, act, size, named, picture| Component {
        id,
        build,
        act,
        size,
        named,
        width: nw,
        picture,
        row: None,
        control: false,
        check_role: true,
    };
    fn busy_only(state: &str) -> bool {
        state != "indeterminate" && state != "busy"
    }
    fn none(_: &str, _: Density) -> Option<&'static str> {
        None
    }
    let mut tooltip = mk(
        "tooltip",
        build_tooltip,
        act_tooltip,
        none,
        Named::Nothing,
        at_rest,
    );
    tooltip.check_role = false;
    let all = [
        mk(
            "toast",
            build_toast,
            act_toast,
            |s, _| match s {
                "text" => Some("text"),
                "action" => Some("regular"),
                _ => None,
            },
            Named::Nothing,
            at_rest,
        ),
        mk(
            "notice",
            build_notice,
            act_notice,
            regular,
            Named::Nothing,
            at_rest,
        ),
        mk(
            "badge",
            build_badge,
            act_badge,
            regular,
            Named::Words,
            at_rest,
        ),
        mk(
            "spinner",
            build_spinner,
            act_spinner,
            |s, _| Some(if s == "large" { "large" } else { "regular" }),
            Named::Nothing,
            at_rest,
        ),
        mk(
            "progress-bar",
            build_progress_bar,
            act_progress_bar,
            regular,
            Named::Label,
            busy_only,
        ),
        mk(
            "progress-ring",
            build_progress_ring,
            act_progress_ring,
            regular,
            Named::Nothing,
            busy_only,
        ),
        mk(
            "empty-state",
            build_empty_state,
            act_empty_state,
            none,
            Named::Nothing,
            at_rest,
        ),
        mk(
            "error-line",
            build_error_line,
            act_error_line,
            none,
            Named::Nothing,
            at_rest,
        ),
        tooltip,
        mk(
            "avatar",
            build_avatar,
            act_avatar,
            avatar_size,
            Named::Label,
            at_rest,
        ),
        mk(
            "tile",
            build_tile,
            act_tile,
            regular,
            Named::Nothing,
            busy_only,
        ),
        mk(
            "meter",
            build_meter,
            act_meter,
            regular,
            Named::Label,
            at_rest,
        ),
        mk(
            "gauge",
            build_gauge,
            act_gauge,
            gauge_size,
            Named::Label,
            at_rest,
        ),
    ];
    kit_case::run(&all, KNOWN);
}
