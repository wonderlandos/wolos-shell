// SPDX-License-Identifier: GPL-2.0-or-later
//! App switcher: glass strip of 64 px app icons, accent ring on the selection, its name below.
//! Super+Tab opens/advances, Super+Shift+Tab goes back, releasing Super focuses (see `switcher::op`).
use crate::kit::anim::{Animated, Clock, Motion};
use crate::kit::motion::Duration as Speed;
use crate::kit::surface::{Keyboard, Spec};
use crate::kit::widgets as w;
use crate::switcher::{self, App, CancelOpen, Cycle, Op};
use gtk::{gdk, glib, prelude::*};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Switcher commands arriving while a commit focuses the selection (audit3 R2A-3): an ⌘Tab
/// then reads as a new open, not as a Tab on the strip about to hide; the Tabs and the Super
/// release after it go to that open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reopen(Option<Queued>);

impl Reopen {
    pub fn push(self, verb: crate::protocol::Verb, target: &str) -> Self {
        match self.0 {
            None if switcher::op(verb, target, false) == Op::Open => {
                Reopen(Some(Queued::default()))
            }
            None => self,
            Some(mut q) => {
                q.push(switcher::op(verb, target, true));
                Reopen(Some(q))
            }
        }
    }

    /// The open to run after the hide, with its queued keys.
    pub fn queued(self) -> Option<Queued> {
        self.0
    }
}

/// Centred overlay that takes the keyboard on demand, not exclusively: with an exclusive layer
/// mapped the compositor refuses `focuswindow`, so the selection could only be focused 60 ms after the
/// strip unmapped, and the old window came forward first (`switcher::commit`).
pub const SPEC: Spec = Spec::panel("wol-panel-switcher", [false; 4], Keyboard::OnDemand);

pub struct Strip {
    pub root: gtk::Box,
    tiles: Vec<(gtk::Box, gtk::Label)>,
    /// The names' opacities on the window's clock ([`Strip::animate`]); None: they jump.
    names: RefCell<Vec<Animated>>,
}

impl Strip {
    pub fn new(apps: &[App], selected: usize) -> Self {
        let root = w::glass_panel(0);
        root.add_css_class("switcher-strip");
        let row = w::hbox(w::S2);
        root.append(&row);
        let tiles: Vec<(gtk::Box, gtk::Label)> = apps
            .iter()
            .map(|a| {
                let tile = w::vbox(w::S2);
                tile.add_css_class("switcher-tile");
                let ring = w::vbox(0);
                ring.add_css_class("switcher-ring");
                ring.set_halign(gtk::Align::Center);
                ring.append(&w::app_icon(&a.icon, w::ICON_XL));
                tile.append(&ring);
                let name = w::centered(&a.name, "switcher-name");
                name.add_css_class("may-ellipsize");
                name.set_max_width_chars(w::TILE_CHARS);
                name.set_ellipsize(gtk::pango::EllipsizeMode::End);
                tile.append(&name);
                row.append(&tile);
                (tile, name)
            })
            .collect();
        let s = Strip {
            root,
            tiles,
            names: Default::default(),
        };
        s.select(selected);
        s
    }

    /// From now on a new selection cross-fades the names (150 ms, on `clock`: the switcher
    /// window's, so the strip and its fade-in share one frame clock).
    pub fn animate(&self, clock: &Clock) {
        let names = self
            .tiles
            .iter()
            .map(|(_, name)| {
                let n = name.clone();
                Animated::on_clock(clock, name.opacity() as f32, move |v| {
                    n.set_opacity(v.into())
                })
            })
            .collect();
        self.names.replace(names);
    }

    pub fn select(&self, index: usize) {
        let names = self.names.borrow();
        for (i, (tile, name)) in self.tiles.iter().enumerate() {
            if i == index {
                tile.add_css_class("selected");
            } else {
                tile.remove_css_class("selected");
            }
            // Only the selection shows its name; the others keep their slot so nothing shifts.
            let to = if i == index { 1.0 } else { 0.0 };
            match names.get(i) {
                Some(a) if to == 1.0 => a.retarget(to, Motion::enter(Speed::Fast)),
                Some(a) => a.retarget(to, Motion::leave(Speed::Fast)),
                None => name.set_opacity(to.into()),
            }
        }
    }

    /// The name of tile `i`'s opacity now.
    pub fn name_opacity(&self, i: usize) -> f64 {
        self.tiles.get(i).map_or(0.0, |(_, n)| n.opacity())
    }
}

