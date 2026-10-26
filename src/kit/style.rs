//! One CssProvider per process. Loaded once; reloaded only when the active theme file or dark preference changes.
use crate::perf::Changed;
use crate::theme;
use gtk::{gdk, gio, glib, prelude::*};
use std::cell::RefCell;
use std::rc::Rc;

/// Installs the theme + `extra` CSS and watches `$XDG_CONFIG_HOME/wol/theme` (inotify/kqueue via GIO, no polling).
pub fn install(extra: &'static str) {
    let display = gdk::Display::default().expect("display");
    let provider = gtk::CssProvider::new();
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_USER,
    );
    icons(&display);
    let path = theme::active_path(&super::config_home());
    let last = Rc::new(RefCell::new(Changed::<String>::new()));
    let load = {
        let (provider, path, last) = (provider.clone(), path.clone(), last.clone());
        move || {
            let id = std::fs::read_to_string(&path).ok();
            let dark = gtk::Settings::default()
                .map(|s| s.is_gtk_application_prefer_dark_theme())
                .unwrap_or(false);
            let t = theme::resolve(id.as_deref(), dark);
            if last.borrow_mut().set(t.id.clone()) {
                provider.load_from_string(&theme::stylesheet(&t, extra));
                if let Some(s) = gtk::Settings::default() {
                    s.set_gtk_application_prefer_dark_theme(t.variant == theme::Variant::Dark);
                }
            }
        }
    };
    load();
    if let Ok(m) =
        gio::File::for_path(&path).monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
    {
        let load = load.clone();
        m.connect_changed(move |_, _, _, _| load());
        // Keep the monitor alive for the process lifetime.
        std::mem::forget(m);
    }
    let _ = glib::MainContext::default();
}

/// Writes the active theme id; every shell process reloads through its file monitor.
pub fn set_active(id: &str) -> std::io::Result<()> {
    let p = theme::active_path(&super::config_home());
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(p, id)
}

pub fn active_id() -> Option<String> {
    std::fs::read_to_string(theme::active_path(&super::config_home()))
        .ok()
        .map(|s| s.trim().to_string())
}

/// WolOS icons: installed share dir, else the repo's design/ tree (dev).
fn icons(display: &gdk::Display) {
    let theme = gtk::IconTheme::for_display(display);
    for p in [
        "/usr/local/share/wol/icons",
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../design/icons/apps"),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../design/brand"),
    ] {
        if std::path::Path::new(p).is_dir() {
            theme.add_search_path(p);
        }
    }
}
