// SPDX-License-Identifier: GPL-2.0-or-later
//! The generic part of a Kit component test (WolOS Design System, ADR 0058 draft): everything a
//! component's cases have in common, so a component adds only its `build` and its `act`.
//!
//! A test binary lists its [`Component`]s and its `KNOWN` ratchet and calls [`run`] from its only
//! `#[test]`. For every component [`run`] reads the spec (`design/components/<id>.toml` through
//! `catalog.json`), builds each `tests.states x tests.densities x tests.variants` case with the
//! Kit, mounts it alone, and holds it to the spec: the drawn size per density, the accessibility
//! role and name, the layout rules of the HIG (section 6), the component's own `act` (keyboard,
//! state, behaviour), and one golden picture plus one view-tree snapshot
//! (`tests/golden-kit/kit-<id>-<state>-<density>-<variant>`).
//!
//! `WOL_KIT_ONLY=<text>` runs the cases whose name contains it, `WOL_GOLDEN_UPDATE=1` rewrites
//! goldens and trees, `WOL_KIT_MEASURE=1` prints every measured length and never fails a size (to
//! write a spec or a `KNOWN` row from what the Kit draws today).
//! Include with `#[path = "support/kit_case.rs"] mod kit_case;` after `kit_test`.
#![allow(dead_code)] // not every binary uses every helper
use super::kit_test::{self, Density, Frame, Mounted, Tester};
use gtk::prelude::*;
use shell::kit::widgets as w;
use shell::theme::Variant;
use std::cell::RefCell;
use std::rc::Rc;

/// What a pending change calls when it is over.
pub type Done = Box<dyn FnOnce(bool)>;
/// What the callbacks of a case recorded, one short sentence each ("changed 3").
pub type Log = Rc<RefCell<Vec<String>>>;

/// One built case: the widget to mount, the widget the spec's role/size/name are about (`probe`,
/// inside it), and what the callbacks recorded.
pub struct Built {
    /// Objects the case needs to stay alive while it runs (a Kit struct whose handlers hang on it).
    pub keep: Vec<Box<dyn std::any::Any>>,
    pub widget: gtk::Widget,
    pub probe: gtk::Widget,
    pub log: Log,
    pub done: Rc<RefCell<Option<Done>>>,
}

impl Built {
    pub fn new(widget: &impl IsA<gtk::Widget>, probe: &impl IsA<gtk::Widget>) -> Built {
        Built {
            keep: Vec::new(),
            widget: widget.as_ref().clone(),
            probe: probe.as_ref().clone(),
            log: Rc::default(),
            done: Rc::default(),
        }
    }
    /// Keeps `x` alive as long as the case.
    pub fn keeping(mut self, x: impl std::any::Any) -> Built {
        self.keep.push(Box::new(x));
        self
    }
    /// A widget that is its own probe.
    pub fn alone(widget: &impl IsA<gtk::Widget>) -> Built {
        Built::new(widget, widget)
    }
    /// The callback log, for a closure that moves it in.
    pub fn logger(&self) -> Log {
        self.log.clone()
    }
}

/// How the spec says the probe is named for assistive technology.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Named {
    /// Not a named control (a picture, a static line): nothing to check.
    Nothing,
    /// A label relation or an accessible label property.
    Label,
    /// The probe shows words of its own (a button's label): a drawn label with text.
    Words,
}

/// One component's test.
pub struct Component {
    pub id: &'static str,
    pub build: fn(&str) -> Built,
    /// What the case does and asserts beyond the generic checks.
    pub act: fn(&mut Ctx),
    /// The spec's `sizes.<name>` the probe is held to in this state and density (None: no size).
    pub size: fn(&str, Density) -> Option<&'static str>,
    pub named: Named,
    /// Width of the frame (the narrow app window by default).
    pub width: i32,
    /// The state's picture is compared (false for a state that is never at rest: a turning ring).
    pub picture: fn(&str) -> bool,
    /// The row that holds the probe (its CSS class), held to `density.<d>.row` (or `class:key`).
    pub row: Option<&'static str>,
    /// The probe's height is the control height of `density.<d>.control`.
    pub control: bool,
    /// The probe has the role the spec names (false for a component whose own widget is a window
    /// of the toolkit that cannot be mounted, like a tooltip).
    pub check_role: bool,
}

