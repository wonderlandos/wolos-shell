// SPDX-License-Identifier: GPL-2.0-or-later
//! SP-1: the shell supervisor (`shell::supervise`), surface fences (`shell::kit::fence`) and the
//! Notification Center kept across a restart (`shell::notify_store`).
use shell::kit::fence;
use shell::notify_store::{self as store, Kept, Saved};
use shell::supervise::{self, Exit};
use shell::ui::notify::{CENTER_MAX, Note};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::{Command, ExitStatus};
use std::time::{Duration, Instant};

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn tmp(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("wol-sp1-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn first_crash_restarts_at_once_then_backoff_doubles_to_3s() {
    assert_eq!(supervise::exit(true, true, 0), Exit::Done, "clean exit");
    assert_eq!(supervise::exit(false, false, 0), Exit::Done, "display gone");
    let d: Vec<Exit> = (0..9).map(|n| supervise::exit(false, true, n)).collect();
    let want = [0, 100, 200, 400, 800, 1600, 3000, 3000, 3000];
    for (got, w) in d.iter().zip(want) {
        assert_eq!(*got, Exit::Respawn(ms(w)));
    }
    assert_eq!(
        supervise::exit(false, true, u32::MAX),
        Exit::Respawn(ms(3000))
    );
}

#[test]
fn a_healthy_run_starts_the_backoff_over() {
    assert_eq!(supervise::next_attempt(4, supervise::HEALTHY), 0);
    assert_eq!(supervise::next_attempt(4, ms(19_999)), 5);
    assert_eq!(supervise::next_attempt(u32::MAX, ms(1)), u32::MAX);
    assert_eq!(supervise::HEALTHY, Duration::from_secs(20));
}

#[test]
fn supervised_flag_picks_the_parent_and_is_not_passed_on() {
    let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert!(supervise::wanted(&a(&["--supervised"]), false));
    assert!(!supervise::wanted(&a(&["--supervised"]), true), "the child");
    assert!(!supervise::wanted(&a(&[]), false), "plain wol-shell");
    assert_eq!(supervise::ui_args(&a(&["--supervised", "x"])), a(&["x"]));
    let note = supervise::respawn_note("signal: 11 (SIGSEGV)", ms(1234), 0);
    assert_eq!(
        note,
        "shell ended (signal: 11 (SIGSEGV)) after 1234 ms; starting a new one (attempt 1)"
    );
}

#[test]
fn display_alive_needs_a_listening_socket() {
    let d = tmp("alive");
    let sock = d.join("wayland-9");
    assert!(!supervise::display_alive(&sock), "no file");
    let l = UnixListener::bind(&sock).unwrap();
    assert!(supervise::display_alive(&sock));
    drop(l);
    // Another test's fork may hold the socket until its exec (close-on-exec): it lets go within
    // microseconds, so the check is simply repeated (no sleeping in tests, RR-049).
    let t = Instant::now();
    while supervise::display_alive(&sock) && t.elapsed() < Duration::from_secs(2) {
        std::thread::yield_now();
    }
    assert!(!supervise::display_alive(&sock), "stale file after a crash");
}

/// Real processes: two crashes (SIGSEGV), then a clean exit. Respawn count reaches the child,
/// the first restart is at once, the second after 100 ms, one note per restart.
#[test]
fn run_respawns_crashed_children_until_a_clean_exit() {
    let mut seen = Vec::new();
    let (mut notes, mut waits) = (Vec::new(), Vec::new());
    let code = supervise::run(
        |respawns| {
            seen.push(respawns);
            let script = if respawns < 2 {
                "kill -SEGV $$"
            } else {
                "exit 0"
            };
            Command::new("sh").args(["-c", script]).status()
        },
        || true,
        |n| notes.push(n.to_string()),
        |d| waits.push(d),
    );
    assert_eq!(code, 0);
    assert_eq!(seen, [0, 1, 2]);
    assert_eq!(waits, [ms(0), ms(100)]);
    assert_eq!(notes.len(), 2);
    assert!(notes[0].contains("SIGSEGV"), "{}", notes[0]);
    assert!(notes[1].ends_with("(attempt 2)"), "{}", notes[1]);
}

#[test]
fn run_stops_when_the_display_is_gone_and_retries_a_missing_binary() {
    let mut n = 0;
    let code = supervise::run(
        |_| {
            n += 1;
            Command::new("sh").args(["-c", "exit 3"]).status()
        },
        || false,
        |_| {},
        |_| {},
    );
    assert_eq!((code, n), (1, 1), "no restart without a display");
    let (mut tries, mut notes) = (0, Vec::new());
    let code = supervise::run(
        |_| -> std::io::Result<ExitStatus> {
            tries += 1;
            if tries < 3 {
                Command::new("/nonexistent/wol-shell").status()
            } else {
                Command::new("true").status()
            }
        },
        || true,
        |s| notes.push(s.to_string()),
        |_| {},
    );
    assert_eq!((code, tries), (0, 3));
    assert!(notes[0].contains("not started: NotFound"), "{}", notes[0]);
}

/// The real binary: `wol-shell --supervised` restarts a shell that cannot start (no display for
/// the toolkit) while the session's display socket listens, gives up once it is gone, and a
/// second supervisor in the same session leaves at once.
#[test]
fn binary_supervises_and_holds_one_guard_per_session() {
    let run = tmp("bin");
    let sock = run.join("wayland-sp1");
    let listener = UnixListener::bind(&sock).unwrap();
    let bin = env!("CARGO_BIN_EXE_wol-shell");
    let cmd = || {
        let mut c = Command::new(bin);
        c.arg("--supervised")
            .env("XDG_RUNTIME_DIR", &run)
            .env("WAYLAND_DISPLAY", "wayland-sp1")
            .env("GDK_BACKEND", "x11")
            .env_remove("DISPLAY")
            .env_remove(supervise::UI_ENV)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
        c
    };
    let mut child = cmd().spawn().unwrap();
    // The supervisor's stderr, line by line from a reader thread: the test waits for what it
    // needs (two restarts, then the end of the stream) instead of sleeping.
    let pipe = child.stderr.take().unwrap();
    let (tx, lines) = std::sync::mpsc::channel::<String>();
    std::thread::Builder::new()
        .name("supervise-stderr".into())
        .spawn(move || {
            use std::io::BufRead;
            for l in std::io::BufReader::new(pipe).lines().map_while(Result::ok) {
                let _ = tx.send(l);
            }
        })
        .unwrap();
    let (mut err, mut restarts) = (String::new(), 0);
    while restarts < 2 {
        let l = lines
            .recv_timeout(Duration::from_secs(20))
            .unwrap_or_else(|_| panic!("{restarts} restarts in 20 s: {err}"));
        restarts += usize::from(l.contains("starting a new one"));
        err.push_str(&l);
        err.push('\n');
    }
    let second = cmd().output().unwrap();
    assert!(second.status.success());
    assert!(
        String::from_utf8_lossy(&second.stderr).contains("a supervisor already runs"),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    drop(listener);
    let t = Instant::now();
    for l in lines.iter() {
        err.push_str(&l);
        err.push('\n');
    }
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(1), "{err}");
    assert!(
        t.elapsed() < Duration::from_secs(5),
        "left within one back-off"
    );
}

/// Stability S2: a shell that dies 21 s after each start (past [`supervise::HEALTHY`]) used to
/// come back at once, forever. Now: never at once after the first restart, [`supervise::SLOW`]
/// apart after [`supervise::SLOW_AFTER`] restarts in ten minutes, and no more after the ceiling.
#[test]
fn a_shell_that_keeps_ending_is_slowed_then_left_down() {
    let s = |n: u64| Duration::from_secs(n);
    let mut r = supervise::Respawns::new();
    assert_eq!(r.ended(true, true, s(1), s(1)), Exit::Done, "clean");
    assert_eq!(
        r.ended(false, false, s(1), s(1)),
        Exit::Done,
        "display gone"
    );
    assert_eq!(r.count(), 0);
    let (mut at, mut delays) = (Duration::ZERO, Vec::new());
    loop {
        at += s(21);
        match r.ended(false, true, s(21), at) {
            Exit::Respawn(d) => {
                delays.push(d);
                at += d;
            }
            Exit::GiveUp => break,
            Exit::Done => panic!("done while the display lives"),
        }
        assert!(delays.len() < 1000, "no ceiling");
    }
    assert_eq!(
        delays[0],
        Duration::ZERO,
        "the first crash restarts at once"
    );
    assert!(
        delays[1..].iter().all(|d| *d >= supervise::FIRST),
        "{delays:?}"
    );
    assert!(
        delays[supervise::SLOW_AFTER..]
            .iter()
            .all(|d| *d >= supervise::SLOW)
    );
    assert_eq!(delays.len(), supervise::CEILING, "{delays:?}");
    assert_eq!(r.count() as usize, supervise::CEILING);
}

/// The windows slide: a shell that crashes now and then (more than ten minutes apart) is always
/// started again, after [`supervise::FIRST`].
#[test]
fn old_restarts_leave_the_window() {
    let mut r = supervise::Respawns::new();
    let mut at = Duration::ZERO;
    for n in 0..100 {
        at += supervise::WINDOW + Duration::from_secs(1);
        let want = if n == 0 {
            Duration::ZERO
        } else {
            supervise::FIRST
        };
        assert_eq!(
            r.ended(false, true, supervise::WINDOW, at),
            Exit::Respawn(want),
            "restart {n}"
        );
        assert_eq!(r.recent(), 1);
    }
    // Crashes at start, in a row: the back-off doubles up to 3 s, then the slow-down, then the end.
    let mut r = supervise::Respawns::new();
    let mut delays = Vec::new();
    let mut at = Duration::ZERO;
    while let Exit::Respawn(d) = r.ended(false, true, ms(50), at) {
        delays.push(d.as_millis() as u64);
        at += d + ms(50);
    }
    assert_eq!(
        delays,
        [
            0, 100, 200, 400, 800, 1600, 3000, 3000, 3000, 3000, 30000, 30000, 30000, 30000, 30000,
            30000, 30000, 30000, 30000, 30000, 30000, 30000, 30000, 30000, 30000, 30000, 30000,
            30000, 30000, 30000
        ]
    );
}

/// `run` itself with a fake clock: every shell runs 21 s and crashes; the supervisor notes the
/// slow-down once, waits in steps of at most [`supervise::TICK`], and stops at the ceiling.
#[test]
fn run_stops_at_the_ceiling_with_a_fake_clock() {
    let clock = std::cell::Cell::new(Duration::ZERO);
    let (mut notes, mut waited, mut starts) = (Vec::new(), Duration::ZERO, 0);
    let mut longest = Duration::ZERO;
    let code = supervise::run_timed(
        |_| {
            starts += 1;
            clock.set(clock.get() + Duration::from_secs(21));
            Command::new("sh").args(["-c", "exit 1"]).status()
        },
        || true,
        |n| notes.push(n.to_string()),
        |d| {
            longest = longest.max(d);
            waited += d;
            clock.set(clock.get() + d);
        },
        || clock.get(),
    );
    assert_eq!(code, 1);
    assert_eq!(starts, supervise::CEILING + 1);
    assert!(waited >= supervise::SLOW, "{waited:?}");
    assert!(longest <= supervise::TICK, "{longest:?}");
    let slow: Vec<&String> = notes
        .iter()
        .filter(|n| n.contains("between restarts"))
        .collect();
    assert_eq!(slow.len(), 1, "{notes:?}");
    let last = notes.last().unwrap();
    assert!(last.contains("not starting it again"), "{last}");
}

/// A slowed supervisor whose display goes away mid-wait leaves at the next step.
#[test]
fn a_long_wait_ends_with_the_display() {
    let clock = std::cell::Cell::new(Duration::ZERO);
    let alive = std::cell::Cell::new(true);
    let mut sleeps = 0;
    let code = supervise::run_timed(
        |_| {
            clock.set(clock.get() + ms(10));
            Command::new("sh").args(["-c", "exit 1"]).status()
        },
        || alive.get(),
        |_| {},
        |d| {
            clock.set(clock.get() + d);
            if d == supervise::TICK {
                sleeps += 1;
                alive.set(false);
            }
        },
        || clock.get(),
    );
    assert_eq!(
        (code, sleeps),
        (1, 1),
        "left after one step of the first long wait"
    );
}

/// Stability S7: a guard that cannot be made is not a guard that is free.
#[test]
fn the_guard_is_taken_once_and_missing_is_not_free() {
    let d = tmp("guard");
    let rt = d.to_str().unwrap();
    let first = supervise::take_guard(rt, "g.lock");
    assert!(matches!(first, supervise::Guard::Held(_)), "{first:?}");
    let second = supervise::take_guard(rt, "g.lock");
    assert!(matches!(second, supervise::Guard::Busy), "{second:?}");
    drop(first);
    // Another test's fork may hold the descriptor until its exec (close-on-exec): a moment, repeated.
    let t = Instant::now();
    let mut again = supervise::take_guard(rt, "g.lock");
    while matches!(again, supervise::Guard::Busy) && t.elapsed() < Duration::from_secs(2) {
        std::thread::yield_now();
        again = supervise::take_guard(rt, "g.lock");
    }
    assert!(matches!(again, supervise::Guard::Held(_)), "{again:?}");
    for bad in ["", "relative/dir", "/nonexistent/wol-sp1"] {
        let g = supervise::take_guard(bad, "g.lock");
        assert!(
            matches!(g, supervise::Guard::Unavailable(_)),
            "{bad}: {g:?}"
        );
    }
    assert!(!Path::new("relative/dir/g.lock").exists());
}

#[test]
fn a_supervisor_without_a_guard_does_not_start() {
    let out = Command::new(env!("CARGO_BIN_EXE_wol-shell"))
        .arg("--supervised")
        .env("XDG_RUNTIME_DIR", "/nonexistent/wol-sp1")
        .env_remove(supervise::UI_ENV)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(err.contains("not starting a supervisor"), "{err}");
    assert!(
        !err.contains("starting a new one"),
        "no shell was started: {err}"
    );
}

/// Stability S3: the stderr file stays small: only an appending file is cut, its lines move to
/// `<file>.1`, and a writer that appends goes on at the start.
#[test]
fn the_stderr_file_is_capped() {
    assert!(supervise::appending(
        "pos:\t0\nflags:\t0102001\nmnt_id:\t30\n"
    ));
    assert!(!supervise::appending("pos:\t0\nflags:\t0100001\n"));
    assert!(!supervise::appending("pos:\t0\n"));
    let d = tmp("log");
    let log = d.join("wol-shell.log");
    assert_eq!(supervise::kept_log(&log), d.join("wol-shell.log.1"));
    let mut w = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .unwrap();
    use std::io::Write;
    w.write_all(&vec![b'x'; 2048]).unwrap();
    assert!(!supervise::cap_log(&log, 4096).unwrap(), "under the cap");
    w.write_all(&vec![b'y'; 4096]).unwrap();
    assert!(supervise::cap_log(&log, 4096).unwrap());
    assert_eq!(
        std::fs::metadata(supervise::kept_log(&log)).unwrap().len(),
        6144
    );
    w.write_all(b"next\n").unwrap();
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        "next\n",
        "no hole before it"
    );
    assert_eq!(supervise::LOG_CAP, 1 << 20);
}

