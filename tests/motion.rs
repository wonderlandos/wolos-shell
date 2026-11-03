// SPDX-License-Identifier: GPL-2.0-or-later
//! The shell's motion library (`kit::motion`, `kit::anim`, HIG section 7): curves and durations
//! are the theme contract libWolkit apps use, springs settle in the documented time, a stagger
//! keeps its order, a retarget turns around without a jump, and reduce motion snaps.
use std::cell::RefCell;
use std::rc::Rc;

use shell::kit::anim::{Animated, Clock, Motion, Value};
use shell::kit::motion::{self, Curve, DURATIONS_MS, Duration, Spring, Stagger};

const KIT_CSS: &str = include_str!("../src/kit/kit.css");
const HYPR_CONF: &str = include_str!("../../../desktop/overlay/usr/share/wolos/hypr/hyprland.conf");
const HIG: &str = include_str!("../../../docs/modules/hig.md");
const WOLKIT_VALUE: &str = include_str!("../../wolkit/src/style/value.rs");
const WOLKIT_TRANSITION: &str = include_str!("../../wolkit/src/style/transition.rs");
const WOLKIT_TOKENS: &str = include_str!("../../wolkit/src/style/tokens.rs");
const WOLKIT_ANIM: &str = include_str!("../../wolkit/src/anim/mod.rs");

const MS: u64 = 1_000_000;

