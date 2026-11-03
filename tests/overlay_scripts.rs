// SPDX-License-Identifier: GPL-2.0-or-later
//! Contract for the desktop scripts the hotkeys run (desktop/overlay/usr/bin, docs/STANDARDS.md
//! §2): POSIX `sh` with the SPDX line, a header and `set -eu`, parse clean under `sh -n` (and
//! `dash -n`, `shellcheck -S error` where installed), and the behaviour the binds rely on, with
//! the tools stubbed: `wol-volume micmute` mutes the default input, `wol-screenshot --copy`
//! writes no file, and a window box or delay that is not digits is refused (ADR 0031 W5).
use std::path::{Path, PathBuf};
use std::process::Command;

const SCRIPTS: [&str; 2] = ["wol-screenshot", "wol-volume"];

fn script(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../desktop/overlay/usr/bin")
        .join(name)
}

fn has(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok()
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wol-overlay-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("bin")).unwrap();
    d
}

/// A stub that appends its name and arguments, one line, to `log`. The line is one write, so
/// two stubs in a pipeline (`grim - | wl-copy`) never interleave their words.
fn stub(dir: &Path, name: &str, log: &Path, extra: &str) {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("bin").join(name);
    let body = format!(
        "#!/bin/sh\nl='{name}'; for a; do l=\"$l $a\"; done; printf '%s\n' \"$l\" >> '{log}'\n{extra}\n",
        log = log.display()
    );
    std::fs::write(&p, body).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn path_with(dir: &Path) -> String {
    format!(
        "{}:{}",
        dir.join("bin").display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

#[test]
fn scripts_are_posix_sh_with_a_header_and_set_eu() {
    for name in SCRIPTS {
        let text = std::fs::read_to_string(script(name)).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.first(), Some(&"#!/bin/sh"), "{name}");
        assert_eq!(
            lines.get(1),
            Some(&"# SPDX-License-Identifier: GPL-2.0-or-later"),
            "{name}"
        );
        assert!(
            lines.get(2).is_some_and(|l| l.starts_with("# ")),
            "{name}: header"
        );
        assert!(lines.contains(&"set -eu"), "{name}: set -eu");
        for bashism in [
            "[[",
            "function ",
            "local ",
            "$'",
            "<<<",
            "echo -e",
            "pipefail",
        ] {
            assert!(!text.contains(bashism), "{name}: bash syntax {bashism:?}");
        }
        let mut checks = vec![("sh", vec!["-n"])];
        if has("dash") {
            checks.push(("dash", vec!["-n"]));
        }
        if has("shellcheck") {
            checks.push(("shellcheck", vec!["--shell=sh", "-S", "error"]));
        } else {
            eprintln!("shellcheck not installed: CI runs it");
        }
        for (tool, args) in checks {
            let out = Command::new(tool)
                .args(&args)
                .arg(script(name))
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{tool} {name}: {}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
}

#[test]
fn the_mic_key_mutes_the_default_input_and_shows_the_osd() {
    let d = tmp("mic");
    let log = d.join("log");
    stub(&d, "wpctl", &log, "");
    stub(&d, "wol", &log, "");
    let run = |word: &str| {
        Command::new("sh")
            .arg(script("wol-volume"))
            .arg(word)
            .env("PATH", path_with(&d))
            .output()
            .unwrap()
    };
    assert!(run("micmute").status.success());
    let said = std::fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(
        said, "wpctl set-mute @DEFAULT_AUDIO_SOURCE@ toggle\nwol shell show osd-mic\n",
        "the mic key shows the microphone's state (audit5 S9-3)"
    );
    assert_eq!(run("dance").status.code(), Some(64));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn copy_only_writes_no_file_and_prints_nothing() {
    let d = tmp("copy");
    let log = d.join("log");
    stub(&d, "grim", &log, "printf png");
    stub(&d, "slurp", &log, "echo '1,2 3x4'");
    stub(&d, "wl-copy", &log, "cat >/dev/null");
    stub(&d, "notify-send", &log, "");
    let shots = d.join("shots");
    let out = Command::new("sh")
        .arg(script("wol-screenshot"))
        .args(["region", "--copy", "--dir"])
        .arg(&shots)
        .env("PATH", path_with(&d))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty(), "nothing to open");
    assert!(!shots.exists(), "no file");
    let said = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(said.contains("grim -g 1,2 3x4 -\n"), "{said}");
    assert!(said.contains("wl-copy -t image/png\n"), "{said}");
    let _ = std::fs::remove_dir_all(&d);
}

/// audit5 S9-6: a failed clipboard copy exited before the path printed (the toolbar showed a
/// failure for a file that exists).
#[test]
fn a_failed_clipboard_copy_still_prints_the_saved_path() {
    let d = tmp("noclip");
    let log = d.join("log");
    stub(
        &d,
        "grim",
        &log,
        "f=; for a; do f=$a; done; printf png > \"$f\"",
    );
    stub(&d, "wl-copy", &log, "exit 1");
    let shots = d.join("shots");
    let out = Command::new("sh")
        .arg(script("wol-screenshot"))
        .args(["full", "--dir"])
        .arg(&shots)
        .env("PATH", path_with(&d))
        .env("WOL_SCREENSHOT_QUIET", "1")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(path.starts_with(&shots.display().to_string()), "{path:?}");
    assert!(Path::new(&path).is_file(), "{path:?}");
    assert!(err.contains("not copied to the clipboard"), "{err}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_window_box_or_delay_that_is_not_digits_is_refused() {
    let d = tmp("bad");
    let log = d.join("log");
    stub(&d, "grim", &log, "");
    for args in [
        &["window", "1,2;id", "3x4"][..],
        &["window", "1,2", "3x4x5"],
        &["window", "12", "3x4"],
        &["window", "1,2"],
        &["full", "--delay", "1;id"],
        &["full", "--dir"],
        &["full", "--bogus"],
        &["sideways"],
    ] {
        let out = Command::new("sh")
            .arg(script("wol-screenshot"))
            .args(args)
            .env("PATH", path_with(&d))
            .env("HOME", &d)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(64), "{args:?}");
    }
    assert!(!log.exists(), "grim never ran");
    // A negative offset (a display left of the main one) is a real window box.
    let out = Command::new("sh")
        .arg(script("wol-screenshot"))
        .args(["window", "-1920,0", "800x600", "--dir"])
        .arg(d.join("shots"))
        .env("PATH", path_with(&d))
        .env("WOL_SCREENSHOT_QUIET", "1")
        .output()
        .unwrap();
    let said = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(said.starts_with("grim -g -1920,0 800x600 "), "{said}");
    let _ = (out, std::fs::remove_dir_all(&d));
}
