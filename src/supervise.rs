// SPDX-License-Identifier: GPL-2.0-or-later
//! `wol-shell --supervised`: a small parent that keeps the shell up. One process holds the bar,
//! dock, wallpaper, widgets, notifications and panels; before this a panic anywhere ended all of
//! them until the next login. The parent runs the same binary with [`UI_ENV`] set and starts a new
//! one when it ends without a clean exit: at once after the very first crash, then 100 ms .. 3 s
//! apart for crashes in a row (never less than 100 ms once it restarted), starting over after a
//! run of [`HEALTHY`], and never while the display is gone (the session is over then). A shell
//! that keeps ending is slowed to [`SLOW`] apart after [`SLOW_AFTER`] restarts in [`WINDOW`] and
//! not started again after [`CEILING`] in [`CEILING_WINDOW`] ([`Respawns`]). Same shape as the lock screen supervisor
//! (`login::lock_exit`, ADR 0012). Std only: no toolkit in the parent.
//!
//! When the session sends its stderr to a file (`2>>` in the session's start line), the
//! supervisor keeps that file small: the previous run's lines move to `<file>.1` at start, and
//! again whenever it grows past [`LOG_CAP`] ([`cap_log`]).
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
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
/// The span the slow-down looks back over.
pub const WINDOW: Duration = Duration::from_secs(10 * 60);
/// More restarts than this in [`WINDOW`] and each next one waits [`SLOW`].
pub const SLOW_AFTER: usize = 10;
/// The wait between restarts of a shell that keeps ending.
pub const SLOW: Duration = Duration::from_secs(30);
/// The span the ceiling looks back over.
pub const CEILING_WINDOW: Duration = Duration::from_secs(60 * 60);
/// This many restarts in [`CEILING_WINDOW`] and the next end is the last: no new shell this
/// session (a shell that dies 20 s after each start gets there in about 20 minutes, one that
/// dies at start in about 10, one that crashes now and then never).
pub const CEILING: usize = 30;
/// Longest single wait: a longer one is cut into these, looking at the display between them.
pub const TICK: Duration = Duration::from_secs(1);
/// Size the session's stderr file may reach before its lines move to `<file>.1`.
pub const LOG_CAP: u64 = 1 << 20;
/// How often the supervisor looks at the size of that file while a shell runs.
pub const LOG_CHECK: Duration = Duration::from_secs(60);

/// What follows the end of one shell process (mirrors `login::LockExit`).
#[derive(Debug, PartialEq, Eq)]
pub enum Exit {
    Done,
    Respawn(Duration),
    /// [`CEILING`] restarts in [`CEILING_WINDOW`]: the shell is not started again this session.
    GiveUp,
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

/// The supervisor's state: crashes in a row since the last healthy run (the back-off of [`exit`])
/// and the restarts of the last [`CEILING_WINDOW`] (the slow-down and the ceiling).
#[derive(Debug, Default)]
pub struct Respawns {
    attempt: u32,
    count: u32,
    /// When each restart of the last [`CEILING_WINDOW`] was decided, oldest first.
    recent: VecDeque<Duration>,
    /// The last restart waited [`SLOW`].
    slow: bool,
}

impl Respawns {
    pub fn new() -> Self {
        Self::default()
    }

    /// Restarts so far (what the next shell gets in [`RESPAWNS_ENV`]).
    pub fn count(&self) -> u32 {
        self.count
    }

    /// Crashes in a row since the last healthy run (0 = none).
    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    /// Restarts within [`WINDOW`] of the last one.
    pub fn recent(&self) -> usize {
        let Some(last) = self.recent.back() else {
            return 0;
        };
        self.recent
            .iter()
            .filter(|t| last.saturating_sub(**t) < WINDOW)
            .count()
    }

    /// The last restart was slowed to [`SLOW`].
    pub fn slow(&self) -> bool {
        self.slow
    }

