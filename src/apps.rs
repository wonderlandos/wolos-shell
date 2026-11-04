// SPDX-License-Identifier: GPL-2.0-or-later
//! Dock pins, running state and the menu bar's frontmost app name (port of Iara.Core.Shell.Frontmost).
use crate::desktop::Entry;

#[derive(Clone, Debug, PartialEq)]
pub struct Pin {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
    pub exec: &'static [&'static str],
    /// The window class of an app whose windows are neither its id nor its program
    /// (`StartupWMClass`: a web app's `chrome-<host>_<path>-Default`); "" for none.
    pub class: &'static str,
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
        class: "",
    },
    Pin {
        id: "wol-launcher",
        name: "Apps",
        icon: "wolos-launcher",
        exec: &["wol-search"],
        class: "",
    },
    Pin {
        id: "helium",
        name: "Browser",
        icon: "wolos-browser",
        exec: &["helium"],
        class: "",
    },
    Pin {
        id: "org.gnome.Geary",
        name: "Mail",
        icon: "wolos-mail",
        exec: &["geary"],
        class: "",
    },
    Pin {
        id: "io.bassi.Amberol",
        name: "Music",
        icon: "wolos-music",
        exec: &["amberol"],
        class: "",
    },
    Pin {
        id: "org.gnome.Shotwell",
        name: "Photos",
        icon: "wolos-photos",
        exec: &["shotwell"],
        class: "",
    },
    Pin {
        id: "org.gnome.Calendar",
        name: "Calendar",
        icon: "wolos-calendar",
        exec: &["gnome-calendar"],
        class: "",
    },
    Pin {
        id: "org.gnome.Gnote",
        name: "Notes",
        icon: "wolos-notes",
        exec: &["gnote"],
        class: "",
    },
    Pin {
        id: "foot",
        name: "Terminal",
        icon: "wolos-terminal",
        exec: &["foot"],
        class: "",
    },
    Pin {
        id: "org.wolos.store",
        name: "App Store",
        icon: "wolos-software",
        exec: &["wol-store"],
        class: "",
    },
    Pin {
        id: "org.wolos.settings",
        name: "System Settings",
        icon: "wolos-settings",
        exec: &["wol-settings"],
        class: "",
    },
];

/// The names a person knows a pinned app by besides its WolOS name: the program and the app id's
/// last part (`Files` is Nautilus, `Browser` is Helium, `Mail` is Geary). Lowercase search words
/// (QA-LN-20: Helium, Geary and Nautilus found nothing).
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
    class: "",
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
/// A pin with a class of its own (a web app) is not its program: the browser's windows are not
/// the web app's.
fn pin_classes(p: &Pin) -> impl Iterator<Item = &str> {
    let prog = p
        .exec
        .first()
        .map(|e| program(e))
        .filter(|_| p.class.is_empty());
    std::iter::once(p.id)
        .chain(prog)
        .chain((!p.class.is_empty()).then_some(p.class))
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

// ---- Apps kept in the Dock (manual webapps.md: "drag its icon to the Dock"; QA-MANUAL-5/43) ----
// An app a person keeps in the Dock is a `DockItem::App { id }` in `shell.json` (its desktop id
// without `.desktop`), placed after the other apps. The pinned apps above come from [`PINS`]; any
// other id is resolved from the installed `.desktop` entries, so a web app launcher
// (`wolos-webapp-<id>.desktop`) is kept like any installed app.

/// What a Search row hands the Dock when it is dragged: this prefix and the desktop id.
pub const DRAG_PREFIX: &str = "wolos-app:";

/// The app id a drop on the Dock names: a Search row ([`DRAG_PREFIX`]), a desktop id
/// (`x.desktop`), or a `.desktop` file (path or `file://` URI, the first line of a URI list).
/// Anything else (text, an image, a folder) is no app.
pub fn dropped_app(text: &str) -> Option<String> {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))?;
    let id = if let Some(id) = line.strip_prefix(DRAG_PREFIX) {
        id
    } else {
        let path = line.strip_prefix("file://").unwrap_or(line);
        let name = path.rsplit('/').next().unwrap_or(path);
        name.strip_suffix(".desktop")?
    };
    let ok = !id.is_empty()
        && id.len() <= 255
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    ok.then(|| id.to_string())
}

