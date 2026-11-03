// SPDX-License-Identifier: GPL-2.0-or-later
//! One CssProvider per process. Loaded once; reloaded only when the active theme file or dark preference changes.
//! A second one holds the user's own `kit.css` rules, above it ([`user_css`]).
use crate::perf::Changed;
use crate::theme;
use gtk::{gdk, gio, glib, prelude::*};
use std::cell::RefCell;
use std::rc::Rc;
use wol_core::theme::accent;

/// Installs the theme + `extra` CSS and watches `$XDG_CONFIG_HOME/wol/theme` (inotify/kqueue via GIO, no polling).
pub fn install(extra: &'static str) {
    // No display (lost before the theme loads): nothing to style; GTK reports it itself.
    let Some(display) = gdk::Display::default() else {
        crate::diag::say!("wol-shell: no display, theme not installed");
        return;
    };
    let provider = gtk::CssProvider::new();
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_USER,
    );
    icons(&display);
    let path = theme::active_path();
    let last = Rc::new(RefCell::new(Changed::<String>::new()));
    let motion = Rc::new(std::cell::Cell::new(None::<bool>));
    let load = {
        let (provider, path, last) = (provider.clone(), path.clone(), last.clone());
        move || {
            // Reduce motion (desktop prefs, same file as the looks): GTK animations follow.
            let reduce =
                wol_core::hyprprefs::load_desktop(std::path::Path::new(&super::config_home()))
                    .reduce_motion;
            if let (Some(on), Some(s)) = (
                super::motion::animations_after(motion.replace(Some(reduce)), reduce),
                gtk::Settings::default(),
            ) {
                s.set_gtk_enable_animations(on);
            }
            let id = std::fs::read_to_string(&path).ok();
            let id = id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from);
            // Auto: light by day, dark at night (wol_core::theme::accent).
            let dark = if accent::is_auto(id.as_deref()) {
                accent::auto_is_dark(super::now().hour)
            } else {
                gtk::Settings::default()
                    .map(|s| s.is_gtk_application_prefer_dark_theme())
                    .unwrap_or(false)
            };
            let t = theme::resolve(id.as_deref(), dark);
            let contrast = contrast_on();
            let glass = t
                .tokens
                .get("glass")
                .and_then(|g| wol_core::looks::glass_css(g, looks_glass()))
                .unwrap_or_default();
            if last.borrow_mut().set(format!(
                "{}{}{glass}",
                t.id,
                if contrast { "+contrast" } else { "" }
            )) {
                let mut css = theme::stylesheet(&t, extra);
                // After the tokens and kit: the look's glass wins over the theme's.
                css.push_str(&glass);
                if contrast {
                    css.push_str(CONTRAST_CSS);
                }
                provider.load_from_string(&css);
                if let Some(s) = gtk::Settings::default() {
                    s.set_gtk_application_prefer_dark_theme(t.variant == theme::Variant::Dark);
                }
            }
        }
    };
    load();
    // An id unknown at the first draw (a theme pack) is read off-thread; draw again when it lands
    // (audit6 S6-2).
    theme::on_refreshed(Rc::new(load.clone()));
    auto_timer(Rc::new(load.clone()));
    // The active-theme file and the installed themes' sources (plugin state, theme folder) may
    // bring back an id missed earlier: forget the misses so it is looked for again (audit7 R7-4).
    let [plugins, themes] = theme::installed_sources();
    // (file, a folder, an installed-themes source)
    for (p, folder, installed) in [
        (path.clone(), false, true),
        (plugins, false, true),
        (themes, true, true),
        (contrast_path(), false, false),
        (looks_path(), false, false),
    ] {
        let f = gio::File::for_path(&p);
        let m = if folder {
            f.monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
        } else {
            f.monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
        };
        if let Ok(m) = m {
            let load = load.clone();
            m.connect_changed(move |_, _, _, _| {
                if installed {
                    theme::forget_missed();
                }
                load();
            });
            // Keep the monitor alive for the process lifetime.
            std::mem::forget(m);
        }
    }
    // The user's own rules, over the kit (live: `ui::ipc::watch_prefs`); read off the GTK thread.
    let user = std::path::Path::new(&super::config_home()).join(USER_CSS_FILE);
    glib::spawn_future_local(async move {
        let text = gio::spawn_blocking(move || std::fs::read_to_string(user).unwrap_or_default())
            .await
            .unwrap_or_default();
        if let Err(e) = user_css(&text) {
            crate::diag::say!("wol-shell: {e} (the user style sheet is left out)");
        }
    });
    let _ = glib::MainContext::default();
}

