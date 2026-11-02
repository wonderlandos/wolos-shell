// SPDX-License-Identifier: GPL-2.0-or-later
//! Every `ShellPrefs` field Settings writes is read by the part of the shell it configures (HIG:
//! every control applies for real or is not shown, ui/settings/mod.rs). Audit2 A2-5: "Show recent
//! applications in Dock" and "Minimize effect speed" moved and persisted but nothing read them.
//! A field counts as read when code outside `ui/settings/` names it (`.field`) or calls a
//! `ShellPrefs` method whose body reads it (`tile_size()` reads `dock_tile_size`).
use std::path::{Path, PathBuf};

const PREFS: &str = include_str!("../../core/src/prefs.rs");

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Field names of `pub struct ShellPrefs`.
fn fields() -> Vec<String> {
    let body = PREFS
        .split_once("pub struct ShellPrefs {")
        .and_then(|(_, rest)| rest.split_once("\n}"))
        .map(|(b, _)| b)
        .expect("ShellPrefs in prefs.rs");
    body.lines()
        .filter_map(|l| l.trim().strip_prefix("pub "))
        .filter_map(|l| l.split_once(':'))
        .map(|(name, _)| name.trim().to_string())
        .collect()
}

/// `impl ShellPrefs` methods: (name, body).
fn methods() -> Vec<(String, String)> {
    let Some((_, rest)) = PREFS.split_once("impl ShellPrefs {") else {
        return vec![];
    };
    let block = rest.split_once("\n}").map_or(rest, |(b, _)| b);
    block
        .split("pub fn ")
        .skip(1)
        .filter_map(|m| {
            let (name, body) = m.split_once('(')?;
            Some((name.trim().to_string(), body.to_string()))
        })
        .collect()
}

/// A name followed by something that is not part of an identifier.
fn names(src: &str, needle: &str) -> bool {
    src.match_indices(needle).any(|(i, _)| {
        src[i + needle.len()..]
            .chars()
            .next()
            .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'))
    })
}

#[test]
fn every_shell_pref_settings_writes_has_a_reader() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rs_files(&src, &mut files);
    let (mut settings, mut readers) = (String::new(), String::new());
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        let rel = f
            .strip_prefix(&src)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if rel.starts_with("ui/settings/") {
            settings.push_str(&text);
        } else {
            readers.push_str(&text);
        }
    }
    let fields = fields();
    assert!(fields.len() >= 8, "parsed only {fields:?}");
    let methods = methods();
    let mut written = 0;
    for f in &fields {
        if !names(&settings, &format!(".{f} =")) && !names(&settings, &format!(".{f}.")) {
            continue;
        }
        written += 1;
        let direct = names(&readers, &format!(".{f}"));
        let via = methods.iter().any(|(m, body)| {
            names(body, &format!("self.{f}")) && readers.contains(&format!(".{m}("))
        });
        assert!(
            direct || via,
            "ShellPrefs::{f} is written by Settings but nothing outside ui/settings reads it: \
             wire it or drop the control and the field"
        );
    }
    assert!(written >= 6, "found only {written} fields Settings writes");
}
