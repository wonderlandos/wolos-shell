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
        let tm = |s: &str| {
            if std::env::var_os("WOL_SHELL_TIMING").is_some() {
                eprintln!("{s} {:?}", t0.elapsed())
            }
        };
        tm("activate");
        kit::style::install("");
        tm("style");
        let sys: Rc<dyn crate::system::System> = Rc::new(crate::system::Sysfs::default());
        let panels: Panels = Default::default();
        panels::wallpaper(app).present();
        tm("wallpaper");
        let bar = menubar::build(app, sys.clone());
        let dock = dock::build(app);
        tm("bar+dock");
        let osd = panels::Osd::build(app);
        menubar::poll_status(bar.clone(), sys.clone(), osd.clone());
        ipc::hypr_events(bar.clone(), dock.clone());
        let mut p = panels.borrow_mut();
        p.insert("osd", osd.window.clone().upcast());
        p.insert("clipboard", panels::clipboard(app));
        p.insert("session", panels::session(app));
        p.insert("switcher", panels::switcher(app));
        p.insert("notifications", notify::start(app));
        drop(p);
        tm("panels");
        ipc::shell_socket(panels.clone(), osd, sys);
        bar.window.connect_map(move |_| {
            if std::env::var_os("WOL_SHELL_TIMING").is_some() {
                eprintln!("wol-shell: first map {:?}", t0.elapsed());
            }
        });
        kit::snapshot_on_map(&bar.window, "menubar");
        kit::snapshot_on_map(&dock.window, "dock");
        bar.window.present();
        dock.window.present();
    });
    app.run_with_args::<&str>(&[])
}

/// One-window panel app (Control Center, Spotlight): Esc or focus loss quits.
pub fn run_panel_app(
    id: &str,
    class: &'static str,
    spec: surface::Spec,
    build: fn(&gtk::ApplicationWindow) -> gtk::Widget,
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
        w.set_child(Some(&build(&w)));
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
