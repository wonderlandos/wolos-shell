// SPDX-License-Identifier: GPL-2.0-or-later
//! The Notification Center across a shell restart (`crate::supervise`): the listed notes, newest
//! first, in `$XDG_STATE_HOME/wolos/notify.json` (owner-only), rewritten off the toolkit thread
//! after every change, because a crash has no exit hook. Only a restarted shell reads it back
//! (`RESPAWNS_ENV` above 0); the first shell of a session starts with an empty Center. Restored
//! notes have no live sender: they come back without buttons, under ids counted down from
//! `u32::MAX` so a new note's id never replaces one.
use crate::ui::notify::{CENTER_MAX, Note};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Relative to `$XDG_STATE_HOME`.
pub const FILE: &str = "wolos/notify.json";
/// A longer file is not read (50 notes with long bodies stay far below).
pub const MAX_BYTES: u64 = 1 << 20;

/// One Center row as saved.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub app: String,
    pub icon: String,
    pub summary: String,
    pub body: String,
    pub critical: bool,
    pub image: String,
}

impl Saved {
    pub fn of(n: &Note) -> Saved {
        Saved {
            app: n.app.clone(),
            icon: n.icon.clone(),
            summary: n.summary.clone(),
            body: n.body.clone(),
            critical: n.critical,
            image: n.image.clone(),
        }
    }

    /// The note for Center row `i` (0 = newest) of a restored list.
    pub fn note(&self, i: usize) -> Note {
        Note {
            id: restored_id(i),
            app: self.app.clone(),
            icon: self.icon.clone(),
            summary: self.summary.clone(),
            body: self.body.clone(),
            critical: self.critical,
            timeout_ms: 0,
            image: self.image.clone(),
            actions: Vec::new(),
        }
    }
}

/// Id of restored row `i`: from `u32::MAX` down, far from the server's ids (counted up from 1).
pub fn restored_id(i: usize) -> u32 {
    u32::MAX - u32::try_from(i.min(CENTER_MAX)).unwrap_or(0)
}

pub fn encode(notes: &[Saved]) -> String {
    serde_json::to_string(&notes[..notes.len().min(CENTER_MAX)]).unwrap_or_else(|_| "[]".into())
}

/// The saved list, at most [`CENTER_MAX`]; a broken file is an empty Center.
pub fn decode(text: &str) -> Vec<Saved> {
    let mut v: Vec<Saved> = serde_json::from_str(text).unwrap_or_default();
    v.truncate(CENTER_MAX);
    v
}

/// Only a restarted shell restores the Center (`respawns` from `RESPAWNS_ENV`).
pub fn restores(respawns: Option<&str>) -> bool {
    respawns
        .and_then(|r| r.trim().parse::<u32>().ok())
        .is_some_and(|n| n > 0)
}

pub fn path(state_home: &Path) -> PathBuf {
    state_home.join(FILE)
}

/// Writes `notes` to `file` (owner-only), through a temporary file and a rename.
pub fn write(file: &Path, notes: &[Saved]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = file.with_extension("json.tmp");
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(encode(notes).as_bytes())?;
    std::fs::rename(&tmp, file)
}

/// The saved list in `file`, or empty.
pub fn read(file: &Path) -> Vec<Saved> {
    shared::bounded::read_text(file, MAX_BYTES)
        .map(|t| decode(&t))
        .unwrap_or_default()
}

/// A writer thread: each list sent replaces the file; a burst writes the latest only.
pub fn saver(file: PathBuf) -> std::sync::mpsc::Sender<Vec<Saved>> {
    let (tx, rx) = std::sync::mpsc::channel::<Vec<Saved>>();
    let _ = std::thread::Builder::new()
        .name("wol-notify-save".into())
        .spawn(move || {
            while let Ok(mut notes) = rx.recv() {
                while let Ok(newer) = rx.try_recv() {
                    notes = newer;
                }
                if let Err(e) = write(&file, &notes) {
                    eprintln!("wol-shell: notification center not saved: {e}");
                }
            }
        });
    tx
}

/// The Center's notes beside its rows (same order and rules as `notify::Center`), saved after
/// every change when a writer is set.
#[derive(Default)]
pub struct Kept {
    notes: std::cell::RefCell<Vec<(u32, Saved)>>,
    tx: Option<std::sync::mpsc::Sender<Vec<Saved>>>,
}

impl Kept {
    /// The session's list: saved to [`FILE`] under the state home. A restarted shell
    /// ([`restores`]) gets last time's notes from [`Kept::restored`]; a first one drops them.
    pub fn start() -> (std::rc::Rc<Kept>, Vec<Note>) {
        let file = path(&wol_core::journal::state_home());
        let back = restores(
            std::env::var(crate::supervise::RESPAWNS_ENV)
                .ok()
                .as_deref(),
        );
        // Small (at most MAX_BYTES), read once, and only after a crash.
        let old = if back { read(&file) } else { Vec::new() };
        let kept = std::rc::Rc::new(Kept {
            notes: Default::default(),
            tx: Some(saver(file)),
        });
        let notes = old.iter().enumerate().map(|(i, s)| s.note(i)).collect();
        if !back {
            kept.save();
        }
        (kept, notes)
    }

    /// Note `n` goes on top (replacing its id), the oldest beyond [`CENTER_MAX`] fall off.
    pub fn push(&self, n: &Note) {
        {
            let mut v = self.notes.borrow_mut();
            v.retain(|(id, _)| *id != n.id);
            v.insert(0, (n.id, Saved::of(n)));
            v.truncate(CENTER_MAX);
        }
        self.save();
    }

    pub fn remove(&self, id: u32) {
        self.notes.borrow_mut().retain(|(x, _)| *x != id);
        self.save();
    }

    pub fn clear(&self) {
        self.notes.borrow_mut().clear();
        self.save();
    }

    /// The notes, newest first.
    pub fn list(&self) -> Vec<Saved> {
        self.notes.borrow().iter().map(|(_, s)| s.clone()).collect()
    }

    fn save(&self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(self.list());
        }
    }
}
