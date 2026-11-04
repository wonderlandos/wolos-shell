// SPDX-License-Identifier: GPL-2.0-or-later
//! wol-shell composition: wallpaper, menu bar, dock, notifications, OSD, clipboard, session, switcher.
pub mod analyze;
pub mod app_host;
pub mod app_view;
pub mod approval;
pub mod capture;
pub mod clipboard;
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
pub mod maker;
pub mod menubar;
pub mod notify;
pub mod overview;
pub mod panels;
pub mod plugins;
pub mod searchbar;
pub mod settings;
pub mod setup;
pub mod shortcut_sheet;
pub mod store;
pub mod switcher;
pub mod theme_picker;
pub mod transient;
pub mod tray_items;
pub mod widgets;
pub mod widgets_builtin;
pub mod widgets_gallery;
pub mod widgets_layout;

use crate::kit::{self, fence, surface};
use gtk::{glib, prelude::*};
use std::cell::{Cell, RefCell};
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
    if crate::envv::is_set("WOL_SHELL_TIMING") {
        let at = glib::monotonic_time() as f64 / 1000.0;
        eprintln!("wol-shell: {stage} {:?} at {at:.1}", t0.elapsed());
    }
}

pub fn run_shell() -> glib::ExitCode {
    let t0 = std::time::Instant::now();
    // A panic names the part it hit (journal); one while a part is built stops that part only.
    fence::install_hook();
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
            if crate::envv::var("WOL_SHELL_TIMING").as_deref() == Some("exit") {
                std::process::exit(0);
            }
        });
        dock_win.connect_map(move |_| mark(t0, "dock map"));
        bar_win.present();
        dock_win.present();
        // Everything below is off the first-frame path.
        let app = app.clone();
        glib::idle_add_local_once(move || {
            // The shell socket first (SP-1): after a restart `wol shell ...` answers as soon as
            // the bar is up. Commands go to the handlers once the parts below fill them in;
            // until then a panel not built yet is simply not there.
            let osd = panels::Osd::build(&app);
            let late: Rc<RefCell<ipc::Handlers>> = Default::default();
            let to_late = late.clone();
            let handlers: ipc::Handlers = vec![Rc::new(move |c: &crate::protocol::Command| {
                to_late.borrow().iter().any(|h| h(c))
            })];
            let approver = approval::handler(&app, sys.clone());
            ipc::shell_socket(panels.clone(), handlers, approver, osd.clone(), sys.clone());
            mark(t0, "socket");
            // The rest after the socket's first requests (idle runs below the socket's priority).
            glib::idle_add_local_once(move || {
                // Each part below is fenced (kit::fence): one that panics while it is built stops
                // alone and shows a card; the bar, the Dock, the OSD and the socket are the core.
                // Desktop widgets (ADR 0010): above the wallpaper, below windows, main display.
                let desktop = fence::guard("desktop widgets", || widgets::build(&app, sys.clone()));
                let mut follow: Vec<gtk::Window> = vec![dock_win.clone().upcast()];
                if let Some((desk, _)) = &desktop {
                    kit::surface::set_monitor(desk, main.as_ref());
                    follow.push(desk.clone().upcast());
                }
                // A wallpaper on every display, a menu bar on the others; follows hotplug.
                let shown = std::cell::Cell::new(false);
                let screens = fence::guard("wallpaper", || {
                    displays::Displays::start(
                        &app,
                        sys.clone(),
                        bar_win.clone(),
                        bar.clone(),
                        follow,
                        Box::new(move || {
                            if !shown.replace(true) {
                                mark(t0, "wallpaper map");
                                kit::reveal::boot_mark("desktop shown");
                            }
                        }),
                    )
                });
                // Lives for the session.
                std::mem::forget(screens);
                let board = desktop.map(|(desk, board)| {
                    desk.present();
                    board
                });
                menubar::poll_status(sys.clone());
                ipc::hypr_events(dock, panels.clone());
                // Esc and the Super release come from wol-hypr; without it, bind them (audit3 R2A-10).
                glib::spawn_future_local(async {
                    let _ = gtk::gio::spawn_blocking(crate::hypr::ensure_key_forwarding).await;
                });
                let (record, dismiss) = build_panels(&app, &sys, &osd, &panels);
                if let Some(board) = &board {
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
                let mut handlers: ipc::Handlers =
                    vec![record, keyboard_handler(bar_keys, dock_keys, dismiss)];
                if let Some(sw) = fence::guard("switcher", || switcher::Switcher::build(&app)) {
                    handlers.insert(
                        0,
                        Rc::new(move |c: &crate::protocol::Command| sw.command(c.verb, &c.target)),
                    );
                }
                if let Some(board) = board {
                    handlers.insert(
                        handlers.len() - 1,
                        Rc::new(move |c: &crate::protocol::Command| board.command(c)),
                    );
                }
                *late.borrow_mut() = handlers;
                mark(t0, "panels");
                fence::guard("file index", searchbar::keep_file_index);
                fence::guard("phones", crate::phones::watch);
                fence::guard("setup", apply_setup);
            });
        });
    });
    app.run_with_args::<&str>(&[])
}