/// The ids the Dock shows now with `items`: the pinned apps where `Apps` stands, and every `App`.
fn shown_ids<'a>(items: &'a [shared::layout::DockItem], pins: &'a [Pin]) -> Vec<&'a str> {
    use shared::layout::DockItem as D;
    let mut v = Vec::new();
    for i in items {
        match i {
            D::Apps => v.extend(pins.iter().map(|p| p.id)),
            D::App { id } => v.push(id.as_str()),
            _ => {}
        }
    }
    v
}

/// `items` with the app `id` kept: a new `App` right after the last app already there (before
/// the separator, Downloads and Trash), or first when there is none. `None` when the Dock shows
/// it already (a pinned app, or kept before).
pub fn keep_in_dock(
    items: &[shared::layout::DockItem],
    pins: &[Pin],
    id: &str,
) -> Option<Vec<shared::layout::DockItem>> {
    use shared::layout::DockItem as D;
    if id.is_empty()
        || shown_ids(items, pins)
            .iter()
            .any(|s| s.eq_ignore_ascii_case(id))
    {
        return None;
    }
    let at = items
        .iter()
        .rposition(|i| matches!(i, D::Apps | D::App { .. }))
        .map_or(0, |n| n + 1);
    let mut v = items.to_vec();
    v.insert(at, D::App { id: id.into() });
    Some(v)
}

/// `items` without the kept app `id`. `None` when it is not one (the pinned apps stay: they are
/// the Dock's own, Settings > Desktop & Dock arranges them).
pub fn remove_from_dock(
    items: &[shared::layout::DockItem],
    id: &str,
) -> Option<Vec<shared::layout::DockItem>> {
    use shared::layout::DockItem as D;
    let v: Vec<D> = items
        .iter()
        .filter(|i| !matches!(i, D::App { id: x } if x == id))
        .cloned()
        .collect();
    (v.len() != items.len()).then_some(v)
}

/// The Dock pin of an installed entry: its name, icon, launcher and window class. One copy per
/// distinct entry for the life of the shell (a Dock holds a handful; [`Pin`] borrows `'static`).
pub fn entry_pin(e: &Entry) -> Pin {
    use std::collections::HashMap;
    use std::sync::Mutex;
    /// Desktop id, name, icon, command and window class: what makes one pin.
    type Key = (String, String, String, Vec<String>, String);
    static MADE: Mutex<Option<HashMap<Key, Pin>>> = Mutex::new(None);
    let key = (
        desktop_stem(&e.id).to_string(),
        e.name.clone(),
        e.icon.clone(),
        e.exec.clone(),
        e.wm_class.clone(),
    );
    let mut made = MADE.lock().unwrap_or_else(|p| p.into_inner());
    let made = made.get_or_insert_with(HashMap::new);
    if let Some(p) = made.get(&key) {
        return p.clone();
    }
    let leak = |s: &str| -> &'static str { Box::leak(s.to_string().into_boxed_str()) };
    let exec: Vec<&'static str> = e.exec.iter().map(|a| leak(a)).collect();
    let p = Pin {
        id: leak(&key.0),
        name: leak(&e.name),
        icon: leak(if e.icon.is_empty() {
            "application-x-executable"
        } else {
            &e.icon
        }),
        exec: Box::leak(exec.into_boxed_slice()),
        class: leak(&e.wm_class),
    };
    made.insert(key, p.clone());
    p
}

/// The pin of the kept app `id`: a pinned app of that id, else the installed entry of that
/// desktop id (`None` when it was uninstalled: its Dock item stays and shows again when it is
/// back).
pub fn kept_pin(id: &str, pins: &[Pin], installed: &[Entry]) -> Option<Pin> {
    if let Some(p) = pins.iter().find(|p| p.id == id) {
        return Some(p.clone());
    }
    installed
        .iter()
        .find(|e| desktop_stem(&e.id) == id)
        .map(entry_pin)
}

/// The kept apps of `items` that are not pinned apps, resolved from `installed` (what the Dock
/// needs read off the GTK thread before it builds the shelf).
pub fn kept_pins(
    items: &[shared::layout::DockItem],
    pins: &[Pin],
    installed: &[Entry],
) -> Vec<Pin> {
    use shared::layout::DockItem as D;
    items
        .iter()
        .filter_map(|i| match i {
            D::App { id } if !pins.iter().any(|p| p.id == id) => kept_pin(id, pins, installed),
            _ => None,
        })
        .collect()
}

