// SPDX-License-Identifier: GPL-2.0-or-later
//! STANDARDS §1 (audit3 R2A-19): every sentence a person reads lives in `shell::copy` and passes
//! `copy::lint`; surfaces carry labels and titles only. A sentence here is a string literal that
//! starts with a capital, has a space, and ends in `.`, `?`, `!` or `…` (a title-case menu item
//! such as "Edit Widgets…" is a label, not a sentence). Log lines (`eprintln!`, `println!`,
//! `panic!`, `.expect(`, `assert*!`) are not on screen and are skipped.
//!
//! The sentences still outside copy.rs are frozen per file in [`BASELINE`] (2026-11-02): move them
//! into `copy.rs` and lower the number, never raise it. [`LINT_BASELINE`] freezes the ones that
//! also fail `copy::lint` (errno-like text, "failed", paths, program names).
#[path = "support/scan.rs"]
mod scan;

use std::collections::BTreeMap;

/// Files that are the copy, not surfaces.
const COPY_FILES: [&str; 4] = [
    "copy.rs",
    "copy_installer.rs",
    "copy_settings.rs",
    "copy_maker.rs",
];

/// Text on a line near one of these is a log or a programmer message, not on screen.
const NOT_ON_SCREEN: [&str; 9] = [
    "eprintln!",
    "println!",
    "eprint!",
    "panic!",
    ".expect(",
    "assert!",
    "assert_eq!",
    "unreachable!",
    "debug_assert",
];

/// Sentence literals outside copy.rs, per file. Only shrinks.
const BASELINE: [(&str, usize); 45] = [
    ("app_session.rs", 13),
    ("login.rs", 1),
    ("packages.rs", 22),
    ("setup.rs", 20),
    ("system.rs", 1),
    ("ui/approval.rs", 5),
    ("ui/drop_shelf.rs", 1),
    ("ui/flatpak_consent.rs", 2),
    ("ui/force_quit.rs", 2),
    ("ui/settings/accessibility.rs", 5),
    ("ui/settings/accounts.rs", 4),
    ("ui/settings/appearance.rs", 1),
    ("ui/settings/assistants.rs", 3),
    ("ui/settings/battery.rs", 3),
    ("ui/settings/control_center.rs", 1),
    ("ui/settings/datetime.rs", 2),
    ("ui/settings/effects.rs", 9),
    ("ui/settings/general.rs", 1),
    ("ui/settings/hardware.rs", 4),
    ("ui/settings/keyboard.rs", 1),
    ("ui/settings/language.rs", 2),
    ("ui/settings/lock.rs", 1),
    ("ui/settings/login_items.rs", 1),
    ("ui/settings/memory_saver.rs", 2),
    ("ui/settings/mod.rs", 1),
    ("ui/settings/network.rs", 1),
    ("ui/settings/notifications.rs", 2),
    ("ui/settings/printers.rs", 5),
    ("ui/settings/privacy.rs", 31),
    ("ui/settings/restore.rs", 3),
    ("ui/settings/sharing.rs", 2),
    ("ui/settings/shortcuts.rs", 3),
    ("ui/settings/software_update.rs", 8),
    ("ui/settings/storage.rs", 2),
    ("ui/settings/trackpad.rs", 4),
    ("ui/settings/wallpaper.rs", 2),
    ("ui/settings/widgets.rs", 4),
    ("ui/settings/wifi.rs", 2),
    ("ui/setup.rs", 5),
    ("ui/shortcut_sheet.rs", 1),
    ("ui/store.rs", 13),
    ("ui/store/details.rs", 4),
    ("ui/widgets_builtin.rs", 7),
    ("ui/widgets_gallery.rs", 5),
    ("ui/widgets_layout.rs", 2),
];

/// Of those, the ones `copy::lint` rejects. Only shrinks.
const LINT_BASELINE: [(&str, usize); 7] = [
    ("ui/settings/accounts.rs", 1),
    ("ui/settings/assistants.rs", 1),
    ("ui/settings/printers.rs", 2),
    ("ui/settings/software_update.rs", 2),
    ("ui/setup.rs", 2),
    ("ui/store.rs", 3),
    ("ui/widgets_gallery.rs", 1),
];

fn is_sentence(s: &str) -> bool {
    let Some(first) = s.chars().next() else {
        return false;
    };
    if !first.is_uppercase() || !s.contains(' ') || s.contains('\n') {
        return false;
    }
    let end = s.chars().next_back().unwrap_or(' ');
    if !matches!(end, '.' | '?' | '!' | '…') {
        return false;
    }
    // "Edit Widgets…", "Add App…": a menu item or button label.
    let title_case = s
        .split_whitespace()
        .all(|w| !w.starts_with(|c: char| c.is_alphabetic()) || w.starts_with(char::is_uppercase));
    !(end == '…' && title_case)
}

/// (file, line, sentence) for every on-screen sentence literal outside copy.rs.
fn sentences() -> Vec<(String, usize, String)> {
    let mut out = Vec::new();
    for s in scan::sources() {
        if COPY_FILES.contains(&s.path.as_str()) {
            continue;
        }
        for (line, lit) in &s.literals {
            if !is_sentence(lit) {
                continue;
            }
            let from = line.saturating_sub(3);
            let near = s.lines[from..*line].join(" ");
            if NOT_ON_SCREEN.iter().any(|m| near.contains(m)) {
                continue;
            }
            out.push((s.path.clone(), *line, lit.clone()));
        }
    }
    out
}

fn per_file(v: &[(String, usize, String)]) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for (f, _, _) in v {
        *m.entry(f.clone()).or_insert(0) += 1;
    }
    m
}

#[test]
fn the_detector_knows_a_sentence_from_a_label() {
    assert!(is_sentence("Nothing downloaded yet."));
    assert!(is_sentence("Open the app first, then add it here."));
    assert!(is_sentence("Searching for printers…"));
    assert!(is_sentence("Remove “{name}”?"));
    assert!(!is_sentence("Edit Widgets…"));
    assert!(!is_sentence("Show…"));
    assert!(!is_sentence("Wi-Fi"));
    assert!(!is_sentence("e.g."));
    assert!(!is_sentence("dispatch focuswindow."));
}

#[test]
fn sentences_live_in_copy() {
    let all = sentences();
    let listing: Vec<String> = all
        .iter()
        .map(|(f, l, s)| format!("{f}:{l}: {s}"))
        .collect();
    let r = std::panic::catch_unwind(|| {
        scan::ratchet(
            "sentences outside shell::copy (move them into copy.rs; R2A-19)",
            &per_file(&all),
            &BASELINE,
        )
    });
    assert!(r.is_ok(), "the sentences:\n{}", listing.join("\n"));
}

#[test]
fn sentences_outside_copy_pass_the_lint_or_are_frozen() {
    let bad: Vec<(String, usize, String)> = sentences()
        .into_iter()
        .filter(|(_, _, s)| !shell::copy::lint(s).is_empty())
        .collect();
    let listing: Vec<String> = bad
        .iter()
        .map(|(f, l, s)| format!("{f}:{l}: {s}: {:?}", shell::copy::lint(s)))
        .collect();
    let r = std::panic::catch_unwind(|| {
        scan::ratchet(
            "sentences outside shell::copy that copy::lint rejects (R2A-19)",
            &per_file(&bad),
            &LINT_BASELINE,
        )
    });
    assert!(r.is_ok(), "the sentences:\n{}", listing.join("\n"));
}
