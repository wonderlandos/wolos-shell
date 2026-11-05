// SPDX-License-Identifier: GPL-2.0-or-later
//! Layout lint (docs/RULES-ui.md "Layout, measured on every golden", RU-700 to RU-706): after a
//! golden case renders, walk its GTK widget tree (allocations, and the rendered pixels for the
//! colour rule) and check the HIG's geometry: controls clear of their container's edges, children
//! inside their parents, control heights from the one list in `shared::theme::scaffold`, toolbar
//! controls on the toolbar's centre line, the toolbar search field narrow and last, control gaps of
//! 8 or 12, and a disabled destructive button drawn dim. Each violation names the case, the widget
//! path, the rule and the numbers. [`BASELINE`] freezes what the tree does not meet yet (growth
//! only, one reason per row); `golden.rs` fails a run that exceeds it.
use gtk::{gdk, prelude::*};
use shared::theme::scaffold;

/// Runs the default main context for `ms`: it blocks between events and wakes for its own timer,
/// so nothing sleeps (RR-049) and every idle and timeout due meanwhile runs.
fn pump(ms: u64) {
    let ctx = gtk::glib::MainContext::default();
    let done = std::rc::Rc::new(std::cell::Cell::new(false));
    let d = done.clone();
    gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(ms), move || d.set(true));
    while !done.get() {
        ctx.iteration(true);
    }
}

/// Rule ids, in the order of the table in docs/RULES-ui.md (rules_ui.rs holds the wording).
pub const EDGE: &str = "RU-700";
pub const INSIDE: &str = "RU-701";
pub const HEIGHT: &str = "RU-702";
pub const CENTRE: &str = "RU-703";
pub const SEARCH: &str = "RU-704";
pub const GAP: &str = "RU-705";
pub const DIM: &str = "RU-706";

/// RU-700: a control keeps at least this far from every edge of its container (the 8 px step).
pub const MIN_EDGE: i32 = shell::kit::widgets::S2;
/// RU-705: the gaps allowed between two controls side by side (8 and 12 px steps).
pub const GAPS: [i32; 2] = [shell::kit::widgets::S2, shell::kit::widgets::S3];
/// RU-704: widest a toolbar search field may be.
pub const SEARCH_MAX: i32 = 320;
/// Rounding of fractional allocations (the same 1 px as `shell::layout::TOLERANCE`).
const TOL: i32 = shell::layout::TOLERANCE;

