//! System Settings (port of apps/IaraSettings) on the HIG AppWindow: sidebar of pages, grouped Section cards.
//! Appearance is live; pages read state through `System`; writes land as `core` exposes them.
use crate::kit::{self, widgets as w};
use crate::system::System;
use gtk::{glib, prelude::*};

pub const PAGES: [&str; 7] = [
    "Appearance",
    "Menu Bar & Dock",
    "Keyboard",
    "Lock & Idle",
    "Hardware",
    "Accounts",
    "About",
];

pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("org.wolos.settings")
        .build();
    app.connect_activate(|app| {
        kit::style::install("");
        let win = gtk::ApplicationWindow::builder()
            .application(app)
            .title("System Settings")
            .default_width(w::APP_SIZE.0)
            .default_height(w::APP_SIZE.1)
            .decorated(false)
            .build();
        let aw = content(&*crate::system::default_system(), &Host::read());
        if let Some(p) = std::env::args().nth(1) {
            aw.select(&p);
        }
        win.set_child(Some(&aw.root));
        kit::snapshot_on_map(&win, "settings");
        win.present();
    });
    app.run_with_args::<&str>(&[])
}

/// Host facts shown by Accounts/About (injected so golden renders do not depend on the build machine).
pub struct Host {
    pub hostname: String,
    pub users: Vec<String>,
}

impl Host {
    pub fn read() -> Self {
        let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
        let shells = std::fs::read_to_string("/etc/shells").unwrap_or_default();
        Host {
            hostname: std::fs::read_to_string("/etc/hostname")
                .unwrap_or_default()
                .trim()
                .to_string(),
            users: crate::login::users(&passwd, &shells),
        }
    }
}

pub fn content(sys: &dyn System, host: &Host) -> w::AppWindow {
    let aw = w::AppWindow::new("System Settings", true, false);
    aw.source_section("Personal");
    aw.source_item(
        "preferences-desktop-appearance-symbolic",
        "Appearance",
        &appearance(),
    );
    aw.source_item("user-desktop-symbolic", "Menu Bar & Dock", &menubar_dock());
    aw.source_item("input-keyboard-symbolic", "Keyboard", &keyboard());
    aw.source_item("system-lock-screen-symbolic", "Lock & Idle", &lock_idle());
    aw.source_section("System");
    aw.source_item("computer-symbolic", "Hardware", &hardware(sys));
    aw.source_item("system-users-symbolic", "Accounts", &accounts(host));
    aw.source_item("help-about-symbolic", "About", &about(host));
    aw
}

fn row(title: &str, control: &impl IsA<gtk::Widget>) -> gtk::Widget {
    w::list_row("", title, "", Some(control.upcast_ref())).upcast()
}

fn appearance() -> gtk::Box {
    let p = w::page("Appearance");
    let sel = match kit::style::active_id().as_deref() {
        Some("wol-light") => 0,
        Some("wol-dark") => 1,
        _ => 2,
    };
    let seg = w::segmented(&["Light", "Dark", "Auto"], sel, |i| {
        let _ = match i {
            0 => kit::style::set_active("wol-light"),
            1 => kit::style::set_active("wol-dark"),
            _ => std::fs::remove_file(crate::theme::active_path()),
        };
    });
    p.append(&w::section(
        "",
        &[
            row("Appearance", &seg),
            row("Accent color", &w::label("Theme default", "secondary")),
        ],
    ));
    p
}

fn menubar_dock() -> gtk::Box {
    let p = w::page("Menu Bar & Dock");
    p.append(&w::section(
        "Menu Bar",
        &[
            row("Show battery percentage", &w::toggle(false, |_| {})),
            row("Show date", &w::toggle(true, |_| {})),
        ],
    ));
    p.append(&w::section(
        "Dock",
        &[
            row(
                "Automatically hide and show the Dock",
                &w::toggle(false, |_| {}),
            ),
            row("Magnification", &w::toggle(true, |_| {})),
        ],
    ));
    p
}

fn keyboard() -> gtk::Box {
    let p = w::page("Keyboard");
    p.append(&w::section(
        "",
        &[
            row(
                "Input source",
                &w::dropdown(&["U.S.", "Brazilian (ABNT2)", "Portuguese"], 0),
            ),
            row(
                "Key repeat rate",
                &w::dropdown(&["Slow", "Normal", "Fast"], 1),
            ),
        ],
    ));
    p
}

fn lock_idle() -> gtk::Box {
    let p = w::page("Lock & Idle");
    p.append(&w::section(
        "",
        &[
            row(
                "Turn display off when inactive",
                &w::dropdown(&["2 minutes", "5 minutes", "10 minutes", "Never"], 1),
            ),
            row(
                "Require password after sleep",
                &w::dropdown(&["Immediately", "After 1 minute", "After 5 minutes"], 0),
            ),
        ],
    ));
    p
}

fn hardware(sys: &dyn System) -> gtk::Box {
    let p = w::page("Hardware");
    let bat = crate::system::battery_label(sys.battery()).unwrap_or_else(|| "No battery".into());
    let net = crate::system::wifi_subtitle(sys.network(), &sys.wifi_networks());
    let br = sys
        .brightness()
        .map_or("Not adjustable".into(), |b| format!("{b}%"));
    p.append(&w::section(
        "",
        &[
            row("Battery", &w::label(&bat, "secondary")),
            row("Network", &w::label(&net, "secondary")),
            row("Display brightness", &w::label(&br, "secondary")),
        ],
    ));
    p
}

fn accounts(host: &Host) -> gtk::Box {
    let p = w::page("Accounts");
    let users = &host.users;
    if users.is_empty() {
        p.append(&w::empty_state(
            "system-users-symbolic",
            "No accounts",
            "Login users appear here.",
            None,
        ));
        return p;
    }
    let rows: Vec<gtk::Widget> = users
        .iter()
        .map(|u| {
            let r = w::list_row("", u, "", None);
            r.prepend(&w::avatar(u, w::ICON_M));
            r.upcast()
        })
        .collect();
    p.append(&w::section("", &rows));
    p
}

fn about(host: &Host) -> gtk::Box {
    let p = w::vbox(w::S3);
    p.add_css_class("page");
    let mark = w::app_icon("wolos-mark", w::ICON_XL);
    mark.set_halign(gtk::Align::Center);
    p.append(&mark);
    p.append(&w::centered("WolOS", "large-title"));
    p.append(&w::centered(
        &format!("Version {}", env!("CARGO_PKG_VERSION")),
        "secondary",
    ));
    p.append(&w::section(
        "",
        &[
            row("Name", &w::label(&host.hostname, "secondary")),
            row("Base", &w::label("FreeBSD 15.1", "secondary")),
        ],
    ));
    p
}
