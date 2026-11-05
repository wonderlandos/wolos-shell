// SPDX-License-Identifier: GPL-2.0-or-later
//! STANDARDS §1 (audit3 R2A-21): the GTK main thread never blocks. In `src/` the blocking calls
//! below appear only inside a `gio::spawn_blocking` / `off_thread` /
//! `std::thread::spawn` closure, or in a function that is only handed to one. Heuristic: a call
//! is off the main thread when a wrapper starts within [`WINDOW`] lines above it (same statement),
//! or when its enclosing `fn` is named within [`WINDOW`] lines after a wrapper in the same file.
//! What is left is frozen per file in [`BASELINE`] (2026-11-02): only shrinks.
#[path = "support/scan.rs"]
mod scan;

use std::collections::BTreeMap;

/// Calls that wait on another process, a socket or the disk.
const BLOCKING: [&str; 29] = [
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
    // A whole picture file decoded into an Image or a Texture (QA-A10-SD-02: Store > Web Apps
    // and the Dock read a downloaded site icon on the spot; `kit::picture::file_image` is the way).
    "set_from_file(",
    "Texture::from_filename(",
    ".auto_updates(",
    ".boot_key_pending(",
    // `System::undo/history/boot_environments` read and write the journal and run the reloads
    // (systemctl, hyprctl, bootctl) of an undo (QA-SET-2: Settings froze 2-4 s on an Undo).
    "sys.undo(",
    "sys().undo(",
    ".history()",
    ".boot_environments()",
];

/// `.output()` / `.status()` count only on a `Command` (within the window).
const NEEDS_COMMAND: [&str; 2] = [".output()", ".status()"];

const WRAPPERS: [&str; 6] = [
    "spawn_blocking",
    // kit::render::on(lane, work): the pictures' own threads.
    "render::on(",
    "off_thread(",
    "thread::spawn",
    "thread::Builder",
    // ui/settings `later(into, load, render)`: `load` runs in gio::spawn_blocking.
    "later(",
];

const WINDOW: usize = 6;

/// Scope: every file under `src/` (SP-5). It was `ui/`, `kit/` and `theme.rs`, which missed GTK
/// code at the top level (`theme.rs` itself until audit6 S6-2) and any new folder. Modules whose
/// functions are blocking by contract ("run off the GTK thread": `hypr.rs`, `system.rs`, ...)
/// are frozen in [`BASELINE`] like the rest, so a new blocking helper there is a choice made
/// in review, not a blind spot.
const SCOPE: [&str; 1] = [""];