/// Violations the tree has today, per (rule, surface): the count may only go down. A row says why
/// the surface is not fixed yet.
pub const BASELINE: &[(&str, &str, usize, &str)] = &[
    (
        EDGE,
        "firstlogin",
        8,
        "accounts/keyboard: assistant buttons are 40 px (.assistant-card button.btn) inside a 52 px list row, 6 px from its edges",
    ),
    (
        EDGE,
        "kit",
        4,
        "kit-toast: Undo sits 5 px inside the toast pill (padding 4); kit-search-field: the field is 4 px under its 52 px page top",
    ),
    (
        EDGE,
        "settings",
        16,
        "Bluetooth device rows: Forget is 1 px from the card's right edge (the row has no list-row padding); Sound sliders 6 px in a 52 px row",
    ),
    (
        EDGE,
        "store",
        12,
        "Get buttons in scrolled pages (hero banner, app tiles) end 4-6 px above the window's bottom edge at rest; \
         +2: the install-failed narrow case (INT-2, same hero banner)",
    ),
    (
        HEIGHT,
        "capture",
        8,
        "the screenshot bar's own 32 px controls (button.capture-mode, capture-close, capture-options): not on the scaffold list",
    ),
    (
        HEIGHT,
        "clipboard",
        4,
        "clipboard history icon buttons are 32 px",
    ),
    (
        HEIGHT,
        "controlcenter",
        6,
        "detail-head compact switch is 20 px (kit.css: 16 px header switch on the 4 px content line)",
    ),
    (
        HEIGHT,
        "displays",
        4,
        "Settings sidebar search 34 px (min-height 32 + the 1 px border); 32 is not a scaffold height: 28 or 36 is the lead's pick",
    ),
    (
        HEIGHT,
        "firstlogin",
        10,
        "assistant fields and drop-downs are 34 px (min-height 32 + the 1 px border); 32 is not a scaffold height: 36 or 28 is the lead's pick",
    ),
    (
        HEIGHT,
        "installer",
        56,
        "assistant fields and drop-downs are 34 px (min-height 32 + the 1 px border); same pick as setup",
    ),
    (
        HEIGHT,
        "kit",
        8,
        "detail-head compact switch 20 px; the colour well menu button is 26 px (a 22 px swatch + padding)",
    ),
    (
        HEIGHT,
        "settings",
        118,
        "sidebar search 34 px (min-height 32 + border, 100); GTK's own MenuButton with no kit style is 34 px (18); \
         +2: the Default Apps case and +2: the case opened from a search result (INT-2, same sidebar search)",
    ),
    (
        HEIGHT,
        "setup",
        158,
        "assistant fields and drop-downs are 34 px (min-height 32 + the 1 px border); 32 is not a scaffold height: 36 or 28 is the lead's pick",
    ),
    (
        HEIGHT,
        "store",
        58,
        "sidebar search 34 px (54) and the store search page field 34 px (4): min-height 32 + border; \
         +4: the install-failed and package-missing wide cases (INT-2, same sidebar search)",
    ),
    (
        GAP,
        "capture",
        4,
        "screenshot bar: Copy and Save 4 px apart; the options menu 10 px from Capture",
    ),
    (GAP, "clipboard", 8, "clipboard row icon buttons 4 px apart"),
    (
        GAP,
        "controlcenter",
        16,
        "Now Playing transport buttons touch (0 px): one group drawn as three flat icon buttons, not a segmented control yet",
    ),
    (
        GAP,
        "kit",
        10,
        "media transport buttons touch (0 px); the stepper's minus and plus are 4 px apart",
    ),
    (
        GAP,
        "motion",
        4,
        "Control Center opening motion: the Now Playing transport buttons touch (0 px)",
    ),
    (
        GAP,
        "settings",
        6,
        "a default button and its MenuButton 4 px apart (a split button drawn as two)",
    ),
    (
        GAP,
        "widget",
        18,
        "media widget transport buttons 16 px apart (12) or touching (6)",
    ),
    (
        GAP,
        "widgets",
        8,
        "widget gallery media transport buttons 16 px apart",
    ),
];

#[derive(Clone, Debug)]
pub struct Violation {
    pub case: String,
    pub path: String,
    pub rule: &'static str,
    pub detail: String,
}

impl Violation {
    /// The case family the violation is counted under: the case name up to its first `-`.
    pub fn surface(&self) -> &str {
        surface(&self.case)
    }
}

pub fn surface(case: &str) -> &str {
    case.split('-').next().unwrap_or(case)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct R {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

impl R {
    fn r(&self) -> i32 {
        self.x + self.w
    }
    fn b(&self) -> i32 {
        self.y + self.h
    }
}

impl std::fmt::Display for R {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}x{}+{}+{}", self.w, self.h, self.x, self.y)
    }
}

fn bounds(w: &gtk::Widget, root: &gtk::Widget) -> Option<R> {
    w.compute_bounds(root).map(|b| R {
        x: b.x().round() as i32,
        y: b.y().round() as i32,
        w: b.width().round() as i32,
        h: b.height().round() as i32,
    })
}

fn shown(w: &gtk::Widget) -> bool {
    w.is_visible() && w.is_child_visible() && !w.is::<gtk::Popover>() && w.width() > 0
}

fn children(w: &gtk::Widget) -> Vec<gtk::Widget> {
    let mut out = Vec::new();
    let mut c = w.first_child();
    while let Some(ch) = c {
        if shown(&ch) {
            out.push(ch.clone());
        }
        c = ch.next_sibling();
    }
    out
}

