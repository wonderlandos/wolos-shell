// SPDX-License-Identifier: GPL-2.0-or-later
//! Dock pins, running state and the menu bar's frontmost app name (port of Iara.Core.Shell.Frontmost).
use crate::desktop::Entry;

#[derive(Clone, Debug, PartialEq)]
pub struct Pin {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
    pub exec: &'static [&'static str],
}

/// No focused window: no app name. WolOS has no resident Finder; naming Files would claim
/// an app that is not running and offer its menu.
pub const DESKTOP_NAME: &str = "";

/// Dock Trash tile: opens the trash in Files.
pub const TRASH: &[&str] = &["nautilus", "trash:///"];

/// Default dock (WolOS design icons). Apps are mature ports (desktop/packages.txt): `id` is the app's
/// Wayland app-id, which the compositor reports as the window class (running dot, menu bar name).
pub const PINS: &[Pin] = &[
    Pin {
        id: "org.gnome.Nautilus",
        name: "Files",
        icon: "wolos-files",
        exec: &["nautilus"],
    },
    Pin {
        id: "wol-launcher",
        name: "Apps",
        icon: "wolos-launcher",
        exec: &["wol-search"],
    },
    Pin {
        id: "atlas",
        name: "Browser",
        icon: "wolos-browser",
        exec: &["atlas"],
    },
    Pin {
        id: "org.gnome.Geary",
        name: "Mail",
        icon: "wolos-mail",
        exec: &["geary"],
    },
    Pin {
        id: "io.bassi.Amberol",
        name: "Music",
        icon: "wolos-music",
        exec: &["amberol"],
    },
    Pin {
        id: "org.gnome.Shotwell",
        name: "Photos",
        icon: "wolos-photos",
        exec: &["shotwell"],
    },
    Pin {
        id: "org.gnome.Calendar",
        name: "Calendar",
        icon: "wolos-calendar",
        exec: &["gnome-calendar"],
    },
    Pin {
        id: "org.gnome.Gnote",
        name: "Notes",
        icon: "wolos-notes",
        exec: &["gnote"],
    },
    Pin {
        id: "foot",
        name: "Terminal",
        icon: "wolos-terminal",
        exec: &["foot"],
    },
    Pin {
        id: "org.wolos.store",
        name: "App Store",
        icon: "wolos-software",
        exec: &["wol-store"],
    },
    Pin {
        id: "org.wolos.settings",
        name: "System Settings",
        icon: "wolos-settings",
        exec: &["wol-settings"],
    },
];

/// The pin a saved Dock layout means: a layout saved before the browser was renamed names it by
/// its old id (`wol_core::browser::LEGACY`), which is the browser pin now.
pub fn current_id(id: &str) -> &str {
    if id == wol_core::browser::LEGACY.pin {
        wol_core::browser::ATLAS.class
    } else {
        id
    }
}

/// The names a person knows a pinned app by besides its WolOS name: the program and the app id's
/// last part (`Files` is Nautilus, `Browser` is Atlas, `Mail` is Geary). Lowercase search words
/// (QA-LN-20: the browser's, Geary's and Nautilus's names found nothing).
pub fn product_words(p: &Pin) -> String {
    let prog = p.exec.first().map(|x| program(x)).unwrap_or_default();
    let tail = p.id.rsplit('.').next().unwrap_or_default();
    let mut v = vec![prog.to_lowercase()];
    if !tail.eq_ignore_ascii_case(prog) {
        v.push(tail.to_lowercase());
    }
    v.retain(|w| !w.is_empty() && w != &p.name.to_lowercase());
    v.join(" ")
}

/// Marks the live system; the installer leaves it behind.
pub const LIVE_MARK: &str = "/etc/wolos-live";

/// Install WolOS, first in the dock while trying WolOS.
pub const INSTALL: Pin = Pin {
    id: "org.wolos.installer",
    name: "Install WolOS",
    icon: "wolos-installer",
    exec: &["/usr/lib/wolos/wol-install"],
};

