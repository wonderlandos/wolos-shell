// SPDX-License-Identifier: GPL-2.0-or-later
//! STANDARDS §1 (audit3 R2A-20): no `unwrap()`, `expect()` or `as` casts that can fail on runtime
//! data. A grep cannot tell runtime data from a constant, so this is a ratchet: the count of
//! `.unwrap()`, `.expect(` and ` as i32` / ` as u32` / ` as usize` outside `#[cfg(test)]` items,
//! per file under `src/`, is frozen in [`BASELINE`] (2026-11-02). A new one is a review question
//! (use `?`, `unwrap_or`, `try_from`); removing one means lowering the number here.
#[path = "support/scan.rs"]
mod scan;

use std::collections::BTreeMap;

const NEEDLES: [&str; 5] = [".unwrap()", ".expect(", " as i32", " as u32", " as usize"];

/// Per file, outside tests. Only shrinks.
const BASELINE: [(&str, usize); 35] = [
    ("bin/wol-window-shot.rs", 5),
    ("clock.rs", 5),
    ("hypr.rs", 1),
    ("install.rs", 1),
    ("install_flow.rs", 1),
    ("kit/assistant.rs", 6),
    ("kit/check.rs", 4),
    ("kit/data.rs", 5),
    ("kit/form.rs", 2),
    // WALL-SHELL: the gate transition resamples a picture (pixel math: u8/u32 to f32 and back, bounded by MAX_WORK).
    ("kit/gate.rs", 17),
    ("kit/pickers.rs", 1),
    ("kit/widgets.rs", 2),
    ("shortcut_sheet.rs", 1),
    ("theme.rs", 1),
    ("ui/dock.rs", 1),
    // SETUP-FL: the keyboard page converts the dropdown index (u32 <-> usize) both ways.
    ("ui/first_login.rs", 2),
    ("ui/flatpak_consent.rs", 1),
    ("ui/installer/disks.rs", 6),
    ("ui/lock.rs", 9),
    ("ui/mod.rs", 1),
    ("ui/notify.rs", 9),
    ("ui/overview.rs", 6),
    ("ui/settings/accessibility.rs", 1),
    ("ui/settings/desktop_dock.rs", 2),
    ("ui/settings/keyboard.rs", 2),
    ("ui/settings/machine.rs", 1),
    ("ui/setup.rs", 3),
    ("ui/store/details.rs", 1),
    ("ui/widgets.rs", 6),
    ("ui/widgets_builtin.rs", 3),
    ("ui/widgets_layout.rs", 2),
    ("widgets/calendar.rs", 3),
    ("widgets/weather.rs", 4),
    ("widgets/worldclock.rs", 3),
    ("winshot.rs", 1),
];

/// Count in one code line; ` as usize` must end the word (` as usizes` is not a cast).
fn count_line(l: &str) -> usize {
    NEEDLES
        .iter()
        .map(|n| {
            l.match_indices(n)
                .filter(|(at, _)| {
                    !l[at + n.len()..]
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_alphanumeric() || c == '_')
                        || n.ends_with('(')
                })
                .count()
        })
        .sum()
}

#[test]
fn the_counter_skips_tests_comments_and_strings() {
    let s = scan::parse(
        "x.rs",
        "let a = x.unwrap(); // y.unwrap()\nlet b = n as u32 + m as usize;\n\
         let c = \"q.unwrap()\";\n#[cfg(test)]\nmod t { fn f() { z.unwrap(); } }\n",
    );
    let n: usize = s.bare.iter().map(|l| count_line(l)).sum();
    assert_eq!(n, 3);
    assert_eq!(count_line("x as u32x"), 0);
}

#[test]
fn unwrap_expect_and_casts_only_shrink() {
    let have: BTreeMap<String, usize> = scan::sources()
        .into_iter()
        .map(|s| (s.path, s.bare.iter().map(|l| count_line(l)).sum()))
        .collect();
    scan::ratchet(
        "unwrap/expect/as casts outside tests (STANDARDS §1; R2A-20)",
        &have,
        &BASELINE,
    );
}