/// One path segment: the GTK type, its CSS classes and, for a label or a button, its text.
fn name(w: &gtk::Widget) -> String {
    let mut s = w.type_().name().trim_start_matches("Gtk").to_string();
    for c in w.css_classes() {
        s.push('.');
        s.push_str(&c);
    }
    let text = w
        .downcast_ref::<gtk::Label>()
        .map(|l| l.text())
        .or_else(|| w.downcast_ref::<gtk::Button>().and_then(|b| b.label()))
        .or_else(|| {
            w.downcast_ref::<gtk::Entry>()
                .and_then(|e| e.placeholder_text())
        })
        .or_else(|| {
            w.downcast_ref::<gtk::SearchEntry>()
                .and_then(|e| e.placeholder_text())
        });
    if let Some(t) = text.filter(|t| !t.is_empty()) {
        s.push_str(&format!(" {:?}", t.as_str()));
    }
    s
}

/// The path from the case's root to `w`: plain layout boxes with no class are left out, so the
/// path reads as the surfaces a person sees (`AppWindow › toolbar › SearchEntry "Filter"`).
fn path(w: &gtk::Widget, root: &gtk::Widget) -> String {
    let mut parts = Vec::new();
    let mut cur = Some(w.clone());
    while let Some(c) = cur {
        if &c == root {
            break;
        }
        let plain = (c.is::<gtk::Box>()
            || c.is::<gtk::Overlay>()
            || c.is::<gtk::WindowHandle>()
            || c.is::<gtk::Viewport>()
            || c.is::<gtk::Stack>())
            && c.css_classes().is_empty();
        if !plain || &c == w {
            parts.push(name(&c));
        }
        cur = c.parent();
    }
    parts.reverse();
    parts.join(" › ")
}

/// The control kinds the HIG sizes; None for anything else.
fn control(w: &gtk::Widget) -> Option<&'static str> {
    if part_of_another(w) {
        return None;
    }
    Some(if w.is::<gtk::SearchEntry>() {
        "SearchEntry"
    } else if w.is::<gtk::Entry>() || w.is::<gtk::PasswordEntry>() || w.is::<gtk::SpinButton>() {
        "Entry"
    } else if w.is::<gtk::Switch>() {
        "Switch"
    } else if w.is::<gtk::DropDown>() {
        "DropDown"
    } else if w.is::<gtk::Scale>() {
        "Scale"
    } else if w.is::<gtk::CheckButton>() {
        "CheckButton"
    } else if w.is::<gtk::MenuButton>() {
        "MenuButton"
    } else if w.is::<gtk::Button>() && is_control_button(w) {
        "Button"
    } else {
        return None;
    })
}

/// Kit push-button classes (the other kit buttons are rows, tiles, pills and menu items: their own
/// specs size them).
const CONTROL_BUTTONS: [&str; 3] = ["btn", "icon-button", "get-button"];
/// Classes GTK itself puts on a button.
const GTK_BUTTON_CLASSES: [&str; 4] = ["text-button", "image-button", "flat", "toggle"];

fn is_control_button(w: &gtk::Widget) -> bool {
    let classes = w.css_classes();
    // A gtk::Button with no kit class is a control too (GTK's own default look).
    classes
        .iter()
        .any(|c| CONTROL_BUTTONS.contains(&c.as_str()))
        || classes
            .iter()
            .all(|c| GTK_BUTTON_CLASSES.contains(&c.as_str()))
}

/// Not a control of its own: the lock screen's borderless field is the text of the pill that
/// draws the control (kit.css `entry.lock-field`), and a menu bar's titles are menus.
fn part_of_another(w: &gtk::Widget) -> bool {
    if w.has_css_class("lock-field") {
        return true;
    }
    let mut a = w.parent();
    while let Some(p) = a {
        if p.has_css_class("menubar-strip") {
            return true;
        }
        a = p.parent();
    }
    false
}

/// Kit classes that draw a background a control sits on (kit.css), and the window of an app case.
const BACKGROUNDS: [&str; 14] = [
    "toolbar",
    "card-list",
    "list-row",
    "sheet",
    "widget-card",
    "glass-panel",
    "module",
    "app-window",
    "toast",
    "assistant-card",
    "process-frame",
    "force-quit-list",
    "status-bar",
    "menu",
];

