// SPDX-License-Identifier: GPL-2.0-or-later
//! Every CSS class the shell puts on a widget (`add_css_class("…")`) is styled in kit.css or
//! contrast.css, or is a named hook code reads back. A class nobody styles is a typo, a rule that
//! was dropped, or a leftover: the audit found 40-odd. The unstyled ones left are frozen here;
//! the list only shrinks (style the class or drop it, then delete it from the list).
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const CSS: [&str; 2] = [
    include_str!("../src/kit/kit.css"),
    include_str!("../src/kit/contrast.css"),
];
const HYPRLAND_CONF: &str =
    include_str!("../../../desktop/overlay/usr/share/wolos/hypr/hyprland.conf");

/// Classes code reads back (`has_css_class`, kit::check's layout-only list): not for styling.
const HOOKS: [&str; 5] = [
    "control",
    "device-row",
    "kit-reveal",
    "may-ellipsize",
    "no-row-check",
];

/// How many [`UNSTYLED`] may hold (audit3 R2A-23): lower it with every class you style or drop;
/// the list only shrinks.
const UNSTYLED_MAX: usize = 35;

/// Still on a widget and styled by nothing (2026-11-01). Style or drop, never add.
const UNSTYLED: &[&str] = &[
    "assistant-body",
    "assistant-busy",
    "assistant-foot",
    "assistant-pager",
    "assistant-stage",
    "assistant-waiting",
    "banner-actions",
    "canvas",
    "capture",
    "disk-bar",
    "dock-stack",
    "drop-shelf",
    "drop-shelf-row",
    "gallery-stage",
    "gauge",
    "hero",
    "hero-art",
    "illustration",
    "lock-backdrop",
    "lock-fit",
    "login-tiles",
    "menu-check",
    "notifications",
    "numeric",
    "overview",
    "overview-window-surface",
    "shortcut-sheet",
    "shortcut-sheet-section",
    "stepper",
    "store-screenshot",
    "store-status",
    "switcher",
    "usage",
    "usage-legend",
    "whole-rows",
];

fn rust_files() -> Vec<PathBuf> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    let mut dirs = vec![src];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                files.push(p);
            }
        }
    }
    files.sort();
    files
}

/// `add_css_class("x")` literals in `src`, with their line numbers.
fn literals(src: &str) -> Vec<(usize, String)> {
    const CALL: &str = "add_css_class(\"";
    let mut out = Vec::new();
    for (n, line) in src.lines().enumerate() {
        let code = line.split("//").next().unwrap_or("");
        for (i, _) in code.match_indices(CALL) {
            let rest = &code[i + CALL.len()..];
            if let Some(end) = rest.find('"') {
                out.push((n + 1, rest[..end].to_string()));
            }
        }
    }
    out
}

/// `.name` tokens of a stylesheet.
fn classes_in(css: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let b = css.as_bytes();
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'-' || c == b'_';
    for (i, _) in css.match_indices('.') {
        let rest = &b[i + 1..];
        if rest
            .first()
            .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_')
        {
            let len = rest.iter().take_while(|c| ident(**c)).count();
            out.insert(css[i + 1..i + 1 + len].to_string());
        }
    }
    out
}

fn styled() -> HashSet<String> {
    CSS.iter().flat_map(|c| classes_in(c)).collect()
}

/// Every class used, as (file:line, class).
fn used() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    for f in rust_files() {
        let text = std::fs::read_to_string(&f).unwrap();
        let rel = f.strip_prefix(root).unwrap().display().to_string();
        for (n, c) in literals(&text) {
            out.push((format!("{rel}:{n}"), c));
        }
    }
    out
}

#[test]
fn detectors_read_rust_and_css() {
    assert_eq!(
        literals("a.add_css_class(\"x-y\"); // add_css_class(\"no\")"),
        [(1, "x-y".to_string())]
    );
    let css = classes_in(".a-b label, button.c:hover { opacity: 0.5; } .d_e {}");
    let want: HashSet<String> = ["a-b", "c", "d_e"].iter().map(|s| s.to_string()).collect();
    assert_eq!(css, want);
    assert!(styled().contains("glass-panel"));
    assert!(styled().contains("menubar-strip"));
}

#[test]
fn every_class_on_a_widget_is_styled_or_a_known_hook() {
    let styled = styled();
    let bad: Vec<String> = used()
        .into_iter()
        .filter(|(_, c)| {
            !styled.contains(c) && !HOOKS.contains(&c.as_str()) && !UNSTYLED.contains(&c.as_str())
        })
        .map(|(at, c)| format!("{at}: \"{c}\""))
        .collect();
    assert!(
        bad.is_empty(),
        "classes no stylesheet styles (add a rule to kit.css or drop the class):\n{}",
        bad.join("\n")
    );
}

/// The frozen list is exact: an entry that got styled or dropped comes off it.
#[test]
fn the_unstyled_list_only_shrinks() {
    let styled = styled();
    let used: HashSet<String> = used().into_iter().map(|(_, c)| c).collect();
    for &c in UNSTYLED {
        assert!(
            used.contains(c),
            "{c}: no longer used, delete it from UNSTYLED"
        );
        assert!(
            !styled.contains(c),
            "{c}: styled now, delete it from UNSTYLED"
        );
        assert!(!HOOKS.contains(&c), "{c}: listed twice");
    }
    assert!(
        UNSTYLED.len() <= UNSTYLED_MAX,
        "UNSTYLED has {} classes, the ratchet allows {UNSTYLED_MAX}: style the new class instead",
        UNSTYLED.len()
    );
    if UNSTYLED.len() < UNSTYLED_MAX {
        eprintln!(
            "note: UNSTYLED shrank: lower UNSTYLED_MAX to {}",
            UNSTYLED.len()
        );
    }
    let mut sorted = UNSTYLED.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted, UNSTYLED, "sorted, each once");
}

/// The menu bar's height is one number: `w::MENUBAR_HEIGHT` (Control Center's chrome), kit.css's
/// strip and hyprbars' title bar in the desktop conf, so the panel never overlaps or floats.
#[test]
fn the_menu_bar_height_is_one_number() {
    let h = shell::kit::widgets::MENUBAR_HEIGHT;
    assert!(
        CSS[0].contains(&format!(".menubar-strip {{ min-height: {h}px;")),
        "kit.css .menubar-strip min-height is not {h}px"
    );
    assert!(
        HYPRLAND_CONF.contains(&format!("bar_height = {h}\n"))
            || HYPRLAND_CONF.contains(&format!("bar_height = {h}\r\n")),
        "hyprland.conf hyprbars bar_height is not {h}"
    );
}