/// Blocking calls on the main thread, per file. Only shrinks.
const BASELINE: [(&str, usize); 44] = [
    // Top-level modules, counted when the scope became all of `src/` (SP-5, frozen 2026-11-03).
    // Most are blocking helpers by contract (their callers hand them to a worker; the heuristic
    // cannot follow a call into another file); each row only shrinks.
    // load_shown (the file of the update check already told about) and the malware lists are
    // only called inside gio::spawn_blocking (`alerts::read`); the scan sees the calls, not the
    // thread they run on.
    ("alerts.rs", 2),
    ("app_session.rs", 1),
    // A command-line tool (menu bar verbs bound to keys): no GTK main loop to stall.
    ("bin/wol-app.rs", 7),
    ("desktop.rs", 2),
    ("files.rs", 2),
    ("focus.rs", 1),
    ("hypr.rs", 1),
    ("install.rs", 3),
    ("install_flow.rs", 1),
    ("login.rs", 3),
    ("offers.rs", 2),
    ("packages.rs", 2),
    ("sessions.rs", 1),
    ("setup.rs", 1),
    // E2E-SHARE: the usage file, the applications folders and the text file are read and written by blocking helpers whose callers are all worker threads.
    ("share_targets.rs", 3),
    ("software_update.rs", 1),
    // The `--supervised` parent (SP-1): std only, no GTK main loop to stall. Runs the shell
    // (`.status()`), journals restarts, checks the display socket and reads where its stderr goes.
    ("supervise.rs", 3),
    ("system.rs", 8),
    // SETUP-FL: Facts::read at the page build is two small file reads and one `gsettings get`, once per
    // account at its first sign-in, before the desktop exists (nothing else is on screen to freeze).
    ("ui/first_login.rs", 2),
    ("widgets/weather.rs", 1),
    // Picture/Image file decodes, counted since audit4 S3-3 (frozen 2026-11-02).
    // + Texture::from_filename (QA-A10-SD-02 needle, counted 2026-11-05): the shipped illustration
    // SVG of `surface`, an image file of the package, not outside input.
    ("kit/assistant.rs", 2),
    // Texture::from_filename (QA-A10-SD-02 needle, counted 2026-11-05): the MPRIS art file a media
    // app names, outside input decoded on the spot; reported to the CTO as a kit::picture follow-up.
    ("kit/controls.rs", 1),
    ("kit/data.rs", 1),
    // wallpaper_choice: one small config file at desktop build and on change.
    ("kit/pickers.rs", 1),
    // kit::picture::decode_with, the one bounded picture decode: blocking by contract; its callers
    // are `file_image` (inside gio::spawn_blocking), notify's preload and a golden render
    // (QA-A9-SHELL-05).
    ("kit/picture.rs", 1),
    // theme id file on the theme monitor's change signal: one line.
    ("kit/style.rs", 1),
    // Pixbuf::from_file_at_scale in the icon loader's closure, counted since the S7-4 token
    // (frozen 2026-11-02).
    ("kit/widgets.rs", 1),
    ("ui/analyze.rs", 1),
    // Texture::from_filename (QA-A10-SD-02 needle, counted 2026-11-05): the screenshot the shell
    // just took (thumbnail) and the one Markup opens, pictures of this screen it made itself.
    ("ui/capture.rs", 2),
    // register_icons: the few small SVG files of a Kit window's named icons, read from the local theme
    // when the window is mounted (WOLKIT-1TO1; INT-2: a row, the page cannot draw an icon it has not read).
    ("ui/app_wolkit.rs", 1),
    // downloads_dir: user-dirs.dirs once when the Dock is built.
    ("ui/dock.rs", 1),
    ("ui/flatpak_consent.rs", 1),
    ("ui/installer.rs", 4),
    ("ui/ipc.rs", 1),
    ("ui/lock.rs", 5),
    // remove_sandbox: the preview's few named files, once, after `app.run()` returned (the
    // SandboxGuard drops when no window or main loop is left), so nothing waits on it.
    ("ui/maker/preview.rs", 1),
    ("ui/mod.rs", 2),
    ("ui/settings/login_items.rs", 2),
    ("ui/settings/machine.rs", 1),
    ("ui/settings/privacy.rs", 1),
    ("ui/shortcut_sheet.rs", 1),
    // The analysis graph Picture (audit4 S3-3 count).
    ("ui/store.rs", 1),
    // Split out of store.rs, same calls as before: the app's icon Image (audit4 S3-3 count) and a
    // local screenshot's Pixbuf::from_file_at_scale (S7-4 token, frozen 2026-11-02); a remote
    // screenshot is already decoded in `gio::spawn_blocking`.
    ("ui/store/details.rs", 2),
    // Texture::from_filename (QA-A10-SD-02 needle, counted 2026-11-05): a tray item's icon given
    // as a file path, outside input decoded on the spot; reported to the CTO as a kit::picture
    // follow-up.
    ("ui/tray_items.rs", 1),
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
            "blocking calls on the GTK thread in src/ (move them into gio::spawn_blocking; \
             STANDARDS §1, R2A-21)",
            &have,
            &BASELINE,
        )
    });
    assert!(r.is_ok(), "the calls:\n{}", listing.join("\n"));
}

/// SP-5 (audit6 S6-2): the scope was `ui/` and `kit/`, so `theme.rs` walked the theme folders on
/// the GTK thread unseen. Every file under `src/` is checked, top-level ones included.
#[test]
fn the_scope_is_every_source_file() {
    let all = scan::sources();
    assert!(all.iter().any(|s| s.path == "theme.rs"));
    let out: Vec<&str> = all
        .iter()
        .map(|s| s.path.as_str())
        .filter(|p| !SCOPE.iter().any(|s| p.starts_with(s)))
        .collect();
    assert!(out.is_empty(), "outside the ratchet: {out:?}");
}