/// kit.css rules that take the background off a container inside another: (container class,
/// ancestor class). The approval sheet is the glass panel itself; a widget card's module is the card.
const NO_BACKGROUND: [(&str, &str); 2] = [("sheet", "approval"), ("module", "widget-card")];

/// `w` draws a background: one of [`BACKGROUNDS`] (unless [`NO_BACKGROUND`] takes it off), a header
/// bar, or the window of an app case (a `preview` window shows the desktop picture: the content
/// draws its own panel on it).
fn background(w: &gtk::Widget) -> Option<String> {
    if w.css_name() == "headerbar" {
        return Some("headerbar".into());
    }
    if let Some(c) = BACKGROUNDS.iter().find(|c| w.has_css_class(c)) {
        let inside = |class: &str| {
            let mut a = w.parent();
            while let Some(p) = a {
                if p.has_css_class(class) {
                    return true;
                }
                a = p.parent();
            }
            false
        };
        let off = NO_BACKGROUND
            .iter()
            .any(|(class, within)| class == c && inside(within));
        if !off {
            return Some((*c).into());
        }
    }
    (w.is::<gtk::Window>() && !w.has_css_class("preview")).then(|| "window".into())
}

/// Containers that show only part of their content (scrolled away, mid-transition).
fn clips(w: &gtk::Widget) -> bool {
    w.is::<gtk::ScrolledWindow>()
        || w.is::<gtk::Viewport>()
        || w.is::<gtk::ListView>()
        || w.is::<gtk::GridView>()
        || w.is::<gtk::ColumnView>()
        || w.is::<gtk::Revealer>()
        || w.is::<gtk::Stack>()
}

struct Ctx<'a> {
    case: &'a str,
    root: gtk::Widget,
    pixels: Option<(i32, i32, Vec<u8>)>,
    out: Vec<Violation>,
}

impl Ctx<'_> {
    fn push(&mut self, w: &gtk::Widget, rule: &'static str, detail: String) {
        self.out.push(Violation {
            case: self.case.to_string(),
            path: path(w, &self.root),
            rule,
            detail,
        });
    }
}

/// Every violation in `win` (rendered as `tex`) for golden case `case`.
pub fn lint(case: &str, win: &gtk::Window, tex: Option<&gdk::Texture>) -> Vec<Violation> {
    let root: gtk::Widget = win.clone().upcast();
    let mut cx = Ctx {
        case,
        root: root.clone(),
        pixels: tex.map(pixels),
        out: Vec::new(),
    };
    walk(&mut cx, &root, None, None, None, false);
    cx.out
}

/// `bg`: nearest ancestor with a background (widget, kind, bounds); `view`: the visible area of the
/// nearest scroller; `in_control`: inside a control (its parts are the control's own business).
fn walk(
    cx: &mut Ctx,
    w: &gtk::Widget,
    bg: Option<(gtk::Widget, String, R)>,
    view: Option<R>,
    parent: Option<R>,
    in_control: bool,
) {
    let Some(me) = bounds(w, &cx.root) else {
        return;
    };
    // RU-701: inside the parent, the parts of a control too (an entry's clear button).
    if let Some(p) = parent {
        let over = [
            (p.x - me.x, "left"),
            (p.y - me.y, "top"),
            (me.r() - p.r(), "right"),
            (me.b() - p.b(), "bottom"),
        ];
        let sides: Vec<String> = over
            .iter()
            .filter(|(px, _)| *px > TOL)
            .map(|(px, side)| format!("{px} px past the {side}"))
            .collect();
        if !sides.is_empty() {
            cx.push(
                w,
                INSIDE,
                format!("{me} in parent {p}: {}", sides.join(", ")),
            );
        }
    }
    let kind = (!in_control).then(|| control(w)).flatten();
    let visible = view.is_none_or(|v| {
        me.x >= v.x - TOL && me.y >= v.y - TOL && me.r() <= v.r() + TOL && me.b() <= v.b() + TOL
    });
    if let (Some(kind), true) = (kind, visible) {
        check_control(cx, w, kind, me, bg.as_ref());
    }
    let bg = match background(w) {
        Some(k) => Some((w.clone(), k, me)),
        None => bg,
    };
    let view = if w.is::<gtk::ScrolledWindow>() || w.is::<gtk::Viewport>() {
        Some(view.map_or(me, |v| intersect(v, me)))
    } else {
        view
    };
    let kids = children(w);
    if !in_control && kind.is_none() {
        gaps(cx, w, &kids);
    }
    let parent = (!clips(w)).then_some(me);
    for c in &kids {
        walk(
            cx,
            c,
            bg.clone(),
            view,
            parent,
            in_control || kind.is_some(),
        );
    }
}

