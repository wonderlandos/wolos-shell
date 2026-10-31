// SPDX-License-Identifier: GPL-2.0-or-later
//! Click outside closes the open panel on every display: Hyprland's focus grab
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
            self.grab.replace(None).inspect(|g| g.destroy());
            let _ = self.conn.flush();
            (self.on_cleared)();
        }
    }

    /// Grabs with these windows whitelisted; empty releases the grab.
    pub fn set(&self, windows: &[gtk::Window]) {
        if let Some(g) = self.grab.replace(None) {
            g.destroy();
        }
        let surfaces: Vec<WlSurface> = windows
            .iter()
            .filter(|w| w.is_mapped())
            .filter_map(|w| surface_of(&self.conn, w))
            .collect();
        if !surfaces.is_empty() {
            let g = self.manager.create_grab(&self.queue.borrow().handle(), ());
            for s in &surfaces {
                g.add_surface(s);
            }
            g.commit();
            self.grab.replace(Some(g));
        }
        let _ = self.conn.flush();
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
}
