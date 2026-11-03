// SPDX-License-Identifier: GPL-2.0-or-later
//! STANDARDS §1 (audit3 R2A-21): the GTK main thread never blocks. In `src/ui` and `src/kit` the
//! blocking calls below appear only inside a `gio::spawn_blocking` / `off_thread` /
//! `std::thread::spawn` closure, or in a function that is only handed to one. Heuristic: a call
//! is off the main thread when a wrapper starts within [`WINDOW`] lines above it (same statement),
//! or when its enclosing `fn` is named within [`WINDOW`] lines after a wrapper in the same file.
//! What is left is frozen per file in [`BASELINE`] (2026-11-02): only shrinks.
#[path = "support/scan.rs"]
mod scan;

use std::collections::BTreeMap;

/// Calls that wait on another process, a socket or the disk.
const BLOCKING: [&str; 23] = [
    "hypr::request(",
    "hypr::clients(",
    // The typed requests in `shell::hypr` (R2A-17) wait on the compositor too.
    "hypr::read_clients",
    "hypr::active_window(",
    "hypr::active_workspace(",
    "hypr::read_fullscreen",
    "hypr::zone(",
    "hypr::layers(",
    "hypr::dispatch(",
    "UnixStream::connect(",
    "fs::read_dir(",
    "fs::read_to_string(",
    "desktop::load_all(",
    ".output()",
    ".status()",
    // Decode a whole picture file on the spot (audit4 S3-3: a 4K screenshot per banner).
    "Picture::for_filename(",
    "Image::from_file(",
    // Walk the theme folders and every enabled plugin's theme pack (audit5 S8-7).
    "ThemeCatalog::load(",
    "ThemeCatalog::installed(",
    "packs::themes(",
    // A whole picture decoded (any size), and Machine calls that run systemctl or stat (S7-4/S7-7).
    "Pixbuf::from_file",
    ".auto_updates(",
    ".boot_key_pending(",
];

/// `.output()` / `.status()` count only on a `Command` (within the window).
const NEEDS_COMMAND: [&str; 2] = [".output()", ".status()"];

const WRAPPERS: [&str; 5] = [
    "spawn_blocking",
    "off_thread(",
    "thread::spawn",
    "thread::Builder",
    // ui/settings `later(into, load, render)`: `load` runs in gio::spawn_blocking.
    "later(",
];

const WINDOW: usize = 6;

/// Scope: code that runs on the GTK thread.
const SCOPE: [&str; 2] = ["ui/", "kit/"];

/// Blocking calls on the main thread, per file. Only shrinks.
const BASELINE: [(&str, usize); 18] = [
    // Picture/Image file decodes, counted since audit4 S3-3 (frozen 2026-11-02).
    ("kit/assistant.rs", 1),
    ("kit/data.rs", 1),
    // wallpaper_choice: one small config file at desktop build and on change.
    ("kit/pickers.rs", 1),
    // theme id file on the theme monitor's change signal: one line.
    ("kit/style.rs", 1),
    // Pixbuf::from_file_at_scale in the icon loader's closure, counted since the S7-4 token
    // (frozen 2026-11-02).
    ("kit/widgets.rs", 1),
    ("ui/analyze.rs", 1),
    ("ui/app_view.rs", 1),
    // downloads_dir: user-dirs.dirs once when the Dock is built.
    ("ui/dock.rs", 1),
    // One Image::from_file of the app's icon (audit4 S3-3 count).
    ("ui/flatpak_consent.rs", 2),
    ("ui/installer.rs", 4),
    ("ui/ipc.rs", 1),
    ("ui/lock.rs", 5),
    ("ui/mod.rs", 3),
    ("ui/settings/login_items.rs", 2),
    // + the demo Machine's `Demo.auto_updates()` delegation (S7-7 token; no I/O there).
    ("ui/settings/machine.rs", 2),
    ("ui/settings/privacy.rs", 1),
    ("ui/shortcut_sheet.rs", 1),
    // Screenshot Picture and an icon Image (audit4 S3-3 count), and the screenshot thumb's
    // Pixbuf::from_file_at_scale (S7-4 token, frozen 2026-11-02).
    ("ui/store.rs", 3),
];

/// The name of the `fn` that encloses line `i` (0-based), if any.
fn enclosing_fn(bare: &[String], i: usize) -> Option<String> {
    bare[..=i].iter().rev().find_map(|l| {
        let at = l.find("fn ")?;
        if at > 0
            && l[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
        {
            return None;
        }
        let name: String = l[at + 3..]
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        (!name.is_empty()).then_some(name)
    })
}

fn is_word_in(hay: &str, word: &str) -> bool {
    hay.match_indices(word).any(|(at, _)| {
        let before = hay[..at].chars().next_back();
        let after = hay[at + word.len()..].chars().next();
        let id = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        !id(before) && !id(after)
    })
}

/// (line, call) for every blocking call in `s` the heuristic finds on the main thread.
fn on_main_thread(s: &scan::Src) -> Vec<(usize, &'static str)> {
    let bare = &s.bare;
    // Functions named right after a wrapper: handed to a worker thread.
    let mut offloaded = Vec::new();
    for (i, l) in bare.iter().enumerate() {
        if WRAPPERS.iter().any(|w| l.contains(w)) {
            offloaded.push(bare[i..(i + WINDOW).min(bare.len())].join(" "));
        }
    }
    let mut out = Vec::new();
    for (i, l) in bare.iter().enumerate() {
        for call in BLOCKING {
            if !l.contains(call) {
                continue;
            }
            let above = bare[i.saturating_sub(WINDOW)..=i].join(" ");
            if NEEDS_COMMAND.contains(&call) && !above.contains("Command") {
                continue;
            }
            if WRAPPERS.iter().any(|w| above.contains(w)) {
                continue;
            }
            if enclosing_fn(bare, i).is_some_and(|f| offloaded.iter().any(|o| is_word_in(o, &f))) {
                continue;
            }
            out.push((i + 1, call));
        }
    }
    out
}

#[test]
fn the_heuristic_sees_wrappers_and_offloaded_functions() {
    let s = scan::parse(
        "ui/x.rs",
        "fn c() -> Option<String> {\n    hypr::request(\"j/y\")\n}\n\
         fn f() {\n    s.status();\n}\n\
         fn a() {\n    let j = hypr::request(\"j/x\");\n}\n\
         fn e() {\n    let o = Command::new(\"x\")\n        .output();\n}\n\
         fn b() {\n    gio::spawn_blocking(|| hypr::clients());\n}\n\
         fn d() {\n    let t = gio::spawn_blocking(c);\n}\n",
    );
    let hits = on_main_thread(&s);
    assert_eq!(hits, [(8, "hypr::request("), (12, ".output()")]);
}

#[test]
fn the_main_thread_never_blocks() {
    let mut have = BTreeMap::new();
    let mut listing = Vec::new();
    for s in scan::sources() {
        if !SCOPE.iter().any(|p| s.path.starts_with(p)) {
            continue;
        }
        let hits = on_main_thread(&s);
        for (l, c) in &hits {
            listing.push(format!("{}:{l}: {c}", s.path));
        }
        have.insert(s.path.clone(), hits.len());
    }
    let r = std::panic::catch_unwind(|| {
        scan::ratchet(
            "blocking calls on the GTK thread in src/ui, src/kit (move them into \
             gio::spawn_blocking; STANDARDS §1, R2A-21)",
            &have,
            &BASELINE,
        )
    });
    assert!(r.is_ok(), "the calls:\n{}", listing.join("\n"));
}
