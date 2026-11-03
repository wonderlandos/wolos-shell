// SPDX-License-Identifier: GPL-2.0-or-later
//! Click outside closes the open panel on every display: the compositor's focus grab
//! (`hyprland_focus_grab_v1`, protocols/hyprland-focus-grab-v1.xml). The shell names the
//! surfaces a click may land on (the open panel, menu bars, the Dock); a click anywhere else, on
//! any display, clears the grab and the panel closes. A small Wayland client on GTK's own
//! connection: GDK hands out its `wl_display` and each window's `wl_surface`. Without the protocol
//! (another compositor, the golden harness) `FocusGrab::new` is None and `ui::transient` keeps its
//! click-catcher.
use gtk::{gdk, glib, prelude::*};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    backend::{Backend, ObjectId},
    globals::{GlobalListContents, registry_queue_init},
    protocol::{wl_registry, wl_surface::WlSurface},
};

#[allow(dead_code, non_camel_case_types, clippy::all)]
mod proto {
    use wayland_client;
    use wayland_client::protocol::*;
    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/hyprland-focus-grab-v1.xml");
    }
    use self::__interfaces::*;
    wayland_scanner::generate_client_code!("protocols/hyprland-focus-grab-v1.xml");
}
use proto::{
    hyprland_focus_grab_manager_v1::HyprlandFocusGrabManagerV1 as Manager,
    hyprland_focus_grab_v1::{self as grab, HyprlandFocusGrabV1 as Grab},
};

unsafe extern "C" {
    fn gdk_wayland_display_get_wl_display(d: *mut gdk::ffi::GdkDisplay) -> *mut std::ffi::c_void;
    fn gdk_wayland_surface_get_wl_surface(s: *mut gdk::ffi::GdkSurface) -> *mut std::ffi::c_void;
}

/// What the grab holds: the surfaces a click may land on without closing the panel.
/// Empty: no grab (nothing is open).
pub fn whitelist<T: Clone>(open: &[T], keep: &[T]) -> Vec<T> {
    if open.is_empty() {
        return vec![];
    }
    open.iter().chain(keep).cloned().collect()
}

/// How [`FocusGrab::set`] moves from the surfaces it `held` to the ones `wanted`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan<T> {
    /// Same set (or nothing held and nothing wanted): no request at all.
    Keep,
    /// Nothing wanted any more: destroy the grab.
    Release,
    /// A grab is live: add and remove on it, also when another panel takes over. Never destroys
    /// it, so the compositor never refocuses the app behind (which closed the panel). The new panel has
    /// the keyboard from its own map (an on-demand layer takes focus), not from the grab's order.
    Adjust { add: Vec<T>, remove: Vec<T> },
    /// No grab yet: a fresh one with `wanted`.
    Recreate,
}

/// Live (lead, v0.12): Control Center closed itself on open. Every sync destroyed and recreated
/// the grab; the compositor gave the keyboard back to the app behind, `activewindow` fired and the shell
/// read it as "another app in front". The plan keeps a grab alive while its panel is open.
pub fn plan<T: PartialEq + Clone>(held: &[T], wanted: &[T]) -> Plan<T> {
    if wanted.is_empty() {
        return if held.is_empty() {
            Plan::Keep
        } else {
            Plan::Release
        };
    }
    if held == wanted {
        return Plan::Keep;
    }
    // Audit3 R2A-1: recreating the grab for a panel switch destroyed it (the compositor refocused the
    // window under the pointer) and the new grab, made before the new panel mapped, did not
    // accept it, so its map cleared the grab: either way the new panel closed itself.
    if held.is_empty() {
        return Plan::Recreate;
    }
    Plan::Adjust {
        add: wanted
            .iter()
            .filter(|s| !held.contains(s))
            .cloned()
            .collect(),
        remove: held
            .iter()
            .filter(|s| !wanted.contains(s))
            .cloned()
            .collect(),
    }
}