/// The real binary on a session start line's `2>>` file: the previous run's lines move to
/// `<file>.1` before the first shell starts.
#[test]
fn the_supervisor_starts_a_fresh_stderr_file() {
    let run = tmp("fresh");
    let log = run.join("wol-shell.log");
    std::fs::write(&log, "a line of the last session\n").unwrap();
    let err = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_wol-shell"))
        .arg("--supervised")
        .env("XDG_RUNTIME_DIR", &run)
        .env("WAYLAND_DISPLAY", "wayland-none")
        .env("GDK_BACKEND", "x11")
        .env_remove("DISPLAY")
        .env_remove(supervise::UI_ENV)
        .stdin(std::process::Stdio::null())
        .stderr(err)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "no display: one shell, then done"
    );
    let now = std::fs::read_to_string(&log).unwrap();
    assert!(!now.contains("last session"), "{now}");
    let kept = std::fs::read_to_string(supervise::kept_log(&log)).unwrap();
    assert_eq!(kept, "a line of the last session\n");
}

#[test]
fn guard_stops_the_part_that_panicked_only() {
    let ok = fence::guard("clock", || 7);
    assert_eq!(ok, Some(7));
    let bad: Option<u32> = fence::guard("clipboard", || panic!("boom"));
    assert_eq!(bad, None);
    // Nested: the inner part fails, the outer one goes on.
    let outer = fence::guard("outer", || fence::guard("inner", || panic!("x")).is_none());
    assert_eq!(outer, Some(true));
    assert_eq!(fence::failed(), ["clipboard", "inner"]);
}