/// A running app the Dock shows although it is neither pinned nor kept (manual apps.md: "the
/// Dock shows it with a dot while it runs"), after the kept apps.
#[derive(Clone, Debug, PartialEq)]
pub struct Running {
    pub pin: Pin,
    /// It has an installed entry, so Keep in Dock can keep it by its desktop id.
    pub keepable: bool,
}

/// The running apps of `classes` (window classes, lowercased: `hypr::running_classes`) that no
/// tile in `shown` stands for, each once, in class order: by its installed entry (name, icon,
/// launcher), else by its class in words with the class as icon name.
pub fn running_unpinned(
    classes: &std::collections::HashSet<String>,
    shown: &[Pin],
    installed: &[Entry],
) -> Vec<Running> {
    let mut sorted: Vec<&String> = classes.iter().filter(|c| !c.is_empty()).collect();
    sorted.sort();
    let mut out: Vec<Running> = Vec::new();
    for class in sorted {
        let one = std::collections::HashSet::from([class.clone()]);
        if shown.iter().any(|p| is_running(p, &one)) {
            continue;
        }
        let (pin, keepable) = match entry_for(class, installed) {
            Some(e) => (entry_pin(e), true),
            None => (
                entry_pin(&Entry {
                    id: class.clone(),
                    name: humanised_class(class),
                    icon: class.clone(),
                    ..Default::default()
                }),
                false,
            ),
        };
        if !out.iter().any(|r| r.pin.id == pin.id) {
            out.push(Running { pin, keepable });
        }
    }
    out
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
        assert_eq!(frontmost_name("helium", "t", PINS, &installed), "Browser");
        assert_eq!(pin_for_entry(&installed[0], PINS), None);
        let helium = crate::desktop::Entry {
            id: "helium.desktop".into(),
            exec: vec!["helium".into()],
            wm_class: "helium".into(),
            ..Default::default()
        };
        assert_eq!(pin_for_entry(&helium, PINS).map(|p| p.id), Some("helium"));
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
            ("Helium", "wolos-browser"),
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
            ["foot".to_string(), "helium".into()].into();
        let running: Vec<&str> = PINS
            .iter()
            .filter(|p| is_running(p, &classes))
            .map(|p| p.name)
            .collect();
        assert_eq!(running, ["Browser", "Terminal"]);
    }
    /// QA-MANUAL-5/43: a web app dragged to the Dock (or Keep in Dock in Search) is kept after
    /// the pinned apps, once, and comes off again; the pinned apps cannot be dropped twice.
    #[test]
    fn a_web_app_is_kept_in_the_dock_once_and_removed_again() {
        // A web app launcher's id (`wol_core::webapps::PREFIX`, written out it reads as an icon name).
        let mail = format!("{}mail", wol_core::webapps::PREFIX);
        use shared::layout::{DockItem as D, Layout};
        let items = Layout::default().dock_items;
        let kept = keep_in_dock(&items, PINS, &mail).unwrap();
        assert_eq!(
            kept,
            [
                D::Apps,
                D::App { id: mail.clone() },
                D::Separator,
                D::Downloads,
                D::Trash
            ]
        );
        assert_eq!(keep_in_dock(&kept, PINS, &mail), None, "once");
        assert_eq!(
            keep_in_dock(&items, PINS, "org.gnome.Nautilus"),
            None,
            "Files is pinned"
        );
        let two = keep_in_dock(&kept, PINS, "org.gnome.Maps").unwrap();
        assert_eq!(
            two[2],
            D::App {
                id: "org.gnome.Maps".into()
            },
            "after the last app"
        );
        assert_eq!(remove_from_dock(&two, &mail).unwrap()[1], two[2]);
        assert_eq!(remove_from_dock(&items, "org.gnome.Nautilus"), None);
        assert_eq!(
            keep_in_dock(&[], PINS, "x").unwrap(),
            [D::App { id: "x".into() }]
        );
    }

    /// What a drop names: a Search row, a desktop id, a `.desktop` file; never other text.
    #[test]
    fn a_drop_names_an_app_only_when_it_is_one() {
        // A web app launcher's id (`wol_core::webapps::PREFIX`, written out it reads as an icon name).
        let mail = format!("{}mail", wol_core::webapps::PREFIX);
        for (text, want) in [
            ("wolos-app:wolos-webapp-mail", Some(mail.as_str())),
            ("org.gnome.Maps.desktop", Some("org.gnome.Maps")),
            (
                "file:///home/a/.local/share/applications/wolos-webapp-mail.desktop\r\n",
                Some(mail.as_str()),
            ),
            ("/usr/share/applications/foot.desktop", Some("foot")),
            ("hello", None),
            ("file:///home/a/Pictures/cat.png", None),
            ("wolos-app:../../x", None),
            ("wolos-app:", None),
            ("", None),
        ] {
            assert_eq!(dropped_app(text).as_deref(), want, "{text:?}");
        }
    }

    /// A kept web app is its launcher in the Dock: its name, icon and command, and the running
    /// dot follows its windows' own class (`StartupWMClass`), not the browser's.
    #[test]
    fn a_kept_web_app_runs_as_its_launcher() {
        // A web app launcher's id (`wol_core::webapps::PREFIX`, written out it reads as an icon name).
        let mail = format!("{}mail", wol_core::webapps::PREFIX);
        use shared::layout::{DockItem as D, Layout};
        let text = wol_core::webapps::desktop_entry(
            &wol_core::webapps::WebApp::new(
                "Mail",
                "https://mail.example.com",
                wol_core::webapps::Profile::Own,
            )
            .unwrap(),
            std::path::Path::new("/h/.local/share"),
        );
        let e = crate::desktop::parse("wolos-webapp-mail.desktop", &text).unwrap();
        let items = keep_in_dock(&Layout::default().dock_items, PINS, &mail).unwrap();
        let kept = kept_pins(&items, PINS, std::slice::from_ref(&e));
        assert_eq!(kept.len(), 1);
        let p = &kept[0];
        assert_eq!((p.id, p.name), (mail.as_str(), "Mail"));
        assert_eq!(
            p.exec,
            e.exec.iter().map(String::as_str).collect::<Vec<_>>()
        );
        assert_eq!(p.class, e.wm_class);
        let mine: std::collections::HashSet<String> = [e.wm_class.to_lowercase()].into();
        assert!(is_running(p, &mine), "its own window");
        let browser: std::collections::HashSet<String> = ["helium".to_string()].into();
        assert!(!is_running(p, &browser), "the browser is not the web app");
        assert_eq!(entry_pin(&e), *p, "one copy per entry");
        assert!(
            kept_pins(&items, PINS, &[]).is_empty(),
            "uninstalled: not shown"
        );
        assert!(
            kept_pins(&[D::App { id: "foot".into() }], PINS, &[]).is_empty(),
            "a pin is no kept app"
        );
    }
    /// QA apps.md:60: an app that runs without a Dock icon gets one after the kept apps, with
    /// its installed name and icon and Keep in Dock; one window class nothing names shows by its
    /// class in words; a pinned or kept app is never shown twice.
    #[test]
    fn running_apps_without_a_tile_get_one() {
        let calc = Entry {
            id: "org.gnome.Calculator.desktop".into(),
            name: "Calculator".into(),
            exec: vec!["gnome-calculator".into()],
            icon: "accessories-calculator".into(),
            wm_class: String::new(),
        };
        let classes: std::collections::HashSet<String> = [
            "foot".to_string(),
            "org.gnome.calculator".into(),
            "gcr-prompter".into(),
            String::new(),
        ]
        .into();
        let r = running_unpinned(&classes, PINS, std::slice::from_ref(&calc));
        let names: Vec<(&str, bool)> = r.iter().map(|x| (x.pin.name, x.keepable)).collect();
        assert_eq!(names, [("Gcr Prompter", false), ("Calculator", true)]);
        assert_eq!(r[1].pin.id, "org.gnome.Calculator");
        assert_eq!(r[1].pin.icon, "accessories-calculator");
        assert!(
            r.iter().all(|x| is_running(&x.pin, &classes)),
            "each lights its dot"
        );
        let mut shown = PINS.to_vec();
        shown.push(r[1].pin.clone());
        let again = running_unpinned(&classes, &shown, std::slice::from_ref(&calc));
        assert_eq!(again.len(), 1, "kept: not twice");
        assert!(running_unpinned(&Default::default(), PINS, &[]).is_empty());
    }
}
