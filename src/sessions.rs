// SPDX-License-Identifier: GPL-2.0-or-later
//! Sessions in the shell (ADR 0032 "Sessions"; model in `wol_core::sessions`, verbs in
//! `controller::sessions`). With the `ai-sessions` module installed, the shell keeps
//! `$XDG_STATE_HOME/wolos/sessions.json` current by running `wol sessions save` every
//! [`SAVE_EVERY`] (a crash, a power cut or a restart loses at most that much), and at login offers
//! once to reopen last time's windows (`wol sessions restore`).
//!
//! Invariant: the offer comes before the first save of the new session, so the new, empty
//! desktop never overwrites the set it offers; one offer per session (a marker under
//! `$XDG_RUNTIME_DIR`, written once the offer is answered, gone at logout). Nothing here runs on
//! the GTK thread: [`start`] works on its own thread and hands the answer to the UI through a callback.
use std::path::{Path, PathBuf};
use std::time::Duration;
use wol_core::sessions::{self, Saved};

/// How often the window set is saved while the session lives.
pub const SAVE_EVERY: Duration = Duration::from_secs(60);
/// A set older than this is not offered (30 days, in seconds).
pub const MAX_AGE_SECS: u64 = 30 * 24 * 3600;
/// The module that turns sessions on (`wolos-module-ai-sessions`).
pub const MODULE_PACKAGE: &str = "wolos-module-ai-sessions";
/// Marker under `$XDG_RUNTIME_DIR`: this session already offered.
pub const OFFERED: &str = "wolos-sessions-offered";

/// `wol sessions save`, as the autosave runs it.
pub fn save_argv() -> [&'static str; 3] {
    ["wol", "sessions", "save"]
}

/// `wol sessions restore`, run when the person accepts the offer.
pub fn restore_argv() -> [&'static str; 3] {
    ["wol", "sessions", "restore"]
}

/// Windows to offer from a saved set read at `now`: 0 when there is nothing, when it is older than
/// [`MAX_AGE_SECS`] or dated in the future (a clock that jumped).
pub fn offer_count(saved: &Saved, now: u64) -> usize {
    match now.checked_sub(saved.saved_at) {
        Some(age) if age <= MAX_AGE_SECS => saved.windows.len(),
        _ => 0,
    }
}

/// What the shell does at login.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Begin {
    /// Sessions are off (module not installed): no offer, no autosave.
    Off,
    /// Nothing to offer (or offered already this session): start saving.
    Save,
    /// Ask "reopen N windows?"; save starts once answered.
    Offer(usize),
}

/// The decision from what was read: module installed, offered already, the file's text, now.
pub fn begin(installed: bool, offered: bool, text: Option<&str>, now: u64) -> Begin {
    if !installed {
        return Begin::Off;
    }
    if offered {
        return Begin::Save;
    }
    let n = text.map_or(0, |t| offer_count(&sessions::parse(t), now));
    if n == 0 { Begin::Save } else { Begin::Offer(n) }
}

fn state_file() -> PathBuf {
    wol_core::journal::state_home().join(sessions::FILE)
}

fn marker() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|d| !d.is_empty())
        .map(|d| Path::new(&d).join(OFFERED))
}