fn intersect(a: R, b: R) -> R {
    let (x, y) = (a.x.max(b.x), a.y.max(b.y));
    R {
        x,
        y,
        w: (a.r().min(b.r()) - x).max(0),
        h: (a.b().min(b.b()) - y).max(0),
    }
}

fn check_control(
    cx: &mut Ctx,
    w: &gtk::Widget,
    kind: &'static str,
    me: R,
    bg: Option<&(gtk::Widget, String, R)>,
) {
    // RU-702: one of the control heights; check boxes and sliders draw a 16 px mark in a row that
    // gives them their height (HIG section 2), so they are not sized here.
    if !matches!(kind, "CheckButton" | "Scale") && !scaffold::CONTROL_HEIGHTS.contains(&me.h) {
        cx.push(
            w,
            HEIGHT,
            format!(
                "{kind} is {} px high; control heights are {:?}",
                me.h,
                scaffold::CONTROL_HEIGHTS
            ),
        );
    }
    if let Some((_, bkind, p)) = bg {
        let (l, t, r, b) = (me.x - p.x, me.y - p.y, p.r() - me.r(), p.b() - me.b());
        let near: Vec<String> = [(l, "left"), (t, "top"), (r, "right"), (b, "bottom")]
            .iter()
            .filter(|(d, _)| *d < MIN_EDGE)
            .map(|(d, side)| format!("{d} px from the {side}"))
            .collect();
        // RU-700: off every edge of its container by the 8 px step.
        if !near.is_empty() {
            cx.push(
                w,
                EDGE,
                format!(
                    "{kind} {me} in {bkind} {p}: {} (at least {MIN_EDGE})",
                    near.join(", ")
                ),
            );
        }
        let bar = bkind == "toolbar" || bkind == "headerbar";
        if bar {
            // RU-703: on the bar's centre line.
            let off = ((me.y * 2 + me.h) - (p.y * 2 + p.h)).abs() / 2;
            if off > TOL {
                cx.push(
                    w,
                    CENTRE,
                    format!("{kind} centre is {off} px off the {bkind}'s ({me} in {p})"),
                );
            }
        }
        if bar && kind == "SearchEntry" {
            // RU-704: narrow, and the last thing on the bar, at its padding from the right.
            let mut why = Vec::new();
            if me.w > SEARCH_MAX {
                why.push(format!("{} px wide (at most {SEARCH_MAX})", me.w));
            }
            if r > scaffold::TOOLBAR_PADDING + TOL {
                why.push(format!(
                    "{r} px from the right edge (the bar's padding is {})",
                    scaffold::TOOLBAR_PADDING
                ));
            }
            if !why.is_empty() {
                cx.push(
                    w,
                    SEARCH,
                    format!("{me} in {bkind} {p}: {}", why.join(", ")),
                );
            }
        }
    }
    // RU-706: a destructive button that cannot be pressed reads as off, not as a pale warning.
    if kind == "Button"
        && w.has_css_class("btn-destructive")
        && !w.is_sensitive()
        && let Some((r, g, b)) = cx.pixels.as_ref().and_then(|px| fill(px, me))
        && reddish(r, g, b)
    {
        cx.push(
            w,
            DIM,
            format!(
                "disabled destructive button is filled rgb({r}, {g}, {b}), a red; use the dim (control) fill"
            ),
        );
    }
}

