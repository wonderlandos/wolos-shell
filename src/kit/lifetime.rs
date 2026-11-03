// SPDX-License-Identifier: GPL-2.0-or-later
//! Work tied to a window's life: its timers and callbacks end with it. A surface that comes and
//! goes with a display (wallpaper, menu bar) must not leave a minute tick or a reload hook behind
//! each time, or a session that docks and undocks grows without bound.
use gtk::{glib, prelude::*};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

thread_local! {
    /// Minute ticks running now (the leak test counts them).
    static TICKS: Cell<usize> = const { Cell::new(0) };
}

/// Minute ticks running now, session-long ones included.
pub fn live_ticks() -> usize {
    TICKS.get()
}

/// One end action, with the key [`Lifetime::child`] drops it by.
type Action = (u64, Box<dyn FnOnce()>);

#[derive(Default)]
struct Inner {
    ended: Cell<bool>,
    on_end: RefCell<Vec<Action>>,
    next: Cell<u64>,
}

/// Removes a source from the context it runs on (the thread's default one).
fn remove(id: glib::SourceId) {
    if let Some(s) = glib::MainContext::ref_thread_default().find_source_by_id(&id) {
        s.destroy();
    }
}

/// Ends once: runs its end actions (remove timers, drop callbacks). Cheap to clone.
#[derive(Clone, Default)]
pub struct Lifetime(Rc<Inner>);

impl Lifetime {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ends when `w` is destroyed.
    pub fn of_window(w: &impl IsA<gtk::Window>) -> Self {
        let l = Self::new();
        l.tie(w);
        l
    }

    pub fn tie(&self, w: &impl IsA<gtk::Window>) {
        let l = self.clone();
        w.as_ref().connect_destroy(move |_| l.end());
    }

    pub fn alive(&self) -> bool {
        !self.0.ended.get()
    }

    /// Runs `f` when this ends (at once if it already has).
    pub fn on_end(&self, f: impl FnOnce() + 'static) {
        self.push(Box::new(f));
    }

    /// Adds an end action; its key, or None when it ran at once (already ended).
    fn push(&self, f: Box<dyn FnOnce()>) -> Option<u64> {
        if !self.alive() {
            f();
            return None;
        }
        let key = self.0.next.get();
        self.0.next.set(key + 1);
        self.0.on_end.borrow_mut().push((key, f));
        Some(key)
    }

    /// End actions waiting now (the leak test counts them).
    pub fn pending(&self) -> usize {
        self.0.on_end.borrow().len()
    }

    /// A lifetime that ends with this one, or earlier on its own: then it leaves nothing behind
    /// here (audit5 S8-11: a bar re-arranged all session kept one closure per arrange).
    pub fn child(&self) -> Lifetime {
        let child = Lifetime::new();
        let weak = Rc::downgrade(&child.0);
        let Some(key) = self.push(Box::new(move || {
            if let Some(c) = weak.upgrade() {
                Lifetime(c).end();
            }
        })) else {
            return child;
        };
        let parent = Rc::downgrade(&self.0);
        child.on_end(move || {
            if let Some(p) = parent.upgrade() {
                p.on_end.borrow_mut().retain(|(k, _)| *k != key);
            }
        });
        child
    }

    pub fn end(&self) {
        if self.0.ended.replace(true) {
            return;
        }
        let fs = std::mem::take(&mut *self.0.on_end.borrow_mut());
        for (_, f) in fs {
            f();
        }
    }

    /// Removes `id` when this ends.
    pub fn own(&self, id: glib::SourceId) {
        let id = RefCell::new(Some(id));
        self.on_end(move || {
            if let Some(id) = id.take() {
                remove(id);
            }
        });
    }
}

/// Calls `tick` now and on every minute boundary (one wakeup a minute) until `life` ends.
pub fn every_minute_while(life: &Lifetime, tick: impl Fn() + 'static) {
    if !life.alive() {
        return;
    }
    let tick: Rc<dyn Fn()> = Rc::new(tick);
    let pending: Rc<RefCell<Option<glib::SourceId>>> = Default::default();
    TICKS.set(TICKS.get() + 1);
    {
        let pending = pending.clone();
        life.on_end(move || {
            if let Some(id) = pending.take() {
                remove(id);
            }
            TICKS.set(TICKS.get() - 1);
        });
    }
    fn arm(tick: Rc<dyn Fn()>, pending: Rc<RefCell<Option<glib::SourceId>>>) {
        let secs = crate::clock::secs_to_next_minute(super::now().second);
        let p = pending.clone();
        let id = glib::timeout_add_seconds_local_once(secs, move || {
            p.take();
            tick();
            arm(tick, p);
        });
        pending.replace(Some(id));
    }
    tick();
    arm(tick, pending);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ending_runs_each_action_once_and_late_actions_at_once() {
        let n = Rc::new(Cell::new(0));
        let l = Lifetime::new();
        let c = n.clone();
        l.on_end(move || c.set(c.get() + 1));
        assert!(l.alive());
        l.end();
        l.end();
        assert_eq!(n.get(), 1);
        let c = n.clone();
        l.on_end(move || c.set(c.get() + 10));
        assert_eq!(n.get(), 11, "an action added after the end runs at once");
    }

    /// Audit5 S8-11: a child that ends first unlinks itself; one alive ends with its parent.
    #[test]
    fn children_end_with_the_parent_and_leave_nothing_when_they_end_first() {
        let parent = Lifetime::new();
        let base = parent.pending();
        let mut child = parent.child();
        for _ in 0..50 {
            child.end();
            child = parent.child();
        }
        assert_eq!(parent.pending(), base + 1, "only the live child is held");
        let n = Rc::new(Cell::new(0));
        let c = n.clone();
        child.on_end(move || c.set(c.get() + 1));
        parent.end();
        assert!(!child.alive());
        assert_eq!(n.get(), 1);
        let late = parent.child();
        assert!(!late.alive(), "a child of an ended lifetime is born ended");
    }

    #[test]
    fn minute_ticks_stop_with_their_lifetime() {
        let ctx = glib::MainContext::new();
        ctx.with_thread_default(|| {
            let base = live_ticks();
            let lives: Vec<Lifetime> = (0..20).map(|_| Lifetime::new()).collect();
            let calls = Rc::new(Cell::new(0));
            for l in &lives {
                let c = calls.clone();
                every_minute_while(l, move || c.set(c.get() + 1));
            }
            assert_eq!(calls.get(), 20, "each ticks once at the start");
            assert_eq!(live_ticks(), base + 20);
            lives.iter().for_each(Lifetime::end);
            assert_eq!(live_ticks(), base);
            let dead = Lifetime::new();
            dead.end();
            every_minute_while(&dead, || panic!("never ticks"));
            assert_eq!(live_ticks(), base);
        })
        .unwrap();
    }
}
