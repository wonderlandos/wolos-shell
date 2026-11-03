// SPDX-License-Identifier: GPL-2.0-or-later
//! STANDARDS §1 (audit3 R2A-17): the compositor is spoken to through one module, `shell::hypr`.
//! Raw JSON requests (`hypr::request("j/…")`), hand-built `"dispatch …"` strings and private
//! `.socket2.sock` readers appear only in `src/hypr.rs` and the one event reader
//! (`ui/ipc.rs` `hypr_events`). Everything else is frozen in [`BASELINE`]: the counts only come
//! down (typed helpers like `hypr::clients()`, a `Dispatch` type, the shared event stream); a
//! file may not grow past its row (`scan::ratchet`).
#[path = "support/scan.rs"]
mod scan;

use std::collections::BTreeMap;

const NEEDLES: [&str; 3] = ["hypr::request(\"j/", "\"dispatch ", "socket2.sock"];

/// The module itself.
const HOME: &str = "hypr.rs";

/// Uses outside `hypr.rs` (2026-11-02). `ui/ipc.rs` keeps 1 for its `.socket2.sock` reader, the
/// one event stream; the rest is debt. Lower a number when you remove a use; never raise one.
/// The menus, `wol-app`, Capture, the Dock, Overview and the switcher moved to
/// `hypr::Dispatch` and the typed requests (audit3 R2A-17).
const BASELINE: [(&str, usize); 2] = [("ui/ipc.rs", 4), ("ui/transient.rs", 1)];

fn counts() -> BTreeMap<String, usize> {
    scan::sources()
        .into_iter()
        .filter(|s| s.path != HOME)
        .map(|s| {
            let n = s.lines.iter().map(|l| scan::count_in(l, &NEEDLES)).sum();
            (s.path, n)
        })
        .collect()
}

#[test]
fn the_reader_sees_code_not_comments_or_tests() {
    let s = scan::parse(
        "x.rs",
        "// hypr::request(\"j/clients\")\nlet a = hypr::request(\"j/clients\"); /* \"dispatch x\" */\n\
         #[cfg(test)]\nmod tests { fn t() { let _ = \"dispatch y\"; } }\n",
    );
    let n: usize = s.lines.iter().map(|l| scan::count_in(l, &NEEDLES)).sum();
    assert_eq!(n, 1);
    assert_eq!(s.literals, [(2, "j/clients".to_string())]);
}

#[test]
fn hyprland_ipc_stays_in_the_hypr_module() {
    scan::ratchet(
        "hypr::request(\"j/…\"), \"dispatch …\" or .socket2.sock outside src/hypr.rs \
         (use the typed helpers in shell::hypr; R2A-17)",
        &counts(),
        &BASELINE,
    );
}
