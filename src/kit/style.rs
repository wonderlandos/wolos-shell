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
    let path = theme::active_path();
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
    theme::save(id)
}

pub fn active_id() -> Option<String> {
    theme::saved()
}

const PRODUCT_ICONS: &str = "WolOS";

/// The shell always draws the product icon theme (Adwaita in dev), never the host user's
/// (`~/.config/gtk-4.0/settings.ini`): same look everywhere and a large theme such as
/// breeze costs ~100 ms of startup.
fn icon_theme(dirs: &[&str], exists: impl Fn(&str) -> bool) -> &'static str {
    if dirs.iter().any(|d| exists(&format!("{d}/{PRODUCT_ICONS}"))) {
        PRODUCT_ICONS
    } else {
        "Adwaita"
    }
}

/// WolOS icons: installed share dir, else the repo's design/ tree (dev).
fn icons(display: &gdk::Display) {
    let theme = gtk::IconTheme::for_display(display);
    let dirs: Vec<String> = theme
        .search_path()
        .iter()
        .filter_map(|p| p.to_str().map(str::to_string))
        .collect();
    let dirs: Vec<&str> = dirs.iter().map(String::as_str).collect();
    let name = icon_theme(&dirs, |p| {
        std::path::Path::new(p).join("index.theme").is_file()
    });
    gtk::Settings::for_display(display).set_gtk_icon_theme_name(Some(name));
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

thread_local! { static FIXED: std::cell::OnceCell<gtk::CssProvider> = const { std::cell::OnceCell::new() }; }

/// Loads one theme, no file watch (golden renders).
pub fn apply(t: &theme::Theme, extra: &str) {
    let display = gdk::Display::default().expect("display");
    FIXED.with(|c| {
        let p = c.get_or_init(|| {
            let p = gtk::CssProvider::new();
            gtk::style_context_add_provider_for_display(
                &display,
                &p,
                gtk::STYLE_PROVIDER_PRIORITY_USER,
            );
            icons(&display);
            p
        });
        p.load_from_string(&theme::stylesheet(t, extra));
    });
    if let Some(s) = gtk::Settings::default() {
        s.set_gtk_application_prefer_dark_theme(t.variant == theme::Variant::Dark);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_icon_theme_is_the_product_theme() {
        let dirs = |have: &'static [&'static str]| move |d: &str| have.contains(&d);
        assert_eq!(icon_theme(&["/a", "/b"], dirs(&["/b/WolOS"])), "WolOS");
        assert_eq!(icon_theme(&["/a"], dirs(&[])), "Adwaita");
    }
}