/// The dock of this kind of system.
pub fn pins(live: bool) -> Vec<Pin> {
    let first = live.then_some(INSTALL);
    first.into_iter().chain(PINS.iter().cloned()).collect()
}

/// The dock here (the system doesn't change kind while it runs).
pub fn here() -> &'static [Pin] {
    static HERE: std::sync::OnceLock<Vec<Pin>> = std::sync::OnceLock::new();
    HERE.get_or_init(|| pins(is_live()))
}

/// This is the live system ([`LIVE_MARK`]); read once, the system doesn't change kind while it
/// runs.
pub fn is_live() -> bool {
    static LIVE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *LIVE.get_or_init(|| std::path::Path::new(LIVE_MARK).exists())
}

fn program(exec0: &str) -> &str {
    exec0.rsplit('/').next().unwrap_or(exec0)
}

/// Window classes that belong to the pin: its id (the app-id) and its program.
fn pin_classes(p: &Pin) -> impl Iterator<Item = &str> {
    std::iter::once(p.id).chain(p.exec.first().map(|e| program(e)))
}

/// Running when a window class (lowercased by `hypr::running_classes`) is one of the pin's.
pub fn is_running(p: &Pin, classes: &std::collections::HashSet<String>) -> bool {
    pin_classes(p).any(|c| classes.contains(&c.to_lowercase()))
}

/// App-ids are mixed case (org.gnome.Nautilus); compare case-insensitively.
fn pin_for<'a>(class: &str, pins: &'a [Pin]) -> Option<&'a Pin> {
    pins.iter().find(|p| {
        pin_classes(p).any(|c| c.eq_ignore_ascii_case(class)) || p.name.eq_ignore_ascii_case(class)
    })
}

/// The pin an installed .desktop entry is (desktop id or program), so Apps lists it once,
/// under the WolOS name.
/// An entry whose windows have a class of their own (`StartupWMClass`, a web app running the
/// browser) is its own app, never the pin of the program it runs.
pub fn pin_for_entry<'a>(e: &Entry, pins: &'a [Pin]) -> Option<&'a Pin> {
    let id = desktop_stem(&e.id);
    let prog = e.exec.first().map(|x| program(x));
    pins.iter().find(|p| {
        let own_class =
            !e.wm_class.is_empty() && !pin_classes(p).any(|c| c.eq_ignore_ascii_case(&e.wm_class));
        pin_classes(p)
            .any(|c| c.eq_ignore_ascii_case(id) || (!own_class && prog.is_some_and(|x| x == c)))
    })
}

/// A desktop id without its one `.desktop` suffix: `org.telegram.desktop.desktop` is the app
/// `org.telegram.desktop` (`trim_end_matches` took both and named the window by its class).
pub fn desktop_stem(id: &str) -> &str {
    id.strip_suffix(".desktop").unwrap_or(id)
}

fn entry_for<'a>(class: &str, installed: &'a [Entry]) -> Option<&'a Entry> {
    installed.iter().find(|e| {
        desktop_stem(&e.id).eq_ignore_ascii_case(class)
            || (!e.wm_class.is_empty() && e.wm_class.eq_ignore_ascii_case(class))
            || e.exec
                .first()
                .is_some_and(|x| program(x).eq_ignore_ascii_case(class))
    })
}

/// Pin name, else installed .desktop Name, else window title, else class; empty class = desktop.
pub fn frontmost_name(class: &str, title: &str, pins: &[Pin], installed: &[Entry]) -> String {
    if class.is_empty() && title.is_empty() {
        return DESKTOP_NAME.into();
    }
    if let Some(p) = pin_for(class, pins) {
        return p.name.into();
    }
    if let Some(e) = entry_for(class, installed) {
        return e.name.clone();
    }
    if class.is_empty() {
        title.into()
    } else {
        humanised_class(class)
    }
}