/// RU-705: two controls side by side in a horizontal box are 8 or 12 px apart, never touching.
/// A segmented or linked group is one control drawn in parts.
fn gaps(cx: &mut Ctx, w: &gtk::Widget, kids: &[gtk::Widget]) {
    let horizontal = w
        .downcast_ref::<gtk::Box>()
        .is_some_and(|b| b.orientation() == gtk::Orientation::Horizontal);
    if !horizontal || w.has_css_class("segmented") || w.has_css_class("linked") {
        return;
    }
    let mut prev: Option<(gtk::Widget, R)> = None;
    for c in kids {
        let Some(r) = bounds(c, &cx.root) else {
            continue;
        };
        if r.w == 0 {
            continue;
        }
        let is_control = control(c).is_some();
        if let Some((pw, pr)) = &prev
            && is_control
            && control(pw).is_some()
        {
            let gap = r.x - pr.r();
            if !GAPS.iter().any(|g| (gap - g).abs() <= TOL) {
                let detail = format!(
                    "{} px between {} and this {} ({})",
                    gap,
                    name(pw),
                    control(c).unwrap_or("control"),
                    if gap <= 0 {
                        "they touch".to_string()
                    } else {
                        format!("gaps are {GAPS:?}")
                    }
                );
                cx.push(c, GAP, detail);
            }
        }
        prev = Some((c.clone(), r));
    }
}

fn pixels(t: &gdk::Texture) -> (i32, i32, Vec<u8>) {
    let (w, h) = (t.width(), t.height());
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let mut d = gdk::TextureDownloader::new(t);
    d.set_format(gdk::MemoryFormat::R8g8b8a8);
    let bytes = d.download_bytes();
    let (data, stride) = (bytes.0, bytes.1);
    for y in 0..h as usize {
        let row = &data[y * stride..y * stride + w as usize * 4];
        buf[y * w as usize * 4..(y + 1) * w as usize * 4].copy_from_slice(row);
    }
    (w, h, buf)
}

/// The colour most of `r` is painted (the fill under the label), quantised to 8 levels a channel.
fn fill(px: &(i32, i32, Vec<u8>), r: R) -> Option<(u8, u8, u8)> {
    let (w, h, data) = px;
    let mut count: std::collections::HashMap<(u8, u8, u8), usize> = Default::default();
    // Inside the corner radius, so the corners' background does not count.
    for y in (r.y + 4).max(0)..(r.b() - 4).min(*h) {
        for x in (r.x + 8).max(0)..(r.r() - 8).min(*w) {
            let i = ((y * w + x) * 4) as usize;
            let k = (data[i] & 0xf8, data[i + 1] & 0xf8, data[i + 2] & 0xf8);
            *count.entry(k).or_default() += 1;
        }
    }
    count.into_iter().max_by_key(|(_, n)| *n).map(|(k, _)| k)
}

/// A red hue (within 30 degrees of red) with some saturation: a danger fill, faded or not.
fn reddish(r: u8, g: u8, b: u8) -> bool {
    let (r, g, b) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    if max == 0.0 || (max - min) / max < 0.2 || max != r {
        return false;
    }
    let hue = 60.0 * ((g - b) / (max - min));
    (-30.0..=30.0).contains(&hue)
}

/// Counts per (rule, surface) over `all` that exceed [`BASELINE`], and rows that can be lowered.
pub fn ratchet(all: &[Violation]) -> (Vec<String>, Vec<String>) {
    let mut counts: std::collections::BTreeMap<(&str, &str), usize> = Default::default();
    for v in all {
        *counts.entry((v.rule, v.surface())).or_default() += 1;
    }
    let base = |rule: &str, surface: &str| {
        BASELINE
            .iter()
            .find(|b| b.0 == rule && b.1 == surface)
            .map_or(0, |b| b.2)
    };
    let mut over = Vec::new();
    for ((rule, surface), n) in &counts {
        let max = base(rule, surface);
        if *n > max {
            over.push(format!("{rule} {surface}: {n} violations, baseline {max}"));
        }
    }
    let lower = BASELINE
        .iter()
        .filter_map(|(rule, surface, max, _)| {
            let n = counts.get(&(*rule, *surface)).copied().unwrap_or(0);
            (n < *max).then(|| format!("{rule} {surface}: {n} now, baseline {max}: lower it"))
        })
        .collect();
    (over, lower)
}

