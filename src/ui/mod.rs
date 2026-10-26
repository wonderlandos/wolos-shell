//! wol-shell composition: wallpaper, menu bar, dock, notifications, OSD, clipboard, session, switcher.
pub mod control_center;
pub mod dock;
pub mod ipc;
pub mod lock;
pub mod menubar;
pub mod notify;
pub mod panels;
pub mod settings;
pub mod spotlight;

use crate::kit::{self, surface};
use gtk::{glib, prelude::*};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Panels toggled over the shell socket, by name.
pub type Panels = Rc<RefCell<HashMap<&'static str, gtk::Window>>>;

pub fn window(app: &gtk::Application, class: &str, spec: surface::Spec) -> gtk::ApplicationWindow {
    let w = gtk::ApplicationWindow::new(app);
    w.add_css_class(class);
    surface::place(&w, spec);
    w
}

pub fn run_shell() -> glib::ExitCode {
    let t0 = std::time::Instant::now();
    let app = gtk::Application::builder()
        .application_id("org.wolos.shell")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| {
        kit::style::install("");
        let sys = crate::system::default_system();
        let panels: Panels = Default::default();
        let (bar_win, bar) = menubar::build(app, &*sys);
        let (dock_win, dock) = dock::build(app);
        kit::snapshot_on_map(&bar_win, "menubar");
        kit::snapshot_on_map(&dock_win, "dock");
        bar_win.connect_map(move |_| {
            if std::env::var_os("WOL_SHELL_TIMING").is_some() {
                eprintln!("wol-shell: first map {:?}", t0.elapsed());
            }
        });
        bar_win.present();
        dock_win.present();
        // Everything below is off the first-frame path.
        let app = app.clone();
        glib::idle_add_local_once(move || {
            panels::wallpaper(&app).present();
            let osd = panels::Osd::build(&app);
            menubar::poll_status(bar.clone(), sys.clone(), osd.clone());
            ipc::hypr_events(bar, dock);
            let mut p = panels.borrow_mut();
            p.insert("osd", osd.window.clone());
            p.insert("clipboard", panels::clipboard(&app));
            p.insert("session", panels::session(&app));
            p.insert("switcher", panels::switcher(&app));
            p.insert("notifications", notify::start(&app));
            drop(p);
            ipc::shell_socket(panels, osd, sys);
        });
    });
    app.run_with_args::<&str>(&[])
}

/// One-window panel app (Control Center, Spotlight): Esc or focus loss quits.
pub fn run_panel_app(
    id: &str,
    class: &'static str,
    spec: surface::Spec,
    build: fn(&gtk::ApplicationWindow, Rc<dyn crate::system::System>) -> gtk::Widget,
) -> glib::ExitCode {
    let app = gtk::Application::builder().application_id(id).build();
    app.connect_activate(move |app| {
        // Second launch toggles: the running instance closes.
        if let Some(w) = app.active_window() {
            w.close();
            return;
        }
        kit::style::install("");
        let w = window(app, class, spec);
        w.set_child(Some(&build(&w, crate::system::default_system())));
        let keys = gtk::EventControllerKey::new();
        let w2 = w.clone();
        keys.connect_key_pressed(move |_, k, _, _| {
            if k == gtk::gdk::Key::Escape {
                w2.close();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        w.add_controller(keys);
        kit::snapshot_on_map(&w, class);
        w.present();
    });
    app.run_with_args::<&str>(&[])
}
