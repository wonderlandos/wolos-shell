// SPDX-License-Identifier: GPL-2.0-or-later
//! wol-screenshot contract with the shell: prints the saved path and exits, even though
//! wl-copy leaves a clipboard daemon running. Live on the compositor: the daemon inherited stdout,
//! so the shell waited forever for EOF and never showed the thumbnail.
use std::time::{Duration, Instant};

const SCRIPT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../desktop/overlay/usr/bin/wol-screenshot"
);

fn stub(dir: &std::path::Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn prints_the_path_and_returns_while_the_clipboard_daemon_lives() {
    let tmp = std::env::temp_dir().join(format!("wol-shot-test-{}", std::process::id()));
    let bin = tmp.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    // grim writes the file; wl-copy forks a long-lived daemon like the real one.
    stub(&bin, "grim", r#"for a; do f=$a; done; printf png > "$f""#);
    stub(&bin, "wl-copy", "cat >/dev/null; (sleep 30) &");
    stub(&bin, "notify-send", "exit 0");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let t0 = Instant::now();
    let out = std::process::Command::new("sh")
        .args([SCRIPT, "full", "--dir"])
        .arg(tmp.join("shots"))
        .env("PATH", path)
        .env("WOL_SCREENSHOT_QUIET", "1")
        .output()
        .unwrap();
    let took = t0.elapsed();
    let printed = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        took < Duration::from_secs(5),
        "took {took:?}: stdout held open"
    );
    assert!(
        printed.ends_with(".png") && printed.contains("/shots/Screenshot "),
        "{printed}"
    );
}

/// Audit5 S9-6: no clipboard (wl-copy fails) and no banner (notify-send fails): the picture is
/// saved, its path still prints and the exit is 0, so the capture toolbar shows its thumbnail.
#[test]
fn a_clipboard_failure_still_prints_the_saved_path() {
    let tmp = std::env::temp_dir().join(format!("wol-shot-noclip-{}", std::process::id()));
    let bin = tmp.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    stub(&bin, "grim", r#"for a; do f=$a; done; printf png > "$f""#);
    stub(&bin, "wl-copy", "cat >/dev/null; exit 1");
    stub(&bin, "notify-send", "exit 1");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = std::process::Command::new("sh")
        .args([SCRIPT, "full", "--dir"])
        .arg(tmp.join("shots"))
        .env("PATH", path)
        .env_remove("WOL_SCREENSHOT_QUIET")
        .output()
        .unwrap();
    let printed = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    let saved = std::path::Path::new(&printed).is_file();
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(out.status.success(), "{err}");
    assert!(printed.contains("/shots/Screenshot "), "{printed}");
    assert!(saved, "the printed file exists: {printed}");
    assert!(err.contains("not copied to the clipboard"), "{err}");
}

/// ⇧⌘3 saves to the Desktop (macOS) and the banner names the folder, never the full path; the
/// picture rides along as the banner's thumbnail (`image-path` hint).
#[test]
fn saves_to_the_desktop_and_the_banner_shows_a_thumbnail_not_a_path() {
    let tmp = std::env::temp_dir().join(format!("wol-shot-banner-{}", std::process::id()));
    let bin = tmp.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let said = tmp.join("notify-args");
    stub(&bin, "grim", r#"for a; do f=$a; done; printf png > "$f""#);
    stub(&bin, "wl-copy", "cat >/dev/null");
    stub(
        &bin,
        "notify-send",
        &format!(
            r#"for a; do printf '%s\n' "$a"; done > '{}'"#,
            said.display()
        ),
    );
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = std::process::Command::new("sh")
        .args([SCRIPT, "full"])
        .env("PATH", path)
        .env("HOME", &tmp)
        .env_remove("XDG_DESKTOP_DIR")
        .env_remove("WOL_SCREENSHOT_QUIET")
        .output()
        .unwrap();
    let printed = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let args = std::fs::read_to_string(&said).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        printed.starts_with(&format!("{}/Desktop/Screenshot ", tmp.display())),
        "{printed}"
    );
    let args: Vec<&str> = args.lines().collect();
    assert!(args.contains(&"Screenshot saved to Desktop"), "{args:?}");
    assert!(
        args.contains(&format!("string:image-path:{printed}").as_str()),
        "{args:?}"
    );
    assert!(
        !args.contains(&printed.as_str()),
        "the path as text: {args:?}"
    );
}

/// C6-5: without a runtime dir, recording is refused (exit 1) and nothing in /tmp is read or
/// signalled; wol-zoom still toggles but remembers nothing outside the runtime dir.
#[test]
fn no_runtime_dir_means_no_fixed_tmp_names() {
    let tmp = std::env::temp_dir().join(format!("wol-shot-norun-{}", std::process::id()));
    let bin = tmp.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let said = tmp.join("hyprctl-args");
    stub(&bin, "wf-recorder", "exit 0");
    stub(
        &bin,
        "hyprctl",
        &format!(
            r#"[ "$1" = getoption ] && {{ echo "float: 3.000000"; exit 0; }}; echo "$*" >> '{}'"#,
            said.display()
        ),
    );
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    for mode in ["stop", "record"] {
        let out = std::process::Command::new("sh")
            .args([SCRIPT, mode])
            .env("PATH", &path)
            .env("HOME", &tmp)
            .env_remove("XDG_RUNTIME_DIR")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1), "{mode}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("XDG_RUNTIME_DIR"));
    }
    assert!(!tmp.join("Movies").exists(), "no recording started");
    let zoom = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../desktop/overlay/usr/bin/wol-zoom"
    );
    let out = std::process::Command::new("sh")
        .args([zoom, "toggle"])
        .env("PATH", &path)
        .env_remove("XDG_RUNTIME_DIR")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let args = std::fs::read_to_string(&said).unwrap();
    let _ = std::fs::remove_dir_all(&tmp);
    assert_eq!(args.trim(), "keyword cursor:zoom_factor 1");
}