#[test]
fn break_list_names_parts_exactly() {
    assert!(fence::broken(Some("clipboard"), "clipboard"));
    assert!(fence::broken(Some("themes, clipboard"), "clipboard"));
    assert!(!fence::broken(Some("clipboards"), "clipboard"));
    assert!(!fence::broken(Some(""), "clipboard"));
    assert!(!fence::broken(None, "clipboard"));
}

#[test]
fn journal_lines_name_the_part() {
    assert_eq!(
        fence::journal_line(Some("notifications"), "boom at src/ui/notify.rs:1:1"),
        "notifications stopped (the rest keeps running): boom at src/ui/notify.rs:1:1"
    );
    assert_eq!(
        fence::journal_line(None, "x"),
        "panic outside every part: x"
    );
    let s: Box<dyn std::any::Any + Send> = Box::new(String::from("owned"));
    assert_eq!(fence::panic_text(&*s), "owned");
    let s: Box<dyn std::any::Any + Send> = Box::new("static");
    assert_eq!(fence::panic_text(&*s), "static");
    let s: Box<dyn std::any::Any + Send> = Box::new(5u8);
    assert_eq!(fence::panic_text(&*s), "panic");
}

#[test]
fn the_card_reads_as_a_friendly_sentence() {
    let p = shell::copy::lint(fence::HINT);
    assert!(p.is_empty(), "{p:?}");
    assert!(
        fence::NAMESPACE.starts_with("wol-panel-"),
        "blurred like every panel"
    );
}

