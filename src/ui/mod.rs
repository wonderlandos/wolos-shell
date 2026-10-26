//! wol-shell composition: wallpaper, menu bar, dock, notifications, OSD, clipboard, session, switcher.
pub mod control_center;
pub mod dock;
pub mod ipc;
pub mod lock;
pub mod menubar;
pub mod notify;
pub mod overview;
pub mod panels;
pub mod settings;
pub mod setup;
pub mod spotlight;
pub mod store;
pub mod switcher;

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

/// `WOL_SHELL_TIMING=1`: startup stage times on stderr.
fn mark(t0: std::time::Instant, stage: &str) {
    if std::env::var_os("WOL_SHELL_TIMING").is_some() {
        eprintln!("wol-shell: {stage} {:?}", t0.elapsed());
    }
}

pub fn run_shell() -> glib::ExitCode {
    let t0 = std::time::Instant::now();
    let app = gtk::Application::builder()
        .application_id("org.wolos.shell")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_startup(move |_| mark(t0, "startup"));
    app.connect_activate(move |app| {
        mark(t0, "activate");
        kit::style::install("");
        mark(t0, "css");
        let _ = gtk::IconTheme::for_display(&gtk::gdk::Display::default().unwrap())
            .has_icon("wolos-glyph-symbolic");
        mark(t0, "icontheme");
        let sys = crate::system::default_system();
        let panels: Panels = Default::default();
        let (bar_win, bar) = menubar::build(app, &*sys);
        mark(t0, "menubar");
        let (dock_win, dock) = dock::build(app);
        mark(t0, "dock");
        kit::snapshot_on_map(&bar_win, "menubar");
        kit::snapshot_on_map(&dock_win, "dock");
        bar_win.connect_map(move |_| {
            mark(t0, "first map");
            // WOL_SHELL_TIMING=exit: measure startup only (10-run median script).
            if std::env::var("WOL_SHELL_TIMING").as_deref() == Ok("exit") {
                std::process::exit(0);
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
            p.insert("notifications", notify::start(&app));
            p.insert("overview", overview::build(&app));
            drop(p);
            let sw = switcher::Switcher::build(&app);
            let handlers: ipc::Handlers = vec![Rc::new(move |c: &crate::protocol::Command| {
                sw.command(c.verb, &c.target)
            })];
            ipc::shell_socket(panels, handlers, osd, sys);
            apply_setup();
        });
    });
    app.run_with_args::<&str>(&[])
}

/// First session after the setup assistant (ADR 0004): its keyboard layout becomes the Hyprland
/// input fragment (unless one exists), then `setup.json` is marked applied. Auto appearance needs
/// nothing here: no theme file means Auto (`kit::style`).
fn apply_setup() {
    use wol_core::prefs;
    let home = wol_core::theme::config_home();
    let (file, done) = (
        home.join(prefs::SETUP_FILE),
        home.join(prefs::SETUP_APPLIED),
    );
    let Ok(json) = std::fs::read_to_string(&file) else {
        return;
    };
    let dir = home.join(prefs::HYPR_DIR);
    let kb = dir.join("keyboard.conf");
    if let (false, Some(conf)) = (kb.exists(), prefs::setup_keyboard(&json)) {
        let user = home.join(prefs::HYPR_USER);
        let glob = dir.join("*.conf");
        let cur = std::fs::read_to_string(&user).ok();
        let ok = std::fs::create_dir_all(&dir).is_ok() && std::fs::write(&kb, conf).is_ok();
        if let (true, Some(t)) = (
            ok,
            prefs::ensure_source(cur.as_deref(), &glob.to_string_lossy()),
        ) {
            let _ = std::fs::write(&user, t);
        }
        let _ = std::process::Command::new("hyprctl").arg("reload").spawn();
    }
    let _ = std::fs::rename(file, done);
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
