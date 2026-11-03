// SPDX-License-Identifier: GPL-2.0-or-later
//! SP-2: live config. Saving `shell.json`, `widgets.json` or the user's `kit.css` applies within
//! 200 ms without `wol shell reload`; a file that does not load keeps the last good state and its
//! one notice names the key (`shell::ui::ipc::watch_prefs`, `shell::kit::style::user_css`).
use gtk::glib;
use shell::kit::style;
use shell::ui::ipc;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};
use wol_core::prefs::{ConfigError, SHELL_NAME, ShellPrefs, WIDGETS_NAME};

/// The goal: a save shows within this.
const APPLY_WITHIN: Duration = Duration::from_millis(200);
/// How long a save that must not apply is watched.
const QUIET: Duration = Duration::from_millis(400);

#[test]
fn events_pick_the_watched_file_and_skip_own_saves() {
    assert_eq!(ipc::WATCHED, ["shell.json", "widgets.json", "kit.css"]);
    assert_eq!(ipc::DEBOUNCE_MS, 100);
    assert_eq!(ipc::touched("shell.json", None, 7), Some(0));
    assert_eq!(ipc::touched("kit.css", None, 7), Some(2));
    assert_eq!(ipc::touched("plugins.json", None, 7), None);
    assert_eq!(
        ipc::touched("widgets.json.7-ab12.tmp", None, 7),
        None,
        "temp file"
    );
    // The shell's own atomic save: its temp file renamed over the file.
    assert_eq!(
        ipc::touched("widgets.json.7-ab12.tmp", Some("widgets.json"), 7),
        None
    );
    // Another process (Settings, an editor) saving the same way.
    assert_eq!(
        ipc::touched("widgets.json.8-ab12.tmp", Some("widgets.json"), 7),
        Some(1)
    );
    assert_eq!(
        ipc::touched("shell.json.77-x.tmp", Some("shell.json"), 7),
        Some(0)
    );
    assert_eq!(
        ipc::touched(".shell.json.swp", Some("shell.json"), 7),
        Some(0)
    );
    // An editor moving the old file aside before it writes the new one.
    assert_eq!(ipc::touched("shell.json", Some("shell.json~"), 7), Some(0));
}

#[test]
fn the_notice_names_the_key_in_plain_words() {
    let e = |file, key: Option<&str>, line| ConfigError {
        file,
        key: key.map(String::from),
        line,
        detail: "invalid type: string \"big\", expected u32".into(),
    };
    let t = ipc::notice_text(&e(SHELL_NAME, Some("dock_tile_size"), None));
    assert!(
        t.starts_with("“dock_tile_size” in your menu bar and Dock settings"),
        "{t}"
    );
    let t = ipc::notice_text(&e(WIDGETS_NAME, None, Some(3)));
    assert!(t.contains("desktop widgets") && t.contains("line 3"), "{t}");
    let t = ipc::notice_text(&e(style::USER_CSS_NAME, Some("colr"), Some(2)));
    assert!(t.contains("“colr” on line 2 of your style sheet"), "{t}");
    for f in [SHELL_NAME, WIDGETS_NAME, style::USER_CSS_NAME] {
        for t in [
            ipc::notice_text(&e(f, Some("k"), None)),
            ipc::notice_text(&e(f, None, None)),
        ] {
            let p = shell::copy::lint(&t);
            assert!(p.is_empty(), "{t}: {p:?}");
            assert!(
                !t.contains(".json") && !t.contains(".css") && !t.contains("expected"),
                "{t}"
            );
            assert!(t.chars().count() <= shell::copy::MAX_LEN, "{t}");
        }
    }
}

