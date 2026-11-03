// SPDX-License-Identifier: GPL-2.0-or-later
//! Settings search over every pane and the rows on it (System Settings' own search): a query
//! matches a pane by its title or search words ([`super::SIDEBAR`]) and a row by its title
//! ([`ROWS`]); a hit on a subpage also shows its parent in the sidebar ([`parent`]).
//! Invariant: every page has a [`ROWS`] entry and every entry names a page (tested below), so a
//! new pane cannot ship unsearchable.

use super::datetime::rows as dt;
use super::desktop_dock::rows as dd;
use super::trackpad::rows as tp;

/// Row titles per page (sidebar panes and subpages), as the page shows them; panes that name
/// their titles (`rows` modules) are read from there, so the two cannot drift.
pub(super) const ROWS: [(&str, &[&str]); 34] = [
    (
        "Wi-Fi",
        &[
            "Wi-Fi",
            "Network Details",
            "Known Networks",
            "Other Networks",
        ],
    ),
    ("Bluetooth", &["Bluetooth", "My Devices"]),
    (
        "Network",
        &[
            "IP address",
            "Subnet mask",
            "Router",
            "DNS servers",
            "Search domains",
            "IPv6 address",
            "Hardware address",
        ],
    ),
    (
        "Notifications",
        &["Do Not Disturb", "Focus", "Notification Center", "Turn off"],
    ),
    (
        "Sound",
        &[
            "Output volume",
            "Mute",
            "Balance",
            "Allow louder",
            "Input volume",
            "Input level",
            "Mute input",
            "Automatic input level",
            "Reduce background noise",
            "Bluetooth codec",
            "Use the headset microphone automatically",
            "Sample rate",
            "Buffer size",
            "Dropouts",
            "Pro Audio",
            "Keep device awake",
            "Ignore hardware volume",
            "Diagnostics",
            "Reset audio",
        ],
    ),
    (
        "General",
        &[
            "About",
            "Software Update",
            "Storage",
            "Login Items",
            "Sharing",
        ],
    ),
    (
        "Appearance",
        &["Light", "Dark", "Auto", "Accent color", "Themes"],
    ),
    (
        "Accessibility",
        &[
            "Increase contrast",
            "Reduce transparency",
            "Reduce motion",
            "Pointer size",
            "Zoom",
        ],
    ),
    (
        "Control Center",
        &["Always Shown", "Show When You Want Them"],
    ),
    (
        "Desktop & Dock",
        &[
            dd::MAGNIFY,
            dd::AUTOHIDE,
            dd::SIZE,
            dd::DELAY,
            dd::BATTERY,
            dd::DATE,
            dd::TILE,
            dd::ARRANGEMENT,
            dd::GAPS,
            dd::SHORTCUTS,
            dd::FLOATING,
        ],
    ),
    (
        "Displays",
        &[
            "Resolution",
            "Refresh Rate",
            "Scale",
            "Arrangement",
            "Night Shift",
        ],
    ),
    ("Wallpaper", &["Dynamic", "Your Photo"]),
    (
        "Effects & Looks",
        &[
            "Corner radius",
            "Space around windows",
            "Blur strength",
            "Blur quality",
            "Color behind glass",
            "Active window",
            "Other windows",
            "Gradient direction",
            "Border width",
            "Shadow size",
            "Dim other windows",
            "Windows open with",
            "Desktops change with",
            "Speed",
            "Import a Look",
            "Export This Look",
            "Restore Defaults",
        ],
    ),
    (
        "Widgets",
        &["On the Desktop", "Add Widgets", "Arrange widgets"],
    ),
    (
        "Battery",
        &[
            "Battery level",
            "Battery health",
            "Charge cycles",
            "Energy Mode",
            "When the lid is closed",
            "Memory Saver",
            "App Nap",
        ],
    ),
    (
        "Lock Screen",
        &["Lock screen when inactive", "Lock when the computer sleeps"],
    ),
    (
        "Privacy & Security",
        &[
            "Screen Recording",
            "Remote Control",
            "Clipboard History",
            "Firewall",
            "System protections",
            "Disk encryption",
            "Startup Security",
            "Malware Check",
            "Full Signatures",
            "App Connections",
        ],
    ),
    (
        "Users & Groups",
        &["Users", "Add User", "Full name", "Account name", "Password"],
    ),
    (
        "Keyboard",
        &[
            "Key repeat rate",
            "Delay until repeat",
            "Keyboard Shortcuts",
            "Input Sources",
        ],
    ),
    (
        "Trackpad & Mouse",
        &[
            tp::SPEED,
            tp::TAP,
            tp::SECONDARY,
            tp::NATURAL,
            tp::WHILE_TYPING,
        ],
    ),
    (
        "Printers",
        &["Add Printer", "Set as Default", "Remove Printer"],
    ),
    ("Hardware", &["Needs Attention", "Devices", "Fix"]),
    (
        "Assistants & Automation",
        &[
            "Allow assistants and scripts",
            "What Helpers May Do",
            "Helpers Seen",
            "Always Allowed",
            "Connect a Helper",
        ],
    ),
    (
        "Modules",
        &["Installed", "Available", "Finish Setup", "Install"],
    ),
    ("Restore", &["Recent Changes", "Restore Points", "Undo"]),
    (
        "Keyboard Shortcuts",
        &["Show All Shortcuts", "Custom Shortcuts", "Restore Defaults"],
    ),
    (
        "About",
        &[
            "Name",
            "Processor",
            "Graphics",
            "Memory",
            "Startup disk",
            "Kernel",
        ],
    ),
    (
        "Software Update",
        &[
            "Automatic updates",
            "Download new updates",
            "Install app updates",
            "Check Now",
        ],
    ),
    (
        "Storage",
        &[
            "Startup disk",
            "Available",
            "Encryption",
            "Kept for restore points",
        ],
    ),
    ("Date & Time", &[dt::AUTOMATIC, dt::ZONE]),
    (
        "Language & Region",
        &["Preferred language", "Input Sources"],
    ),
    ("Login Items", &["Open at Login"]),
    (
        "Sharing",
        &["Computer name", "Remote Login", "AirPlay Receiver"],
    ),
    (
        "Startup Security",
        &[
            "Secure Boot",
            "Keys",
            "Kernel lockdown",
            "Security chip",
            "Measured startup",
            "Sealed system",
            "Finish Secure Boot setup",
            "Attested gaming mode",
        ],
    ),
];

