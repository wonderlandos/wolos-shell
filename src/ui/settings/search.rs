// SPDX-License-Identifier: GPL-2.0-or-later
//! Settings search over every pane and the rows on it (System Settings' own search): a query
//! matches a pane by its title or search words ([`super::SIDEBAR`]) and a row by its title
//! ([`ROWS`]); a hit on a subpage also shows its parent in the sidebar ([`parent`]).
//! Invariant: every page has a [`ROWS`] entry and every entry names a page (tested below), so a
//! new pane cannot ship unsearchable.

use super::datetime::rows as dt;
use super::desktop_dock::rows as dd;
use super::trackpad::rows as tp;
use crate::copy::settings as copy;

/// Row titles per page (sidebar panes and subpages), as the page shows them; panes that name
/// their titles (`rows` modules) are read from there, so the two cannot drift.
pub(super) const ROWS: [(&str, &[&str]); 38] = [
    (
        "Wi-Fi",
        &[
            "Wi-Fi",
            "Network Details",
            "Known Networks",
            "Other Networks",
            "Saved Networks",
            "This Network",
            "Scan for Networks",
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
            "Firewall",
            "Airplane Mode",
            "IP settings",
            "VPN",
            "VPN password",
            "Add VPN",
            "Proxy",
            "DNS over TLS",
        ],
    ),
    (
        "IP Settings",
        &["Configure IPv4", "Address", "Router", "DNS servers"],
    ),
    (
        "Proxy",
        &[
            "Proxy",
            "Configuration address",
            "Web proxy (HTTP)",
            "Secure web proxy (HTTPS)",
            "SOCKS proxy",
            "Don’t use the proxy for",
        ],
    ),
    ("DNS over TLS", &["DNS over TLS", "DNS servers"]),
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
            "Default Apps",
            "Sharing",
            "Shell in the terminal",
        ],
    ),
    (
        "Appearance",
        &[
            "Light",
            "Dark",
            "Auto",
            "Accent color",
            "Themes",
            "Wallpaper",
            "Theme from your wallpaper",
            "Make a theme",
        ],
    ),
    (
        "Accessibility",
        &[
            "Larger text",
            "Increase contrast",
            "Reduce transparency",
            "Reduce motion",
            "Give me more time",
            "Pointer size",
            "Zoom",
            "Display",
            "Timing",
            "Pointer and Zoom",
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
            dd::POSITION,
            dd::SIZE,
            dd::DELAY,
            dd::BATTERY,
            dd::DATE,
            dd::TILE,
            dd::ARRANGEMENT,
            dd::GAPS,
            dd::SNAP,
            dd::SHORTCUTS,
            dd::FLOATING,
            "Dock",
            "Menu Bar",
            "Windows",
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
    ("Wallpaper", &["Dynamic", "Your Photo", "Your own photo"]),
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
            "Windows",
            "Glass",
            "Borders",
            "Depth",
            "Motion",
            "Looks",
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
            "Energy",
            "Save Memory For",
            "Memory",
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
            "System protections",
            "Disk encryption",
            "Startup Security",
            "Malware Check",
            "Send error reports",
            "Full Signatures",
            "App Connections",
            "Privacy",
            "Security",
            "Screen sharing",
            "Display",
            "Flatpak app",
            "Full Signature Database",
            "Malware",
        ],
    ),
    (
        "Users & Groups",
        &[
            "Users",
            "Add User",
            "Full name",
            "Account name",
            "Password",
            "Allow this user to administer this computer",
            "Change Password…",
        ],
    ),
    (
        "Keyboard",
        &[
            "Key repeat rate",
            "Delay until repeat",
            "Keyboard Shortcuts",
            "Input Sources",
            "Input Methods",
        ],
    ),
    (
        "Trackpad & Mouse",
        &[
            tp::SPEED,
            tp::SCROLL,
            tp::TAP,
            tp::SECONDARY,
            tp::NATURAL,
            tp::WHILE_TYPING,
            "Trackpad",
            "Mouse",
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
            "AI assistants",
            "Scripts and Terminal",
        ],
    ),
    (
        "Modules",
        &[
            "Installed",
            "Available",
            "Finish Setup",
            "Install",
            "Could Not Be Added",
        ],
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
            "Battery",
            "System",
            "Software",
        ],
    ),
    (
        "Software Update",
        &[
            "Automatic updates",
            "Download new updates",
            "Download on metered connections",
            "Install app updates",
            "Update channel",
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
            "Restore points",
        ],
    ),
    ("Date & Time", &[dt::AUTOMATIC, dt::ZONE, "Date and time"]),
    (
        "Language & Region",
        &["Preferred language", "Input Sources"],
    ),
    ("Login Items", &["Open at Login"]),
    (
        "Default Apps",
        &[
            copy::DEFAULT_APPS_WEB,
            copy::DEFAULT_APPS_MAIL,
            copy::DEFAULT_APPS_FILES,
            copy::DEFAULT_APPS_IMAGES,
            copy::DEFAULT_APPS_MUSIC,
            copy::DEFAULT_APPS_VIDEO,
            copy::DEFAULT_APPS_PDF,
            copy::DEFAULT_APPS_TEXT,
            copy::DEFAULT_APPS_CALENDAR,
        ],
    ),
    (
        "Sharing",
        &[
            "Computer name",
            "Remote Login",
            "AirPlay Receiver",
            "Internet Sharing",
            "Password",
        ],
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
            "Release",
            "Gaming",
        ],
    ),
];