/// The panels `wol shell show <name>` opens, each built under its own fence (one that panics
/// stops alone and shows a card), and the parts that serve them. Returns the screenshot handler
/// and the banner dismisser for the socket's handlers.
fn build_panels(
    app: &gtk::Application,
    sys: &Rc<dyn crate::system::System>,
    osd: &Rc<panels::Osd>,
    panels: &Panels,
) -> (capture::Handler, Rc<dyn Fn()>) {
    let mut p = panels.borrow_mut();
    p.insert("osd", osd.window.clone());
    let panel = |name: &str, build: &dyn Fn() -> gtk::Window| fence::fence_window(app, name, build);
    p.insert("clipboard", panel("clipboard", &|| clipboard::panel(app)));
    p.insert("themes", panel("themes", &|| theme_picker::panel(app)));
    p.insert("session", panel("session", &|| panels::session(app)));
    let (center, dismiss) = fence::guard("notifications", || notify::start(app))
        .unwrap_or_else(|| (fence::stand_in(app, "notifications"), Rc::new(|| ())));
    // App windows and app notifications through libwol (ADR 0020).
    fence::guard("app host", app_host::listen);
    fence::guard("app notices", crate::app_notices::start);
    // Sessions (ADR 0032): offer last time's windows once, then keep the set saved.
    fence::guard("sessions", || {
        crate::sessions::start(crate::sessions::ask_by_notification)
    });
    p.insert("notifications", center);
    p.insert("overview", panel("overview", &|| overview::build(app)));
    let cc = panel("control-center", &|| {
        control_center::panel(app, sys.clone())
    });
    p.insert("control-center", cc);
    p.insert(
        "force-quit",
        panel("force-quit", &|| force_quit::build(app)),
    );
    p.insert(
        "shortcuts",
        panel("shortcuts", &|| shortcut_sheet::build(app)),
    );
    p.insert("shelf", panel("shelf", &|| drop_shelf::build(app)));
    let (shot, record) = fence::guard("screenshot", || capture::build(app)).unwrap_or_else(|| {
        let none: capture::Handler = Rc::new(|_| false);
        (fence::stand_in(app, "screenshot"), none)
    });
    p.insert("screenshot", shot);
    drop(p);
    (record, dismiss)
}

/// `wol shell show menubar|dock` (⌃F2, ⌃F3) moves the keyboard into that bar, `hide` gives it
/// back; `wol shell hide banners` and `hide transient` (Esc) dismiss every notification banner.
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
            // Esc (`hide transient`, forwarded by wol-hypr, or the fallback bind without it) also
            // dismisses the banners; not handled here, so the transient set still closes.
            (transient::GROUP, Verb::Hide) => {
                dismiss();
                return false;
            }
            _ => return false,
        }
        true
    })
}

/// First session after the setup assistant (ADR 0004): its keyboard layout becomes the compositor
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
    let kb = home.join(prefs::KEYBOARD_FILE);
    if let (false, Some(lua)) = (kb.exists(), prefs::setup_keyboard(&json)) {
        // Atomic writes, and the user hyprland.lua under the journal lock: the theme switch and
        // the upgrade carry edit it at the start of the same session, and none may lose another's
        // line (a lost system `require` is no desktop at the next login).
        let ok = std::fs::create_dir_all(&dir).is_ok()
            && shared::atomic::atomic_write(&kb, 0o644, lua.as_bytes()).is_ok();
        if ok && let Err(e) = wol_core::hyprprefs::edit_user_lua(&home, prefs::ensure_require) {
            crate::diag::say!("wol-shell: {}: {e}", prefs::HYPR_USER);
        }
        let _ = std::process::Command::new("hyprctl").arg("reload").spawn();
    }
    let _ = std::fs::rename(file, done);
}

/// `wol-search --warm`: the process stays up with no window, started by the session
/// (`wolos-search.service`), so a press of Super+Space only activates it over D-Bus: the display,
/// the portal registry and the theme were paid for at login, not on the press (QA-LN-3: 0.4-0.9 s
/// and the first letters lost).
pub const WARM_ARG: &str = "--warm";