/// The sidebar pane a subpage is reached from (itself for a sidebar pane).
pub(super) fn parent(page: &str) -> &str {
    match page {
        "About" | "Software Update" | "Storage" | "Date & Time" | "Language & Region"
        | "Login Items" | "Sharing" => "General",
        "Keyboard Shortcuts" => "Keyboard",
        "Startup Security" => "Privacy & Security",
        p => p,
    }
}

/// One search result: the page to open and the row that matched (None: the page itself).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub page: &'static str,
    pub row: Option<&'static str>,
}

fn words(q: &str) -> Vec<String> {
    q.split_whitespace().map(str::to_lowercase).collect()
}

fn all_in(hay: &str, q: &[String]) -> bool {
    let hay = hay.to_lowercase();
    q.iter().all(|w| hay.contains(w.as_str()))
}

/// Pages whose title or search words match every word of `query`, then rows whose title (with
/// its page's title) does; each page's own hit first, at most one hit per row.
pub fn hits(query: &str) -> Vec<Hit> {
    let q = words(query);
    if q.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<Hit> = super::SIDEBAR
        .iter()
        .flat_map(|g| g.iter())
        .filter(|p| all_in(&format!("{} {}", p.title, p.words), &q))
        .map(|p| Hit {
            page: p.title,
            row: None,
        })
        .collect();
    for (page, rows) in ROWS {
        for r in rows {
            if *r != page && all_in(&format!("{r} {page}"), &q) {
                out.push(Hit {
                    page,
                    row: Some(*r),
                });
            }
        }
        if !super::PAGES.contains(&page) && all_in(page, &q) && !out.iter().any(|h| h.page == page)
        {
            out.push(Hit { page, row: None });
        }
    }
    out
}

/// The page Return in the search field opens: a page the query names ("keyboard": Keyboard, not
/// Bluetooth, whose search words say keyboard); else the first hit, except that a pane found only
/// through its search words gives way to the subpage whose row matches by itself ("secure boot":
/// Startup Security, not Privacy & Security; "time zone": Date & Time, not General).
pub fn enter_target(query: &str) -> Option<&'static str> {
    let q = words(query);
    let found = hits(query);
    if let Some(named) = found.iter().find(|h| all_in(h.page, &q)) {
        return Some(named.page);
    }
    let first = found.first()?;
    if first.row.is_some() {
        return Some(first.page);
    }
    let deeper = found.iter().find(|h| {
        h.page != first.page && parent(h.page) == first.page && h.row.is_some_and(|r| all_in(r, &q))
    });
    Some(deeper.map_or(first.page, |h| h.page))
}