/// The sidebar pane a subpage is reached from (itself for a sidebar pane).
pub(super) fn parent(page: &str) -> &str {
    match page {
        "About" | "Software Update" | "Storage" | "Date & Time" | "Language & Region"
        | "Login Items" | "Default Apps" | "Sharing" => "General",
        "Keyboard Shortcuts" => "Keyboard",
        "Startup Security" => "Privacy & Security",
        "IP Settings" | "Proxy" | "DNS over TLS" => "Network",
        p => p,
    }
}

/// Rows panes build from their own tables (one title, written once): Control Center's tiles and
/// modules, the kinds of app permission in Privacy & Security (QA-SET-43: "screenshot" found
/// the pane but not its row, a Privacy kind found nothing).
pub(super) fn data_rows() -> [(&'static str, Vec<&'static str>); 2] {
    [
        (
            "Control Center",
            super::control_center::ALWAYS
                .iter()
                .map(|a| a.1)
                .chain(
                    wol_core::prefs::ControlCenterPrefs::MODULES
                        .iter()
                        .map(|m| m.1),
                )
                .collect(),
        ),
        (
            "Privacy & Security",
            super::privacy::KINDS.iter().map(|k| k.1).collect(),
        ),
    ]
}

/// One search result: the page to open and the row that matched (None: the page itself).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub page: &'static str,
    pub row: Option<&'static str>,
}

/// Characters of a query that count: no row title is longer; a pasted page of text (5 kB) hid
/// every pane and left Esc unable to clear it (QA-LIVE-10).
pub const QUERY_MAX: usize = 64;

/// `q` cut to [`QUERY_MAX`] characters (on a character boundary).
pub fn capped(q: &str) -> &str {
    q.char_indices().nth(QUERY_MAX).map_or(q, |(i, _)| &q[..i])
}

fn words(q: &str) -> Vec<String> {
    capped(q)
        .split_whitespace()
        .map(str::to_lowercase)
        .collect()
}

/// Every word of `q` starts a word of `hay`, with or without its hyphens (`wifi` finds `Wi-Fi`;
/// `date` does not find `Update`: QA-LIVE-35).
fn all_in(hay: &str, q: &[String]) -> bool {
    let hay = hay.to_lowercase();
    let plain = hay.replace('-', "");
    // A word of `text` is a run of letters and hyphens; each part after a hyphen counts too.
    let starts = |text: &str, w: &str| {
        text.split(|c: char| !c.is_alphanumeric() && c != '-')
            .flat_map(|t| std::iter::once(t).chain(t.split('-')))
            .any(|t| t.starts_with(w))
    };
    q.iter()
        .all(|w| starts(&hay, w.as_str()) || starts(&plain, w.as_str()))
}

/// Hits by meaning that are shown after word hits: at most this many, and each needs this cosine
/// (a word hit already answered the query; a weaker neighbour would only crowd it).
const EXTRA_AFTER_WORDS: usize = 3;
const EXTRA_COS_AFTER_WORDS: f32 = 0.5;