/// A fenced build that panics gives the card, a good one its own widget (needs a display; the
/// golden harness's broadway, or a session).
#[test]
fn fence_gives_the_card_in_place_of_a_panicking_widget() {
    if gtk::init().is_err() {
        eprintln!("skipped: no display for the toolkit");
        return;
    }
    use gtk::prelude::*;
    let good = fence::fence("label", || gtk::Label::new(Some("hi")).upcast());
    assert!(good.is::<gtk::Label>());
    let bad = fence::fence("widgets", || panic!("broken"));
    assert_eq!(bad.widget_name(), "widgets");
    assert!(bad.has_css_class("glass-panel"));
}

fn note(id: u32, summary: &str) -> Note {
    Note {
        id,
        app: "Mail".into(),
        icon: "mail".into(),
        summary: summary.into(),
        body: "b".into(),
        critical: false,
        timeout_ms: 5000,
        image: String::new(),
        actions: vec![("default".into(), "Open".into())],
    }
}

#[test]
fn kept_follows_the_center_rules() {
    let k = Kept::default();
    k.push(&note(1, "a"));
    k.push(&note(2, "b"));
    k.push(&note(1, "a2"));
    let sums: Vec<String> = k.list().into_iter().map(|s| s.summary).collect();
    assert_eq!(sums, ["a2", "b"], "replaced on top");
    k.remove(2);
    assert_eq!(k.list().len(), 1);
    for i in 10..(10 + CENTER_MAX as u32 + 5) {
        k.push(&note(i, "n"));
    }
    assert_eq!(k.list().len(), CENTER_MAX);
    k.clear();
    assert!(k.list().is_empty());
}

