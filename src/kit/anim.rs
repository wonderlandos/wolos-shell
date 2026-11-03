// SPDX-License-Identifier: GPL-2.0-or-later
//! The shell's frame clock and the [`Animated`] handle (`docs/modules/hig.md` section 7).
//!
//! An [`Animated`] is one number (an opacity, a translation, a scale) that moves toward a target
//! along a [`Curve`] for a [`Duration`], or follows a [`Spring`]. [`Animated::retarget`] may be
//! called at any time: the value turns around from where it is now (no jump back to the start),
//! a spring keeps its velocity, and a retarget to the target it already heads for changes
//! nothing (a second hover does not restart the move). Every frame the value is handed to the
//! `apply` closure; the `settled` closure runs once each time it reaches its target.
//!
//! All the handles of one window share one [`Clock`]: a single `add_tick_callback` on the root
//! window that is installed while something moves and removed when nothing does (no idle
//! wakeups). When the window unmaps, the clock finishes every animation (value = target,
//! `settled` runs): a hidden panel never waits on a frame that will not come. Under reduce
//! motion ([`super::motion::reduced`]) a retarget jumps to the target at once.
//!
//! Tests and goldens use [`Clock::manual`] and drive time with [`Clock::step`].
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use gtk::glib;
use gtk::prelude::*;

use super::motion::{Curve, Duration, Spring, SpringState};

/// How a value gets to its target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Motion {
    /// Along `curve` in `ms` milliseconds.
    Tween { curve: Curve, ms: u64 },
    /// Following a spring (no fixed duration).
    Spring(Spring),
}

impl Motion {
    /// Enter: `ease` at the given duration.
    pub fn enter(d: Duration) -> Motion {
        Motion::Tween {
            curve: Curve::Ease,
            ms: d.ms(),
        }
    }

    /// Leave: `in` at the given duration.
    pub fn leave(d: Duration) -> Motion {
        Motion::Tween {
            curve: Curve::In,
            ms: d.ms(),
        }
    }