/// Pages whose title or search words match every word of `query`, then rows whose title (with
/// its page's title) does; each page's own hit first, at most one hit per row. What the query
/// means follows (or, when no word matches, is all there is): "make the cursor bigger" finds
/// Pointer size, "deixar o texto maior" finds Larger text, once the shipped table is loaded.
pub fn hits(query: &str) -> Vec<Hit> {
    match super::search_rows::global() {
        Some((sem, table)) => hits_with(
            query,
            Some(&super::search_rows::Ctx {
                sem,
                table,
                map: None,
            }),
        ),
        None => hits_with(query, None),
    }
}

/// [`hits`] with the meaning index given (tests use their own table).
pub(super) fn hits_with(query: &str, cx: Option<&super::search_rows::Ctx>) -> Vec<Hit> {
    let mut out = word_hits(query);
    let Some(cx) = cx else { return out };
    let capped = capped(query);
    if capped.trim().is_empty() {
        return out;
    }
    let by_words = out.len();
    let mut extra = 0;
    for (h, cos) in cx.sem.find(cx, capped) {
        if out.contains(&h) {
            continue;
        }
        if by_words > 0 && (cos < EXTRA_COS_AFTER_WORDS || extra >= EXTRA_AFTER_WORDS) {
            continue;
        }
        out.push(h);
        extra += 1;
    }
    out
}