fn run(argv: &[&str]) -> bool {
    std::process::Command::new(argv[0])
        .args(&argv[1..])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Saves the window set now, at most [`SAVE_AT_END`] seconds (Log Out, Restart and Shut Down save
/// before any window is asked to close: ADR 0032 promises it, QA-FILES-5). Blocking: not on the
/// UI thread.
pub fn save_now() -> bool {
    let secs = SAVE_AT_END.to_string();
    run(&["timeout", &secs, "wol", "sessions", "save"])
}

/// Longest the save at the end of a session may take, in seconds (a hung save never holds it).
pub const SAVE_AT_END: u32 = 10;

/// The sessions file when it is at most `sessions::MAX_BYTES` (a longer one is not read; the core
/// checks again).
fn read_bounded(p: &Path) -> Option<String> {
    shared::bounded::read_text(p, sessions::MAX_BYTES as u64)
}

/// The autosave loop on its own thread; never returns while the session lives.
pub fn spawn_autosave() {
    let _ = std::thread::Builder::new()
        .name("wol-sessions".into())
        .spawn(|| {
            loop {
                std::thread::sleep(SAVE_EVERY);
                let _ = run(&save_argv());
            }
        });
}

/// Key of the notification action that says yes.
pub const REOPEN_ACTION: &str = "reopen";

/// The `notify-send` argv that asks to reopen `n` windows and waits for the answer (`-w`): it
/// prints [`REOPEN_ACTION`] when the person clicks Reopen, nothing when they dismiss it.
pub fn offer_argv(n: usize) -> Vec<String> {
    vec![
        "notify-send".into(),
        "-a".into(),
        "WolOS".into(),
        "-i".into(),
        "utilities-terminal".into(),
        "-w".into(),
        "-A".into(),
        format!("{REOPEN_ACTION}={}", crate::copy::SESSIONS_REOPEN),
        crate::copy::sessions_offer(n),
        crate::copy::SESSIONS_OFFER_BODY.into(),
    ]
}

/// What the notification answered: yes only for the Reopen action.
pub fn said_yes(stdout: &str) -> bool {
    stdout.trim() == REOPEN_ACTION
}

/// The offer as a notification with a Reopen button (the shell's own notification centre shows
/// it); blocks this worker thread until it is answered or dismissed. False when it cannot be shown.
pub fn ask_by_notification(n: usize) -> bool {
    let argv = offer_argv(n);
    std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .is_ok_and(|o| said_yes(&String::from_utf8_lossy(&o.stdout)))
}

/// Ask `ask(n)` and only then write the "offered" marker: a shell that restarts while the offer
/// is still open (unanswered) offers again instead of going straight to autosave, which would save
/// the new, empty desktop over the set that was never reopened.
pub fn offer<F>(mark: Option<&Path>, n: usize, ask: F) -> bool
where
    F: FnOnce(usize) -> bool,
{
    let yes = ask(n);
    if let Some(m) = mark {
        let _ = std::fs::write(m, b"");
    }
    yes
}

/// At login, off the GTK thread: decide, and when there is something to reopen call `ask(n)`
/// (on this worker thread; the UI marshals it to the main loop). `ask` returns whether the person
/// said yes; then the windows are reopened, and saving starts either way.
pub fn start<F>(ask: F)
where
    F: FnOnce(usize) -> bool + Send + 'static,
{
    let _ = std::thread::Builder::new()
        .name("wol-sessions-start".into())
        .spawn(move || {
            let installed = run(&["pacman", "-Qq", MODULE_PACKAGE]);
            let mark = marker();
            let offered = mark.as_ref().is_some_and(|m| m.exists());
            let text = read_bounded(&state_file());
            match begin(
                installed,
                offered,
                text.as_deref(),
                wol_core::journal::now(),
            ) {
                Begin::Off => {}
                Begin::Save => spawn_autosave(),
                Begin::Offer(n) => {
                    if offer(mark.as_deref(), n, ask) {
                        let _ = run(&restore_argv());
                    }
                    spawn_autosave();
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(at: u64, n: usize) -> String {
        let one = r#"{"workspace":1,"cwd":"/h","kind":"terminal","command":"","title":"t"}"#;
        format!(
            r#"{{"version":1,"saved_at":{at},"windows":[{}]}}"#,
            vec![one; n].join(",")
        )
    }

    #[test]
    fn the_offer_comes_once_and_only_for_a_recent_set() {
        let now = 10 * MAX_AGE_SECS;
        let t = saved(now - 60, 3);
        assert_eq!(begin(true, false, Some(&t), now), Begin::Offer(3));
        assert_eq!(
            begin(true, true, Some(&t), now),
            Begin::Save,
            "offered already"
        );
        assert_eq!(
            begin(false, false, Some(&t), now),
            Begin::Off,
            "module not installed"
        );
        assert_eq!(begin(true, false, None, now), Begin::Save);
        assert_eq!(begin(true, false, Some("garbage"), now), Begin::Save);
        assert_eq!(
            begin(true, false, Some(&saved(now - 60, 0)), now),
            Begin::Save
        );
        let old = saved(now - MAX_AGE_SECS - 1, 2);
        assert_eq!(begin(true, false, Some(&old), now), Begin::Save, "too old");
        let future = saved(now + 5, 2);
        assert_eq!(
            begin(true, false, Some(&future), now),
            Begin::Save,
            "clock jumped"
        );
    }

    #[test]
    fn the_shell_runs_the_wol_verbs() {
        assert_eq!(save_argv(), ["wol", "sessions", "save"]);
        assert_eq!(restore_argv(), ["wol", "sessions", "restore"]);
        assert!(SAVE_EVERY >= Duration::from_secs(30));
    }

    #[test]
    fn the_marker_is_written_only_after_the_answer() {
        let d = std::env::temp_dir().join(format!("wol-sess-mark-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let m = d.join(OFFERED);
        let seen = offer(Some(&m), 4, |n| {
            assert_eq!(n, 4);
            // Unanswered so far: a restarting shell sees no marker and offers again.
            assert!(!m.exists());
            assert_eq!(
                begin(true, m.exists(), Some(&saved(100, 4)), 160),
                Begin::Offer(4)
            );
            true
        });
        assert!(seen);
        assert!(m.exists(), "answered: one offer per session");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_offer_is_one_notification_with_a_reopen_button() {
        let a = offer_argv(3);
        assert_eq!(a[0], "notify-send");
        assert!(a.contains(&"-w".to_string()), "waits for the answer");
        assert!(a.contains(&"reopen=Reopen".to_string()), "{a:?}");
        assert_eq!(a[a.len() - 2], "Reopen 3 windows from last time?");
        assert_eq!(
            offer_argv(1)[a.len() - 2],
            "Reopen 1 window from last time?"
        );
        assert!(crate::copy::lint(crate::copy::SESSIONS_OFFER_BODY).is_empty());
        assert!(said_yes("reopen\n"));
        assert!(!said_yes("") && !said_yes("dismissed"));
    }

    #[test]
    fn a_big_file_is_refused_before_it_is_parsed() {
        let d = std::env::temp_dir().join(format!("wol-shell-sessions-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("s.json");
        std::fs::write(&p, " ".repeat(sessions::MAX_BYTES + 100)).unwrap();
        let t = read_bounded(&p);
        assert_eq!(
            t, None,
            "over the cap: not read at all (shared::bounded::read_text)"
        );
        assert_eq!(begin(true, false, t.as_deref(), 0), Begin::Save);
        std::fs::write(&p, " ".repeat(sessions::MAX_BYTES)).unwrap();
        assert_eq!(read_bounded(&p).map(|t| t.len()), Some(sessions::MAX_BYTES));
        let _ = std::fs::remove_dir_all(&d);
    }
}
