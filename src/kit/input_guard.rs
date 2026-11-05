// SPDX-License-Identifier: GPL-2.0-or-later
//! The input that answers the Authenticate sheet stays with the sheet (QA-STORE-60). The page
//! under the sheet takes input again the moment the sheet goes, and what answered it is often
//! still going: a Return held a moment too long repeats into the sidebar row that has the
//! keyboard back (twenty activations in the test, the Store reloaded its page), and the second
//! click of a double click on Allow lands on the tile that was under it. So the page waits for
//! the input to be over: no key and no pointer button is down, and a double-click time has passed
//! since the last one. Real events only count: a signal emitted by code never passes the
//! controllers below, so a test that clicks through the API is not slowed.
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// The longest the page waits for a key that never reports its release (its release went to
/// another window): then it takes input anyway.
const MOST: Duration = Duration::from_secs(2);

/// What is down, and when input last happened, in one window.
#[derive(Default)]
pub(crate) struct Guard {
    keys: RefCell<HashSet<u32>>,
    buttons: Cell<u32>,
    last: Cell<Option<Instant>>,
    /// Counts sheets that went down; a timer of an earlier one does nothing.
    round: Cell<u32>,
    /// What to do when the input may be over: re-enable the page.
    release: RefCell<Option<Rc<dyn Fn()>>>,
}

/// A double click's time: a second click this soon after the first belongs to the same gesture.
fn settle() -> Duration {
    let ms = gtk::Settings::default()
        .map_or(400, |s| s.gtk_double_click_time())
        .clamp(200, 1000);
    Duration::from_millis(u64::try_from(ms).unwrap_or(400))
}

impl Guard {
    /// Watches `root`'s window for key and button events (the capture phase, never taking one),
    /// from the time it is mapped. `release` runs whenever the input may be over; it checks.
    pub(crate) fn watch(self: &Rc<Self>, root: &gtk::Box, release: Rc<dyn Fn()>) {
        *self.release.borrow_mut() = Some(release);
        let (g, wired) = (self.clone(), Cell::new(false));
        root.connect_map(move |b| {
            let Some(win) = b.root().and_downcast::<gtk::Window>() else {
                return;
            };
            if wired.replace(true) {
                return;
            }
            let keys = gtk::EventControllerKey::new();
            keys.set_propagation_phase(gtk::PropagationPhase::Capture);
            let k = g.clone();
            keys.connect_key_pressed(move |_, _, code, _| {
                k.keys.borrow_mut().insert(code);
                k.last.set(Some(Instant::now()));
                gtk::glib::Propagation::Proceed
            });
            let k = g.clone();
            keys.connect_key_released(move |_, _, code, _| {
                k.keys.borrow_mut().remove(&code);
                k.last.set(Some(Instant::now()));
                k.over();
            });
            win.add_controller(keys);
            let click = gtk::GestureClick::new();
            click.set_button(0);
            click.set_propagation_phase(gtk::PropagationPhase::Capture);
            let k = g.clone();
            click.connect_begin(move |_, _| {
                k.buttons.set(k.buttons.get() + 1);
                k.last.set(Some(Instant::now()));
            });
            let k = g.clone();
            click.connect_end(move |_, _| {
                k.buttons.set(k.buttons.get().saturating_sub(1));
                k.last.set(Some(Instant::now()));
                k.over();
            });
            win.add_controller(click);
        });
    }

    /// Nothing is down and the last input is a double-click time behind.
    pub(crate) fn quiet(&self) -> bool {
        self.keys.borrow().is_empty()
            && self.buttons.get() == 0
            && self.last.get().is_none_or(|t| t.elapsed() >= settle())
    }

    fn over(&self) {
        let release = self.release.borrow().clone();
        if let Some(r) = release {
            r();
        }
    }

    /// The sheet went up: a pending release of an earlier one is void.
    pub(crate) fn up(&self) {
        self.round.set(self.round.get().wrapping_add(1));
    }

    /// The sheet went down. The page is let go by [`Self::over`] when the input is over, by a
    /// timer for a double-click time after the last input, and at the latest after [`MOST`].
    pub(crate) fn down(self: &Rc<Self>) {
        let round = self.round.get();
        let left = self
            .last
            .get()
            .map_or(Duration::ZERO, |t| settle().saturating_sub(t.elapsed()));
        if left.is_zero() {
            return self.over();
        }
        for (wait, force) in [(left, false), (MOST, true)] {
            let g = self.clone();
            // fixed wait: the input that answered the sheet is not something a person reads, it
            // is a finger still moving; "Give me more time" has nothing to lengthen here.
            gtk::glib::timeout_add_local_once(wait, move || {
                if g.round.get() != round {
                    return;
                }
                if force {
                    g.keys.borrow_mut().clear();
                    g.buttons.set(0);
                    g.last.set(None);
                }
                g.over();
            });
        }
    }
}
