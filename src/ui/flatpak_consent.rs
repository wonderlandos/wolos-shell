//! wol-flatpak-consent: Flatpak asks before an app gets its permissions
//! (`$FLATPAK_WOLOS_CONSENT_HELPER install|update <app-id> user|system -- <perm>...`). One glass
//! sheet with the app's name and icon and each permission in plain words; Allow exits 0,
//! Don't Allow (or closing it) exits 1. Flatpak records the grants itself.
use crate::kit::{self, widgets as w};
use gtk::{glib, prelude::*};
use std::cell::Cell;
use std::rc::Rc;
use wol_core::flatpak::{self, Key, Perm, Request, Scope};

/// What the sheet shows.
pub struct Info {
    pub name: String,
    /// An icon file (`.png`/`.svg`) or an icon name.
    pub icon: String,
    pub update: bool,
    pub perms: Vec<Perm>,
}

pub fn title(i: &Info) -> String {
    if i.update {
        format!("“{}” now also wants to:", i.name)
    } else {
        format!("“{}” would like to:", i.name)
    }
}

pub fn detail(i: &Info) -> &'static str {
    if i.update {
        "The update asks for more than before. You can change this later in Settings > Privacy & Security."
    } else {
        "You can change this later in Settings > Privacy & Security."
    }
}

pub fn perm_icon(p: &Perm) -> &'static str {
    match (p.key, p.value.as_str()) {
        (Key::Filesystems, _) => "folder-symbolic",
        (Key::Shared, "network") => "network-wireless-symbolic",
        (Key::Sockets, "pulseaudio") => "audio-speakers-symbolic",
        (Key::Sockets, "cups") => "printer-symbolic",
        (Key::Sockets, _) => "video-display-symbolic",
        (Key::Devices, "all") => "camera-photo-symbolic",
        (Key::Devices, _) => "computer-symbolic",
        (Key::Features, "bluetooth") => "bluetooth-active-symbolic",
        _ => "security-high-symbolic",
    }
}

/// Name and icon of the app from its exported desktop file (an update), else from its id.
pub fn lookup(req: &Request, home: &str) -> (String, String) {
    let dir = match req.scope {
        Scope::User => format!("{home}/{}", flatpak::USER_DIR),
        Scope::System => flatpak::SYSTEM_DIR.to_owned(),
    };
    let (desktop, icon) = flatpak::export_paths(&dir, &req.app_id);
    let name = std::fs::read_to_string(desktop)
        .ok()
        .and_then(|t| flatpak::desktop_name(&t))
        .unwrap_or_else(|| {
            req.app_id
                .rsplit('.')
                .next()
                .unwrap_or(&req.app_id)
                .to_owned()
        });
    let icon = if std::path::Path::new(&icon).exists() {
        icon
    } else {
        req.app_id.clone()
    };
    (name, icon)
}

fn app_image(icon: &str) -> gtk::Image {
    if icon.starts_with('/') {
        let i = gtk::Image::from_file(icon);
        i.set_pixel_size(w::ICON_L);
        i
    } else if gtk::IconTheme::for_display(&gtk::gdk::Display::default().expect("display"))
        .has_icon(icon)
    {
        w::app_icon(icon, w::ICON_L)
    } else {
        w::app_icon(w::PACKAGE_ICON, w::ICON_L)
    }
}

/// The sheet: (root, Allow, Don't Allow).
pub fn content(i: &Info) -> (gtk::Widget, gtk::Button, gtk::Button) {
    let root = w::glass_panel(0);
    root.add_css_class("approval");
    root.set_width_request(w::CONTROL_CENTER_WIDTH + w::S8);
    let b = w::vbox(w::S3);
    let head = w::hbox(w::S3);
    head.append(&app_image(&i.icon));
    let t = w::vbox(0);
    t.set_hexpand(true);
    t.set_valign(gtk::Align::Center);
    let tl = w::label(&title(i), "title");
    tl.set_wrap(true);
    t.append(&tl);
    let why = w::label(detail(i), "secondary");
    why.set_wrap(true);
    t.append(&why);
    head.append(&t);
    b.append(&head);
    let rows: Vec<gtk::Widget> = i
        .perms
        .iter()
        .map(|p| w::list_row(perm_icon(p), &p.plain(), "", None).upcast())
        .collect();
    b.append(&w::section("", &rows));
    let row = w::hbox(w::S2);
    row.append(&w::spacer());
    let deny = w::button("Don’t Allow", w::Kind::Default, || {});
    let allow = w::button("Allow", w::Kind::Primary, || {});
    row.append(&deny);
    row.append(&allow);
    b.append(&row);
    let sheet = w::sheet(&b);
    sheet.set_reveal_child(true);
    root.append(&sheet);
    (root.upcast(), allow, deny)
}