struct State {
    cleared: Rc<Cell<bool>>,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<Manager, ()> for State {
    fn event(
        _: &mut Self,
        _: &Manager,
        _: <Manager as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<Grab, ()> for State {
    fn event(
        s: &mut Self,
        _: &Grab,
        e: grab::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(e, grab::Event::Cleared) {
            s.cleared.set(true);
        }
    }
}

pub struct FocusGrab {
    conn: Connection,
    queue: RefCell<EventQueue<State>>,
    state: RefCell<State>,
    manager: Manager,
    grab: RefCell<Option<Grab>>,
    /// The surfaces the live grab whitelists, in order (first = keyboard).
    held: RefCell<Vec<WlSurface>>,
    cleared: Rc<Cell<bool>>,
    on_cleared: Rc<dyn Fn()>,
}

/// The `wl_surface` of a mapped window, on `conn`.
fn surface_of(conn: &Connection, w: &gtk::Window) -> Option<WlSurface> {
    use glib::translate::ToGlibPtr;
    let s = w.surface()?;
    if !s.type_().name().starts_with("GdkWayland") {
        return None;
    }
    let ptr = unsafe { gdk_wayland_surface_get_wl_surface(s.to_glib_none().0) };
    if ptr.is_null() {
        return None;
    }
    let id = unsafe { ObjectId::from_ptr(WlSurface::interface(), ptr.cast()) }.ok()?;
    WlSurface::from_id(conn, id).ok()
}

impl FocusGrab {
    /// None when the display is not Wayland or the compositor has no focus grab.
    pub fn new(display: &gdk::Display, on_cleared: Rc<dyn Fn()>) -> Option<Rc<FocusGrab>> {
        use glib::translate::ToGlibPtr;
        if display.type_().name() != "GdkWaylandDisplay" {
            return None;
        }
        let ptr = unsafe { gdk_wayland_display_get_wl_display(display.to_glib_none().0) };
        if ptr.is_null() {
            return None;
        }
        let backend = unsafe { Backend::from_foreign_display(ptr.cast()) };
        let conn = Connection::from_backend(backend);
        let (globals, mut queue) = registry_queue_init::<State>(&conn).ok()?;
        let qh = queue.handle();
        let manager: Manager = globals.bind(&qh, 1..=1, ()).ok()?;
        let cleared = Rc::new(Cell::new(false));
        let mut state = State {
            cleared: cleared.clone(),
        };
        queue.roundtrip(&mut state).ok()?;
        let me = Rc::new(FocusGrab {
            conn,
            queue: RefCell::new(queue),
            state: RefCell::new(state),
            manager,
            grab: RefCell::new(None),
            held: RefCell::new(Vec::new()),
            cleared,
            on_cleared,
        });
        me.watch();
        Some(me)
    }

    /// Reads and handles our events whenever the connection has some (GTK reads its own).
    fn watch(self: &Rc<Self>) {
        use std::os::fd::AsFd;
        // A duplicate of the connection's socket: closing ours never closes GTK's.
        let Ok(fd) = self.conn.backend().poll_fd().as_fd().try_clone_to_owned() else {
            return;
        };
        let Ok(sock) = gtk::gio::Socket::from_fd(fd) else {
            return;
        };
        let weak = Rc::downgrade(self);
        let source = gtk::gio::prelude::SocketExtManual::create_source(
            &sock,
            glib::IOCondition::IN,
            None::<&gtk::gio::Cancellable>,
            Some("wol-focus-grab"),
            glib::Priority::DEFAULT,
            move |_, _| {
                let Some(me) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                me.pump();
                glib::ControlFlow::Continue
            },
        );
        source.attach(None);
    }

    fn pump(&self) {
        let mut q = self.queue.borrow_mut();
        if let Some(guard) = q.prepare_read() {
            let _ = guard.read();
        }
        let _ = q.dispatch_pending(&mut self.state.borrow_mut());
        drop(q);
        if self.cleared.replace(false) {
            self.release();
            let _ = self.conn.flush();
            (self.on_cleared)();
        }
    }

    fn release(&self) {
        self.held.borrow_mut().clear();
        self.grab.replace(None).inspect(|g| g.destroy());
    }

    /// Grabs with these windows whitelisted (the mapped ones); empty releases the grab.
    /// Idempotent: the same set again sends nothing, a changed set adjusts the live grab, and a
    /// grab is only made when none is live (see [`plan`]). True when this call destroyed a grab:
    /// The compositor then hands the keyboard to a window, which is not "another app in front"
    /// (`transient::Dismisser::expect_refocus`).
    pub fn set(&self, windows: &[gtk::Window]) -> bool {
        let wanted: Vec<WlSurface> = windows
            .iter()
            .filter(|w| w.is_mapped())
            .filter_map(|w| surface_of(&self.conn, w))
            .collect();
        // A local: the borrow of `held` must end before `release` writes it.
        let p = plan(&self.held.borrow(), &wanted);
        let destroyed = matches!(p, Plan::Release);
        match p {
            Plan::Keep => return false,
            Plan::Release => self.release(),
            Plan::Adjust { add, remove } => {
                if let Some(g) = &*self.grab.borrow() {
                    for s in &remove {
                        g.remove_surface(s);
                    }
                    for s in &add {
                        g.add_surface(s);
                    }
                    g.commit();
                }
            }
            Plan::Recreate => {
                let g = self.manager.create_grab(&self.queue.borrow().handle(), ());
                for s in &wanted {
                    g.add_surface(s);
                }
                g.commit();
                self.grab.replace(Some(g));
            }
        }
        *self.held.borrow_mut() = wanted;
        let _ = self.conn.flush();
        destroyed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grab_holds_the_open_panel_and_the_bars_only_while_one_is_open() {
        assert!(
            whitelist::<u8>(&[], &[1, 2]).is_empty(),
            "nothing open: no grab"
        );
        assert_eq!(whitelist(&[9], &[1, 2]), [9, 1, 2]);
    }

    /// Opening the Control Center synced the grab twice (visible, then mapped); the second sync
    /// destroyed and recreated it, the compositor refocused the app behind and the panel closed itself.
    #[test]
    fn syncing_the_same_surfaces_again_touches_nothing() {
        let panel_and_bars = [9, 1, 2];
        assert_eq!(plan(&panel_and_bars, &panel_and_bars), Plan::<u8>::Keep);
        assert_eq!(
            plan::<u8>(&[], &[]),
            Plan::Keep,
            "nothing open, nothing held"
        );
    }

    #[test]
    fn a_grab_is_created_once_adjusted_while_open_and_released_on_close() {
        assert_eq!(plan::<u8>(&[], &[9, 1, 2]), Plan::Recreate, "first open");
        assert_eq!(
            plan(&[9, 1, 2], &[9, 1, 2, 3]),
            Plan::Adjust {
                add: vec![3],
                remove: vec![]
            },
            "a display (menu bar) plugged in while open"
        );
        assert_eq!(
            plan(&[9, 1, 2], &[9, 2]),
            Plan::Adjust {
                add: vec![],
                remove: vec![1]
            }
        );
        assert_eq!(plan(&[9, 1, 2], &[]), Plan::<u8>::Release, "panel closed");
    }

    /// Audit3 R2A-1: another panel takes over (Control Center -> Notification Center). The live
    /// grab swaps the panels; destroying it refocused the window under the pointer and the new
    /// panel closed itself.
    #[test]
    fn switching_panels_adjusts_the_live_grab() {
        assert_eq!(
            plan(&[9, 1, 2], &[8, 1, 2]),
            Plan::Adjust {
                add: vec![8],
                remove: vec![9]
            }
        );
        assert_eq!(
            plan(&[9, 1, 2], &[1, 2, 8]),
            Plan::Adjust {
                add: vec![8],
                remove: vec![9]
            },
            "order does not matter"
        );
    }
}