pub fn regular(_: &str, _: Density) -> Option<&'static str> {
    Some("regular")
}
pub fn at_rest(_: &str) -> bool {
    true
}

/// Everything one case may read and report.
pub struct Ctx<'a> {
    pub t: &'a Tester,
    pub spec: &'a serde_json::Value,
    pub state: &'a str,
    pub frame: Frame,
    pub b: &'a Built,
    pub m: &'a Mounted,
    pub name: String,
    pub run: &'a mut Run,
    pub id: &'static str,
}

impl Ctx<'_> {
    /// Records a problem sentence when `ok` is false.
    pub fn expect(&mut self, ok: bool, what: impl std::fmt::Display) {
        if !ok {
            self.run.bad.push(format!("{}: {what}", self.name));
        }
    }
    /// `expect(a == b)` with both values in the sentence.
    pub fn same<T: PartialEq + std::fmt::Debug>(&mut self, got: T, want: T, what: &str) {
        if got != want {
            self.run
                .bad
                .push(format!("{}: {what}: got {got:?}, want {want:?}", self.name));
        }
    }
    pub fn probe(&self) -> &gtk::Widget {
        &self.b.probe
    }
    /// The `T` inside the built widget (the first of its kind), or a problem.
    pub fn find<T: IsA<gtk::Widget>>(&mut self) -> Option<T> {
        let found = self.m.find::<T>().into_iter().next();
        if found.is_none() {
            self.run.bad.push(format!(
                "{}: no {} in the built widget",
                self.name,
                std::any::type_name::<T>()
                    .rsplit("::")
                    .next()
                    .unwrap_or("widget")
            ));
        }
        found
    }
    /// The log of the case's callbacks.
    pub fn log(&self) -> Vec<String> {
        self.b.log.borrow().clone()
    }
    /// Tab must land on `target` or inside it (an entry or a menu button keeps the keyboard on a
    /// child of its own).
    pub fn tab_reaches(&mut self, target: &impl IsA<gtk::Widget>, what: &str) {
        // From rest: a window that has just presented already holds the first focusable control.
        self.m.unfocus();
        let got = self.m.tab();
        let ok = got
            .as_ref()
            .is_some_and(|g| g == target.upcast_ref::<gtk::Widget>() || g.is_ancestor(target));
        let desc = |g: Option<&gtk::Widget>| {
            g.map_or("nothing".to_string(), |g| {
                format!(
                    "{} {:?} at {:?}",
                    g.type_().name(),
                    g.css_classes(),
                    self.m.rect(g)
                )
            })
        };
        let msg = format!(
            "Tab reaches {what} first (it reached {}, wanted {})",
            desc(got.as_ref()),
            desc(Some(target.upcast_ref()))
        );
        self.expect(ok, msg);
    }
    /// Tab must not land on `target` or inside it (a disabled control).
    pub fn tab_skips(&mut self, target: &impl IsA<gtk::Widget>, what: &str) {
        self.m.unfocus();
        let got = self.m.tab();
        let ok = !got
            .as_ref()
            .is_some_and(|g| g == target.upcast_ref::<gtk::Widget>() || g.is_ancestor(target));
        self.expect(ok, format_args!("Tab skips {what}"));
    }
    /// A length of the case, held to the spec unless frozen (see [`Run::hold`]).
    pub fn hold(&mut self, check: &str, measured: i32, want: i32) {
        self.run.hold(self.id, check, measured, want);
    }
}

fn near(a: i32, b: i32) -> bool {
    (a - b).abs() <= 1
}

/// The ratchet: (component, check, measured), frozen where the Kit does not meet its spec yet. The
/// measured value is text: px for a length ("52"), the role or name state for accessibility.
pub type Known = &'static [(&'static str, &'static str, &'static str)];

/// Per-run state: the problems, the KNOWN rows that some case measured, and the measured lines.
pub struct Run {
    pub known: Known,
    pub bad: Vec<String>,
    pub seen: Vec<String>,
    pub report: Vec<String>,
    pub measure: bool,
}

