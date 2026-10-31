// SPDX-License-Identifier: GPL-2.0-or-later
//! Every layer surface the shell builds survives its display being unplugged: either
//! `survive_output_loss` (the primary menu bar, Dock and desktop come back; panels hide),
//! `set_hide_on_close`, or it belongs to one display (`set_monitor`: `ui::displays` builds it
//! when the display comes and drops it when the display goes). Otherwise gtk4-layer-shell's
//! close on output loss destroys it for good.
use std::path::Path;

/// Windows that are meant to go away when closed: a panel app's own window and its
/// click-catcher (Search quits on close).
const CLOSES_ON_PURPOSE: &[(&str, usize)] = &[("mod.rs", 1), ("transient.rs", 1)];

fn count(s: &str, pat: &str) -> usize {
    s.matches(pat).count()
}

#[test]
fn shell_surfaces_survive_their_display_going_away() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui");
    let mut files = vec![];
    let mut stack = vec![dir];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                files.push(p);
            }
        }
    }
    let mut bad = vec![];
    for f in files {
        let src = std::fs::read_to_string(&f).unwrap();
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        let built = count(&src, "super::window(")
            + count(&src, "let w = window(app")
            + count(&src, "let win = window(app");
        let exempt = CLOSES_ON_PURPOSE
            .iter()
            .find(|e| e.0 == name)
            .map_or(0, |e| e.1);
        let kept = count(&src, "survive_output_loss(")
            + count(&src, "set_hide_on_close(true)")
            + count(&src, "surface::set_monitor(");
        if built > kept + exempt {
            bad.push(format!("{name}: {built} surfaces, {kept} kept"));
        }
    }
    assert!(bad.is_empty(), "surfaces destroyed on output loss: {bad:?}");
}
