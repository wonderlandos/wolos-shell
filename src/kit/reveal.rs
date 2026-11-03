// SPDX-License-Identifier: GPL-2.0-or-later
//! First surface of a compositor (ADR 0012): content fades in from the black that the loader splash,
//! the console and the compositor show, and leaves the same way, so every hand-off is black to black.
//! Login window, setup assistant and start-up lock screen use it; golden renders start it shown.
//!
//! The fade runs on the window's motion clock (`kit::anim`, `docs/modules/hig.md` section 7): in
//! along `out` over `--motion-slow` (a large surface entering), back to black along `in` over
//! `--motion-normal`, one step faster. Under reduce motion it snaps. Widgets that enter with the
//! surface (the lock screen's clock, avatar and field) wait for it with [`when_shown`].
use super::anim::{Animated, Motion};
use super::motion::{Curve, Duration};
use gtk::{glib, prelude::*};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Same as `--glass-motion` in kit.css (the assistant's page cross-fade, `kit::assistant`).
pub const REVEAL_MS: u32 = 320;
/// Frames the content may wait for `ready` (e.g. the blurred wallpaper) before it shows anyway.
pub const MAX_WAIT_FRAMES: u32 = 90;
/// Greeter compositors start with `cursor:invisible` (login.conf, setup.conf); the pointer appears
/// with the window, never over the black frames before it.
pub const SHOW_POINTER: [&str; 4] = ["hyprctl", "keyword", "cursor:invisible", "false"];

/// Fading in from black: `out`, `--motion-slow`.
pub fn show_motion() -> Motion {
    Motion::tween(Curve::Out, Duration::Slow)
}

/// Fading back to black: `in`, `--motion-normal`.
pub fn hide_motion() -> Motion {
    Motion::leave(Duration::Normal)
}

/// Grace after a fade before its end is forced (a stalled frame clock never keeps a surface up).
const STALL_SLACK_MS: u64 = 250;

/// Fade-in/fade-out wrapper. Put [`Reveal::root`] as the window child. The revealer always shows
/// its child; the fade is the root's opacity on the motion clock (the class `kit-reveal` and the
/// type let [`exit_faded`] find every one).
pub struct Reveal {
    pub root: gtk::Revealer,
}

/// One-shot completion callback.
type Done = Box<dyn FnOnce()>;

/// The motion state of one [`Reveal`], kept by the GTK thread while its root lives (the
/// `Reveal` value itself is often dropped once the window is set up).
struct Entry {
    root: glib::WeakRef<gtk::Revealer>,
    /// Created on first use, when the root is in its window (one clock per window).
    fade: RefCell<Option<Animated>>,
    /// The fade in has started (or it never faded: shown at once).
    shown: Cell<bool>,
    waiting: RefCell<Vec<Done>>,
}

thread_local! {
    static REVEALS: RefCell<Vec<Rc<Entry>>> = const { RefCell::new(Vec::new()) };
}

fn entry(root: &gtk::Revealer) -> Option<Rc<Entry>> {
    REVEALS.with_borrow_mut(|v| {
        v.retain(|e| e.root.upgrade().is_some());
        v.iter()
            .find(|e| e.root.upgrade().as_ref() == Some(root))
            .cloned()
    })
}

impl Entry {
    fn fade(&self) -> Option<Animated> {
        let root = self.root.upgrade()?;
        let mut f = self.fade.borrow_mut();
        Some(
            f.get_or_insert_with(|| {
                let weak = root.downgrade();
                Animated::new(&root, root.opacity() as f32, move |v| {
                    if let Some(r) = weak.upgrade() {
                        r.set_opacity(v.into());
                    }
                })
            })
            .clone(),
        )
    }

    /// Starts the fade in and runs what waited for it.
    fn show(&self) {
        if let Some(f) = self.fade() {
            f.retarget(1.0, show_motion());
        }
        self.shown.set(true);
        for f in self.waiting.take() {
            f()
        }
    }
}

impl Reveal {
    /// `from_black`: `child` starts transparent over the window's black (`window.lock-window`) and
    /// [`show_when`](Self::show_when) fades it in; else it shows at once (a lock over a running
    /// desktop). Golden renders always start shown.
    pub fn new(child: &impl IsA<gtk::Widget>, from_black: bool) -> Reveal {
        let golden = std::env::var_os("WOL_FAKE_NOW").is_some();
        let shown = golden || !from_black;
        let root = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::None)
            .reveal_child(true)
            .hexpand(true)
            .vexpand(true)
            .child(child)
            .build();
        root.set_opacity(if shown { 1.0 } else { 0.0 });
        root.add_css_class("kit-reveal");
        REVEALS.with_borrow_mut(|v| {
            v.push(Rc::new(Entry {
                root: root.downgrade(),
                fade: RefCell::new(None),
                shown: Cell::new(shown),
                waiting: RefCell::new(Vec::new()),
            }))
        });
        Reveal { root }
    }

    /// Fades in once the window has a frame on screen (so the fade itself is seen) and `ready()`
    /// holds, or after [`MAX_WAIT_FRAMES`]; then `shown()` runs once (pointer, boot mark).
    pub fn show_when(&self, ready: impl Fn() -> bool + 'static, shown: impl FnOnce() + 'static) {
        let frames = Cell::new(0u32);
        let shown = RefCell::new(Some(shown));
        self.root.add_tick_callback(move |r, _| {
            frames.set(frames.get() + 1);
            if frames.get() < 2 || (!ready() && frames.get() < MAX_WAIT_FRAMES) {
                return glib::ControlFlow::Continue;
            }
            if let Some(e) = entry(r) {
                e.show();
            } else {
                r.set_opacity(1.0);
            }
            if let Some(f) = shown.take() {
                f()
            }
            glib::ControlFlow::Break
        });
    }

    /// Fades back to black, then `done` (e.g. exit: the next compositor starts on black too).
    pub fn hide_then(&self, done: impl FnOnce() + 'static) {
        let done: Rc<RefCell<Option<Done>>> = Rc::new(RefCell::new(Some(Box::new(done))));
        let finish = move || {
            // Taken before the call: `f` may land here again (the unlock unmaps the window and
            // its clock settles the same fade), which must find nothing, not a held borrow.
            let f = done.borrow_mut().take();
            if let Some(f) = f {
                f()
            }
        };
        let fade = entry(&self.root).and_then(|e| e.fade());
        let Some(fade) = fade.filter(|_| self.root.is_mapped() && self.root.opacity() > 0.0) else {
            self.root.set_opacity(0.0);
            finish();
            return;
        };
        // Animations off or a stalled frame clock: never stay up.
        let ms = match hide_motion() {
            Motion::Tween { ms, .. } => ms,
            Motion::Spring(_) => 0,
        };
        glib::timeout_add_local_once(
            std::time::Duration::from_millis(ms + STALL_SLACK_MS),
            finish.clone(),
        );
        fade.connect_settled(move |v| {
            if v == 0.0 {
                finish()
            }
        });
        fade.retarget(0.0, hide_motion());
    }
}