impl Run {
    /// Holds a measured length to the spec's `want` (one px of slack), unless KNOWN freezes it.
    pub fn hold(&mut self, id: &str, check: &str, measured: i32, want: i32) {
        self.judge(
            id,
            check,
            &measured.to_string(),
            &format!("drawn {measured} px, the spec says {want}"),
            near(measured, want),
        );
    }
    /// The one rule behind every check: `ok` passes; otherwise the value must be frozen in KNOWN
    /// exactly as measured (`got`), and a frozen value that moved fails until the row is lowered.
    pub fn judge(&mut self, id: &str, check: &str, got: &str, why: &str, ok: bool) {
        let key = format!("{id}:{check}");
        if self.measure {
            self.report.push(format!("{id} {check}: {got} ({why})"));
            return;
        }
        if let Some(k) = self.known.iter().find(|k| k.0 == id && k.1 == check) {
            if !self.seen.contains(&key) {
                self.seen.push(key);
            }
            if got != k.2 {
                self.bad.push(format!(
                    "{id} {check}: {why}; KNOWN says {}: the Kit changed; lower or remove the KNOWN row",
                    k.2
                ));
            }
        } else if !ok {
            self.bad.push(format!("{id} {check}: {why}"));
        }
    }
}

fn variant_of(v: &str) -> Variant {
    if v == "light" {
        Variant::Light
    } else {
        Variant::Dark
    }
}

/// The accessible role name as the spec writes it (`switch`, `spinbutton`).
pub fn role_name(w: &impl IsA<gtk::Widget>) -> String {
    format!("{:?}", w.as_ref().accessible_role()).to_lowercase()
}

fn dim(spec: &serde_json::Value, path: &[&str]) -> Option<i32> {
    let mut v = spec;
    for p in path {
        v = &v[*p];
    }
    v.as_i64().map(|n| n as i32)
}