/// A window class nothing installed names, as words: the reverse-DNS prefix and a trailing
/// `desktop`/`app` go, `-` and `_` become spaces, each word is capitalised
/// (`org.telegram.desktop` -> `Telegram`, `gcr-prompter` -> `Gcr Prompter`; QA-LIVE1-14). A class
/// with no word left stays as it is.
pub fn humanised_class(class: &str) -> String {
    const PREFIX: [&str; 8] = ["org", "com", "io", "net", "de", "fr", "dev", "app"];
    const SUFFIX: [&str; 3] = ["desktop", "app", "gtk"];
    let mut parts: Vec<&str> = class.split('.').filter(|p| !p.is_empty()).collect();
    if parts.len() > 1 {
        while parts.len() > 1 && SUFFIX.contains(&parts[parts.len() - 1].to_lowercase().as_str()) {
            parts.pop();
        }
        while parts.len() > 1 && PREFIX.contains(&parts[0].to_lowercase().as_str()) {
            parts.remove(0);
        }
    }
    // Several parts left (`gnome.Calendar`): the app is the last one.
    let name = parts.last().copied().unwrap_or(class);
    let words: Vec<String> = name
        .split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect())
                .unwrap_or_default()
        })
        .collect();
    if words.is_empty() {
        class.into()
    } else {
        words.join(" ")
    }
}

