// SPDX-License-Identifier: GPL-2.0-or-later
//! "Give me more time" (Settings > Accessibility > Timeouts, QA-ACC-6) as the shell sees it: the
//! account's setting ([`shared::patience`]), read when first asked and kept current by a file
//! monitor in every shell process ([`watch`], started with the style sheet). Every timed sheet,
//! banner, toast and countdown asks [`current`] when its timer starts.
use shared::patience::{FILE, Patience};
use std::sync::atomic::{AtomicU8, Ordering};

/// 0 = not read yet; else 1 + the index in [`Patience::ALL`].
static CURRENT: AtomicU8 = AtomicU8::new(0);

fn code(p: Patience) -> u8 {
    let at = Patience::ALL.iter().position(|x| *x == p).unwrap_or(0);
    1 + u8::try_from(at).unwrap_or(0)
}

thread_local! {
    /// The file monitor behind [`watch`]: alive as long as the process.
    static MONITOR: std::cell::RefCell<Option<gtk::gio::FileMonitor>> =
        const { std::cell::RefCell::new(None) };
}

fn path() -> std::path::PathBuf {
    std::path::Path::new(&super::config_home()).join(FILE)
}

/// Reads the file now.
fn reread() -> Patience {
    let p = Patience::read(std::path::Path::new(&super::config_home()));
    CURRENT.store(code(p), Ordering::Relaxed);
    p
}

/// The setting in force. The first call reads the file (a few bytes).
pub fn current() -> Patience {
    match CURRENT.load(Ordering::Relaxed) {
        0 => reread(),
        n => Patience::ALL[usize::from(n - 1) % Patience::ALL.len()],
    }
}

/// Keeps [`current`] in step with the file, whoever writes it (Settings in another process, a
/// script). Call once per process, with the GTK main loop running.
pub fn watch() {
    use gtk::{gio, prelude::*};
    reread();
    let f = gio::File::for_path(path());
    if let Ok(m) = f.monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE) {
        m.connect_changed(|_, _, _, _| {
            reread();
        });
        MONITOR.with(|k| *k.borrow_mut() = Some(m));
    }
}

/// Makes this process use `p` now, for a file written by the journal (the monitor of [`watch`]
/// would only catch up a moment later).
pub fn remember(p: Patience) {
    CURRENT.store(code(p), Ordering::Relaxed);
}

/// Saves the setting: the file holds `2`, `5` or `never`; normal removes it. Every shell process
/// follows through [`watch`].
pub fn save(p: Patience) -> std::io::Result<()> {
    let path = path();
    if p == Patience::Normal {
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    } else {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(&path, format!("{}\n", p.word()))?;
    }
    CURRENT.store(code(p), Ordering::Relaxed);
    Ok(())
}