/// Every case of one component; adds to `run`.
fn component_cases(t: &Tester, c: &Component, only: Option<&str>, run: &mut Run) {
    let spec = kit_test::spec(c.id);
    let states = kit_test::strings(&spec["tests"]["states"]);
    let densities = kit_test::strings(&spec["tests"]["densities"]);
    let variants = kit_test::strings(&spec["tests"]["variants"]);
    assert!(
        !states.is_empty() && !densities.is_empty() && !variants.is_empty(),
        "{}: the spec lists its cases",
        c.id
    );
    let mut ran = 0;
    // Variant outermost: the stylesheet is applied once per variant, not once per case.
    for v in &variants {
        for state in &states {
            for d in &densities {
                let density = Density::parse(d).unwrap_or_else(|| panic!("{}: density {d}", c.id));
                let frame = Frame {
                    width: c.width,
                    density,
                    variant: variant_of(v),
                };
                let name = format!("{}-{state}-{}", c.id, frame.tag());
                if only.is_some_and(|o| !name.contains(o)) {
                    continue;
                }
                ran += 1;
                let b = (c.build)(state);
                let m = t.mount(&b.widget, frame);
                let n_before = run.bad.len();
                // The picture is of the state as built, before the case acts on it.
                if (c.picture)(state) {
                    // At rest: a freshly presented window hands the focus to its first control
                    // (a field drew its ring and selected its text in the picture).
                    m.unfocus();
                    run.bad.extend(m.golden(t, &name));
                }
                one_case(t, c, &spec, state, frame, &b, &m, &name, run);
                println!(
                    "{name}: {}",
                    if run.bad.len() == n_before {
                        "ok"
                    } else {
                        "FAIL"
                    }
                );
            }
        }
    }
    if only.is_none() {
        assert!(ran > 0, "{}: no case ran", c.id);
        // A KNOWN row that no case measured is stale.
        for k in run.known.iter().filter(|k| k.0 == c.id) {
            let key = format!("{}:{}", k.0, k.1);
            if !run.seen.contains(&key) && !run.measure {
                run.bad
                    .push(format!("KNOWN row {key} is measured by no case: remove it"));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)] // one call per case; a struct would only rename the arguments
fn one_case(
    t: &Tester,
    c: &Component,
    spec: &serde_json::Value,
    state: &str,
    f: Frame,
    b: &Built,
    m: &Mounted,
    name: &str,
    run: &mut Run,
) {
    let dn = f.density.name();
    let id = c.id;

    // Sizes per density, from the spec.
    if let Some(size) = (c.size)(state, f.density) {
        let (_, _, pw, ph) = m.rect(&b.probe);
        if let Some(want) = dim(spec, &["sizes", size, "width"]) {
            run.hold(id, &format!("{dn}.sizes.{size}.width"), pw, want);
        }
        if let Some(want) = dim(spec, &["sizes", size, "height"]) {
            run.hold(id, &format!("{dn}.sizes.{size}.height"), ph, want);
        }
    }
    if let Some(class) = c.row {
        // "slider-row:row_stack": the class, then the density key when it is not `row`.
        let (class, key) = class.split_once(':').unwrap_or((class, "row"));
        if let Some(row) = m.by_class(class).into_iter().next() {
            let (_, _, _, rh) = m.rect(&row);
            if let Some(want) = dim(spec, &["density", dn, key]) {
                run.hold(id, &format!("{dn}.{key}"), rh, want);
            }
        } else {
            run.bad
                .push(format!("{name}: no .{class} row around the probe"));
        }
    }
    // At standard density the control is the probe's own size (sizes.regular); the control height
    // of the other densities is held here.
    if c.control && f.density != Density::Standard {
        if let Some(want) = dim(spec, &["density", dn, "control"]) {
            let (_, _, _, ph) = m.rect(&b.probe);
            run.hold(id, &format!("{dn}.control"), ph, want);
        }
    }

    // The layout rules of HIG section 6.
    for i in m.issues() {
        run.bad.push(format!("{name}: layout: {i}"));
    }

    // Accessibility: the role the spec names, a name for assistive technology.
    let mut ctx = Ctx {
        t,
        spec,
        state,
        frame: f,
        b,
        m,
        name: name.to_string(),
        id,
        run,
    };
    if let (true, Some(want)) = (c.check_role, spec["a11y"]["gtk_role"].as_str()) {
        let got = role_name(&b.probe);
        ctx.run.judge(
            id,
            "a11y.role",
            &got,
            &format!("accessible role {got}, the spec says {want}"),
            got == want,
        );
    }
    let named = match c.named {
        Named::Nothing => true,
        Named::Label => {
            gtk::test_accessible_has_relation(&b.probe, gtk::AccessibleRelation::LabelledBy)
                || gtk::test_accessible_has_property(&b.probe, gtk::AccessibleProperty::Label)
        }
        Named::Words => m
            .find::<gtk::Label>()
            .iter()
            .any(|l| l.is_drawable() && !l.text().is_empty()),
    };
    ctx.run.judge(
        id,
        "a11y.name",
        if named { "named" } else { "nameless" },
        "the control has no name for assistive technology",
        named,
    );
    (c.act)(&mut ctx);
}

/// Runs every component of a test binary: the body of its only `#[test]`.
pub fn run(components: &[Component], known: Known) {
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
    let mut run = Run {
        known,
        bad: Vec::new(),
        seen: Vec::new(),
        report: Vec::new(),
        measure: std::env::var_os("WOL_KIT_MEASURE").is_some(),
    };
    for c in components {
        component_cases(&t, c, only.as_deref(), &mut run);
    }
    for r in &run.report {
        println!("measured: {r}");
    }
    println!("renders and trees: {}", t.out.display());
    if run.measure {
        for b in &run.bad {
            println!("problem: {b}");
        }
        return;
    }
    assert!(
        run.bad.is_empty(),
        "kit components off their spec:\n{}",
        run.bad.join("\n")
    );
}

/// A frame at the narrow app width, standard density, light: for the extra mounts of an `act`.
pub fn narrow(density: Density, variant: Variant) -> Frame {
    Frame {
        width: w::APP_NARROW.0,
        density,
        variant,
    }
}
