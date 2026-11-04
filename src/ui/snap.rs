// SPDX-License-Identifier: GPL-2.0-or-later
//! Window snapping surfaces (the lines come from wol-hypr, `crate::snap`): the drop preview, an
//! accent-tinted rounded box over the place a dragged window will take, on a surface that takes no
//! input; and the snap layouts chooser, a small glass popup with the halves and the quarters
//! that opens under a window's zoom button after a rest on it.
use crate::kit::motion::Duration as Speed;
use crate::kit::surface::{Keyboard, Spec};
use crate::kit::widgets as w;
use crate::protocol::{Command, Verb};
use crate::snap::{self, Line};
use gtk::{glib, prelude::*};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The preview covers its whole output (no exclusive zone to respect), so the frame wol-hypr sends
/// in output coordinates is the box's place. Input-less (see [`no_input`]).
pub const PREVIEW: Spec = Spec::panel("wol-snap-preview", [true; 4], Keyboard::None);
/// The chooser, placed by pixels from the output's top left. A `wol-panel-*` layer: glass, and
/// wol-hypr forwards Esc to close it (`hide transient`).
pub const CHOOSER: Spec = Spec::panel(
    "wol-panel-snaplayouts",
    [true, false, true, false],
    Keyboard::None,
);

/// Thumbnail of one layout (halves, quarters): px.
const THUMB: (i32, i32) = (96, 64);
/// Gap between a thumbnail's cells (px).
const CELL_GAP: i32 = 3;
/// Gap between the screen edge and the chooser (px).
const MARGIN: i32 = 8;
/// A chooser the pointer never reached goes away after this (ms).
const IDLE_MS: u32 = 1800;
/// The pointer left the chooser: it goes away after this (ms).
const LEAVE_MS: u32 = 220;

/// An empty input region: clicks and pointer motion go to the windows below.
fn no_input(win: &impl IsA<gtk::Window>) {
    win.as_ref().connect_realize(|win| {
        if let Some(s) = win.surface() {
            s.set_input_region(Some(&gtk::cairo::Region::create()));
        }
    });
}

pub struct Snap {
    preview: gtk::ApplicationWindow,
    frame: gtk::Fixed,
    boxed: gtk::Box,
    shown_on: RefCell<String>,
    chooser: gtk::ApplicationWindow,
    /// The window the chooser snaps (its address) while it is up.
    target: RefCell<Option<String>>,
    inside: Cell<bool>,
    /// Bumped on every show and hide: a timer from an earlier one does nothing.
    epoch: Cell<u64>,
}

fn monitor(output: &str) -> Option<gtk::gdk::Monitor> {
    super::displays::monitors()
        .into_iter()
        .find(|(name, _)| name == output)
        .map(|(_, m)| m)
}

impl Snap {
    pub fn build(app: &gtk::Application) -> Rc<Snap> {
        let preview = super::window(app, "snap", PREVIEW);
        crate::kit::surface::survive_output_loss(&preview, PREVIEW);
        no_input(&preview);
        let frame = gtk::Fixed::new();
        let boxed = preview_box(1, 1);
        frame.put(&boxed, 0.0, 0.0);
        preview.set_child(Some(&frame));
        super::transient::fade_on_map(preview.upcast_ref(), Speed::Fast);

        let chooser = super::window(app, "panel", CHOOSER);
        crate::kit::surface::survive_output_loss(&chooser, CHOOSER);
        super::transient::fade_on_map(chooser.upcast_ref(), Speed::Fast);
        let me = Rc::new(Snap {
            preview,
            frame,
            boxed,
            shown_on: Default::default(),
            chooser,
            target: Default::default(),
            inside: Cell::new(false),
            epoch: Cell::new(0),
        });
        let weak = Rc::downgrade(&me);
        me.chooser.set_child(Some(&layouts(move |zone| {
            if let Some(s) = weak.upgrade() {
                s.choose(zone);
            }
        })));
        // Esc (`hide transient`) and a click elsewhere take the chooser away with the other popups.
        let weak = Rc::downgrade(&me);
        super::transient::also_close(Rc::new(move || {
            if let Some(s) = weak.upgrade() {
                s.hide_chooser();
            }
        }));
        let motion = gtk::EventControllerMotion::new();
        let weak = Rc::downgrade(&me);
        motion.connect_enter(move |_, _, _| {
            if let Some(s) = weak.upgrade() {
                s.inside.set(true);
                s.epoch.set(s.epoch.get() + 1);
            }
        });
        let weak = Rc::downgrade(&me);
        motion.connect_leave(move |_| {
            if let Some(s) = weak.upgrade() {
                s.inside.set(false);
                s.hide_after(LEAVE_MS);
            }
        });
        me.chooser.add_controller(motion);
        me
    }

    /// A line from wol-hypr; true when it was one.
    pub fn command(self: &Rc<Self>, c: &Command) -> bool {
        if c.verb != Verb::Snap {
            return false;
        }
        match Line::of(&c.target) {
            Some(Line::PreviewOff) => super::transient::dismiss(&self.preview),
            Some(Line::Preview {
                output, x, y, w, h, ..
            }) => self.show_preview(&output, (x, y, w, h)),
            Some(Line::Layouts {
                output,
                address,
                x,
                y,
            }) => self.show_chooser(&output, address, (x, y)),
            None => {}
        }
        true
    }