/// Runs `f` once the [`Reveal`] around `widget` starts fading in: at once when it is already shown
/// or `widget` is in none (a golden render, the lock over a running desktop).
pub fn when_shown(widget: &impl IsA<gtk::Widget>, f: impl FnOnce() + 'static) {
    let root = std::iter::successors(widget.as_ref().parent(), |w| w.parent())
        .filter_map(|w| w.downcast::<gtk::Revealer>().ok())
        .find(|r| r.has_css_class("kit-reveal"));
    match root.and_then(|r| entry(&r)) {
        Some(e) if !e.shown.get() => e.waiting.borrow_mut().push(Box::new(f)),
        _ => f(),
    }
}

/// Shows the pointer on a greeter compositor ([`SHOW_POINTER`]).
pub fn show_pointer() {
    super::spawn(&SHOW_POINTER.map(String::from));
}

/// Fades every window's [`Reveal`] to black, then exits 0 (login window, setup assistant done).
pub fn exit_faded() {
    let reveals: Vec<gtk::Revealer> = gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok()?.child())
        .filter_map(|c| c.downcast::<gtk::Revealer>().ok())
        .filter(|r| r.has_css_class("kit-reveal"))
        .collect();
    let Some((first, rest)) = reveals.split_first() else {
        std::process::exit(0)
    };
    for r in rest {
        Reveal { root: r.clone() }.hide_then(|| {});
    }
    Reveal {
        root: first.clone(),
    }
    .hide_then(|| std::process::exit(0));
}

/// One boot timeline line to the journal via `logger` (`journalctl -b -t wolos | grep wolos-boot`),
/// next to wol-login's.
pub fn boot_mark(stage: &str) {
    if std::env::var_os("WOL_FAKE_NOW").is_some() {
        return;
    }
    let line = shared::login::boot_mark_now(stage);
    super::spawn(&["logger", "-p", "user.notice", "-t", "wolos", &line].map(String::from));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DT-7: the fade in from black and back runs on the motion clock's tokens: a large surface
    /// enters along `out` over `--motion-slow`, and leaves along `in` one step faster.
    #[test]
    fn the_reveal_moves_on_the_contract() {
        assert_eq!(show_motion(), Motion::tween(Curve::Out, Duration::Slow));
        assert_eq!(hide_motion(), Motion::leave(Duration::Normal));
        assert!(Curve::CONTRACT.contains(&Curve::Out));
        let mut v = super::super::anim::Value::new(0.0);
        assert!(v.retarget(1.0, show_motion(), 0));
        v.tick(0);
        assert!(!v.tick(200_000_000) && v.value() > 0.5 && v.value() < 1.0);
        assert!(v.tick(400_000_000));
        assert_eq!(v.value(), 1.0);
    }

    #[test]
    fn pointer_and_motion_match_the_compositor_and_the_stylesheet() {
        assert_eq!(
            SHOW_POINTER.join(" "),
            "hyprctl keyword cursor:invisible false"
        );
        let css = include_str!("kit.css");
        assert!(css.contains(&format!("--glass-motion: {REVEAL_MS}ms;")));
    }
}
