// SPDX-License-Identifier: GPL-2.0-or-later
//! Everything System Monitor reads or changes on the system, and nothing else: the one sampler
//! thread (`wol_core::monitor::sampler`, once a second, only what the open page needs), the
//! installed apps, the user manager's services over D-Bus, and the signals of Quit and End
//! Process. Every function here blocks and runs off the GTK thread; the pages never call them.
use super::model::{Startup, StartupRow};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};
use wol_core::monitor::Root;
use wol_core::monitor::procs::Target;
use wol_core::monitor::sampler::{Sampler, Want};
use wol_core::monitor::startup;

/// How often the graphs move.
pub const TICK: Duration = Duration::from_secs(1);

const PROCS: u8 = 1;
const DISKS: u8 = 2;
const PSS: u8 = 4;

/// What the open page needs, shared with the sampler thread.
#[derive(Clone, Default)]
pub struct Wants(Arc<AtomicU8>);

impl Wants {
    pub fn set(&self, w: Want) {
        let bits = u8::from(w.procs) * PROCS + u8::from(w.disks) * DISKS + u8::from(w.pss) * PSS;
        self.0.store(bits, Ordering::Relaxed);
    }
    pub fn get(&self) -> Want {
        let b = self.0.load(Ordering::Relaxed);
        Want {
            procs: b & PROCS != 0,
            disks: b & DISKS != 0,
            pss: b & PSS != 0,
        }
    }
}

/// This session's user (the owner of our own `/proc` entry): its apps are the ones listed.
fn session_uid() -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").ok().map(|m| m.uid())
}

/// Starts the sampler thread. It ends when the receiver is dropped (the window closed).
pub fn start(wants: Wants) -> async_channel::Receiver<wol_core::monitor::sampler::Sample> {
    let (tx, rx) = async_channel::bounded(2);
    let spawned = std::thread::Builder::new()
        .name("wol-monitor".into())
        .spawn(move || {
            let mut s = Sampler::new(Root::system(), session_uid());
            let t0 = Instant::now();
            let mut next = t0;
            loop {
                let sample = s.sample(t0.elapsed().as_secs_f64(), wants.get());
                if tx.send_blocking(sample).is_err() {
                    return;
                }
                // On the second, not a second after the work: no drift, one wakeup a tick.
                next += TICK;
                let now = Instant::now();
                if next > now {
                    std::thread::sleep(next - now);
                } else {
                    next = now;
                }
            }
        });
    if let Err(e) = spawned {
        crate::diag::say!("wol-monitor: no sampler thread: {e}");
    }
    rx
}

/// Runs `work` on a worker thread and `then` with its result on the GTK thread.
pub fn later<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    then: impl FnOnce(T) + 'static,
) {
    gtk::glib::spawn_future_local(async move {
        if let Ok(v) = gtk::gio::spawn_blocking(work).await {
            then(v);
        }
    });
}

/// The installed apps (names and icons of the app scopes), then `then` on the GTK thread.
pub fn load_entries(then: impl FnOnce(Vec<crate::desktop::Entry>) + 'static) {
    later(entries, then);
}

fn entries() -> Vec<crate::desktop::Entry> {
    crate::desktop::load_all()
}

/// [`startup_rows`] off the GTK thread.
pub fn load_startup(then: impl FnOnce(Startup) + 'static) {
    later(startup_rows, then);
}

/// [`set_unit`] off the GTK thread; `done(ok)`. A unit outside the person's own folder is
/// refused here whatever the page offered.
pub fn switch_unit(name: &str, on: bool, done: super::Done) {
    let name = name.to_owned();
    later(move || set_unit(&name, on), done);
}

/// What a Quit or End Process did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sent {
    /// Every process took the signal.
    All,
    /// Some had ended (or their pid now names another process) and were left alone.
    Partly,
    /// None was there any more: nothing was sent.
    Gone,
    /// The kernel refused (another account, the system).
    Refused,
}

/// [`signal`] off the GTK thread, to the targets that are still the ones the person chose.
pub fn send(targets: Vec<Target>, sig: &'static str, done: impl FnOnce(Sent) + 'static) {
    later(
        move || match session_uid() {
            Some(uid) => signal(&Root::system(), uid, &targets, sig),
            None => Sent::Refused,
        },
        done,
    );
}