/// The lint finds each of its rules on a bar built to break them, and nothing on the same bar
/// built right: the proof that the rules fire (run once before the cases).
/// `render` draws a window as the golden run does (the pixels RU-706 reads).
pub fn self_check(render: impl Fn(&gtk::Window) -> gdk::Texture) {
    use shell::kit::widgets as w;
    let bar_with = |good: bool| {
        let win = gtk::Window::builder()
            .decorated(false)
            .default_width(640)
            .default_height(scaffold::KIT_TOOLBAR_HEIGHT)
            .build();
        win.add_css_class("preview");
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, if good { w::S2 } else { 0 });
        bar.add_css_class("toolbar");
        bar.set_valign(gtk::Align::Start);
        bar.set_size_request(640, scaffold::KIT_TOOLBAR_HEIGHT);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        bar.append(&spacer);
        let end = w::button("End Process", w::Kind::Destructive, || {});
        end.set_sensitive(false);
        end.set_valign(gtk::Align::Center);
        let field = w::search_field("Filter Processes");
        field.set_valign(gtk::Align::Center);
        if !good {
            // The 2026-11-05 System Monitor bar: glued to the top, taller than a control, wide.
            end.set_valign(gtk::Align::Start);
            field.set_valign(gtk::Align::Start);
            field.set_size_request(400, 34);
            end.set_size_request(-1, 34);
        }
        bar.append(&end);
        bar.append(&field);
        if !good {
            // Something after the search field, flush against it.
            bar.append(&w::icon_button("open-menu-symbolic", "Menu", || {}));
        }
        win.set_child(Some(&bar));
        win.present();
        pump(600);
        win
    };
    let rules = |v: &[Violation]| {
        let mut r: Vec<&str> = v.iter().map(|v| v.rule).collect();
        r.sort();
        r.dedup();
        r
    };
    let bad = bar_with(false);
    let found = lint("self-check-bad", &bad, None);
    bad.destroy();
    for id in [EDGE, HEIGHT, CENTRE, SEARCH, GAP] {
        assert!(
            rules(&found).contains(&id),
            "layout lint: {id} does not fire on the broken bar; found {found:#?}"
        );
    }
    let good = bar_with(true);
    let tex = render(&good);
    let clean = lint("self-check-good", &good, Some(&tex));
    good.destroy();
    assert!(
        clean.is_empty(),
        "layout lint: the bar built right has violations: {clean:#?}"
    );
    // RU-701 and RU-706 on their own: a child pushed out of its parent, a pale red fill.
    assert!(reddish(0xe8, 0x9a, 0x9a) && reddish(0xc0, 0x30, 0x30));
    assert!(!reddish(0x80, 0x80, 0x88) && !reddish(0x74, 0x52, 0xf5));
    let p = gtk::Fixed::new();
    p.set_size_request(40, 40);
    let child = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    child.add_css_class("probe");
    child.set_size_request(30, 30);
    // A fixed child placed up and left of its parent's origin: 10 px out on two sides.
    p.put(&child, -10.0, -10.0);
    let win = gtk::Window::builder().decorated(false).build();
    win.add_css_class("preview");
    let holder = gtk::Box::new(gtk::Orientation::Vertical, 0);
    holder.set_halign(gtk::Align::Start);
    holder.set_valign(gtk::Align::Start);
    holder.append(&p);
    win.set_child(Some(&holder));
    win.present();
    pump(300);
    let out = lint("self-check-inside", &win, None);
    win.destroy();
    assert!(
        out.iter()
            .any(|v| v.rule == INSIDE && v.path.ends_with(".probe")),
        "layout lint: RU-701 does not fire on a child 10 px out of its parent: {out:#?}"
    );
}