    fn show_preview(&self, output: &str, (x, y, w, h): (i32, i32, i32, i32)) {
        if *self.shown_on.borrow() != output {
            crate::kit::surface::set_monitor(&self.preview, monitor(output).as_ref());
            *self.shown_on.borrow_mut() = output.into();
        }
        self.boxed.set_size_request(w, h);
        self.frame.move_(&self.boxed, f64::from(x), f64::from(y));
        self.preview.set_visible(true);
        // A drop shown again while it fades out keeps (turns around) instead of hiding.
        if let Some(p) = super::transient::presence(self.preview.upcast_ref()) {
            p.keep();
        }
    }

    fn show_chooser(self: &Rc<Self>, output: &str, address: String, button: (i32, i32)) {
        let screen = monitor(output).map_or((1280, 800), |m| {
            (m.geometry().width(), m.geometry().height())
        });
        let (_, nat, _, _) = self.chooser.measure(gtk::Orientation::Horizontal, -1);
        let (_, nat_h, _, _) = self.chooser.measure(gtk::Orientation::Vertical, -1);
        let (x, y) = snap::chooser_at(screen, button, (nat, nat_h), MARGIN);
        crate::kit::surface::set_monitor(&self.chooser, monitor(output).as_ref());
        crate::kit::surface::set_origin(&self.chooser, x, y);
        *self.target.borrow_mut() = Some(address);
        self.inside.set(false);
        self.epoch.set(self.epoch.get() + 1);
        self.chooser.set_visible(true);
        self.hide_after(IDLE_MS);
    }

    /// Hides the chooser in `ms` unless the pointer is in it by then or it was shown again.
    fn hide_after(self: &Rc<Self>, ms: u32) {
        let at = self.epoch.get() + 1;
        self.epoch.set(at);
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(std::time::Duration::from_millis(u64::from(ms)), move || {
            if let Some(s) = weak.upgrade()
                && s.epoch.get() == at
                && !s.inside.get()
            {
                s.hide_chooser();
            }
        });
    }

    fn hide_chooser(&self) {
        self.epoch.set(self.epoch.get() + 1);
        self.target.borrow_mut().take();
        super::transient::dismiss(&self.chooser);
    }

    /// A place was clicked: the window snaps there (compositor socket, off the UI thread).
    fn choose(self: &Rc<Self>, zone: &str) {
        let address = self.target.borrow().clone();
        self.hide_chooser();
        let Some(req) = address.and_then(|a| snap::request(zone, &a)) else {
            return;
        };
        glib::spawn_future_local(async move {
            let _ = gtk::gio::spawn_blocking(move || crate::hypr::request(&req)).await;
        });
    }
}

/// The chooser's content: the halves and the quarters, each place a button that calls `pick`
/// with its zone word.
pub fn layouts(pick: impl Fn(&str) + Clone + 'static) -> gtk::Box {
    let root = w::glass_panel(0);
    root.add_css_class("snap-layouts");
    let row = w::hbox(w::S3);
    root.append(&row);
    for zones in [&snap::ZONES[..2], &snap::ZONES[2..]] {
        let thumb = gtk::Fixed::new();
        thumb.add_css_class("snap-thumb");
        thumb.set_size_request(THUMB.0, THUMB.1);
        for zone in zones {
            let Some((x, y, cw, ch)) = snap::cell(zone) else {
                continue;
            };
            let b = gtk::Button::new();
            b.add_css_class("snap-cell");
            b.set_tooltip_text(Some(crate::copy::snap_zone(zone)));
            // The cell's share of the thumbnail (in halves), inset by the gap on every side.
            b.set_size_request(
                cw * THUMB.0 / 2 - 2 * CELL_GAP,
                ch * THUMB.1 / 2 - 2 * CELL_GAP,
            );
            let (pick, zone) = (pick.clone(), (*zone).to_string());
            b.connect_clicked(move |_| pick(&zone));
            thumb.put(
                &b,
                f64::from(x * THUMB.0 / 2 + CELL_GAP),
                f64::from(y * THUMB.1 / 2 + CELL_GAP),
            );
        }
        row.append(&thumb);
    }
    root
}

/// The drop preview's box, `w` x `h` px.
pub fn preview_box(w: i32, h: i32) -> gtk::Box {
    let b = w::vbox(0);
    b.add_css_class("snap-preview");
    b.set_size_request(w, h);
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_surfaces_are_named_for_the_compositor_rules() {
        assert_eq!(PREVIEW.namespace, "wol-snap-preview");
        assert_eq!(PREVIEW.keyboard, Keyboard::None);
        assert_eq!(PREVIEW.anchor, [true; 4], "covers its output");
        assert_eq!(
            PREVIEW.zone(),
            crate::kit::surface::Zone::Ignore,
            "frames are in output coordinates, not inside the bar and Dock"
        );
        assert!(CHOOSER.namespace.starts_with("wol-panel-"), "glass, Esc");
        assert_eq!(CHOOSER.keyboard, Keyboard::None);
    }
}
