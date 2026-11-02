// SPDX-License-Identifier: GPL-2.0-or-later
//! STANDARDS §1 (audit3 R2A-22): one helper per job. `kit::surface::on_escape` is the Esc
//! handler, `kit::surface::screen_of` the screen-geometry lookup, and new cross-module state is
//! not another `thread_local!`. The copies still around are frozen per file (2026-11-02): only
//! shrinks; switch a caller to the helper and lower its row.
#[path = "support/scan.rs"]
mod scan;

use std::collections::BTreeMap;

/// Where the helpers live, and `kit/form.rs` (a text field's Esc cancels the edit).
const HOMES: [&str; 2] = ["kit/surface.rs", "kit/form.rs"];

/// `Key::Escape` outside the helper, per file.
const ESCAPE: [(&str, usize); 10] = [
    ("kit/assistant.rs", 1),
    ("ui/analyze.rs", 1),
    ("ui/approval.rs", 1),
    ("ui/flatpak_consent.rs", 1),
    ("ui/lock.rs", 1),
    ("ui/mod.rs", 1),
    ("ui/notify.rs", 1),
    ("ui/switcher.rs", 1),
    ("ui/widgets.rs", 1),
    ("ui/widgets_builtin.rs", 1),
];

/// `monitor_at_surface` outside `kit/surface.rs`, per file.
const MONITOR: [(&str, usize); 0] = [];

/// `thread_local!` in `src/ui`, per file.
const THREAD_LOCAL: [(&str, usize); 8] = [
    ("ui/control_center/mod.rs", 1),
    ("ui/drop_shelf.rs", 1),
    ("ui/ipc.rs", 3),
    ("ui/lock.rs", 1),
    ("ui/menubar.rs", 1),
    ("ui/panels.rs", 2),
    ("ui/settings/mod.rs", 2),
    ("ui/transient.rs", 2),
];

fn per_file(needle: &str, keep: impl Fn(&str) -> bool) -> BTreeMap<String, usize> {
    scan::sources()
        .into_iter()
        .filter(|s| keep(&s.path))
        .map(|s| {
            let n = s.bare.iter().map(|l| l.matches(needle).count()).sum();
            (s.path, n)
        })
        .collect()
}

#[test]
fn esc_goes_through_on_escape() {
    scan::ratchet(
        "Key::Escape handlers outside kit::surface::on_escape (R2A-22)",
        &per_file("Key::Escape", |p| !HOMES.contains(&p)),
        &ESCAPE,
    );
}

#[test]
fn screen_geometry_goes_through_screen_of() {
    scan::ratchet(
        "monitor_at_surface outside kit::surface::screen_of (R2A-22)",
        &per_file("monitor_at_surface", |p| p != HOMES[0]),
        &MONITOR,
    );
}

#[test]
fn ui_thread_locals_only_shrink() {
    scan::ratchet(
        "thread_local! in src/ui (pass state to the builders instead; R2A-22)",
        &per_file("thread_local!", |p| p.starts_with("ui/")),
        &THREAD_LOCAL,
    );
}
