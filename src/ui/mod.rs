//! wol-shell composition: wallpaper, menu bar, dock, notifications, OSD, clipboard, session, switcher.
pub mod app_host;
pub mod app_view;
pub mod approval;
pub mod capture;
pub mod control_center;
pub mod displays;
pub mod dock;
pub mod drop_shelf;
pub mod flatpak_consent;
pub mod force_quit;
pub mod installer;
pub mod ipc;
pub mod lock;
pub mod lua_widgets;
pub mod menubar;
pub mod notify;
pub mod overview;
pub mod panels;
pub mod settings;
pub mod setup;
pub mod shortcut_sheet;
pub mod spotlight;
pub mod store;
pub mod switcher;
pub mod transient;
pub mod widgets;
pub mod widgets_builtin;
pub mod widgets_gallery;
pub mod widgets_layout;

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
    surface::terminal_clipboard_keys(&w);
    w
}

/// `WOL_SHELL_TIMING=1`: startup stage times on stderr, plus CLOCK_MONOTONIC ms (`at`) so a
/// session-start script can measure from login (design/tools/perf/session_start.py).
fn mark(t0: std::time::Instant, stage: &str) {
    if std::env::var_os("WOL_SHELL_TIMING").is_some() {
        let at = glib::monotonic_time() as f64 / 1000.0;
        eprintln!("wol-shell: {stage} {:?} at {at:.1}", t0.elapsed());
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
        // The main display's surfaces go there from the first frame (no remap later).
        let main = displays::monitors().into_iter().next().map(|m| m.1);
        let (bar_win, bar) = menubar::build(app, &*sys, main.as_ref(), true);
        mark(t0, "menubar");
        let (dock_win, dock, dock_keys) = dock::build(app);
        kit::surface::set_monitor(&dock_win, main.as_ref());
        let bar_keys = menubar::keyboard(&bar_win, &bar.root);
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
        dock_win.connect_map(move |_| mark(t0, "dock map"));
        bar_win.present();
        dock_win.present();
        // Everything below is off the first-frame path.
        let app = app.clone();
        glib::idle_add_local_once(move || {
            // Desktop widgets (ADR 0010): above the wallpaper, below windows, main display.
            let (desk, board) = widgets::build(&app, sys.clone());
            kit::surface::set_monitor(&desk, main.as_ref());
            // A wallpaper on every display, a menu bar on the others; follows hotplug.
            let shown = std::cell::Cell::new(false);
            let screens = displays::Displays::start(
                &app,
                sys.clone(),
                bar_win.clone(),
                bar.clone(),
                vec![dock_win.clone().upcast(), desk.clone().upcast()],
                Box::new(move || {
                    if !shown.replace(true) {
                        mark(t0, "wallpaper map");
                        kit::reveal::boot_mark("desktop shown");
                    }
                }),
            );
            // Lives for the session.
            std::mem::forget(screens);
            desk.present();
            let osd = panels::Osd::build(&app);
            menubar::poll_status(sys.clone());
            ipc::hypr_events(dock, panels.clone());
            let mut p = panels.borrow_mut();
            p.insert("osd", osd.window.clone());
            p.insert("clipboard", panels::clipboard(&app));
            p.insert("session", panels::session(&app));
            let (center, dismiss) = notify::start(&app);
            // App windows and app notifications through libwol (ADR 0020).
            app_host::listen();
            crate::app_notices::start();
            p.insert("notifications", center);
            p.insert("overview", overview::build(&app));
            p.insert("control-center", control_center::panel(&app, sys.clone()));
            p.insert("force-quit", force_quit::build(&app));
            p.insert("shortcuts", shortcut_sheet::build(&app));
            p.insert("shelf", drop_shelf::build(&app));
            let (shot, record) = capture::build(&app);
            p.insert("screenshot", shot);
            drop(p);
            {
                let b = board.clone();
                transient::also_close(Rc::new(move || b.set_edit(false)));
                let p = panels.clone();
                board.on_edit(Rc::new(move || {
                    transient::close_panels(&p);
                    transient::close_apps();
                }));
            }
            transient::manage(&app, &panels);
            transient::close_on_press(&dock_win, &panels);
            transient::close_on_menus(&bar_win, &panels);
            let sw = switcher::Switcher::build(&app);
            let handlers: ipc::Handlers = vec![
                Rc::new(move |c: &crate::protocol::Command| sw.command(c.verb, &c.target)),
                record,
                Rc::new(move |c: &crate::protocol::Command| board.command(c)),
                keyboard_handler(bar_keys, dock_keys, dismiss),
            ];
            let approver = approval::handler(&app, sys.clone());
            ipc::shell_socket(panels, handlers, approver, osd, sys);
            spotlight::keep_file_index();
            crate::phones::watch();
            apply_setup();
        });
    });
    app.run_with_args::<&str>(&[])
}

/// `wol shell show menubar|dock` (⌃F2, ⌃F3) moves the keyboard into that bar, `hide` gives it
/// back; `wol shell hide banners` (Esc) dismisses every notification banner.
fn keyboard_handler(
    bar: Rc<dyn Fn(bool)>,
    dock: Rc<dyn Fn(bool)>,
    dismiss: Rc<dyn Fn()>,
) -> Rc<dyn Fn(&crate::protocol::Command) -> bool> {
    use crate::protocol::Verb;
    Rc::new(move |c: &crate::protocol::Command| {
        let take = matches!(c.verb, Verb::Show | Verb::Toggle);
        match (c.target.as_str(), c.verb) {
            ("menubar", Verb::Show | Verb::Toggle | Verb::Hide) => bar(take),
            ("dock", Verb::Show | Verb::Toggle | Verb::Hide) => dock(take),
            ("banners", Verb::Hide | Verb::Toggle) => dismiss(),
            _ => return false,
        }
        true
    })
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

/// One-window panel app (Control Center, Spotlight): Esc, a click outside, another app in front
/// or a workspace switch quits.
pub fn run_panel_app(
    id: &str,
    class: &'static str,
    spec: surface::Spec,
    build: fn(&gtk::ApplicationWindow, Rc<dyn crate::system::System>) -> gtk::Widget,
) -> glib::ExitCode {
    let t0 = std::time::Instant::now();
    let app = gtk::Application::builder().application_id(id).build();
    app.connect_startup(move |_| mark(t0, "startup"));
    // `hide` over D-Bus: a shell panel opened (`transient::close_outside_members`).
    let hide = gtk::gio::SimpleAction::new("hide", None);
    {
        let app = app.downgrade();
        hide.connect_activate(move |_, _| {
            if let Some(app) = app.upgrade() {
                app.windows().iter().for_each(|w| w.close());
            }
        });
    }
    app.add_action(&hide);
    app.connect_activate(move |app| {
        mark(t0, "activate");
        // Second launch toggles: the running instance closes.
        if !app.windows().is_empty() {
            app.windows().iter().for_each(|w| w.close());
            return;
        }
        kit::style::install("");
        mark(t0, "css");
        let w = window(app, class, spec);
        w.set_child(Some(&build(&w, crate::system::default_system())));
        mark(t0, "built");
        w.connect_map(move |_| {
            mark(t0, "first map");
            if std::env::var("WOL_SHELL_TIMING").as_deref() == Ok("exit") {
                std::process::exit(0);
            }
        });
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
        let w3 = w.clone();
        transient::close_outside(app, &w, Rc::new(move || w3.close()));
        w.present();
    });
    app.run_with_args::<&str>(&[])
}