    /// One shell ended `at` (time since the supervisor started) after running `ran`: what
    /// follows. [`exit`] gives the back-off; once the shell was restarted the wait is never under
    /// [`FIRST`], after [`SLOW_AFTER`] restarts in [`WINDOW`] never under [`SLOW`], and after
    /// [`CEILING`] in [`CEILING_WINDOW`] the answer is [`Exit::GiveUp`].
    pub fn ended(&mut self, clean: bool, display_alive: bool, ran: Duration, at: Duration) -> Exit {
        // A healthy run starts the back-off over before this crash's wait.
        if ran >= HEALTHY {
            self.attempt = 0;
        }
        let base = match exit(clean, display_alive, self.attempt) {
            Exit::Respawn(d) => d,
            other => return other,
        };
        while self
            .recent
            .front()
            .is_some_and(|t| at.saturating_sub(*t) >= CEILING_WINDOW)
        {
            self.recent.pop_front();
        }
        if self.recent.len() >= CEILING {
            return Exit::GiveUp;
        }
        self.recent.push_back(at);
        let mut delay = base;
        if self.count > 0 {
            delay = delay.max(FIRST);
        }
        self.slow = self.recent() > SLOW_AFTER;
        if self.slow {
            delay = delay.max(SLOW);
        }
        self.attempt = next_attempt(self.attempt, ran);
        self.count = self.count.saturating_add(1);
        Exit::Respawn(delay)
    }
}

/// Journal line when restarts slow down to [`SLOW`] apart.
pub fn slow_note() -> String {
    format!(
        "shell ended {} times in {} min; waiting {} s between restarts",
        SLOW_AFTER + 1,
        WINDOW.as_secs() / 60,
        SLOW.as_secs()
    )
}

/// Journal line when the supervisor stops at [`CEILING`].
pub fn give_up_note(status: &str) -> String {
    format!(
        "shell ended ({status}) after {CEILING} restarts in {} min; not starting it again this session",
        CEILING_WINDOW.as_secs() / 60
    )
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

/// Longest [`display_alive`] waits for the display to take a connection.
pub const PROBE: Duration = Duration::from_secs(2);

/// The display lives when its socket takes a connection (a stale file after a compositor crash
/// does not). A compositor that is there but does not answer within [`PROBE`] is alive (slow is
/// not gone): the supervisor must not give the session up for it, nor wait on it.
pub fn display_alive(socket: &Path) -> bool {
    let (tx, rx) = std::sync::mpsc::channel();
    let socket = socket.to_owned();
    let probe = std::thread::Builder::new()
        .name("wol-shell-probe".into())
        .spawn(move || {
            let up = std::os::unix::net::UnixStream::connect(&socket)
                .map(|s| {
                    let _ = shared::bounded::limit_socket(&s, shared::bounded::CLIENT_TIMEOUT);
                })
                .is_ok();
            let _ = tx.send(up);
        });
    probe.is_ok_and(|_| rx.recv_timeout(PROBE).unwrap_or(true))
}

/// The supervisor loop, with its effects passed in (tests drive it with plain processes).
/// `spawn(respawns)` runs one shell to its end; `alive()` checks the display; `note` gets the
/// journal line of each restart; `sleep` waits. Returns the process exit code.
pub fn run(
    spawn: impl FnMut(u32) -> std::io::Result<ExitStatus>,
    alive: impl Fn() -> bool,
    note: impl FnMut(&str),
    sleep: impl FnMut(Duration),
) -> u8 {
    let start = Instant::now();
    run_timed(spawn, alive, note, sleep, || start.elapsed())
}

/// [`run`] with its clock passed in too (`now()`: time since the supervisor started), so tests
/// can make each shell run as long as they like.
pub fn run_timed(
    mut spawn: impl FnMut(u32) -> std::io::Result<ExitStatus>,
    alive: impl Fn() -> bool,
    mut note: impl FnMut(&str),
    mut sleep: impl FnMut(Duration),
    mut now: impl FnMut() -> Duration,
) -> u8 {
    let mut r = Respawns::new();
    loop {
        let started = now();
        let out = spawn(r.count());
        let at = now();
        let ran = at.saturating_sub(started);
        let clean = out.as_ref().is_ok_and(|s| s.success());
        let attempt = if ran >= HEALTHY { 0 } else { r.attempt() };
        let st = || match &out {
            Ok(s) => s.to_string(),
            Err(e) => format!("not started: {:?}", e.kind()),
        };
        let was_slow = r.slow();
        match r.ended(clean, alive(), ran, at) {
            Exit::Done => return if clean { 0 } else { 1 },
            Exit::GiveUp => {
                note(&give_up_note(&st()));
                return 1;
            }
            Exit::Respawn(delay) => {
                note(&respawn_note(&st(), ran, attempt));
                if r.slow() && !was_slow {
                    note(&slow_note());
                }
                if !wait(delay, &alive, &mut sleep) {
                    return 1;
                }
            }
        }
    }
}

/// Sleeps `delay`; one longer than [`TICK`] in steps, ending early (false) once the display is
/// gone, so a slowed supervisor does not outlive the session.
fn wait(delay: Duration, alive: &impl Fn() -> bool, sleep: &mut impl FnMut(Duration)) -> bool {
    if delay <= TICK {
        sleep(delay);
        return true;
    }
    let mut left = delay;
    while !left.is_zero() {
        let step = left.min(TICK);
        sleep(step);
        left -= step;
        if !left.is_zero() && !alive() {
            return false;
        }
    }
    true
}

/// The one-per-session lock file of a supervisor.
#[derive(Debug)]
pub enum Guard {
    /// Taken: held while the file stays open.
    Held(std::fs::File),
    /// Another supervisor holds it.
    Busy,
    /// No guard could be made (no runtime directory, no permission): nobody can tell whether
    /// another supervisor runs.
    Unavailable(std::io::Error),
}

/// Takes the lock file `name` in the session's runtime directory `runtime` (absolute, or
/// [`Guard::Unavailable`]: a relative path would name a file in whatever folder this started in).
pub fn take_guard(runtime: &str, name: &str) -> Guard {
    let dir = Path::new(runtime);
    if !dir.is_absolute() {
        return Guard::Unavailable(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no session runtime directory",
        ));
    }
    let f = match std::fs::File::create(dir.join(name)) {
        Ok(f) => f,
        Err(e) => return Guard::Unavailable(e),
    };
    match f.try_lock() {
        Ok(()) => Guard::Held(f),
        Err(std::fs::TryLockError::WouldBlock) => Guard::Busy,
        Err(std::fs::TryLockError::Error(e)) => Guard::Unavailable(e),
    }
}

/// `/proc/self/fdinfo/<fd>` says the descriptor appends (`O_APPEND` in its octal `flags:`).
pub fn appending(fdinfo: &str) -> bool {
    const O_APPEND: u32 = 0o2000;
    fdinfo
        .lines()
        .find_map(|l| l.strip_prefix("flags:"))
        .and_then(|f| u32::from_str_radix(f.trim(), 8).ok())
        .is_some_and(|f| f & O_APPEND != 0)
}

/// The file this process's stderr appends to, if it is one (the session's `2>>` line); None for a
/// terminal, a pipe or the journal, and for a file opened without appending (cutting that one
/// would leave a hole where the writer goes on).
pub fn stderr_log() -> Option<PathBuf> {
    let info = std::fs::read_to_string("/proc/self/fdinfo/2").ok()?;
    if !appending(&info) {
        return None;
    }
    let p = std::fs::read_link("/proc/self/fd/2").ok()?;
    std::fs::metadata(&p).ok()?.is_file().then_some(p)
}

/// Where [`rotate`] keeps the previous lines of `log`: `<log>.1`.
pub fn kept_log(log: &Path) -> PathBuf {
    let mut p = log.as_os_str().to_owned();
    p.push(".1");
    PathBuf::from(p)
}

/// Moves the lines of `log` to [`kept_log`] (replacing what was there) and empties `log` in
/// place: every writer appends, so the next line lands at its start.
pub fn rotate(log: &Path) -> std::io::Result<()> {
    std::fs::copy(log, kept_log(log))?;
    std::fs::OpenOptions::new()
        .write(true)
        .open(log)?
        .set_len(0)
}

/// [`rotate`]s `log` once it is larger than `cap` bytes; true when it did.
pub fn cap_log(log: &Path, cap: u64) -> std::io::Result<bool> {
    if std::fs::metadata(log)?.len() <= cap {
        return Ok(false);
    }
    rotate(log).map(|()| true)
}

/// `wol-shell --supervised`: one supervisor per session runs the shell until a clean exit or the
/// end of the display. Restarts go to stderr (the session log) and the journal (`journalctl -t
/// wol-shell`), so the next crash shows its cause.
pub fn main(args: &[String]) -> u8 {
    let var = |k: &str| crate::envv::var(k).unwrap_or_default();
    let runtime = var("XDG_RUNTIME_DIR");
    let socket = crate::login::wayland_socket(&runtime, &var("WAYLAND_DISPLAY"));
    // Held for the process. Without a guard a second supervisor could not be told apart, so this
    // one does not start (two shells would fight over every surface and the socket).
    let _guard = match take_guard(&runtime, GUARD) {
        Guard::Held(g) => g,
        Guard::Busy => {
            crate::diag::say!("wol-shell: a supervisor already runs in this session");
            return 0;
        }
        Guard::Unavailable(e) => {
            crate::diag::say!(
                "wol-shell: no session guard in '{runtime}' ({e}); not starting a supervisor"
            );
            return 1;
        }
    };
    let log = stderr_log();
    let (_done, done_rx) = std::sync::mpsc::channel::<()>();
    if let Some(l) = &log {
        // A fresh file per session start; the previous run's lines stay in `<file>.1`.
        if std::fs::metadata(l).is_ok_and(|m| m.len() > 0) {
            let _ = rotate(l);
        }
        let l = l.clone();
        let _ = std::thread::Builder::new()
            .name("wol-shell-log".into())
            .spawn(move || {
                // Waits on `done`, which `main` drops on its way out: no poll loop.
                while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) =
                    done_rx.recv_timeout(LOG_CHECK)
                {
                    let _ = cap_log(&l, LOG_CAP);
                }
            });
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| "wol-shell".into());
    let pass = ui_args(args);
    run(
        |respawns| {
            if let Some(l) = &log {
                let _ = cap_log(l, LOG_CAP);
            }
            Command::new(&exe)
                .args(&pass)
                .env(UI_ENV, "1")
                .env(RESPAWNS_ENV, respawns.to_string())
                .stdin(Stdio::null())
                .status()
        },
        || display_alive(&socket),
        |n| {
            crate::diag::say!("wol-shell: {n}");
            let _ = Command::new("logger")
                .args(["-t", "wol-shell", "-p", "user.warning", "--", n])
                .status();
        },
        std::thread::sleep,
    )
}
