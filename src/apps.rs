//! Dock pins, running state and the menu bar's frontmost app name (port of Iara.Core.Shell.Frontmost).
use crate::desktop::Entry;

#[derive(Clone, Debug, PartialEq)]
pub struct Pin {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
    pub exec: &'static [&'static str],
}

pub const DESKTOP_NAME: &str = "Finder";

/// Default dock (WolOS design icons).
pub const PINS: &[Pin] = &[
    Pin {
        id: "wol-files",
        name: "Files",
        icon: "wolos-files",
        exec: &["wol-files"],
    },
    Pin {
        id: "wol-launcher",
        name: "Launchpad",
        icon: "wolos-launcher",
        exec: &["wol-spotlight"],
    },
    Pin {
        id: "firefox",
        name: "Browser",
        icon: "wolos-browser",
        exec: &["firefox"],
    },
    Pin {
        id: "wol-mail",
        name: "Mail",
        icon: "wolos-mail",
        exec: &["wol-mail"],
    },
    Pin {
        id: "wol-music",
        name: "Music",
        icon: "wolos-music",
        exec: &["wol-music"],
    },
    Pin {
        id: "wol-photos",
        name: "Photos",
        icon: "wolos-photos",
        exec: &["wol-photos"],
    },
    Pin {
        id: "wol-calendar",
        name: "Calendar",
        icon: "wolos-calendar",
        exec: &["wol-calendar"],
    },
    Pin {
        id: "wol-notes",
        name: "Notes",
        icon: "wolos-notes",
        exec: &["wol-notes"],
    },
    Pin {
        id: "foot",
        name: "Terminal",
        icon: "wolos-terminal",
        exec: &["foot"],
    },
    Pin {
        id: "wol-settings",
        name: "System Settings",
        icon: "wolos-settings",
        exec: &["wol-settings"],
    },
];

fn program(exec0: &str) -> &str {
    exec0.rsplit('/').next().unwrap_or(exec0)
}

/// Running when a window class matches the pin id or its program (case-insensitive).
pub fn is_running(p: &Pin, classes: &std::collections::HashSet<String>) -> bool {
    classes.contains(&p.id.to_lowercase())
        || p.exec
            .first()
            .is_some_and(|e| classes.contains(&program(e).to_lowercase()))
}

/// Pin name, else installed .desktop Name, else window title, else class; empty class = desktop.
pub fn frontmost_name(class: &str, title: &str, pins: &[Pin], installed: &[Entry]) -> String {
    if class.is_empty() && title.is_empty() {
        return DESKTOP_NAME.into();
    }
    let lc = class.to_lowercase();
    if let Some(p) = pins
        .iter()
        .find(|p| p.id == lc || p.exec.first().is_some_and(|e| program(e) == lc))
    {
        return p.name.into();
    }
    if let Some(e) = installed.iter().find(|e| {
        e.id.trim_end_matches(".desktop")
            .eq_ignore_ascii_case(class)
            || e.exec
                .first()
                .is_some_and(|x| program(x).eq_ignore_ascii_case(class))
    }) {
        return e.name.clone();
    }
    if class.is_empty() {
        title.into()
    } else {
        class.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmost_names() {
        let installed = [Entry {
            id: "org.gnome.Calculator.desktop".into(),
            name: "Calculator".into(),
            exec: vec!["gnome-calculator".into()],
            icon: String::new(),
        }];
        for (c, t, w) in [
            ("", "", "Finder"),
            ("foot", "~", "Terminal"),
            ("org.gnome.Calculator", "x", "Calculator"),
            ("gnome-calculator", "", "Calculator"),
            ("", "xterm", "xterm"),
            ("weird", "T", "weird"),
        ] {
            assert_eq!(frontmost_name(c, t, PINS, &installed), w, "{c}");
        }
    }

    #[test]
    fn running_by_class() {
        let classes: std::collections::HashSet<String> =
            ["foot".to_string(), "firefox".into()].into();
        let running: Vec<&str> = PINS
            .iter()
            .filter(|p| is_running(p, &classes))
            .map(|p| p.name)
            .collect();
        assert_eq!(running, ["Browser", "Terminal"]);
    }
}