/// The word-prefix hits alone (pages whose title or search words match every word of `query`,
/// then rows).
fn word_hits(query: &str) -> Vec<Hit> {
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
    for (page, rows) in ROWS
        .iter()
        .map(|(p, r)| (*p, r.to_vec()))
        .chain(data_rows())
    {
        for r in &rows {
            if *r != page
                && all_in(&format!("{r} {page}"), &q)
                && !out.iter().any(|h| h.page == page && h.row == Some(*r))
            {
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
    /// QA-LIVE-35: "Date" found "Update" (a substring); a query word starts a word now.
    #[test]
    fn a_query_word_starts_a_word() {
        assert!(all_in("Software Update", &["upd".into()]));
        assert!(!all_in("Software Update", &["date".into()]));
        assert!(all_in("Wi-Fi", &["wifi".into()]));
        assert!(all_in("Date & Time", &["date".into(), "tim".into()]));
        let pages: Vec<&str> = hits("date").iter().map(|h| h.page).collect();
        assert!(pages.contains(&"Date & Time"), "{pages:?}");
        assert!(!pages.contains(&"Software Update"), "{pages:?}");
    }

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

    /// QA-LIVE-10: a 5000-character paste was searched whole.
    #[test]
    fn query_capped_esc_clears() {
        let long = "é".repeat(5000);
        assert_eq!(capped(&long).chars().count(), QUERY_MAX);
        assert_eq!(capped("dark"), "dark");
        let longest = ROWS
            .iter()
            .flat_map(|r| r.1.iter())
            .map(|t| t.chars().count())
            .max();
        assert!(
            longest.unwrap() <= QUERY_MAX,
            "every row title fits the cap"
        );
        assert_eq!(hits(&format!("dark{}", " x".repeat(3000))), hits("dark x"));
        // Esc: the field's stop-search clears it (mod.rs build).
        let src = include_str!("mod.rs");
        assert!(src.contains("field.connect_stop_search(|s| s.set_text(\"\"))"));
    }

    /// The page each pane file renders (for the row index below).
    const FILES: [(&str, &str, &str); 35] = [
        ("wifi.rs", "Wi-Fi", include_str!("wifi.rs")),
        ("network.rs", "Network", include_str!("network.rs")),
        ("network_vpn.rs", "Network", include_str!("network_vpn.rs")),
        (
            "ip_settings.rs",
            "IP Settings",
            include_str!("ip_settings.rs"),
        ),
        ("proxy.rs", "Proxy", include_str!("proxy.rs")),
        (
            "dns_over_tls.rs",
            "DNS over TLS",
            include_str!("dns_over_tls.rs"),
        ),
        (
            "notifications.rs",
            "Notifications",
            include_str!("notifications.rs"),
        ),
        ("general.rs", "General", include_str!("general.rs")),
        ("appearance.rs", "Appearance", include_str!("appearance.rs")),
        (
            "accessibility.rs",
            "Accessibility",
            include_str!("accessibility.rs"),
        ),
        (
            "control_center.rs",
            "Control Center",
            include_str!("control_center.rs"),
        ),
        (
            "desktop_dock.rs",
            "Desktop & Dock",
            include_str!("desktop_dock.rs"),
        ),
        ("wallpaper.rs", "Wallpaper", include_str!("wallpaper.rs")),
        ("effects.rs", "Effects & Looks", include_str!("effects.rs")),
        ("widgets.rs", "Widgets", include_str!("widgets.rs")),
        ("battery.rs", "Battery", include_str!("battery.rs")),
        (
            "memory_saver.rs",
            "Battery",
            include_str!("memory_saver.rs"),
        ),
        ("lock.rs", "Lock Screen", include_str!("lock.rs")),
        (
            "privacy.rs",
            "Privacy & Security",
            include_str!("privacy.rs"),
        ),
        (
            "malware.rs",
            "Privacy & Security",
            include_str!("malware.rs"),
        ),
        ("accounts.rs", "Users & Groups", include_str!("accounts.rs")),
        ("keyboard.rs", "Keyboard", include_str!("keyboard.rs")),
        (
            "trackpad.rs",
            "Trackpad & Mouse",
            include_str!("trackpad.rs"),
        ),
        ("printers.rs", "Printers", include_str!("printers.rs")),
        ("hardware.rs", "Hardware", include_str!("hardware.rs")),
        (
            "assistants.rs",
            "Assistants & Automation",
            include_str!("assistants.rs"),
        ),
        ("modules.rs", "Modules", include_str!("modules.rs")),
        (
            "shortcuts.rs",
            "Keyboard Shortcuts",
            include_str!("shortcuts.rs"),
        ),
        ("about.rs", "About", include_str!("about.rs")),
        ("datetime.rs", "Date & Time", include_str!("datetime.rs")),
        (
            "login_items.rs",
            "Login Items",
            include_str!("login_items.rs"),
        ),
        ("sharing.rs", "Sharing", include_str!("sharing.rs")),
        (
            "startup_security.rs",
            "Startup Security",
            include_str!("startup_security.rs"),
        ),
        ("storage.rs", "Storage", include_str!("storage.rs")),
        ("displays.rs", "Displays", include_str!("displays.rs")),
    ];

    /// Row and section titles a pane writes as literals: `<kind>_row("Title"`, `info("Title"`,
    /// `w::section("Title"`, `w::list_row(icon, "Title"`.
    fn titles(code: &str) -> Vec<&str> {
        fn lit(after: &str) -> Option<&str> {
            let a = after.trim_start();
            let a = a.strip_prefix('"')?;
            let t = &a[..a.find('"')?];
            (!t.is_empty() && !t.contains(['{', '\\'])).then_some(t)
        }
        let mut out = Vec::new();
        for m in [
            "toggle_row(",
            "picker_row(",
            "pending_picker_row(",
            "entry_row(",
            "stepper_row(",
            "slider_row(",
            "segmented_row(",
            "button_row(",
            "info(",
            "w::section(",
        ] {
            for part in code.split(m).skip(1) {
                out.extend(lit(part));
            }
        }
        for part in code.split("w::list_row(").skip(1) {
            if let Some((_, rest)) = part.split_once(',') {
                out.extend(lit(rest));
            }
        }
        out
    }

    /// QA-SET-43: rows of many panes (Privacy, Keyboard, Startup Security, Assistants, Modules)
    /// were not in the search index; this generalises `every_displays_row_is_indexed`.
    #[test]
    fn every_pane_row_indexed() {
        let mut missing = Vec::new();
        for (file, page, src) in FILES {
            let code = src.split("#[cfg(test)]").next().unwrap_or(src);
            let rows = ROWS.iter().find(|r| r.0 == page).map(|r| r.1).unwrap();
            // Empty states ("No printers yet") are not rows to find.
            for t in titles(code).into_iter().filter(|t| !t.starts_with("No ")) {
                if t != page && !rows.contains(&t) {
                    missing.push(format!("{file}: {t:?} ({page})"));
                }
            }
        }
        missing.sort();
        missing.dedup();
        assert!(
            missing.is_empty(),
            "rows not in ROWS:\n{}",
            missing.join("\n")
        );
        // The tables panes build rows from.
        for (page, rows) in data_rows() {
            for r in rows {
                let h = hits(r);
                assert!(
                    h.iter()
                        .any(|h| h.page == page && (h.row == Some(r) || r == page)),
                    "{page}: {r:?} not found: {h:?}"
                );
            }
        }
        assert!(
            hits("screenshot").contains(&Hit {
                page: "Control Center",
                row: Some("Screenshot")
            }),
            "QA-SET-43 repro"
        );
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
                    dd::POSITION,
                    dd::SIZE,
                    dd::DELAY,
                    dd::BATTERY,
                    dd::DATE,
                    dd::TILE,
                    dd::ARRANGEMENT,
                    dd::GAPS,
                    dd::SNAP,
                    dd::SHORTCUTS,
                    dd::FLOATING,
                ],
            ),
            (
                "Trackpad & Mouse",
                include_str!("trackpad.rs"),
                &[
                    tp::SPEED,
                    tp::SCROLL,
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

    /// The table the tests search with: the wolsem fixture (a potion-multilingual cut to the words
    /// of the manual, these rows and the seven-language copy tables).
    fn fixture() -> wolsem::Table {
        let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../wolsem/tests/data/mini.wst");
        wolsem::Table::open(std::path::Path::new(p)).unwrap_or_else(|e| panic!("{e}"))
    }

    /// Natural-language questions, in four languages, land on the row (or its page) in the top
    /// three: the meaning index after the word hits. No row is translated.
    #[test]
    fn natural_questions_find_their_row() {
        let table = fixture();
        let sem = super::super::search_rows::Sem::new(&table);
        let cx = super::super::search_rows::Ctx {
            sem: &sem,
            table: &table,
            map: None,
        };
        let golden: [(&str, &str, Option<&str>); 14] = [
            ("turn off the firewall", "Network", Some("Firewall")),
            (
                "make the cursor bigger",
                "Accessibility",
                Some("Pointer size"),
            ),
            ("reduce animations", "Accessibility", Some("Reduce motion")),
            (
                "make the display warmer at night",
                "Displays",
                Some("Night Shift"),
            ),
            (
                "remove noise from my microphone",
                "Sound",
                Some("Reduce background noise"),
            ),
            ("change my password", "Users & Groups", Some("Password")),
            ("hide the dock", "Desktop & Dock", None),
            (
                "stop a click when sound starts",
                "Sound",
                Some("Keep device awake"),
            ),
            ("the mouse is too slow", "Trackpad & Mouse", None),
            ("join a wireless network", "Wi-Fi", None),
            ("desligar o firewall", "Network", Some("Firewall")),
            ("tela mais quente à noite", "Displays", Some("Night Shift")),
            ("deixar o cursor maior", "Accessibility", None),
            ("Passwort ändern", "Users & Groups", None),
        ];
        let mut missed = Vec::new();
        for (q, page, row) in golden {
            let found = hits_with(q, Some(&cx));
            let top: Vec<_> = found.iter().take(3).collect();
            let ok = top
                .iter()
                .any(|h| h.page == page && (row.is_none() || h.row == row));
            if !ok {
                missed.push(format!("{q:?} wants {page}/{row:?}, got {top:?}"));
            }
        }
        assert!(missed.is_empty(), "{}", missed.join("\n"));
        // Words first: a query that names a row keeps the word hit on top and adds little after it.
        let found = hits_with("balance", Some(&cx));
        assert_eq!(
            found.first(),
            Some(&Hit {
                page: "Sound",
                row: None
            })
        );
        assert!(found.contains(&Hit {
            page: "Sound",
            row: Some("Balance")
        }));
        assert_eq!(
            found[..2],
            word_hits("balance")[..2],
            "the word hits stay first, in order"
        );
        assert!(
            found.len() <= word_hits("balance").len() + EXTRA_AFTER_WORDS,
            "{found:?}"
        );
        // Nonsense finds nothing, with or without the meaning index.
        assert!(hits_with("no such thing anywhere", Some(&cx)).is_empty());
        assert!(hits_with("qwertyuiop asdfgh", Some(&cx)).is_empty());
        assert!(hits_with("", Some(&cx)).is_empty());
        // Without a table nothing changes.
        assert!(hits_with("make the cursor bigger", None).is_empty());
    }

    /// Every row a sentence is bound to is a row of its page (the copy is read from `copy_settings`).
    #[test]
    fn described_rows_exist() {
        for (page, row, text) in super::super::search_rows::DESCRIBED {
            let rows = ROWS
                .iter()
                .find(|r| r.0 == page)
                .map(|r| r.1)
                .unwrap_or_default();
            assert!(rows.contains(&row), "{page}/{row} is not a row");
            assert!(!text.is_empty());
        }
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