/// Sidebar panes to keep while searching: each hit's page or its parent.
pub fn panes(query: &str) -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    for h in hits(query) {
        let p = super::PAGES
            .iter()
            .copied()
            .find(|x| *x == parent(h.page))
            .unwrap_or(h.page);
        if !v.contains(&p) {
            v.push(p);
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_is_indexed_once() {
        let pages: Vec<&str> = super::super::PAGES
            .iter()
            .chain(super::super::SUBPAGES.iter())
            .copied()
            .collect();
        for p in &pages {
            assert_eq!(ROWS.iter().filter(|r| r.0 == *p).count(), 1, "{p}");
        }
        for (p, rows) in ROWS {
            assert!(pages.contains(&p), "{p} is not a page");
            assert!(!rows.is_empty(), "{p}");
            assert!(super::super::PAGES.contains(&parent(p)), "{p}'s parent");
        }
    }

    #[test]
    fn rows_are_found_on_their_page() {
        for (q, page, row) in [
            ("balance", "Sound", "Balance"),
            ("noise", "Sound", "Reduce background noise"),
            ("buffer", "Sound", "Buffer size"),
            ("secure boot", "Startup Security", "Secure Boot"),
            ("attested", "Startup Security", "Attested gaming mode"),
            ("tap", "Trackpad & Mouse", "Tap to click"),
            ("computer name", "Sharing", "Computer name"),
            ("finish setup", "Modules", "Finish Setup"),
        ] {
            let h = hits(q);
            assert!(
                h.contains(&Hit {
                    page,
                    row: Some(row)
                }),
                "{q}: {h:?}"
            );
        }
        assert!(hits("").is_empty());
        assert!(hits("no such thing anywhere").is_empty());
    }

    #[test]
    fn every_displays_row_is_indexed() {
        let src = include_str!("displays.rs");
        let code = src.split("#[cfg(test)]").next().unwrap_or(src);
        let rows = ROWS
            .iter()
            .find(|r| r.0 == "Displays")
            .map(|r| r.1)
            .unwrap_or_default();
        for part in code.split("row(\"").skip(1) {
            let title = &part[..part.find('"').unwrap_or(0)];
            assert!(
                rows.contains(&title),
                "Displays row {title:?} is not in ROWS"
            );
        }
        assert_eq!(enter_target("refresh rate"), Some("Displays"));
    }

    #[test]
    fn panes_with_named_titles_use_them() {
        // The title is written once, in the pane's `rows` module; the page and ROWS read it.
        let panes: [(&str, &str, &[&str]); 3] = [
            (
                "Desktop & Dock",
                include_str!("desktop_dock.rs"),
                &[
                    dd::MAGNIFY,
                    dd::AUTOHIDE,
                    dd::SIZE,
                    dd::DELAY,
                    dd::BATTERY,
                    dd::DATE,
                    dd::TILE,
                    dd::ARRANGEMENT,
                    dd::GAPS,
                    dd::SHORTCUTS,
                    dd::FLOATING,
                ],
            ),
            (
                "Trackpad & Mouse",
                include_str!("trackpad.rs"),
                &[
                    tp::SPEED,
                    tp::TAP,
                    tp::SECONDARY,
                    tp::NATURAL,
                    tp::WHILE_TYPING,
                ],
            ),
            (
                "Date & Time",
                include_str!("datetime.rs"),
                &[dt::AUTOMATIC, dt::ZONE],
            ),
        ];
        for (page, src, titles) in panes {
            let rows = ROWS.iter().find(|r| r.0 == page).map(|r| r.1).unwrap();
            let code = src.split("#[cfg(test)]").next().unwrap_or(src);
            for t in titles {
                assert!(rows.contains(t), "{page}: {t:?} is not in ROWS");
                let lit = format!("\"{t}\"");
                assert_eq!(code.matches(&lit).count(), 1, "{page}: {t:?} written twice");
            }
        }
        assert_eq!(enter_target("keep floating"), Some("Desktop & Dock"));
    }

    #[test]
    fn return_opens_the_subpage_that_has_the_row() {
        assert_eq!(enter_target("secure boot"), Some("Startup Security"));
        assert_eq!(enter_target("time zone"), Some("Date & Time"));
        assert_eq!(enter_target("computer name"), Some("Sharing"));
        assert_eq!(enter_target("keyboard"), Some("Keyboard"), "the page named");
        assert_eq!(enter_target("shortcuts"), Some("Keyboard Shortcuts"));
        assert_eq!(enter_target("wifi"), Some("Wi-Fi"));
        assert_eq!(enter_target("balance"), Some("Sound"));
        assert_eq!(enter_target(""), None);
        assert_eq!(enter_target("no such thing anywhere"), None);
    }

    #[test]
    fn a_subpage_hit_keeps_its_parent_in_the_sidebar() {
        assert!(panes("secure boot").contains(&"Privacy & Security"));
        assert!(panes("computer name").contains(&"General"));
        assert!(panes("blur").contains(&"Effects & Looks"));
        assert_eq!(
            panes("wifi").first(),
            Some(&"Wi-Fi"),
            "the pane's own hit first"
        );
    }
}