/// C6-5: wol-zoom keeps the level it toggles away from in the person's own runtime dir only, and
/// reads it back only when the file is theirs (the owner check is POSIX `find -user`).
#[test]
fn zoom_toggle_remembers_the_level_in_the_runtime_dir() {
    let tmp = std::env::temp_dir().join(format!("wol-zoom-run-{}", std::process::id()));
    let (bin, run) = (tmp.join("bin"), tmp.join("run"));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&run).unwrap();
    let (said, level) = (tmp.join("hyprctl-args"), tmp.join("level"));
    // getoption answers the level last set (1 until a keyword call), keyword records it.
    stub(
        &bin,
        "hyprctl",
        &format!(
            r#"if [ "$1" = getoption ]; then echo "float: $(cat '{l}' 2>/dev/null || echo 3.000000)"; exit 0; fi; echo "$*" >> '{s}'; echo "$3" > '{l}'"#,
            l = level.display(),
            s = said.display()
        ),
    );
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let zoom = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../desktop/overlay/usr/bin/wol-zoom"
    );
    let toggle = || {
        let out = std::process::Command::new("sh")
            .args([zoom, "toggle"])
            .env("PATH", &path)
            .env("XDG_RUNTIME_DIR", &run)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    toggle(); // 3 -> 1, 3 saved
    assert_eq!(
        std::fs::read_to_string(run.join("wol-zoom"))
            .unwrap()
            .trim(),
        "3.000000"
    );
    toggle(); // 1 -> the saved 3
    // Junk in the file is never passed on: the default level 2.
    std::fs::write(run.join("wol-zoom"), "1; exec rm\n").unwrap();
    std::fs::write(&level, "1.000000").unwrap();
    toggle();
    let args = std::fs::read_to_string(&said).unwrap();
    let _ = std::fs::remove_dir_all(&tmp);
    assert_eq!(
        args.lines().collect::<Vec<_>>(),
        [
            "keyword cursor:zoom_factor 1",
            "keyword cursor:zoom_factor 3.000000",
            "keyword cursor:zoom_factor 2"
        ]
    );
}

/// C6-5: a pid file in the runtime dir that is not a regular file of ours is never trusted.
#[test]
fn record_state_lives_in_the_runtime_dir() {
    let tmp = std::env::temp_dir().join(format!("wol-shot-run-{}", std::process::id()));
    let (bin, run) = (tmp.join("bin"), tmp.join("run"));
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&run).unwrap();
    stub(&bin, "wf-recorder", "sleep 30");
    stub(&bin, "notify-send", "exit 0");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let go = |mode: &str| {
        std::process::Command::new("sh")
            .args([SCRIPT, mode])
            .env("PATH", &path)
            .env("HOME", &tmp)
            .env("XDG_RUNTIME_DIR", &run)
            .output()
            .unwrap()
    };
    assert!(go("record").status.success());
    let pidf = run.join("wol-record.pid");
    assert!(pidf.is_file(), "the pid file is in the runtime dir");
    let out = go("stop");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("Screen Recording "));
    assert!(!pidf.exists());
    let _ = std::fs::remove_dir_all(&tmp);
}
