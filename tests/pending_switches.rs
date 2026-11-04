// SPDX-License-Identifier: GPL-2.0-or-later
//! Settings controls that change something outside the window wait for the change and go back,
//! with the reason under the pane, when it fails or is cancelled (RC-01, RH-011): switches are
//! `w::pending_toggle` / `form::pending_toggle_row`, menus `form::pending_picker*`, entries
//! `form::pending_entry_row`, app grants `form::pending_permission_row`. A plain control there
//! would show a change that never happened (QA-SET-4, -9, -19, -29, -37, -39, -40, QA-HUNT-10).
use std::path::Path;

/// Plain controls: their handler runs after the widget already shows the new value.
const PLAIN: [&str; 7] = [
    "w::toggle(",
    "form::toggle_row(",
    "form::picker_row(",
    "form::search_picker_row(",
    "form::permission_row(",
    "form::entry_row(",
    "connect_selected_notify(",
];

/// What reaches the host from a handler: an admin action, a journaled save, a spawn.
const REACHES: [&str; 12] = [
    "ctx.act(",
    "act_switch(",
    "act_say(",
    "save_journaled(",
    "save_entry(",
    "save_shell(",
    "focus::write(",
    "change(",
    "kit::spawn(",
    ".set_helpers_enabled(",
    ".set_helper_capability(",
    "admin(",
];

/// The argument list that starts right after `open` (an opening parenthesis already consumed):
/// up to its matching `)`.
fn args(after: &str) -> &str {
    let mut depth = 1usize;
    for (i, c) in after.char_indices() {
        match c {
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => {
                depth -= 1;
                if depth == 0 {
                    return &after[..i];
                }
            }
            _ => {}
        }
    }
    after
}

/// Every (file, control, handler text) in Settings whose plain control reaches the host.
fn offenders() -> Vec<String> {
    // WOL_RULE_DIR: another copy of the panes (the proof run against the unfixed tree).
    let dir = std::env::var_os("WOL_RULE_DIR").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui/settings"),
        std::path::PathBuf::from,
    );
    let mut bad = Vec::new();
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .collect();
    files.sort();
    for p in files {
        let src = std::fs::read_to_string(&p).unwrap();
        let code = src.split("#[cfg(test)]").next().unwrap();
        for plain in PLAIN {
            for (at, _) in code.match_indices(plain) {
                // `form::pending_toggle_row(` contains no plain name; `w::pending_toggle(` neither.
                let body = args(&code[at + plain.len()..]);
                if let Some(r) = REACHES.iter().find(|r| body.contains(*r)) {
                    let line = code[..at].lines().count() + 1;
                    bad.push(format!(
                        "{}:{line}: {plain}..) calls {r}..): make it a pending control",
                        p.file_name().unwrap().to_string_lossy()
                    ));
                }
            }
        }
    }
    bad
}

#[test]
fn settings_preference_switches_revert_on_failure() {
    let bad = offenders();
    assert!(
        bad.is_empty(),
        "plain controls that reach the host:\n{}",
        bad.join("\n")
    );
}

/// The rule sees through nested closures: a switch that saves inside a `match` still counts.
#[test]
fn the_rule_reads_nested_handlers() {
    let body = args("on, move |v| { match x { _ => ctx.act(a, d) } }) ; after(");
    assert!(body.contains("ctx.act("));
    assert!(!body.contains("after("));
}