    pub fn tween(curve: Curve, d: Duration) -> Motion {
        Motion::Tween { curve, ms: d.ms() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Run {
    Idle,
    Tween {
        from: f32,
        curve: Curve,
        dur_ns: u64,
        delay_ns: u64,
        /// Set on the first frame after the retarget.
        start_ns: Option<u64>,
    },
    Spring {
        spring: Spring,
        span: f32,
        delay_ns: u64,
        start_ns: Option<u64>,
        last_ns: Option<u64>,
    },
}

/// The pure state of one animated value: no GTK, explicit time in nanoseconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Value {
    pos: f32,
    vel: f32,
    target: f32,
    run: Run,
    last_ns: Option<u64>,
}

impl Value {
    pub fn new(v: f32) -> Value {
        Value {
            pos: v,
            vel: 0.0,
            target: v,
            run: Run::Idle,
            last_ns: None,
        }
    }

    pub fn value(&self) -> f32 {
        self.pos
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    /// Units per second over the last frame (handed to a spring on a retarget).
    pub fn velocity(&self) -> f32 {
        self.vel
    }

    pub fn is_moving(&self) -> bool {
        self.run != Run::Idle
    }

    /// Jump to `v` and stop.
    pub fn jump(&mut self, v: f32) {
        *self = Value::new(v);
    }

    /// Head for `target` with `motion`, starting `delay_ms` after the next frame. False when
    /// nothing changes: already there and still, or already heading there the same way.
    pub fn retarget(&mut self, target: f32, motion: Motion, delay_ms: u64) -> bool {
        if target == self.target && self.same_motion(motion) {
            return false;
        }
        if target == self.pos && self.vel == 0.0 && delay_ms == 0 {
            self.jump(target);
            return false;
        }
        let delay_ns = delay_ms.saturating_mul(1_000_000);
        self.target = target;
        self.run = match motion {
            Motion::Tween { curve, ms } => Run::Tween {
                from: self.pos,
                curve,
                dur_ns: ms.saturating_mul(1_000_000),
                delay_ns,
                start_ns: None,
            },
            Motion::Spring(spring) => Run::Spring {
                spring,
                span: (target - self.pos).abs(),
                delay_ns,
                start_ns: None,
                last_ns: None,
            },
        };
        true
    }

    fn same_motion(&self, m: Motion) -> bool {
        match (self.run, m) {
            (Run::Idle, _) => true,
            (Run::Tween { curve, dur_ns, .. }, Motion::Tween { curve: c, ms }) => {
                curve == c && dur_ns == ms.saturating_mul(1_000_000)
            }
            (Run::Spring { spring, .. }, Motion::Spring(s)) => spring == s,
            _ => false,
        }
    }

    /// Advance to frame time `now_ns`. True when this frame reached the target (settled).
    pub fn tick(&mut self, now_ns: u64) -> bool {
        let prev = self.pos;
        let dt_s = self
            .last_ns
            .map_or(0.0, |l| now_ns.saturating_sub(l) as f32 / 1e9);
        self.last_ns = Some(now_ns);
        let done = match &mut self.run {
            Run::Idle => return false,
            Run::Tween {
                from,
                curve,
                dur_ns,
                delay_ns,
                start_ns,
            } => {
                let start = *start_ns.get_or_insert(now_ns.saturating_add(*delay_ns));
                if now_ns < start {
                    return false;
                }
                let t = if *dur_ns == 0 {
                    1.0
                } else {
                    (now_ns - start) as f32 / *dur_ns as f32
                };
                self.pos = *from + (self.target - *from) * curve.at(t);
                t >= 1.0
            }
            Run::Spring {
                spring,
                span,
                delay_ns,
                start_ns,
                last_ns,
            } => {
                let start = *start_ns.get_or_insert(now_ns.saturating_add(*delay_ns));
                if now_ns < start {
                    return false;
                }
                let dt = last_ns.map_or(0.0, |l| now_ns.saturating_sub(l) as f32 / 1e9);
                *last_ns = Some(now_ns);
                let s = spring.step(
                    SpringState {
                        pos: self.pos,
                        vel: self.vel,
                    },
                    self.target,
                    dt,
                );
                self.pos = s.pos;
                self.vel = s.vel;
                Spring::at_rest(s, self.target, *span)
            }
        };
        if done {
            self.jump(self.target);
            self.last_ns = Some(now_ns);
            return true;
        }
        if !matches!(self.run, Run::Spring { .. }) && dt_s > 0.0 {
            self.vel = (self.pos - prev) / dt_s;
        }
        false
    }
}

type Apply = Box<dyn Fn(f32)>;
type Settled = RefCell<Option<Box<dyn Fn(f32)>>>;

struct Shared {
    value: RefCell<Value>,
    apply: Apply,
    settled: Settled,
    clock: Clock,
    queued: Cell<bool>,
}

/// One animated number on a window's [`Clock`]. Cheap to clone (all clones are the same value).
#[derive(Clone)]
pub struct Animated(Rc<Shared>);

impl Animated {
    /// A value starting at `initial` on the clock of `widget`'s window. `apply` gets every new
    /// value (it is not called for `initial`).
    pub fn new(
        widget: &impl IsA<gtk::Widget>,
        initial: f32,
        apply: impl Fn(f32) + 'static,
    ) -> Animated {
        Animated::on_clock(&Clock::of(widget), initial, apply)
    }

    /// A value on an explicit clock (tests, goldens, several widgets in step).
    pub fn on_clock(clock: &Clock, initial: f32, apply: impl Fn(f32) + 'static) -> Animated {
        Animated(Rc::new(Shared {
            value: RefCell::new(Value::new(initial)),
            apply: Box::new(apply),
            settled: RefCell::new(None),
            clock: clock.clone(),
            queued: Cell::new(false),
        }))
    }

    /// Runs `f(target)` each time the value reaches its target (after the last `apply`). One
    /// closure per handle; a new one replaces the old.
    pub fn connect_settled(&self, f: impl Fn(f32) + 'static) {
        self.0.settled.replace(Some(Box::new(f)));
    }

    pub fn value(&self) -> f32 {
        self.0.value.borrow().value()
    }

    pub fn target(&self) -> f32 {
        self.0.value.borrow().target()
    }

    pub fn is_moving(&self) -> bool {
        self.0.value.borrow().is_moving()
    }

    /// Head for `target` with `motion` from wherever the value is now.
    pub fn retarget(&self, target: f32, motion: Motion) {
        self.retarget_after(target, motion, 0);
    }

    /// [`Animated::retarget`], starting `delay_ms` after the next frame (a [`super::motion::Stagger`]
    /// delay). Under reduce motion: jumps at once, the delay is ignored.
    pub fn retarget_after(&self, target: f32, motion: Motion, delay_ms: u64) {
        if self.0.clock.is_reduced() {
            return self.set(target);
        }
        let changed = self.0.value.borrow_mut().retarget(target, motion, delay_ms);
        if changed {
            self.0.clock.add(self);
        } else if !self.is_moving() && self.0.queued.get() {
            // A retarget back to where it rests: nothing to animate, but it is settled.
            self.settle();
        }
    }

    /// Jump to `v` (no animation): `apply(v)` then `settled(v)`.
    pub fn set(&self, v: f32) {
        self.0.value.borrow_mut().jump(v);
        (self.0.apply)(v);
        self.settle();
    }

    /// End the current move now: jump to the target.
    pub fn finish(&self) {
        let t = self.target();
        self.set(t);
    }

    fn settle(&self) {
        self.0.queued.set(false);
        let v = self.value();
        if let Some(f) = self.0.settled.borrow().as_ref() {
            f(v);
        }
    }

    /// One frame: the new value to `apply`, and `settled` when it got there.
    fn frame(&self, now_ns: u64) {
        let (done, before, v) = {
            let mut val = self.0.value.borrow_mut();
            let before = val.value();
            (val.tick(now_ns), before, val.value())
        };
        if v != before || done {
            (self.0.apply)(v);
        }
        if done {
            self.settle();
        }
    }
}

struct ClockInner {
    /// None for a manual clock.
    host: Option<glib::WeakRef<gtk::Widget>>,
    items: RefCell<Vec<Weak<Shared>>>,
    tick: RefCell<Option<gtk::TickCallbackId>>,
    reduced: Cell<Option<bool>>,
    now_ns: Cell<u64>,
}

/// The frame clock of one window. Clone shares it.
#[derive(Clone)]
pub struct Clock(Rc<ClockInner>);

thread_local! {
    static CLOCKS: RefCell<Vec<(glib::WeakRef<gtk::Widget>, Clock)>> = const { RefCell::new(Vec::new()) };
}

impl Clock {
    /// The clock of `widget`'s window (its root; the widget itself before it has one). One per
    /// window for the life of the window.
    pub fn of(widget: &impl IsA<gtk::Widget>) -> Clock {
        let w = widget.as_ref();
        let host: gtk::Widget = w.root().map_or_else(|| w.clone(), |r| r.upcast());
        CLOCKS.with(|c| {
            let mut c = c.borrow_mut();
            c.retain(|(k, _)| k.upgrade().is_some());
            if let Some((_, clock)) = c.iter().find(|(k, _)| k.upgrade().as_ref() == Some(&host)) {
                return clock.clone();
            }
            let clock = Clock(Rc::new(ClockInner {
                host: Some(host.downgrade()),
                items: RefCell::new(Vec::new()),
                tick: RefCell::new(None),
                reduced: Cell::new(None),
                now_ns: Cell::new(0),
            }));
            let weak = Rc::downgrade(&clock.0);
            host.connect_unmap(move |_| {
                if let Some(inner) = weak.upgrade() {
                    Clock(inner).finish_all();
                }
            });
            c.push((host.downgrade(), clock.clone()));
            clock
        })
    }

    /// A clock with no window: time moves only by [`Clock::step`]. Animations run (reduce motion
    /// off) unless [`Clock::set_reduced`] says otherwise.
    pub fn manual() -> Clock {
        Clock(Rc::new(ClockInner {
            host: None,
            items: RefCell::new(Vec::new()),
            tick: RefCell::new(None),
            reduced: Cell::new(Some(false)),
            now_ns: Cell::new(0),
        }))
    }

    /// Force reduce motion on or off for this clock (None: follow [`super::motion::reduced`]).
    pub fn set_reduced(&self, r: Option<bool>) {
        self.0.reduced.set(r);
    }

    pub fn is_reduced(&self) -> bool {
        self.0.reduced.get().unwrap_or_else(super::motion::reduced)
    }

    /// Values still moving.
    pub fn len(&self) -> usize {
        self.0
            .items
            .borrow()
            .iter()
            .filter_map(Weak::upgrade)
            .filter(|s| s.queued.get())
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// True while the tick callback is installed (a window clock with something moving).
    pub fn is_ticking(&self) -> bool {
        self.0.tick.borrow().is_some()
    }

    /// Run one frame at `now_ns` (monotonic nanoseconds). The window clock calls it from its
    /// tick callback; tests call it by hand.
    pub fn step(&self, now_ns: u64) {
        self.0.now_ns.set(now_ns);
        // Snapshot: `apply`/`settled` may retarget other values on this clock.
        let items: Vec<Rc<Shared>> = self
            .0
            .items
            .borrow()
            .iter()
            .filter_map(Weak::upgrade)
            .collect();
        for sh in items {
            Animated(sh).frame(now_ns);
        }
        // Keep what still moves (a value settles out of the list; a retarget during this frame
        // put it back), once each.
        let mut seen: Vec<Rc<Shared>> = Vec::new();
        self.0.items.borrow_mut().retain(|w| match w.upgrade() {
            Some(s) if s.queued.get() && !seen.iter().any(|p| Rc::ptr_eq(p, &s)) => {
                seen.push(s);
                true
            }
            _ => false,
        });
    }

    /// Frame time of the last [`Clock::step`].
    pub fn now_ns(&self) -> u64 {
        self.0.now_ns.get()
    }

    /// Jump every running value to its target and stop ticking.
    pub fn finish_all(&self) {
        let items: Vec<Rc<Shared>> = self
            .0
            .items
            .take()
            .into_iter()
            .filter_map(|w| w.upgrade())
            .collect();
        for sh in items {
            Animated(sh).finish();
        }
        self.stop_ticking();
    }

    fn add(&self, a: &Animated) {
        if !a.0.queued.replace(true) {
            self.0.items.borrow_mut().push(Rc::downgrade(&a.0));
        }
        self.start_ticking();
    }

    fn stop_ticking(&self) {
        if let Some(id) = self.0.tick.take() {
            id.remove();
        }
    }

    fn start_ticking(&self) {
        let Some(host) = self.0.host.as_ref().and_then(|h| h.upgrade()) else {
            return;
        };
        if self.0.tick.borrow().is_some() {
            return;
        }
        let weak = Rc::downgrade(&self.0);
        let id = host.add_tick_callback(move |_, fc| {
            let Some(inner) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let clock = Clock(inner);
            clock.step(
                u64::try_from(fc.frame_time())
                    .unwrap_or(0)
                    .saturating_mul(1_000),
            );
            if clock.0.items.borrow().is_empty() {
                // Returning Break removes the callback; forget the id so it is not removed twice.
                drop(clock.0.tick.take());
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
        self.0.tick.replace(Some(id));
    }
}