/// `$XDG_CONFIG_HOME/wolos/kit.css`: the user's own rules for the shell's look.
pub const USER_CSS_FILE: &str = "wolos/kit.css";
/// The file name in notices and logs.
pub const USER_CSS_NAME: &str = "kit.css";
/// Just above the kit and theme provider (`STYLE_PROVIDER_PRIORITY_USER`): a user rule as
/// specific as the kit's wins; nothing else in the shell sits higher.
pub const USER_CSS_PRIORITY: u32 = gtk::STYLE_PROVIDER_PRIORITY_USER + 1;

thread_local! { static USER_CSS: std::cell::OnceCell<gtk::CssProvider> = const { std::cell::OnceCell::new() }; }

/// Loads `text` (what [`USER_CSS_FILE`] holds, empty when there is none) into the user's own
/// provider ([`USER_CSS_PRIORITY`]). Text with a mistake changes nothing (the last good rules
/// stay) and the error names the first bad property and its line. Ok without a display.
pub fn user_css(text: &str) -> Result<(), wol_core::prefs::ConfigError> {
    let Some(display) = gdk::Display::default() else {
        return Ok(());
    };
    check_css(text)?;
    USER_CSS.with(|c| {
        c.get_or_init(|| {
            let p = gtk::CssProvider::new();
            gtk::style_context_add_provider_for_display(&display, &p, USER_CSS_PRIORITY);
            p
        })
        .load_from_string(text);
    });
    Ok(())
}

/// The user rules now in force (empty before the first [`user_css`]).
pub fn user_css_text() -> String {
    USER_CSS.with(|c| c.get().map(|p| p.to_str().to_string()).unwrap_or_default())
}

/// The first error GTK's parser reports in `css` (warnings such as a deprecated property do not
/// count): its line (1-based) and the name it quotes, when it quotes one (`No property named
/// "colr"`). Needs GTK initialised.
pub fn check_css(css: &str) -> Result<(), wol_core::prefs::ConfigError> {
    let scratch = gtk::CssProvider::new();
    let first: Rc<RefCell<Option<wol_core::prefs::ConfigError>>> = Rc::default();
    let f = first.clone();
    scratch.connect_parsing_error(move |_, section, err| {
        if f.borrow().is_some() || err.kind::<gtk::CssParserError>().is_none() {
            return;
        }
        let detail = err.message().to_string();
        *f.borrow_mut() = Some(wol_core::prefs::ConfigError {
            file: USER_CSS_NAME,
            key: quoted(&detail),
            line: Some(section.start_location().lines() + 1),
            detail,
        });
    });
    scratch.load_from_string(css);
    first.take().map_or(Ok(()), Err)
}

/// The first `"..."` in a parser message.
fn quoted(msg: &str) -> Option<String> {
    let (_, rest) = msg.split_once('"')?;
    let (name, _) = rest.split_once('"')?;
    (!name.is_empty()).then(|| name.to_string())
}

/// Re-evaluates the theme at each light/dark switch time (only changes anything in Auto).
fn auto_timer(load: Rc<dyn Fn()>) {
    let t = super::now();
    let secs = accent::secs_to_next_switch(t.hour, t.minute, t.second).max(1);
    glib::timeout_add_seconds_local_once(secs, move || {
        load();
        auto_timer(load);
    });
}