#[test]
fn saved_notes_round_trip_and_come_back_without_buttons() {
    let d = tmp("store");
    let file = store::path(&d);
    assert!(file.ends_with("wolos/notify.json"));
    assert!(store::read(&file).is_empty(), "no file");
    let saved: Vec<Saved> = (0..60)
        .map(|i| Saved::of(&note(i, &format!("n{i}"))))
        .collect();
    store::write(&file, &saved).unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "notes are private");
    assert!(!file.with_extension("json.tmp").exists());
    let back = store::read(&file);
    assert_eq!(back.len(), CENTER_MAX);
    assert_eq!(back[0], saved[0]);
    let n = back[0].note(0);
    assert!(n.actions.is_empty());
    assert_eq!(n.id, u32::MAX);
    let ids: std::collections::HashSet<u32> = (0..CENTER_MAX).map(store::restored_id).collect();
    assert_eq!(ids.len(), CENTER_MAX);
    assert!(ids.iter().all(|i| *i > u32::MAX - 100));
    std::fs::write(&file, "{not json").unwrap();
    assert!(store::read(&file).is_empty(), "broken file: empty Center");
}

#[test]
fn only_a_restarted_shell_restores() {
    assert!(!store::restores(None));
    assert!(!store::restores(Some("0")));
    assert!(!store::restores(Some("x")));
    assert!(store::restores(Some("1")));
    assert!(store::restores(Some("12")));
}