/// An X11-only app renders 1:1 through XWayland: on a HiDPI screen it gets its own scale
/// (this account's override, which also covers a system install). Best effort.
fn scale_x11(req: &Request) {
    if !flatpak::x11_only(&req.perms) {
        return;
    }
    let t = std::time::Duration::from_secs(3);
    let scale = wol_core::system::run_with_timeout(&["hyprctl", "monitors", "-j"], t)
        .map_or(1.0, |j| flatpak::max_monitor_scale(&j));
    if let Some(argv) = flatpak::x11_scale_argv(&req.app_id, scale) {
        let a: Vec<&str> = argv.iter().map(String::as_str).collect();
        let _ = wol_core::system::run_with_timeout(&a, t);
    }
}

/// Exit 0 when allowed. Nothing to ask (no permission it can name) allows without a sheet.
pub fn run() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(req) = Request::parse(&args) else {
        eprintln!("wol-flatpak-consent: unexpected arguments: {args:?}");
        return glib::ExitCode::FAILURE;
    };
    // X11 is a display need, not access: granted without asking (Settings shows it as Display).
    let ask = flatpak::to_ask(&req.perms);
    if ask.is_empty() {
        scale_x11(&req);
        return glib::ExitCode::SUCCESS;
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let (name, icon) = lookup(&req, &home);
    let info = Rc::new(Info {
        name,
        icon,
        update: req.update,
        perms: ask,
    });
    let allowed = Rc::new(Cell::new(false));
    let app = gtk::Application::builder()
        .application_id("org.wolos.flatpak-consent")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    {
        let allowed = allowed.clone();
        app.connect_activate(move |app| {
            kit::style::install("");
            let win = gtk::ApplicationWindow::builder()
                .application(app)
                .title("Allow")
                .decorated(false)
                .resizable(false)
                .build();
            let (root, allow, deny) = content(&info);
            win.set_child(Some(&root));
            let (a, wn) = (allowed.clone(), win.clone());
            allow.connect_clicked(move |_| {
                a.set(true);
                wn.close();
            });
            let wn = win.clone();
            deny.connect_clicked(move |_| wn.close());
            let keys = gtk::EventControllerKey::new();
            let wn = win.clone();
            keys.connect_key_pressed(move |_, key, _, _| {
                if key == gtk::gdk::Key::Escape {
                    wn.close();
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            });
            win.add_controller(keys);
            win.present();
        });
    }
    app.run_with_args::<&str>(&[]);
    if allowed.get() {
        scale_x11(&req);
        glib::ExitCode::SUCCESS
    } else {
        glib::ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(update: bool) -> Info {
        Info {
            name: "GIMP".into(),
            icon: String::new(),
            update,
            perms: vec![Perm::parse("filesystems=home").unwrap()],
        }
    }

    #[test]
    fn sheet_text() {
        assert_eq!(title(&info(false)), "“GIMP” would like to:");
        assert_eq!(title(&info(true)), "“GIMP” now also wants to:");
        assert!(detail(&info(true)).starts_with("The update asks for more"));
        assert_eq!(perm_icon(&info(false).perms[0]), "folder-symbolic");
    }

    #[test]
    fn name_falls_back_to_the_id() {
        let a = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let r = Request::parse(&a(&["install", "org.gimp.GIMP", "user", "--"])).unwrap();
        assert_eq!(
            lookup(&r, "/nonexistent"),
            ("GIMP".to_owned(), "org.gimp.GIMP".to_owned())
        );
    }
}