/// Pin icon, else the installed .desktop Icon, else the class itself (icon theme lookup).
pub fn icon_for(class: &str, pins: &[Pin], installed: &[Entry]) -> String {
    if let Some(p) = pin_for(class, pins) {
        return p.icon.into();
    }
    match entry_for(class, installed) {
        Some(e) if !e.icon.is_empty() => e.icon.clone(),
        _ => class.to_lowercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_leads_the_dock_only_while_trying_wolos() {
        let live = pins(true);
        assert_eq!(live[0], INSTALL, "first thing in the live dock");
        assert_eq!(&live[1..], PINS);
        assert_eq!(
            pins(false),
            PINS,
            "an installed system has nothing to install"
        );
    }

    /// Live-1 M7: Telegram (Flatpak, desktop id `org.telegram.desktop.desktop`, window class
    /// `org.telegram.desktop`) read "org.telegram.desktop" in the menu bar.
    #[test]
    fn an_app_id_ending_in_desktop_is_named() {
        let tg = Entry {
            id: "org.telegram.desktop.desktop".into(),
            name: "Telegram".into(),
            exec: vec!["/usr/bin/flatpak".into(), "run".into()],
            icon: "org.telegram.desktop".into(),
            wm_class: String::new(),
        };
        let installed = [tg];
        assert_eq!(
            frontmost_name("org.telegram.desktop", "Telegram", PINS, &installed),
            "Telegram"
        );
        assert_eq!(
            icon_for("org.telegram.desktop", PINS, &installed),
            "org.telegram.desktop"
        );
        assert_eq!(desktop_stem("vlc.desktop"), "vlc");
        assert_eq!(desktop_stem("vlc"), "vlc");
    }

    /// QA-LIVE1-14: the menu bar names an app by its desktop entry, else by words, never by its
    /// raw reverse-DNS class.
    #[test]
    fn app_name_from_desktop_id() {
        let entry = |id: &str, name: &str| Entry {
            id: id.into(),
            name: name.into(),
            exec: vec!["/usr/bin/x".into()],
            icon: String::new(),
            wm_class: String::new(),
        };
        let installed = [
            entry("org.telegram.desktop.desktop", "Telegram"),
            entry("org.gnome.eog.desktop", "Eye of GNOME"),
        ];
        for (class, want) in [
            ("org.telegram.desktop", "Telegram"),
            ("org.gnome.eog", "Eye of GNOME"),
            // No entry names these: words, not the id.
            ("gcr-prompter", "Gcr Prompter"),
            ("org.gnome.Calendar", "Calendar"),
            ("io.github.some_app", "Some App"),
            ("Org", "Org"),
        ] {
            assert_eq!(frontmost_name(class, "", &[], &installed), want, "{class}");
        }
    }

    #[test]
    fn settings_window_is_named_and_marked_running() {
        // Live: the menu bar read "org.wolos.settings" (pin id was "wol-settings", not the app id).
        let class = "org.wolos.settings";
        assert_eq!(
            frontmost_name(class, "Settings", PINS, &[]),
            "System Settings"
        );
        let settings = PINS.iter().find(|p| p.exec == ["wol-settings"]).unwrap();
        assert!(is_running(settings, &[class.to_owned()].into()));
    }

    #[test]
    fn frontmost_names() {
        let installed = [Entry {
            id: "org.gnome.Calculator.desktop".into(),
            name: "Calculator".into(),
            exec: vec!["gnome-calculator".into()],
            icon: String::new(),
            wm_class: String::new(),
        }];
        for (c, t, w) in [
            ("", "", ""), // no focused window: no app name (no resident Finder)
            ("foot", "~", "Terminal"),
            ("org.gnome.Calculator", "x", "Calculator"),
            ("gnome-calculator", "", "Calculator"),
            ("", "xterm", "xterm"),
            ("weird", "T", "Weird"), // words, not the raw class (QA-LIVE1-14)
        ] {
            assert_eq!(frontmost_name(c, t, PINS, &installed), w, "{c}");
        }
    }

    /// e2e D1: a web app's window is named after its launcher (`StartupWMClass`), not shown as
    /// `chrome-example.org__-Default` in the menu bar.
    #[test]
    fn web_app_windows_take_their_launchers_name() {
        let text = wol_core::webapps::desktop_entry(
            &wol_core::webapps::WebApp::new(
                "E2EDemo",
                "https://example.org",
                wol_core::webapps::Profile::Own,
            )
            .unwrap(),
            std::path::Path::new("/h/.local/share"),
        );
        let e = crate::desktop::parse("wolos-webapp-e2edemo.desktop", &text).unwrap();
        assert_eq!(e.wm_class, "chrome-example.org__-Default");
        let installed = [e];
        let class = "chrome-example.org__-Default";
        assert_eq!(
            frontmost_name(class, "Example Domain", PINS, &installed),
            "E2EDemo"
        );
        assert_eq!(icon_for(class, PINS, &installed), "web-browser");
        // The browser itself stays the Browser pin, and the web app stays its own app in Apps.
        assert_eq!(frontmost_name("atlas", "t", PINS, &installed), "Browser");
        assert_eq!(pin_for_entry(&installed[0], PINS), None);
        let atlas = crate::desktop::Entry {
            id: "atlas.desktop".into(),
            exec: vec!["atlas".into()],
            wm_class: "atlas".into(),
            ..Default::default()
        };
        assert_eq!(pin_for_entry(&atlas, PINS).map(|p| p.id), Some("atlas"));
    }

    #[test]
    fn a_layout_saved_before_the_rename_keeps_its_browser_pin() {
        let old = wol_core::browser::LEGACY.pin;
        let pin = PINS.iter().find(|p| p.id == current_id(old)).unwrap();
        assert_eq!(pin.name, "Browser");
        assert_eq!(current_id("org.gnome.Nautilus"), "org.gnome.Nautilus");
        assert!(PINS.iter().all(|p| p.id != old));
    }

    #[test]
    fn icons_by_class() {
        let installed = [Entry {
            id: "org.gnome.Calculator.desktop".into(),
            name: "Calculator".into(),
            exec: vec!["gnome-calculator".into()],
            icon: "accessories-calculator".into(),
            wm_class: String::new(),
        }];
        for (c, w) in [
            ("foot", "wolos-terminal"),
            ("Atlas", "wolos-browser"),
            ("gnome-calculator", "accessories-calculator"),
            ("weird", "weird"),
            ("Mail", "wolos-mail"),
        ] {
            assert_eq!(icon_for(c, PINS, &installed), w, "{c}");
        }
    }

    #[test]
    fn running_by_class() {
        let classes: std::collections::HashSet<String> =
            ["foot".to_string(), "atlas".into()].into();
        let running: Vec<&str> = PINS
            .iter()
            .filter(|p| is_running(p, &classes))
            .map(|p| p.name)
            .collect();
        assert_eq!(running, ["Browser", "Terminal"]);
    }
}