const SYSTEMD: &str = "org.freedesktop.systemd1";
const SYSTEMD_PATH: &str = "/org/freedesktop/systemd1";
const MANAGER: &str = "org.freedesktop.systemd1.Manager";

fn manager_call<B, R>(method: &str, body: &B) -> Option<R>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
    R: for<'d> zbus::zvariant::DynamicDeserialize<'d>,
{
    let c = zbus::blocking::Connection::session().ok()?;
    let m = c
        .call_method(Some(SYSTEMD), SYSTEMD_PATH, Some(MANAGER), method, body)
        .ok()?;
    m.body().deserialize::<R>().ok()
}

/// The user's services that start with the session, or that the user manager cannot be asked.
fn startup_rows() -> Startup {
    let Some(files) = manager_call::<_, Vec<(String, String)>>("ListUnitFiles", &()) else {
        return Startup::NotRead;
    };
    let config = crate::kit::config_home();
    Startup::Rows(
        startup::rows(&files, &config)
            .into_iter()
            .map(|unit| StartupRow {
                title: std::fs::read_to_string(&unit.path)
                    .ok()
                    .as_deref()
                    .and_then(startup::description)
                    .unwrap_or_else(|| unit.name.trim_end_matches(".service").to_owned()),
                unit,
            })
            .collect(),
    )
}

/// Switches one of the person's services for the next login (and reloads the user manager).
fn set_unit(name: &str, on: bool) -> bool {
    if !startup::own_unit(std::path::Path::new(&crate::kit::config_home()), name) {
        return false;
    }
    let files = [name];
    let done = if on {
        manager_call::<_, (bool, Vec<(String, String, String)>)>(
            "EnableUnitFiles",
            &(&files[..], false, false),
        )
        .is_some()
    } else {
        manager_call::<_, Vec<(String, String, String)>>("DisableUnitFiles", &(&files[..], false))
            .is_some()
    };
    done && manager_call::<_, ()>("Reload", &()).is_some()
}

/// Sends `signal` (`TERM`, `KILL`) to the targets that are still ours and still the same
/// process (`procs::still_ours`: pid > 1, owned by `uid`, same start time). Never a pid of 0
/// or below: `kill` would read it as a process group.
fn signal(root: &Root, uid: u32, targets: &[Target], signal: &str) -> Sent {
    let (pids, dropped) = wol_core::monitor::procs::still_ours(root, uid, targets);
    if pids.is_empty() {
        return Sent::Gone;
    }
    let mut c = std::process::Command::new("kill");
    c.args(["-s", signal, "--"])
        .args(pids.iter().map(i32::to_string));
    match c.status() {
        Ok(s) if s.success() && dropped == 0 => Sent::All,
        Ok(s) if s.success() => Sent::Partly,
        _ => Sent::Refused,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only what is still the chosen process is signalled; `kill` runs on a real child we own.
    #[test]
    fn a_signal_skips_what_ended_and_never_reaches_init() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = i32::try_from(child.id()).unwrap();
        let root = Root::system();
        let uid = session_uid().unwrap();
        let start = wol_core::monitor::procs::start_of(&root, pid).unwrap();
        let t = |pid, start| Target { pid, start };
        assert_eq!(signal(&root, uid, &[t(1, 0), t(-1, 0)], "TERM"), Sent::Gone);
        assert_eq!(
            signal(&root, uid, &[t(pid, start + 1)], "TERM"),
            Sent::Gone,
            "reused pid"
        );
        assert_eq!(
            signal(&root, uid, &[t(pid, start), t(i32::MAX, 0)], "TERM"),
            Sent::Partly
        );
        assert!(
            child.wait().is_ok_and(|s| !s.success()),
            "the sleep was ended"
        );
    }

    #[test]
    fn a_shipped_unit_is_never_switched() {
        assert!(
            !set_unit("pipewire.service", false),
            "not in the person's own folder"
        );
        assert!(!set_unit("../../etc/x.service", true));
    }

    #[test]
    fn the_open_page_reaches_the_sampler() {
        let w = Wants::default();
        assert_eq!(w.get(), Want::default());
        let all = Want {
            procs: true,
            disks: false,
            pss: true,
        };
        w.clone().set(all);
        assert_eq!(w.get(), all, "shared between clones");
    }
}
