//! System Settings (port of apps/IaraSettings): sidebar + pages. Appearance is live; other pages show state
//! read through `System` and are wired as `core` grows (see report gaps).
use crate::kit::{self, widgets as w};
use crate::system::{Sysfs, System};
use gtk::{glib, prelude::*};

pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("org.wolos.settings")
        .build();
    app.connect_activate(|app| {
        kit::style::install("");
        let win = gtk::ApplicationWindow::builder()
            .application(app)
            .title("System Settings")
            .default_width(820)
            .default_height(560)
            .build();
        win.add_css_class("settings");
        let sys = Sysfs::default();
        let stack = gtk::Stack::new();
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        stack.set_hexpand(true);
        let side = gtk::ListBox::new();
        side.add_css_class("sidebar");
        side.add_css_class("navigation-sidebar");
        for (i, (name, icon, page)) in [
            (
                "Appearance",
                "preferences-desktop-appearance-symbolic",
                appearance(),
            ),
            ("Menu Bar & Dock", "user-desktop-symbolic", menubar_dock()),
            ("Keyboard", "input-keyboard-symbolic", keyboard()),
            ("Lock & Idle", "system-lock-screen-symbolic", lock_idle()),
            ("Hardware", "computer-symbolic", hardware(&sys)),
            ("Accounts", "system-users-symbolic", accounts()),
            ("About", "help-about-symbolic", about()),
        ]
        .into_iter()
        .enumerate()
        {
            let r = w::hbox(8);
            r.append(&gtk::Image::from_icon_name(icon));
            r.append(&w::label(name, ""));
            side.append(&r);
            let p = w::vbox(12);
            p.add_css_class("page");
            p.append(&w::label(name, "page-title"));
            p.append(&page);
            stack.add_named(
                &gtk::ScrolledWindow::builder().child(&p).build(),
                Some(name),
            );
            if i == 0 {
                stack.set_visible_child_name(name);
            }
        }
        let st = stack.clone();
        let names: Vec<String> = [
            "Appearance",
            "Menu Bar & Dock",
            "Keyboard",
            "Lock & Idle",
            "Hardware",
            "Accounts",
            "About",
        ]
        .map(String::from)
        .to_vec();
        let n2 = names.clone();
        side.connect_row_selected(move |_, r| {
            if let Some(r) = r {
                st.set_visible_child_name(&n2[r.index() as usize]);
            }
        });
        // `wol-settings <Page>` (Spotlight, menu) opens that page.
        if let Some(arg) = std::env::args().nth(1) {
            if let Some(i) = names.iter().position(|n| n.eq_ignore_ascii_case(&arg)) {
                side.select_row(side.row_at_index(i as i32).as_ref());
            }
        } else {
            side.select_row(side.row_at_index(0).as_ref());
        }
        let root = w::hbox(0);
        root.append(&side);
        root.append(&stack);
        win.set_child(Some(&root));
        kit::snapshot_on_map(&win, "settings");
        win.present();
    });
    app.run_with_args::<&str>(&[])
}

fn group(rows: &[gtk::Widget]) -> gtk::Box {
    let g = w::vbox(0);
    g.add_css_class("group");
    for r in rows {
        r.add_css_class("row");
        g.append(r);
    }
    g
}

fn row(title: &str, value: &impl IsA<gtk::Widget>) -> gtk::Widget {
    let r = w::hbox(8);
    let l = w::label(title, "");
    l.set_hexpand(true);
    r.append(&l);
    r.append(value);
    r.upcast()
}

fn appearance() -> gtk::Box {
    let sel = match kit::style::active_id().as_deref() {
        Some("wol-light") => 0,
        Some("wol-dark") => 1,
        _ => 2,
    };
    let seg = w::segmented(&["Light", "Dark", "Auto"], sel, |i| {
        let _ = match i {
            0 => kit::style::set_active("wol-light"),
            1 => kit::style::set_active("wol-dark"),
            _ => std::fs::remove_file(crate::theme::active_path(&kit::config_home())),
        };
    });
    group(&[
        row("Appearance", &seg),
        row("Accent color", &w::label("WolOS Violet", "dim")),
    ])
}

fn menubar_dock() -> gtk::Box {
    group(&[
        row("Show battery percentage", &gtk::Switch::new()),
        row("Automatically hide the dock", &gtk::Switch::new()),
        row(
            "Magnification",
            &gtk::Switch::builder().active(true).build(),
        ),
    ])
}

fn keyboard() -> gtk::Box {
    group(&[
        row(
            "Input source",
            &gtk::DropDown::from_strings(&["U.S.", "Brazilian (ABNT2)", "Portuguese"]),
        ),
        row(
            "Key repeat rate",
            &gtk::Scale::with_range(gtk::Orientation::Horizontal, 1.0, 10.0, 1.0),
        ),
    ])
}

fn lock_idle() -> gtk::Box {
    group(&[
        row(
            "Turn display off when inactive",
            &gtk::DropDown::from_strings(&["2 minutes", "5 minutes", "10 minutes", "Never"]),
        ),
        row(
            "Require password after screen saver",
            &gtk::DropDown::from_strings(&["Immediately", "5 seconds", "1 minute"]),
        ),
    ])
}

fn hardware(sys: &dyn System) -> gtk::Box {
    let bat = crate::system::battery_label(sys.battery()).unwrap_or_else(|| "No battery".into());
    let net = crate::system::wifi_subtitle(sys.network(), &sys.wifi_networks());
    let br = sys
        .brightness()
        .map_or("Not adjustable".into(), |b| format!("{b}%"));
    group(&[
        row("Battery", &w::label(&bat, "dim")),
        row("Network", &w::label(&net, "dim")),
        row("Display brightness", &w::label(&br, "dim")),
    ])
}

fn accounts() -> gtk::Box {
    let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
    let shells = std::fs::read_to_string("/etc/shells").unwrap_or_default();
    let rows: Vec<gtk::Widget> = crate::login::users(&passwd, &shells)
        .iter()
        .map(|u| {
            let r = w::hbox(10);
            r.append(&w::avatar(u, 48));
            r.append(&w::label(u, "strong"));
            r.upcast()
        })
        .collect();
    group(&rows)
}

fn about() -> gtk::Box {
    let b = w::vbox(12);
    let mark = gtk::Image::from_icon_name("wolos-mark");
    mark.set_pixel_size(96);
    b.append(&mark);
    let t = w::label("WolOS", "page-title");
    t.set_xalign(0.5);
    b.append(&t);
    let v = w::label(&format!("Version {}", env!("CARGO_PKG_VERSION")), "dim");
    v.set_xalign(0.5);
    b.append(&v);
    let host = std::fs::read_to_string("/etc/hostname").unwrap_or_default();
    b.append(&group(&[
        row("Name", &w::label(host.trim(), "dim")),
        row("Base", &w::label("FreeBSD 15.1", "dim")),
    ]));
    b
}