/// The session starts the supervisor, with the shell's stderr still in the session log.
#[test]
fn the_session_starts_the_supervised_shell() {
    let conf = include_str!("../../../desktop/overlay/usr/share/wolos/hypr/hyprland.conf");
    let lines: Vec<&str> = conf
        .lines()
        .filter(|l| l.starts_with("exec") && l.contains("wol-shell") && !l.contains(".sock"))
        .collect();
    assert_eq!(
        lines,
        // The first sign-in Setup runs first while the session's flag is there (ADR 0013), then the
        // supervised shell whatever it returned: as the user unit that restarts a killed supervisor
        // (QA-LIVE-21), bare only where there is no user manager.
        [
            r#"exec-once = sh -c '[ -e "$XDG_RUNTIME_DIR/wol-first-login" ] && wol-setup --first-login; dbus-update-activation-environment --systemd --all; systemctl --user start wolos-shell.service || exec wol-shell --supervised 2>>"$XDG_RUNTIME_DIR/wol-shell.log"'"#
        ]
    );
}

/// RR-064: a saved Center with a field this shell does not know is not taken (a typo or a file
/// from another version is an empty Center, like any broken file).
#[test]
fn a_saved_list_with_an_unknown_field_is_an_empty_center() {
    let known = r#"[{"app":"Files","summary":"Done"}]"#;
    assert_eq!(store::decode(known).len(), 1);
    let unknown = r#"[{"app":"Files","summary":"Done","surprise":1}]"#;
    assert!(store::decode(unknown).is_empty());
}
