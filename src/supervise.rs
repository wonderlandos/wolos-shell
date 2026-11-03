// SPDX-License-Identifier: GPL-2.0-or-later
//! `wol-shell --supervised`: a small parent that keeps the shell up. One process holds the bar,
//! dock, wallpaper, widgets, notifications and panels; before this a panic anywhere ended all of
//! them until the next login. The parent runs the same binary with [`UI_ENV`] set and starts a new
//! one when it ends without a clean exit: at once after a first crash, then 100 ms .. 3 s apart
//! for crashes in a row, starting over after a run of [`HEALTHY`], and never while the display is gone (the session is over then). Same shape as the
//! lock screen supervisor (`login::lock_exit`, ADR 0012). Std only: no toolkit in the parent.
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// Set by the supervisor for the shell process it runs.
pub const UI_ENV: &str = "WOL_SHELL_UI";
/// How many times the supervisor started the shell again (0 = first start): the restarted shell
/// skips what is once per session.
pub const RESPAWNS_ENV: &str = "WOL_SHELL_RESPAWNS";
/// `wol-shell --supervised` (the session's `exec-once` line).
pub const SUPERVISED: &str = "--supervised";
/// Held (flock) by the running supervisor, in `$XDG_RUNTIME_DIR`: one per session.
pub const GUARD: &str = "wol-shell-supervisor.lock";
/// A shell that stayed up this long was healthy: the next crash starts the back-off over.
pub const HEALTHY: Duration = Duration::from_secs(20);
/// Wait after the second crash in a row (the first restarts at once), and the longest wait.
pub const FIRST: Duration = Duration::from_millis(100);
pub const LONGEST: Duration = Duration::from_secs(3);

/// What follows the end of one shell process (mirrors `login::LockExit`).
#[derive(Debug, PartialEq, Eq)]
pub enum Exit {
    Done,
    Respawn(Duration),
}

/// `clean`: the shell exited with status 0 (asked to quit, `WOL_SHELL_TIMING=exit`). Anything else
/// (a panic, a signal, an abort) brings a new one while the display lives: at once after the
/// first crash, then `100 ms << (attempt - 1)`, at most 3 s, for crashes in a row. `attempt`
/// counts crashes since the last healthy run (0 = this is the first).
pub fn exit(clean: bool, display_alive: bool, attempt: u32) -> Exit {
    if clean || !display_alive {
        return Exit::Done;
    }
    if attempt == 0 {
        return Exit::Respawn(Duration::ZERO);
    }
    Exit::Respawn(FIRST.saturating_mul(1 << (attempt - 1).min(5)).min(LONGEST))
}

/// The crash count after a shell that ran `ran`: 0 after a healthy one, else one more.
pub fn next_attempt(attempt: u32, ran: Duration) -> u32 {
    if ran >= HEALTHY {
        0
    } else {
        attempt.saturating_add(1)
    }
}

/// Journal line for one shell that ended without a clean exit (exit status or signal).
pub fn respawn_note(status: &str, ran: Duration, attempt: u32) -> String {
    format!(
        "shell ended ({status}) after {} ms; starting a new one (attempt {})",
        ran.as_millis(),
        attempt + 1
    )
}

/// Run as the supervisor: `--supervised` given and not already the supervised shell.
pub fn wanted(args: &[String], ui_env: bool) -> bool {
    !ui_env && args.iter().any(|a| a == SUPERVISED)
}

/// Arguments passed on to the shell: all but [`SUPERVISED`].
pub fn ui_args(args: &[String]) -> Vec<String> {
    args.iter().filter(|a| *a != SUPERVISED).cloned().collect()
}

/// The display lives when its socket takes a connection (a stale file after a compositor crash
/// does not).
pub fn display_alive(socket: &Path) -> bool {
    std::os::unix::net::UnixStream::connect(socket).is_ok()
}

/// The supervisor loop, with its effects passed in (tests drive it with plain processes).
/// `spawn(respawns)` runs one shell to its end; `alive()` checks the display; `note` gets the
/// journal line of each restart; `sleep` waits. Returns the process exit code.
pub fn run(
    mut spawn: impl FnMut(u32) -> std::io::Result<ExitStatus>,
    alive: impl Fn() -> bool,
    mut note: impl FnMut(&str),
    mut sleep: impl FnMut(Duration),
) -> u8 {
    let (mut attempt, mut respawns) = (0u32, 0u32);
    loop {
        let started = Instant::now();
        let out = spawn(respawns);
        let ran = started.elapsed();
        let clean = out.as_ref().is_ok_and(|s| s.success());
        // A healthy run starts the back-off over before this crash's wait.
        if ran >= HEALTHY {
            attempt = 0;
        }
        match exit(clean, alive(), attempt) {
            Exit::Done => return if clean { 0 } else { 1 },
            Exit::Respawn(delay) => {
                let st = match &out {
                    Ok(s) => s.to_string(),
                    Err(e) => format!("not started: {:?}", e.kind()),
                };
                note(&respawn_note(&st, ran, attempt));
                attempt = next_attempt(attempt, ran);
                respawns = respawns.saturating_add(1);
                sleep(delay);
            }
        }
    }
}

/// `wol-shell --supervised`: one supervisor per session runs the shell until a clean exit or the
/// end of the display. Restarts go to stderr (the session log) and the journal (`journalctl -t
/// wol-shell`), so the next crash shows its cause.
pub fn main(args: &[String]) -> u8 {
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    let runtime = var("XDG_RUNTIME_DIR");
    let socket = crate::login::wayland_socket(&runtime, &var("WAYLAND_DISPLAY"));
    let guard = std::fs::File::create(Path::new(&runtime).join(GUARD));
    if let Ok(g) = &guard
        && g.try_lock().is_err()
    {
        eprintln!("wol-shell: a supervisor already runs in this session");
        return 0;
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| "wol-shell".into());
    let pass = ui_args(args);
    run(
        |respawns| {
            Command::new(&exe)
                .args(&pass)
                .env(UI_ENV, "1")
                .env(RESPAWNS_ENV, respawns.to_string())
                .stdin(Stdio::null())
                .status()
        },
        || display_alive(&socket),
        |n| {
            eprintln!("wol-shell: {n}");
            let _ = Command::new("logger")
                .args(["-t", "wol-shell", "-p", "user.warning", "--", n])
                .status();
        },
        std::thread::sleep,
    )
}