/// Keys pressed while the strip waits for the compositor's client list (read off the UI thread, audit2
/// A2-10): Tabs move the selection once the list is there, a Super release commits at once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Queued {
    /// Net Tabs forward (Shift+Tab counts back).
    steps: i64,
    commit: bool,
}

impl Queued {
    pub fn push(&mut self, op: Op) {
        match op {
            Op::Next => self.steps += 1,
            Op::Prev => self.steps -= 1,
            Op::Commit => self.commit = true,
            Op::Open | Op::None => {}
        }
    }

    /// The selection after the queued Tabs, from the one the strip opens on.
    pub fn apply(self, mut c: Cycle) -> Cycle {
        // A full round is no move; at most `len` steps whatever was queued.
        let n = self.steps.unsigned_abs() % (c.len.max(1) as u64);
        for _ in 0..n {
            c = if self.steps > 0 { c.next() } else { c.prev() };
        }
        c
    }

    /// Super was released before the list came: switch without showing the strip (a quick
    /// ⌘Tab tap brings the previous app forward).
    pub fn commits(self) -> bool {
        self.commit
    }
}

pub struct Switcher {
    pub window: gtk::Window,
    apps: RefCell<Vec<App>>,
    cycle: Cell<Cycle>,
    strip: RefCell<Option<Strip>>,
    /// Some while the client list is read.
    opening: Cell<Option<Queued>>,
    /// Some while a commit focuses the selection (off the UI thread): an ⌘Tab meanwhile opens a
    /// fresh strip after the hide (audit3 R2A-3).
    committing: Cell<Option<Reopen>>,
    /// A Super release that found nothing open cancels the show racing behind it (audit4 S5-3).
    cancel: Cell<CancelOpen>,
}