/// Writes the active theme id; every shell process reloads through its file monitor.
/// Third-party apps (GTK, Qt, foot) follow on a worker thread: files + gsettings (live dark mode).
pub fn set_active(id: &str) -> std::io::Result<()> {
    theme::save(id)?;
    let id = id.to_string();
    std::thread::spawn(move || {
        let home = wol_core::theme::config_home();
        if let Err(e) = wol_core::theme::apps::apply(&id, &home) {
            crate::diag::say!("wol: app themes: {e}");
        }
    });
    Ok(())
}

/// Increase Contrast flag, relative to `$XDG_CONFIG_HOME`; every shell process reloads on change.
pub const CONTRAST_FILE: &str = "wol/contrast";
/// Token overrides appended to the kit stylesheet while the flag exists.
pub const CONTRAST_CSS: &str = include_str!("contrast.css");

/// Settings > Effects & Looks prefs (`glass` multiplies the theme's glass alpha).
fn looks_path() -> std::path::PathBuf {
    std::path::Path::new(&super::config_home()).join(wol_core::hyprprefs::PREFS_FILE)
}

fn looks_glass() -> f32 {
    wol_core::hyprprefs::load_desktop(std::path::Path::new(&super::config_home()))
        .looks
        .glass
}

fn contrast_path() -> std::path::PathBuf {
    std::path::Path::new(&super::config_home()).join(CONTRAST_FILE)
}

pub fn contrast_on() -> bool {
    contrast_path().exists()
}

pub fn set_contrast(on: bool) -> std::io::Result<()> {
    let p = contrast_path();
    if on {
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(p, "")
    } else {
        match std::fs::remove_file(p) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
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

/// Repo checkout of the icon set (dev, goldens): design/icons laid out as a theme named WolOS in
/// a per-checkout temp dir (links to design/icons/*, index.theme = design/icons/dev.theme from
/// design/tools/icons.py), so the shell draws our symbolics instead of Adwaita's of the same name.
/// None on an installed system (/usr/share/icons/WolOS) or without the design/ tree.
fn dev_icon_root() -> Option<std::path::PathBuf> {
    use std::path::Path;
    let icons = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../design/icons"));
    let index = icons.join("dev.theme");
    if Path::new("/usr/share/icons/WolOS/index.theme").is_file() || !index.is_file() {
        return None;
    }
    let key = icons
        .to_string_lossy()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
        });
    let root = std::env::temp_dir().join(format!("wolos-dev-icons-{key:016x}"));
    let theme = root.join(PRODUCT_ICONS);
    std::fs::create_dir_all(&theme).ok()?;
    for d in ["symbolic", "apps", "places", "devices", "mimetypes"] {
        let link = theme.join(d);
        if !link.exists() {
            let _ = std::fs::remove_file(&link);
            #[cfg(unix)]
            let _ = std::os::unix::fs::symlink(icons.join(d), &link);
        }
    }
    std::fs::copy(&index, theme.join("index.theme")).ok()?;
    Some(root)
}

/// WolOS icons: installed share dir, else the repo's design/ tree (dev).
fn icons(display: &gdk::Display) {
    let theme = gtk::IconTheme::for_display(display);
    if let Some(root) = dev_icon_root() {
        theme.add_search_path(&root);
    }
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
        "/usr/share/icons/WolOS/scalable/apps",
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../design/icons/apps"),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../design/icons/symbolic"),
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
    let Some(display) = gdk::Display::default() else {
        crate::diag::say!("wol-shell: no display, theme not applied");
        return;
    };
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
    fn a_parser_message_names_its_property() {
        assert_eq!(
            quoted(r#"No property named "colr""#).as_deref(),
            Some("colr")
        );
        assert_eq!(quoted("Expected a valid selector"), None);
        assert_eq!(quoted(r#"empty """#), None);
    }

    #[test]
    fn shell_icon_theme_is_the_product_theme() {
        let dirs = |have: &'static [&'static str]| move |d: &str| have.contains(&d);
        assert_eq!(icon_theme(&["/a", "/b"], dirs(&["/b/WolOS"])), "WolOS");
        assert_eq!(icon_theme(&["/a"], dirs(&[])), "Adwaita");
    }
}
