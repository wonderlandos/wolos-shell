// SPDX-License-Identifier: GPL-2.0-or-later
//! QA-MANUAL-5/43 (manual webapps.md: "drag its icon to the Dock"): an app dropped on the Dock is
//! kept in `shell.json` after the pinned apps, the shelf shows it with Remove from Dock, and it
//! comes off again. Needs the toolkit: runs on a private Broadway display (the golden harness's
//! `gtk4-broadwayd`), and skips without one.
use gtk::prelude::*;
use shared::layout::{DockItem, Edge};
use shell::{apps, ui::dock};
use std::path::Path;

/// Kills the private broadwayd on every exit path.
struct Kill(std::process::Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A free Broadway display (the golden harness's scheme, another port range), or None.
fn broadway() -> Option<Kill> {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let n = (0..200)
        .map(|i| 240 + (std::process::id() + i) % 200)
        .find(|n| {
            !Path::new(&runtime)
                .join(format!("broadway{}.socket", n + 1))
                .exists()
                && std::net::TcpListener::bind(("127.0.0.1", (18000 + n) as u16)).is_ok()
        })?;
    let display = format!(":{n}");
    let child = std::process::Command::new("gtk4-broadwayd")
        .args([display.as_str(), "--port", &(18000 + n).to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let kill = Kill(child);
    let sock = Path::new(&runtime).join(format!("broadway{}.socket", n + 1));
    for _ in 0..100 {
        if std::os::unix::net::UnixStream::connect(&sock).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    unsafe {
        std::env::set_var("GDK_BACKEND", "broadway");
        std::env::set_var("BROADWAY_DISPLAY", &display);
    }
    Some(kill)
}

fn kept(config: &Path) -> Vec<DockItem> {
    let text = std::fs::read_to_string(config.join(wol_core::prefs::SHELL_FILE)).ok();
    wol_core::prefs::layout(text.as_deref()).dock_items
}

fn now(config: &Path) -> Option<String> {
    std::fs::read_to_string(config.join(wol_core::prefs::SHELL_FILE)).ok()
}

fn until(mut done: impl FnMut() -> bool) -> bool {
    let ctx = gtk::glib::MainContext::default();
    let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < end {
        if done() {
            return true;
        }
        ctx.iteration(false);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    done()
}

#[test]
fn a_dropped_web_app_is_kept_in_the_dock_and_removed_again() {
    let config = std::env::temp_dir().join(format!("wol-dock-keep-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&config);
    std::fs::create_dir_all(&config).unwrap();
    unsafe { std::env::set_var("XDG_CONFIG_HOME", &config) };
    let _display = broadway();
    if gtk::init().is_err() {
        eprintln!("skipped: no display for the toolkit");
        return;
    }
    let web = DockItem::App {
        id: "wolos-webapp-mail".into(),
    };

    // Dropping a Search row's id on the shelf keeps it: once, after the apps.
    let d = dock::content(&Default::default());
    let target = d
        .root
        .observe_controllers()
        .into_iter()
        .filter_map(Result::ok)
        .find_map(|c| c.downcast::<gtk::DropTarget>().ok())
        .expect("the shelf takes drops");
    let types = target.types();
    assert!(types.contains(&String::static_type()), "{types:?}");
    let dropped = format!("{}wolos-webapp-mail", apps::DRAG_PREFIX).to_value();
    assert!(dock::dropped(&dropped));
    assert!(
        until(|| kept(&config).contains(&web)),
        "shell.json keeps it: {:?}",
        kept(&config)
    );
    let items = kept(&config);
    assert_eq!(items[0], DockItem::Apps);
    assert_eq!(items[1], web, "right after the pinned apps");
    let text = "hello".to_value();
    assert!(!dock::dropped(&text), "text is no app");
    let file = gtk::gdk::FileList::from_array(&[gtk::gio::File::for_path(
        "/home/a/.local/share/applications/org.gnome.Maps.desktop",
    )])
    .to_value();
    assert!(dock::dropped(&file), "a .desktop file from Files");
    assert!(until(|| kept(&config).contains(&DockItem::App {
        id: "org.gnome.Maps".into()
    })));
    assert!(dock::unkeep(now(&config), "org.gnome.Maps").unwrap());
    assert!(
        !dock::keep(now(&config), "wolos-webapp-mail").unwrap(),
        "kept once"
    );
    let undo = wol_core::journal::Journal::for_config(&config);
    assert!(
        !undo.entries().is_empty(),
        "journaled like a Settings change"
    );

    // The shelf shows the kept web app as its launcher, with its own menu.
    let app = wol_core::webapps::WebApp::new(
        "Fastmail",
        "https://mail.example.com",
        wol_core::webapps::Profile::Own,
    )
    .unwrap();
    let text = wol_core::webapps::desktop_entry(&app, Path::new("/h/.local/share"));
    let e = shell::desktop::parse("wolos-webapp-mail.desktop", &text).unwrap();
    let d = dock::content_with(&Default::default(), &items, Edge::Bottom);
    assert!(d.set_kept(apps::kept_pins(
        &items,
        apps::here(),
        std::slice::from_ref(&e)
    )));
    d.refill();
    let pins = d.pins();
    let at = pins
        .iter()
        .position(|p| p.id == "wolos-webapp-mail")
        .expect("shown");
    assert_eq!(at, apps::here().len(), "after the pinned apps");
    assert_eq!(pins[at].name, "Fastmail");
    let mut tiles = vec![];
    let mut c = d.root.first_child();
    while let Some(w) = c {
        if w.tooltip_text().as_deref() == Some("Fastmail") {
            tiles.push(w.clone());
        }
        c = w.next_sibling();
    }
    assert_eq!(tiles.len(), 1, "one tile");
    let menus = tiles[0]
        .observe_controllers()
        .into_iter()
        .filter_map(Result::ok)
        .filter_map(|c| c.downcast::<gtk::GestureClick>().ok())
        .filter(|g| g.button() == gtk::gdk::BUTTON_SECONDARY)
        .count();
    assert_eq!(menus, 1, "right-click: Remove from Dock");

    // Remove from Dock: shell.json drops it, the pinned apps stay.
    assert!(dock::unkeep(now(&config), "wolos-webapp-mail").unwrap());
    assert!(!kept(&config).contains(&web));
    assert!(kept(&config).contains(&DockItem::Apps));
    assert!(
        !dock::unkeep(now(&config), "org.gnome.Nautilus").unwrap(),
        "a pinned app stays"
    );
    let _ = std::fs::remove_dir_all(&config);
}