/// Reference: progress of a cubic Bezier at time fraction t by plain bisection on x.
fn reference(c: Curve, t: f32) -> f32 {
    let Some([x1, y1, x2, y2]) = c.bezier() else {
        return t;
    };
    let b = |a: f64, b: f64, s: f64| {
        let u = 1.0 - s;
        3.0 * u * u * s * a + 3.0 * u * s * s * b + s * s * s
    };
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..60 {
        let mid = (lo + hi) / 2.0;
        if b(x1 as f64, x2 as f64, mid) < t as f64 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    b(y1 as f64, y2 as f64, (lo + hi) / 2.0) as f32
}

#[test]
fn curves_hit_their_ends_and_their_midpoints() {
    for c in Curve::ALL {
        assert_eq!(c.at(0.0), 0.0, "{c:?} at 0");
        assert!((c.at(1.0) - 1.0).abs() < 1e-5, "{c:?} at 1");
        assert!((c.at(0.5) - reference(c, 0.5)).abs() < 1e-3, "{c:?} at .5");
        // Clamped outside 0..1.
        assert_eq!(c.at(-1.0), 0.0);
        assert!((c.at(2.0) - 1.0).abs() < 1e-5);
        assert_eq!(Curve::from_name(c.name()), Some(c));
    }
    // Monotonic: none of the contract's curves overshoots; the two spring-like ones do.
    let peak = |c: Curve| {
        (0..=100)
            .map(|i| c.at(i as f32 / 100.0))
            .fold(0.0f32, f32::max)
    };
    for c in Curve::ALL {
        let contract = Curve::CONTRACT.contains(&c);
        if contract || c == Curve::Soft {
            let mut last = 0.0;
            for i in 0..=100 {
                let v = c.at(i as f32 / 100.0);
                assert!(v + 1e-4 >= last, "{c:?} goes back at {i}%");
                last = v;
            }
        }
        assert_eq!(
            peak(c) > 1.001,
            matches!(c, Curve::Glass | Curve::Overshoot),
            "{c:?} overshoot"
        );
    }
    assert!(
        peak(Curve::Glass) > peak(Curve::Overshoot),
        "glass overshoots more"
    );
    assert_eq!(Curve::ALL[..5], Curve::CONTRACT);
    // Shapes: ease leaves fast, in leaves slow, linear is the diagonal.
    assert!(Curve::Ease.at(0.5) > 0.85);
    assert!(Curve::In.at(0.5) < 0.35);
    assert_eq!(Curve::Linear.at(0.5), 0.5);
    let mid = Curve::InOut.at(0.5);
    assert!(
        Curve::In.at(0.5) < mid && mid < Curve::Ease.at(0.5),
        "in-out sits between"
    );
    assert_eq!(Curve::Linear.css(), "linear");
    assert_eq!(Curve::In.css(), "cubic-bezier(0.4, 0, 1, 1)");
}

/// Same names and control points as libWolkit's `Ease` (read from its source: the shell does
/// not link libWolkit).
#[test]
fn curves_are_libwolkits_ease_verbatim() {
    let ease_line = WOLKIT_VALUE
        .lines()
        .find(|l| l.contains("pub enum Ease"))
        .expect("libWolkit Ease");
    for (variant, c) in [
        ("Standard", Curve::Ease),
        ("Linear", Curve::Linear),
        ("In", Curve::In),
        ("Out", Curve::Out),
        ("InOut", Curve::InOut),
    ] {
        assert!(
            ease_line.contains(&format!("{variant} = \"{}\"", c.name())),
            "{variant} is named {} in libWolkit: {ease_line}",
            c.name()
        );
        let want = match c.bezier() {
            None => "None".to_string(),
            Some(p) => format!(
                "Some([{}])",
                p.iter()
                    .map(|v| format!("{v:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        assert!(
            WOLKIT_TRANSITION.contains(&format!("Ease::{variant} => {want},")),
            "libWolkit Ease::{variant} is {want}"
        );
    }
}

#[test]
fn durations_are_the_theme_contract() {
    assert_eq!(DURATIONS_MS, [150, 250, 400]);
    assert!(
        WOLKIT_ANIM.contains("DURATIONS_NS: [u64; 3] = [150_000_000, 250_000_000, 400_000_000];")
    );
    for (d, (name, val)) in Duration::ALL.into_iter().zip(&shared::theme::SCALE[12..15]) {
        assert_eq!(d.var(), *name);
        assert_eq!(format!("{}ms", d.ms()), *val);
    }
}

fn root_rule() -> &'static str {
    let a = KIT_CSS.find(":root {").expect(":root");
    &KIT_CSS[a..a + KIT_CSS[a..].find("\n}").expect("end of :root")]
}

/// kit.css defines the durations itself (the rules resolve with no theme loaded) and its two
/// curve tokens are the contract's `ease` and `in`.
#[test]
fn kit_css_defines_the_motion_tokens() {
    let root = root_rule();
    for d in Duration::ALL {
        assert!(
            root.contains(&format!("{}: {}ms;", d.var(), d.ms())),
            "{} in :root",
            d.var()
        );
    }
    let css_points = |c: Curve| {
        let p = c.bezier().unwrap();
        let f = |v: f32| {
            let s = format!("{v}");
            s.strip_prefix("0.").map_or(s.clone(), |r| format!(".{r}"))
        };
        format!(
            "cubic-bezier({},{},{},{})",
            f(p[0]),
            f(p[1]),
            f(p[2]),
            f(p[3])
        )
    };
    assert!(
        root.contains(&format!("--ease-out: {};", css_points(Curve::Ease))),
        "{root}"
    );
    assert!(
        root.contains(&format!("--ease-in: {};", css_points(Curve::In))),
        "{root}"
    );
    assert!(
        root.contains(&format!("--ease-in-out: {};", css_points(Curve::InOut))),
        "{root}"
    );
}

/// The named curves outside the contract are the ones already in use: kit.css `--glass-spring`
/// and the compositor's `spring` and `ease` beziers.
#[test]
fn the_extra_curves_are_the_ones_in_use() {
    let css_points = |c: Curve| {
        let p = c.bezier().unwrap();
        let f = |v: f32| {
            let s = format!("{v}");
            s.strip_prefix("0.").map_or(s.clone(), |r| format!(".{r}"))
        };
        format!(
            "cubic-bezier({},{},{},{})",
            f(p[0]),
            f(p[1]),
            f(p[2]),
            f(p[3])
        )
    };
    assert!(root_rule().contains(&format!("--glass-spring: {};", css_points(Curve::Glass))));
    let conf_points = |c: Curve| {
        let p = c.bezier().unwrap();
        p.iter()
            .map(|v| format!("{v}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    for (name, c) in [("spring", Curve::Overshoot), ("ease", Curve::Soft)] {
        let want = format!("bezier = {name}, {}", conf_points(c));
        assert!(
            HYPR_CONF.lines().any(|l| l.trim() == want),
            "hyprland.conf: {want}"
        );
    }
    for c in Curve::ALL {
        assert!(
            HIG.lines()
                .any(|l| l.starts_with(&format!("| `{}` (`Curve::", c.name()))),
            "HIG curve table lists `{}`",
            c.name()
        );
    }
}

/// Keyframes move transform (and opacity), never margins: a margin per frame is a relayout.
#[test]
fn keyframes_animate_transform_not_margins() {
    let mut n = 0;
    for l in KIT_CSS.lines().filter(|l| l.starts_with("@keyframes")) {
        n += 1;
        assert!(!l.contains("margin") && !l.contains("padding"), "{l}");
    }
    assert!(n >= 2, "dock-bounce and shake");
    assert!(KIT_CSS.contains("@keyframes dock-bounce { 0%, 100% { transform: translateY(0); }"));
    assert!(KIT_CSS.contains("@keyframes shake { 0%, 100% { transform: translateX(0); }"));
}

/// The settle time of each preset, from the HIG table row "| `name` | ... | N ms |".
fn documented_settle_ms(name: &str) -> u64 {
    let row = HIG
        .lines()
        .find(|l| l.starts_with(&format!("| `{name}` |")))
        .unwrap_or_else(|| panic!("HIG section 7 has a `{name}` spring row"));
    let cell = row.trim_end_matches('|').rsplit('|').next().unwrap().trim();
    cell.trim_end_matches(" ms")
        .parse()
        .unwrap_or_else(|_| panic!("{row}"))
}

#[test]
fn springs_settle_within_two_percent_in_the_documented_time() {
    let presets = [
        ("gentle", Spring::GENTLE),
        ("snappy", Spring::SNAPPY),
        ("bouncy", Spring::BOUNCY),
        ("window-open", motion::window::OPEN.spring()),
        ("window-close", motion::window::CLOSE.spring()),
    ];
    for (name, s) in presets {
        let doc = documented_settle_ms(name);
        let got = s.settle_ms();
        assert!(
            got <= doc,
            "{name}: settles in {got} ms, documented {doc} ms"
        );
        assert!(
            doc - got <= 20,
            "{name}: the doc ({doc} ms) is honest ({got} ms)"
        );
        // Driven the way kit::anim drives it (60 Hz frames), it is inside 2% by then.
        let mut st = motion::SpringState { pos: 0.0, vel: 0.0 };
        for _ in 0..(doc / 16 + 1) {
            st = s.step(st, 1.0, 0.016);
        }
        assert!((st.pos - 1.0).abs() <= 0.02, "{name} at {doc} ms: {st:?}");
        if name != "bouncy" {
            assert!(s.damping_ratio() > 0.7, "{name} does not wobble");
        }
    }
    // The presets are libWolkit's springs.
    for (s, (k, d)) in [Spring::GENTLE, Spring::SNAPPY, Spring::BOUNCY]
        .into_iter()
        .zip([("170.0", "26.0"), ("320.0", "30.0"), ("260.0", "14.0")])
    {
        assert!(WOLKIT_TOKENS.contains(&format!("stiffness: {k},\n            damping: {d},")));
        assert_eq!(
            (format!("{:?}", s.stiffness), format!("{:?}", s.damping)),
            (k.into(), d.into())
        );
    }
    assert_eq!(Spring::named("snappy"), Some(Spring::SNAPPY));
    assert_eq!(Spring::named("wobbly"), None);
    // The window springs stay the agreed constants.
    assert_eq!(motion::window::OPEN.spring().stiffness, 385.5);
    assert_eq!(motion::window::CLOSE.spring().damping, 30.5);
}

#[test]
fn a_stagger_keeps_its_order_and_its_cap() {
    let s = Stagger::new(5, Stagger::GAP_MS);
    let d: Vec<u64> = (0..5).map(|i| s.delay_ms(i)).collect();
    assert_eq!(d, [0, 30, 60, 90, 120]);
    let r: Vec<u64> = (0..5).map(|i| s.delay_rev_ms(i)).collect();
    assert_eq!(r, [120, 90, 60, 30, 0], "leaving: last first");
    assert_eq!(s.total_ms(Duration::Normal.ms()), 370);
    // A long list shrinks the gap, keeps the order and never waits more than the cap.
    let long = Stagger::new(100, 30);
    let mut last = None;
    for i in 0..100 {
        let v = long.delay_ms(i);
        assert!(last.is_none_or(|l| v > l), "item {i} after item {}", i - 1);
        assert!(v <= Stagger::MAX_SPREAD_MS);
        last = Some(v);
    }
    assert_eq!(long.delay_ms(500), long.delay_ms(99));
    assert_eq!(Stagger::new(0, 30).total_ms(250), 0);
    assert_eq!(Stagger::new(1, 30).delay_ms(0), 0);
}

fn recorder(clock: &Clock, initial: f32) -> (Animated, Rc<RefCell<Vec<f32>>>) {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let s = seen.clone();
    let a = Animated::on_clock(clock, initial, move |v| s.borrow_mut().push(v));
    (a, seen)
}

#[test]
fn a_tween_follows_its_curve_and_settles_once() {
    let clock = Clock::manual();
    let (a, seen) = recorder(&clock, 0.0);
    let settled = Rc::new(RefCell::new(Vec::new()));
    let s2 = settled.clone();
    a.connect_settled(move |v| s2.borrow_mut().push(v));
    a.retarget(100.0, Motion::enter(Duration::Normal));
    assert!(a.is_moving());
    assert_eq!(clock.len(), 1);
    clock.step(1_000 * MS); // first frame: the start
    assert_eq!(a.value(), 0.0);
    clock.step(1_125 * MS); // half way in time
    assert!(
        (a.value() - 100.0 * Curve::Ease.at(0.5)).abs() < 1e-3,
        "{}",
        a.value()
    );
    clock.step(1_250 * MS);
    assert_eq!(a.value(), 100.0);
    assert!(!a.is_moving());
    assert!(clock.is_empty());
    clock.step(1_300 * MS);
    assert_eq!(*settled.borrow(), [100.0], "settled once");
    assert_eq!(*seen.borrow().last().unwrap(), 100.0);
}

/// Interruptible: a retarget mid-way turns around from where the value is; the same target
/// again does not restart the move.
#[test]
fn a_retarget_turns_around_without_a_jump() {
    let clock = Clock::manual();
    let (a, seen) = recorder(&clock, 0.0);
    a.retarget(1.0, Motion::enter(Duration::Normal));
    clock.step(0);
    clock.step(100 * MS);
    let mid = a.value();
    assert!(mid > 0.3 && mid < 1.0, "{mid}");
    // Same target, same motion: keeps going (a second hover does not restart).
    a.retarget(1.0, Motion::enter(Duration::Normal));
    clock.step(116 * MS);
    assert!(a.value() >= mid);
    // Turn around: the next frame starts where it is, not at 1 or 0.
    let here = a.value();
    a.retarget(0.0, Motion::leave(Duration::Fast));
    clock.step(132 * MS);
    assert!(
        (a.value() - here).abs() < 1e-6,
        "no jump: {} vs {here}",
        a.value()
    );
    let mut t = 148 * MS;
    while t <= 300 * MS {
        clock.step(t);
        t += 16 * MS;
    }
    assert_eq!(a.value(), 0.0);
    let v = seen.borrow();
    for w in v.windows(2) {
        assert!(
            (w[1] - w[0]).abs() < 0.5,
            "no 60 Hz frame jumps half the way: {w:?}"
        );
    }
}

/// A spring keeps its velocity across a retarget; a tween hands its speed to a spring.
#[test]
fn a_spring_keeps_its_velocity() {
    let mut v = Value::new(0.0);
    assert!(v.retarget(1.0, Motion::Spring(Spring::SNAPPY), 0));
    v.tick(0);
    for i in 1..=5 {
        v.tick(i * 16 * MS);
    }
    let vel = v.velocity();
    assert!(vel > 0.0);
    v.retarget(2.0, Motion::Spring(Spring::SNAPPY), 0);
    assert_eq!(v.velocity(), vel, "the retarget keeps the velocity");
    let mut t = 6 * 16 * MS;
    while v.is_moving() {
        v.tick(t);
        t += 16 * MS;
        assert!(t < 3_000 * MS, "settles");
    }
    assert_eq!(v.value(), 2.0);
    // Tween -> spring: the speed carries over.
    let mut w = Value::new(0.0);
    w.retarget(1.0, Motion::tween(Curve::Linear, Duration::Normal), 0);
    w.tick(0);
    w.tick(50 * MS);
    let speed = w.velocity();
    assert!((speed - 4.0).abs() < 0.01, "linear 0->1 in 250 ms: {speed}");
    w.retarget(0.0, Motion::Spring(Spring::GENTLE), 0);
    assert_eq!(w.velocity(), speed);
}

#[test]
fn a_stagger_delays_the_start() {
    let clock = Clock::manual();
    let st = Stagger::new(3, 30);
    let items: Vec<_> = (0..3).map(|_| recorder(&clock, 0.0).0).collect();
    for (i, a) in items.iter().enumerate() {
        a.retarget_after(1.0, Motion::enter(Duration::Fast), st.delay_ms(i));
    }
    clock.step(0);
    clock.step(40 * MS);
    let v: Vec<f32> = items.iter().map(Animated::value).collect();
    assert!(v[0] > v[1] && v[1] > 0.0 && v[2] == 0.0, "{v:?}");
    clock.step(st.total_ms(Duration::Fast.ms()) * MS);
    assert!(items.iter().all(|a| a.value() == 1.0));
    assert!(clock.is_empty());
}

#[test]
fn reduce_motion_snaps() {
    let clock = Clock::manual();
    clock.set_reduced(Some(true));
    let (a, seen) = recorder(&clock, 0.0);
    let settled = Rc::new(RefCell::new(0));
    let s2 = settled.clone();
    a.connect_settled(move |_| *s2.borrow_mut() += 1);
    a.retarget_after(1.0, Motion::Spring(Spring::BOUNCY), 90);
    assert_eq!(a.value(), 1.0, "at the target with no frame");
    assert!(!a.is_moving() && clock.is_empty());
    assert_eq!(*seen.borrow(), [1.0]);
    assert_eq!(*settled.borrow(), 1);
}

/// `finish_all` (what a window clock does on unmap): every value at its target, `settled` ran.
#[test]
fn finishing_leaves_nothing_half_way() {
    let clock = Clock::manual();
    let (a, _) = recorder(&clock, 0.0);
    let (b, _) = recorder(&clock, 5.0);
    let done = Rc::new(RefCell::new(Vec::new()));
    let d2 = done.clone();
    a.connect_settled(move |v| d2.borrow_mut().push(v));
    a.retarget(1.0, Motion::leave(Duration::Slow));
    b.retarget(0.0, Motion::Spring(Spring::GENTLE));
    clock.step(0);
    clock.step(50 * MS);
    clock.finish_all();
    assert_eq!((a.value(), b.value()), (1.0, 0.0));
    assert!(clock.is_empty() && !a.is_moving() && !b.is_moving());
    assert_eq!(*done.borrow(), [1.0]);
}

/// A `settled` closure may start the next move (open, then close) on the same clock.
#[test]
fn settled_may_retarget() {
    let clock = Clock::manual();
    let (a, _) = recorder(&clock, 0.0);
    let again = a.clone();
    a.connect_settled(move |v| {
        if v == 1.0 {
            again.retarget(0.0, Motion::leave(Duration::Fast));
        }
    });
    a.retarget(1.0, Motion::enter(Duration::Fast));
    let mut t = 0;
    while t <= 400 * MS {
        clock.step(t);
        t += 16 * MS;
    }
    assert_eq!(a.value(), 0.0);
    assert!(clock.is_empty());
}