/// What an activation of a panel app asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activation {
    /// The warm process's own start: theme in, no window.
    Park,
    /// The panel is open: a second press closes it.
    Close,
    /// Build and show the panel.
    Show,
}

/// The activation for: the warm process's first one (its own start), and whether a window is open.
pub fn activation(first_of_warm: bool, window_open: bool) -> Activation {
    if first_of_warm {
        Activation::Park
    } else if window_open {
        Activation::Close
    } else {
        Activation::Show
    }
}

/// One-window panel app (Control Center, Search): Esc, a click outside, another app in front
/// or a workspace switch quits. `warm` (the binary was given [`WARM_ARG`]): it does not quit, the
/// app is held with no window and each activation shows the panel again.
pub fn run_panel_app(
    id: &str,
    class: &'static str,
    spec: surface::Spec,
    warm: bool,
    build: fn(&gtk::ApplicationWindow, Rc<dyn crate::system::System>) -> gtk::Widget,
) -> glib::ExitCode {
    let t0 = std::time::Instant::now();
    let app = gtk::Application::builder().application_id(id).build();
    app.connect_startup(move |_| mark(t0, "startup"));
    // Held until the process ends: closing the panel's window does not end a warm process.
    let _hold = warm.then(|| app.hold());
    let (parked, css_in) = (Rc::new(Cell::new(warm)), Rc::new(Cell::new(false)));
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
        // A warm process times each press from the press, not from the process start.
        let t0 = if css_in.get() {
            std::time::Instant::now()
        } else {
            t0
        };
        mark(t0, "activate");
        // Second launch toggles: the running instance closes.
        match activation(parked.replace(false), !app.windows().is_empty()) {
            Activation::Close => {
                app.windows().iter().for_each(|w| w.close());
                return;
            }
            Activation::Park => {
                kit::style::install("");
                css_in.set(true);
                mark(t0, "parked");
                return;
            }
            Activation::Show => {}
        }
        if !css_in.replace(true) {
            kit::style::install("");
        }
        mark(t0, "css");
        let w = window(app, class, spec);
        w.set_child(Some(&build(&w, crate::system::default_system())));
        mark(t0, "built");
        w.connect_map(move |_| {
            mark(t0, "first map");
            if crate::envv::var("WOL_SHELL_TIMING").as_deref() == Some("exit") {
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
        hide_shell_panels();
        w.present();
    });
    app.run_with_args::<&str>(&[])
}

/// A panel app opening closes the shell's own transient panels first (one thing open at a time,
/// as a shell panel opening closes Search). Nothing happens without a running shell.
fn hide_shell_panels() {
    use std::io::Write;
    let path = crate::protocol::socket_path(&kit::runtime_dir());
    if let Ok(mut s) = std::os::unix::net::UnixStream::connect(path) {
        let _ = s.write_all(format!("hide {}\n", transient::GROUP).as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// QA-LN-3: the warm process parks on its own start and shows on every later activation; an
    /// open panel closes on the next press (the toggle).
    #[test]
    fn warm_process_parks_then_shows_every_time() {
        assert_eq!(activation(true, false), Activation::Park);
        assert_eq!(activation(false, false), Activation::Show);
        assert_eq!(activation(false, true), Activation::Close);
        // The panel closed (Esc, a launch): the next press shows it again, not a dead toggle.
        let mut open = false;
        let mut seen = Vec::new();
        for _ in 0..4 {
            let a = activation(false, open);
            open = a == Activation::Show;
            seen.push(a);
        }
        assert_eq!(
            seen,
            [
                Activation::Show,
                Activation::Close,
                Activation::Show,
                Activation::Close
            ]
        );
    }

    /// The unit that keeps Search warm restarts it, ends with the session, is wanted by the
    /// session target and has a memory ceiling above the idle budget.
    #[test]
    fn the_warm_search_unit_restarts_and_belongs_to_the_session() {
        let u = include_str!("../../../../files/usr/lib/systemd/user/wolos-search.service");
        let lines: Vec<&str> = u.lines().map(str::trim).collect();
        for want in [
            "Restart=on-failure",
            "PartOf=wolos-session.target",
            "ExecStart=/usr/bin/wol-search --warm",
        ] {
            assert!(lines.contains(&want), "{want}: {u}");
        }
        assert!(
            lines.iter().any(|l| l.starts_with("MemoryMax=")),
            "a ceiling: {u}"
        );
        let target = include_str!("../../../../files/usr/lib/systemd/user/wolos-session.target");
        assert!(target.contains("wolos-search.service"), "{target}");
        assert_eq!(WARM_ARG, "--warm");
    }
}