impl Switcher {
    pub fn build(app: &gtk::Application) -> Rc<Self> {
        let window = super::window(app, "panel", SPEC);
        crate::kit::surface::survive_output_loss(&window, SPEC);
        window.add_css_class("switcher");
        // Fades in each open, on the window's clock (the names' cross-fade shares it).
        super::transient::fade_on_map(window.upcast_ref(), Speed::Normal);
        let s = Rc::new(Switcher {
            window: window.upcast(),
            apps: Default::default(),
            cycle: Cell::new(Cycle::open(0)),
            strip: Default::default(),
            opening: Cell::new(None),
            committing: Cell::new(None),
            cancel: Cell::new(CancelOpen::default()),
        });
        // Fallback when the compositor forwards keys instead of running the binds.
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(&s);
        keys.connect_key_pressed(move |_, k, _, m| {
            let Some(s) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            match k {
                gdk::Key::Escape => s.window.set_visible(false),
                gdk::Key::Tab | gdk::Key::ISO_Left_Tab
                    if m.contains(gdk::ModifierType::SHIFT_MASK) || k == gdk::Key::ISO_Left_Tab =>
                {
                    s.run(Op::Prev)
                }
                gdk::Key::Tab => s.run(Op::Next),
                gdk::Key::Return => s.run(Op::Commit),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        let weak = Rc::downgrade(&s);
        keys.connect_key_released(move |_, k, _, _| {
            if matches!(k, gdk::Key::Super_L | gdk::Key::Super_R)
                && let Some(s) = weak.upgrade()
            {
                s.run(Op::Commit);
            }
        });
        s.window.add_controller(keys);
        s
    }

    /// Shell socket entry point; true when the command was the switcher's.
    pub fn command(self: &Rc<Self>, verb: crate::protocol::Verb, target: &str) -> bool {
        if !target.starts_with("switcher") {
            return false;
        }
        if let Some(r) = self.committing.get() {
            self.committing.set(Some(r.push(verb, target)));
            return true;
        }
        // Opening counts as open: a Tab or the Super release during the read is queued.
        let open = self.window.is_visible() || self.opening.get().is_some();
        let mut cancel = self.cancel.get();
        let op = cancel.filter(verb, target, open, std::time::Instant::now());
        self.cancel.set(cancel);
        self.run(op);
        true
    }

    fn run(self: &Rc<Self>, op: Op) {
        // The strip's own keys during a commit act on a strip that is going away.
        if self.committing.get().is_some() {
            return;
        }
        if let Some(mut q) = self.opening.get() {
            q.push(op);
            return self.opening.set(Some(q));
        }
        match op {
            Op::Open => self.open(Queued::default()),
            Op::Next | Op::Prev => {
                let c = self.cycle.get();
                let c = if op == Op::Next { c.next() } else { c.prev() };
                self.cycle.set(c);
                if let Some(s) = &*self.strip.borrow() {
                    s.select(c.index);
                }
            }
            Op::Commit => {
                let steps = switcher::commit(self.apps.borrow().get(self.cycle.get().index));
                self.committing.set(Some(Reopen::default()));
                let me = self.clone();
                // In order, the focus off the UI thread (a stalled compositor froze the shell).
                glib::spawn_future_local(async move {
                    for step in steps {
                        match step {
                            switcher::Step::Focus(arg) => {
                                let d = crate::hypr::Dispatch::FocusWindow(arg);
                                let _ = gtk::gio::spawn_blocking(move || crate::hypr::dispatch(&d))
                                    .await;
                            }
                            switcher::Step::Hide => me.window.set_visible(false),
                        }
                    }
                    // An ⌘Tab during the commit: a fresh strip on the new order.
                    if let Some(q) = me.committing.take().and_then(Reopen::queued) {
                        me.open(q);
                    }
                });
            }
            Op::None => {}
        }
    }

    /// Reads the client list off the UI thread, then shows the strip with `q` (keys pressed
    /// meanwhile) applied, or commits at once.
    fn open(self: &Rc<Self>, q: Queued) {
        self.opening.set(Some(q));
        let me = self.clone();
        glib::spawn_future_local(async move {
            let clients = gtk::gio::spawn_blocking(crate::hypr::read_clients)
                .await
                .ok()
                .flatten();
            let installed = super::ipc::installed().await;
            let q = me.opening.take().unwrap_or_default();
            let Some(apps) = switcher::to_show(clients.as_deref(), &installed) else {
                return; // no compositor answer or nothing open: never guess
            };
            let cycle = q.apply(Cycle::open(apps.len()));
            let strip = Strip::new(&apps, cycle.index);
            strip.animate(&Clock::of(&me.window));
            me.window.set_child(Some(&strip.root));
            *me.strip.borrow_mut() = Some(strip);
            *me.apps.borrow_mut() = apps;
            me.cycle.set(cycle);
            if q.commits() {
                me.run(Op::Commit);
            } else {
                me.window.set_visible(true);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live on the compositor 0.56: an exclusive switcher could not hand focus to the selection while
    /// mapped; on demand it can, and the refocus after its unmap lands on the selection.
    #[test]
    fn the_switcher_takes_the_keyboard_on_demand() {
        assert_eq!(SPEC.keyboard, Keyboard::OnDemand);
        assert_eq!(SPEC.namespace, "wol-panel-switcher");
        assert_eq!(SPEC.anchor, [false; 4], "centred");
    }

    /// Audit2 A2-10: the client list is read off the UI thread, so Tabs and the Super release
    /// can arrive before the strip exists; none is lost.
    #[test]
    fn keys_during_the_read_are_kept() {
        let mut q = Queued::default();
        assert_eq!(
            q.apply(Cycle::open(4)).index,
            1,
            "opens on the previous app"
        );
        q.push(Op::Next);
        q.push(Op::Next);
        assert_eq!(q.apply(Cycle::open(4)).index, 3);
        q.push(Op::Prev);
        q.push(Op::Prev);
        q.push(Op::Prev);
        assert_eq!(q.apply(Cycle::open(4)).index, 0);
        assert!(!q.commits());
        q.push(Op::Open);
        q.push(Op::None);
        assert_eq!(q.apply(Cycle::open(4)).index, 0);
        q.push(Op::Commit);
        assert!(q.commits(), "a quick tap switches without the strip");
        let mut many = Queued::default();
        for _ in 0..1000 {
            many.push(Op::Next);
        }
        assert!(many.apply(Cycle::open(3)).index < 3);
    }

    /// Audit3 R2A-3: Super released, then ⌘Tab again before the commit's hide: the second
    /// ⌘Tab opens a fresh strip after the hide; its Tab and release go to it.
    #[test]
    fn a_switch_during_the_commit_reopens_after_it() {
        use crate::protocol::Verb::*;
        let idle = Reopen::default();
        assert_eq!(
            idle.push(Hide, "switcher").queued(),
            None,
            "a stray release"
        );
        let r = idle.push(Show, "switcher");
        let mut want = Queued::default();
        assert_eq!(r.queued(), Some(want), "opens again");
        let r = r.push(Show, "switcher").push(Hide, "switcher");
        want.push(Op::Next);
        want.push(Op::Commit);
        assert_eq!(r.queued(), Some(want));
        assert!(
            r.queued().unwrap().commits(),
            "the quick tap switches again"
        );
        assert_eq!(r.queued().unwrap().apply(Cycle::open(3)).index, 2);
    }
}