/// Kills the private display server on every exit path.
struct Kill(std::process::Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wol-sp2-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A save by another program: a temp file renamed over `path` (how Settings and most editors
/// write), or in place.
fn save(path: &Path, text: &str, in_place: bool) {
    if in_place {
        std::fs::write(path, text).unwrap();
    } else {
        let t = path.with_extension("edit-tmp");
        std::fs::write(&t, text).unwrap();
        std::fs::rename(&t, path).unwrap();
    }
}

/// Runs the main loop until `done` or `limit`; the time it took.
fn run_until(limit: Duration, done: impl Fn() -> bool) -> Duration {
    let ctx = glib::MainContext::default();
    let t0 = Instant::now();
    // A heartbeat, so a blocking iteration wakes to look at the clock.
    let beat = glib::timeout_add_local(Duration::from_millis(5), || glib::ControlFlow::Continue);
    while !done() && t0.elapsed() < limit {
        ctx.iteration(true);
    }
    beat.remove();
    t0.elapsed()
}

fn display() -> Kill {
    let n = 40 + (std::process::id() + 97) % 200;
    let display = format!(":{n}");
    let port = (18000 + n).to_string();
    let k = Kill(
        std::process::Command::new("gtk4-broadwayd")
            .args([display.as_str(), "--port", port.as_str()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("gtk4-broadwayd (GTK 4 package) is required"),
    );
    let sock = Path::new(&std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into()))
        .join(format!("broadway{}.socket", n + 1));
    for _ in 0..100 {
        if std::os::unix::net::UnixStream::connect(&sock).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    unsafe {
        std::env::set_var("GDK_BACKEND", "broadway");
        std::env::set_var("BROADWAY_DISPLAY", &display);
    }
    k
}

/// The whole journey on one thread (GTK's): shell.json, widgets.json and kit.css saved while the
/// watch runs, good and bad, then 100 explicit reloads.
#[test]
fn saves_apply_within_200_ms_and_bad_ones_keep_the_last_good_state() {
    let _display = display();
    let cfg = tmp("cfg");
    let bin = tmp("bin");
    let log = bin.join("notices.log");
    // The notices go to a stand-in notify-send that writes one line each.
    let fake = bin.join("notify-send");
    std::fs::write(
        &fake,
        format!("#!/bin/sh\necho \"$*\" >> '{}'\n", log.display()),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", &cfg);
        let path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{}:{path}", bin.display()));
    }
    gtk::init().expect("gtk init");
    let notices = || {
        std::fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect::<Vec<_>>()
    };
    let wolos = cfg.join("wolos");
    std::fs::create_dir_all(&wolos).unwrap();
    let shell_json = wolos.join("shell.json");
    let widgets_json = wolos.join("widgets.json");
    let kit_css = wolos.join("kit.css");
    save(&shell_json, &ShellPrefs::default().to_json(), false);

    let seen: Rc<RefCell<Vec<ShellPrefs>>> = Rc::default();
    let s = seen.clone();
    ipc::on_reload(move |p| s.borrow_mut().push(*p));
    assert_eq!(seen.borrow().len(), 1, "called once at once");
    let hooks = ipc::reload_hooks();
    ipc::watch_prefs();
    let calls = || seen.borrow().len();

    // 1. A good shell.json applies within 200 ms, both ways of saving.
    for (n, in_place) in [(2, false), (3, true)] {
        let p = ShellPrefs {
            show_date: n % 2 == 1,
            dock_tile_size: 40 + n as u32,
            ..Default::default()
        };
        save(&shell_json, &p.to_json(), in_place);
        let took = run_until(Duration::from_secs(2), || calls() >= n);
        assert_eq!(calls(), n, "save {n} applied");
        assert!(took < APPLY_WITHIN, "shell.json applied after {took:?}");
        assert_eq!(*seen.borrow().last().unwrap(), p);
        eprintln!("shell.json save {n} (in place: {in_place}) applied in {took:?}");
    }
    let good = *seen.borrow().last().unwrap();

    // 2. A bad shell.json: no hook, the last good prefs stay, one notice naming the key, also
    // when the same mistake is saved again.
    let bad = r#"{"show_date": false, "dock_tile_size": "big"}"#;
    save(&shell_json, bad, false);
    run_until(QUIET, || false);
    assert_eq!(calls(), 3, "a bad file runs no hook");
    assert_eq!(ipc::shell_prefs(), good, "the last good prefs");
    let p = ipc::problems();
    assert_eq!(p.len(), 1, "{p:?}");
    assert!(p[0].contains("“dock_tile_size”"), "{p:?}");
    save(&shell_json, bad, true);
    run_until(QUIET, || false);
    assert_eq!(calls(), 3);
    let n = notices();
    assert_eq!(n.len(), 1, "one notice per mistake: {n:?}");
    assert!(
        n[0].starts_with("-a WolOS Changes Not Applied “dock_tile_size”"),
        "{n:?}"
    );
    // The explicit verb checks the same way.
    assert_eq!(
        ipc::reload().unwrap_err().key.as_deref(),
        Some("dock_tile_size")
    );
    assert_eq!(calls(), 3);

    // 3. Fixed: applies, the notice is gone.
    save(&shell_json, &good.to_json(), false);
    let took = run_until(Duration::from_secs(2), || calls() >= 4);
    assert!(
        calls() == 4 && took < APPLY_WITHIN,
        "fixed save after {took:?}"
    );
    assert!(ipc::problems().is_empty());

    // 4. widgets.json: another program's save applies; the shell's own atomic save does not; a
    // bad one keeps what shows and names the key.
    save(&widgets_json, "{\"version\":1,\"widgets\":[]}\n", false);
    let took = run_until(Duration::from_secs(2), || calls() >= 5);
    assert!(
        calls() == 5 && took < APPLY_WITHIN,
        "widgets.json after {took:?}"
    );
    shared::atomic::atomic_write(&widgets_json, 0o644, b"{\"version\":1,\"widgets\":[]}\n")
        .unwrap();
    run_until(QUIET, || false);
    assert_eq!(calls(), 5, "the shell's own save is not applied again");
    save(&widgets_json, r#"{"widgets": {}}"#, false);
    run_until(QUIET, || false);
    assert_eq!(calls(), 5);
    let p = ipc::problems();
    assert!(
        p.len() == 1 && p[0].contains("“widgets” in your desktop widgets"),
        "{p:?}"
    );
    save(&widgets_json, "{\"version\":1,\"widgets\":[]}\n", false);
    run_until(Duration::from_secs(2), || calls() >= 6);
    assert!(calls() == 6 && ipc::problems().is_empty());

    // 5. kit.css: good rules apply within 200 ms; a mistake keeps them and names the property.
    assert_eq!(style::user_css_text(), "", "no user file yet");
    save(&kit_css, ".sp2-probe { color: red; }\n", false);
    let took = run_until(Duration::from_secs(2), || {
        style::user_css_text().contains(".sp2-probe")
    });
    assert!(took < APPLY_WITHIN, "kit.css applied after {took:?}");
    eprintln!("kit.css applied in {took:?}");
    save(
        &kit_css,
        ".sp2-probe { color: red; }\n.sp2-other {\n  colr: blue;\n}\n",
        true,
    );
    run_until(QUIET, || false);
    assert!(style::user_css_text().contains(".sp2-probe"));
    assert!(
        !style::user_css_text().contains(".sp2-other"),
        "the last good rules"
    );
    let p = ipc::problems();
    assert!(
        p.len() == 1 && p[0].contains("“colr” on line 3 of your style sheet"),
        "{p:?}"
    );
    assert_eq!(calls(), 6, "a style sheet runs no prefs hook");
    std::fs::remove_file(&kit_css).unwrap();
    let took = run_until(Duration::from_secs(2), || style::user_css_text().is_empty());
    assert!(took < APPLY_WITHIN, "removed kit.css after {took:?}");
    assert!(ipc::problems().is_empty());
    let n = notices();
    assert_eq!(n.len(), 3, "shell.json, widgets.json, kit.css: {n:?}");

    // 6. 100 reloads add no hook.
    for _ in 0..100 {
        ipc::reload().unwrap();
    }
    assert_eq!(calls(), 106);
    assert_eq!(ipc::reload_hooks(), hooks, "hook count after 100 reloads");
}
