// SPDX-License-Identifier: GPL-2.0-or-later
//! Plugin Maker editor (ADR 0031 "Plugin Maker / Theme Studio"): the plugin's `plugin.toml`, Lua
//! scripts and other text files in a monospace editor, saved atomically, with a problems list
//! under it.
//!
//! - The problems are `wol plugin check`'s own: `wol_core::plugins::check_full` with the Lua
//!   step `wol_lua::plugin::lua_problems` ([`check_folder`]). Unsaved text is checked in a
//!   scratch copy of the folder ([`check_draft`]), so the list follows the typing.
//! - The check reruns [`CHECK_AFTER`] after the last edit ([`Turns`]: only the newest edit's
//!   timer runs, and only the newest result is shown), always off the GTK thread.
//! - Invariant: a save writes a fresh file next to the old one and renames it over
//!   ([`save_atomic`]); a failed save leaves the old file as it was.
use super::off_thread;
use crate::copy::maker as copy;
use crate::kit::{data as d, widgets as w};
use gtk::{glib, prelude::*};
use shared::plugins::{Kind, MANIFEST_FILE};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use wol_core::plugins as cp;
use wol_lua::{Services, Stamp};

/// Quiet time after the last keystroke before the problems list is rebuilt (ADR 0031 brief).
pub const CHECK_AFTER: Duration = Duration::from_millis(400);
/// Extensions the editor opens as text.
pub const EDITABLE: [&str; 4] = ["toml", "lua", "json", "conf"];
/// Most files listed for one plugin (the folder check caps the size, this caps the menu).
pub const MAX_FILES: usize = 64;

/// One plugin folder as the editor shows it: its text files (manifest first) and its problems.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Loaded {
    pub dir: PathBuf,
    /// (path relative to `dir`, text).
    pub files: Vec<(String, String)>,
    pub problems: Vec<String>,
}

/// Reads `dir` for the editor. Blocks: call it off the GTK thread. The error is the sentence.
pub fn load(dir: &Path) -> Result<Loaded, &'static str> {
    if !dir.join(MANIFEST_FILE).is_file() {
        return Err(copy::NOT_A_PLUGIN);
    }
    let walked = cp::walk(dir).map_err(|_| copy::NOT_READ)?;
    let mut files: Vec<(String, String)> = walked
        .into_iter()
        .filter(|(rel, _)| {
            Path::new(rel)
                .extension()
                .is_some_and(|x| EDITABLE.iter().any(|e| x == *e))
        })
        .filter_map(|(rel, p)| {
            let big = std::fs::metadata(&p).map_or(true, |m| m.len() > cp::MAX_FILE);
            if big {
                return None;
            }
            std::fs::read_to_string(&p).ok().map(|t| (rel, t))
        })
        .collect();
    files.sort_by(|a, b| (a.0 != MANIFEST_FILE, &a.0).cmp(&(b.0 != MANIFEST_FILE, &b.0)));
    files.truncate(MAX_FILES);
    if files.first().is_none_or(|f| f.0 != MANIFEST_FILE) {
        return Err(copy::NOT_READ);
    }
    Ok(Loaded {
        dir: dir.to_path_buf(),
        problems: check_folder(dir),
        files,
    })
}

/// Services for the check's single run of each script: the clock and the machine, nothing
/// that acts.
struct Headless;
impl Services for Headless {
    fn now(&self) -> Stamp {
        wol_lua::Live.now()
    }
    fn system(&self) -> wol_lua::SystemInfo {
        wol_lua::Live.system()
    }
    fn notify(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
}

/// `wol plugin check`'s own full check (`wol_core::plugins::check_full` with
/// `wol_lua::plugin::lua_problems`, the host's deadlines and services that act on nothing).
/// Blocks: run it off the GTK thread.
pub fn check_folder(dir: &Path) -> Vec<String> {
    cp::check_full(dir, false, |d, m| {
        wol_lua::plugin::lua_problems(
            d,
            m,
            crate::ui::plugins::DEADLINES,
            &|| -> Rc<dyn Services> { Rc::new(Headless) },
        )
    })
    .problems
}

/// Unique scratch folders within this process.
static SCRATCH: AtomicU64 = AtomicU64::new(0);

/// [`check_folder`] on `dir` as it would be with `rel` holding `text`: a scratch copy under the
/// temp folder, named like `dir` (the check wants the folder named after the id), removed after.
pub fn check_draft(dir: &Path, rel: &str, text: &str) -> Vec<String> {
    let n = SCRATCH.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("wol-maker-check-{}-{n}", std::process::id()));
    let name = dir.file_name().unwrap_or_default();
    let copy_to = root.join(name);
    let listed = cp::walk(dir).ok();
    let copied = std::fs::create_dir(&root).is_ok()
        && listed.as_ref().is_some_and(|files| {
            files.iter().all(|(r, p)| {
                let to = copy_to.join(r);
                to.parent()
                    .is_some_and(|d| std::fs::create_dir_all(d).is_ok())
                    && std::fs::copy(p, &to).is_ok()
            })
        });
    let safe = shared::safe_path::safe_join_str(&copy_to, rel).ok();
    // What this call may have written, for the cleanup below.
    let mut written: Vec<PathBuf> = listed
        .unwrap_or_default()
        .iter()
        .map(|(r, _)| copy_to.join(r))
        .collect();
    written.extend(safe.clone());
    let out = match safe {
        Some(target) if copied => {
            let wrote = target
                .parent()
                .is_some_and(|d| std::fs::create_dir_all(d).is_ok())
                && std::fs::write(&target, text).is_ok();
            if wrote {
                check_folder(&copy_to)
            } else {
                check_folder(dir)
            }
        }
        _ => check_folder(dir),
    };
    remove_written(&root, &written);
    out
}

/// Deletes `files` (the ones a call wrote under `root`), then each folder between them and `root`
/// that is empty now, deepest first, `root` last. Nothing else is touched: a folder that still
/// holds something it did not write stays (RR-041, no recursive delete).
fn remove_written(root: &Path, files: &[PathBuf]) {
    // `safe_join_str` paths start at the resolved root: read them relative to `root` either way.
    let real = root.canonicalize().ok();
    let rel = |f: &Path| {
        f.strip_prefix(root)
            .ok()
            .or_else(|| real.as_deref().and_then(|r| f.strip_prefix(r).ok()))
            .map(Path::to_path_buf)
    };
    let mut dirs: Vec<PathBuf> = vec![root.to_path_buf()];
    for r in files.iter().filter_map(|f| rel(f)) {
        let _ = std::fs::remove_file(root.join(&r));
        let mut d = r.parent();
        while let Some(p) = d.filter(|p| !p.as_os_str().is_empty()) {
            let full = root.join(p);
            if !dirs.contains(&full) {
                dirs.push(full);
            }
            d = p.parent();
        }
    }
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for d in dirs {
        let _ = std::fs::remove_dir(d);
    }
}

/// Writes `text` to `path` through a fresh file in the same folder, synced, then renamed over
/// `path` (`shared::atomic::atomic_write`). The file keeps its permission bits (a new one gets
/// 0644). On error the old file is untouched and the scratch file is gone.
pub fn save_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::symlink_metadata(path)
        .ok()
        .filter(std::fs::Metadata::is_file)
        .map_or(0o644, |m| m.permissions().mode() & 0o777);
    shared::atomic::atomic_write(path, mode, text.as_bytes())
}

/// Writes every file of `files` whose text differs from the disk ([`save_atomic`]); false when
/// one could not be written. Blocks: run it off the GTK thread.
pub fn save_all(dir: &Path, files: &[(String, String)]) -> bool {
    let mut ok = true;
    for (rel, text) in files {
        let Ok(p) = shared::safe_path::safe_join_str(dir, rel) else {
            ok = false;
            continue;
        };
        let same = std::fs::read_to_string(&p).is_ok_and(|t| t == *text);
        if !same && save_atomic(&p, text).is_err() {
            ok = false;
        }
    }
    ok
}

/// Reads `dir` off the GTK thread, then hands the result to `done` on it.
pub fn open(dir: PathBuf, done: impl FnOnce(Result<Loaded, &'static str>) + 'static) {
    off_thread(move || load(&dir), done);
}

/// Edit generations: each edit takes the next turn; a timer or a result belongs to the newest
/// edit only when its turn is still current.
#[derive(Clone, Default)]
pub struct Turns(Rc<Cell<u64>>);

impl Turns {
    /// A new edit: its turn (every older one stops being current).
    pub fn next(&self) -> u64 {
        let t = self.0.get().wrapping_add(1);
        self.0.set(t);
        t
    }
    pub fn is_current(&self, t: u64) -> bool {
        self.0.get() == t
    }
}

/// The plugin's display name from its id, as `wol plugin new` makes it ("alice.weather-now" ->
/// "Weather now").
pub fn plugin_name(id: &str) -> String {
    let last = id.rsplit('.').next().unwrap_or(id);
    let mut c = last.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
        .replace('-', " ")
}

/// Why a new plugin was not made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotCreated {
    /// The scaffold's own sentence (the id is not a plugin id).
    Id(String),
    Exists,
    Disk,
}

impl NotCreated {
    pub fn sentence(&self) -> String {
        match self {
            NotCreated::Id(s) => s.clone(),
            NotCreated::Exists => copy::EXISTS.into(),
            NotCreated::Disk => copy::NOT_CREATED.into(),
        }
    }
}

/// `wol plugin new` in the app: the same scaffold (`wol_core::plugins::scaffold`) written into
/// `user_dir/<id>`, which must not exist yet. Blocks: run it off the GTK thread.
pub fn create(user_dir: &Path, id: &str, kinds: &[Kind]) -> Result<PathBuf, NotCreated> {
    let files = cp::scaffold(id, &plugin_name(id), kinds).map_err(NotCreated::Id)?;
    create_files(user_dir, id, &files)
}

/// Writes `files` into the new folder `user_dir/<id>`; on any error the folder is removed.
fn create_files(
    user_dir: &Path,
    id: &str,
    files: &[(String, String)],
) -> Result<PathBuf, NotCreated> {
    let dir = user_dir.join(id);
    if dir.symlink_metadata().is_ok() {
        return Err(NotCreated::Exists);
    }
    std::fs::create_dir_all(user_dir).map_err(|_| NotCreated::Disk)?;
    std::fs::create_dir(&dir).map_err(|_| NotCreated::Exists)?;
    if let Err(e) = write_scaffold(&dir, files) {
        // Never leave a half-written folder behind: a retry would read it as Exists. Only the
        // scaffold's own files go (the folder is new, nothing else can be in it).
        let mine: Vec<PathBuf> = files
            .iter()
            .filter_map(|(rel, _)| shared::safe_path::safe_join_str(&dir, rel).ok())
            .collect();
        remove_written(&dir, &mine);
        return Err(e);
    }
    Ok(dir)
}

/// Writes each `(rel, text)` into the new folder `dir` (no file there may exist yet).
fn write_scaffold(dir: &Path, files: &[(String, String)]) -> Result<(), NotCreated> {
    for (rel, text) in files {
        let Ok(p) = shared::safe_path::safe_join_str(dir, rel) else {
            return Err(NotCreated::Disk);
        };
        let ok = p
            .parent()
            .is_some_and(|d| std::fs::create_dir_all(d).is_ok())
            && std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&p)
                .and_then(|mut f| std::io::Write::write_all(&mut f, text.as_bytes()))
                .is_ok();
        if !ok {
            return Err(NotCreated::Disk);
        }
    }
    Ok(())
}

// ---- The editor view ----

/// The editor page: file menu, text, Save, and the problems under it.
pub struct Editor {
    pub root: gtk::Box,
    dir: PathBuf,
    /// The text of each file as saved last.
    files: RefCell<Vec<(String, String)>>,
    current: Cell<usize>,
    buffer: gtk::TextBuffer,
    problems: gtk::Box,
    status: gtk::Label,
    turns: Turns,
    /// Set while the buffer is filled by code (not an edit).
    filling: Cell<bool>,
    /// Runs after every save (the preview reloads the folder).
    saved: RefCell<Option<Rc<dyn Fn()>>>,
    /// An edit since the last good save.
    dirty: Cell<bool>,
    /// Counts edits only (a Check does not count), so a save knows whether one came meanwhile.
    edits: Turns,
}

impl Editor {
    /// The editor on `l` (already read). Nothing blocks here.
    pub fn new(l: Loaded) -> Rc<Editor> {
        let root = w::vbox(w::S3);
        root.add_css_class("page");
        let head = w::hbox(w::S2);
        head.append(&w::label("Editor", "large-title"));
        head.append(&w::spacer());
        let names: Vec<&str> = l.files.iter().map(|(n, _)| n.as_str()).collect();
        let pick = w::dropdown(&names, 0);
        pick.set_tooltip_text(Some("File"));
        head.append(&pick);
        root.append(&head);
        let view = gtk::TextView::builder()
            .monospace(true)
            .wrap_mode(gtk::WrapMode::None)
            .top_margin(w::S2)
            .bottom_margin(w::S2)
            .left_margin(w::S2)
            .right_margin(w::S2)
            .vexpand(true)
            .build();
        view.update_property(&[gtk::accessible::Property::Label("Plugin file")]);
        let buffer = view.buffer();
        let scroll = d::scroller(&view);
        scroll.set_hscrollbar_policy(gtk::PolicyType::Automatic);
        scroll.set_min_content_height(EDITOR_MIN_HEIGHT);
        root.append(&w::section("", &[scroll.upcast()]));
        let problems = w::vbox(0);
        root.append(&problems);
        let status = w::label("", "secondary");
        status.set_wrap(true);
        root.append(&status);
        let ed = Rc::new(Editor {
            root,
            dir: l.dir,
            files: RefCell::new(l.files),
            current: Cell::new(0),
            buffer,
            problems,
            status,
            turns: Turns::default(),
            filling: Cell::new(false),
            saved: RefCell::new(None),
            dirty: Cell::new(false),
            edits: Turns::default(),
        });
        ed.show_file(0);
        ed.show_problems(&l.problems);
        {
            let e = Rc::downgrade(&ed);
            pick.connect_selected_notify(move |p| {
                if let (Some(e), Ok(i)) = (e.upgrade(), usize::try_from(p.selected())) {
                    e.keep_draft();
                    e.show_file(i);
                }
            });
        }
        {
            let e = Rc::downgrade(&ed);
            ed.buffer.connect_changed(move |_| {
                if let Some(e) = e.upgrade()
                    && !e.filling.get()
                {
                    e.edited();
                }
            });
        }
        ed
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Whether the editor holds text that is not on disk yet.
    pub fn has_unsaved(&self) -> bool {
        self.dirty.get()
    }

    /// `f` runs after each save.
    pub fn on_saved(&self, f: Rc<dyn Fn()>) {
        self.saved.replace(Some(f));
    }

    fn text(&self) -> String {
        let (a, b) = self.buffer.bounds();
        self.buffer.text(&a, &b, false).to_string()
    }

    /// Keeps the shown file's unsaved text when another file is picked (saved on Save).
    fn keep_draft(&self) {
        let t = self.text();
        if let Some(f) = self.files.borrow_mut().get_mut(self.current.get()) {
            f.1 = t;
        }
    }

    fn show_file(&self, i: usize) {
        let text = self.files.borrow().get(i).map(|f| f.1.clone());
        let Some(text) = text else { return };
        self.current.set(i);
        self.filling.set(true);
        self.buffer.set_text(&text);
        self.filling.set(false);
    }

    /// The relative path and text shown now.
    pub fn draft(&self) -> (String, String) {
        let name = self
            .files
            .borrow()
            .get(self.current.get())
            .map(|f| f.0.clone())
            .unwrap_or_default();
        (name, self.text())
    }

    /// An edit: the check runs [`CHECK_AFTER`] after the last one, on the draft.
    fn edited(self: &Rc<Self>) {
        self.dirty.set(true);
        self.edits.next();
        self.status.set_label(copy::UNSAVED);
        let turn = self.turns.next();
        let me = Rc::downgrade(self);
        glib::timeout_add_local_once(CHECK_AFTER, move || {
            let Some(e) = me.upgrade() else { return };
            if e.turns.is_current(turn) {
                e.check_now(turn);
            }
        });
    }

    /// Checks the draft off the GTK thread; the answer is shown when no edit came since.
    fn check_now(self: &Rc<Self>, turn: u64) {
        let (rel, text) = self.draft();
        let dir = self.dir.clone();
        let me = Rc::downgrade(self);
        off_thread(
            move || check_draft(&dir, &rel, &text),
            move |problems| {
                if let Some(e) = me.upgrade()
                    && e.turns.is_current(turn)
                {
                    e.show_problems(&problems);
                }
            },
        );
    }

    /// Saves every file whose text changed (atomically, off the GTK thread), then checks the
    /// folder as saved.
    pub fn save(self: &Rc<Self>) {
        self.keep_draft();
        let files = self.files.borrow().clone();
        let dir = self.dir.clone();
        let turn = self.turns.next();
        let edit = self.edits.next();
        let me = Rc::downgrade(self);
        off_thread(
            move || (save_all(&dir, &files), check_folder(&dir)),
            move |(ok, problems)| {
                let Some(e) = me.upgrade() else { return };
                e.status
                    .set_label(if ok { copy::SAVED } else { copy::NOT_SAVED });
                if ok && e.edits.is_current(edit) {
                    // No edit came while saving: the disk holds what is shown.
                    e.dirty.set(false);
                }
                if e.turns.is_current(turn) {
                    e.show_problems(&problems);
                }
                let f = e.saved.borrow().clone();
                if let Some(f) = f {
                    f();
                }
            },
        );
    }

    /// The Check button: the folder as saved, now.
    pub fn check(self: &Rc<Self>) {
        let dir = self.dir.clone();
        let turn = self.turns.next();
        let me = Rc::downgrade(self);
        off_thread(
            move || check_folder(&dir),
            move |problems| {
                if let Some(e) = me.upgrade()
                    && e.turns.is_current(turn)
                {
                    e.show_problems(&problems);
                }
            },
        );
    }

    fn show_problems(&self, problems: &[String]) {
        while let Some(c) = self.problems.first_child() {
            self.problems.remove(&c);
        }
        let rows: Vec<gtk::Widget> = if problems.is_empty() {
            vec![w::list_row("emblem-default-symbolic", copy::NO_PROBLEMS, "", None).upcast()]
        } else {
            problems
                .iter()
                .map(|p| w::list_row("dialog-warning-symbolic", p, "", None).upcast())
                .collect()
        };
        self.problems.append(&w::section("Problems", &rows));
    }
}

/// The text area's least height (about 14 lines of 13 px text).
const EDITOR_MIN_HEIGHT: i32 = 240;

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wol-maker-ed-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_new_plugin_is_the_cli_scaffold_and_checks_clean() {
        let root = tmp("new");
        let dir = create(
            &root,
            "alice.demo",
            &[Kind::Menubar, Kind::Widget, Kind::Tile],
        )
        .unwrap();
        assert_eq!(dir, root.join("alice.demo"));
        let want = cp::scaffold(
            "alice.demo",
            "Demo",
            &[Kind::Menubar, Kind::Widget, Kind::Tile],
        )
        .unwrap();
        for (rel, text) in &want {
            assert_eq!(
                &std::fs::read_to_string(dir.join(rel)).unwrap(),
                text,
                "{rel}"
            );
        }
        assert_eq!(check_folder(&dir), Vec::<String>::new());
        assert_eq!(
            create(&root, "alice.demo", &[]).unwrap_err(),
            NotCreated::Exists,
            "an existing folder is never written into"
        );
        assert!(matches!(
            create(&root, "Not An Id", &[]),
            Err(NotCreated::Id(s)) if s.contains("reverse-DNS")
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_problems_are_the_cli_check_and_follow_the_draft() {
        let root = tmp("draft");
        let dir = create(&root, "alice.draft", &[Kind::Menubar]).unwrap();
        let l = load(&dir).unwrap();
        assert_eq!(l.files[0].0, MANIFEST_FILE, "the manifest comes first");
        assert!(l.files.iter().any(|f| f.0 == "bar.lua"));
        assert!(l.problems.is_empty(), "{:?}", l.problems);
        // A draft that breaks the manifest: the same sentence the folder check gives.
        let broken = l.files[0].1.replace("bar.lua", "gone.lua");
        let p = check_draft(&dir, MANIFEST_FILE, &broken);
        assert_eq!(
            p,
            ["plugin.toml names gone.lua, but the folder has no such file."]
        );
        // A Lua draft that does not load: named by its file, as the CLI does.
        let p = check_draft(&dir, "bar.lua", "return 1");
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].starts_with("bar.lua: "), "{p:?}");
        // The real folder was never touched by the drafts.
        assert!(check_folder(&dir).is_empty());
        assert!(
            std::fs::read_dir(std::env::temp_dir())
                .unwrap()
                .flatten()
                .all(|e| !e
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&format!("wol-maker-check-{}-", std::process::id()))),
            "scratch copies are removed"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_failed_create_leaves_no_folder() {
        let root = tmp("partial");
        let dir = root.join("alice.partial");
        // The second file cannot be written: the first one is, then the whole folder goes.
        let files = vec![
            ("plugin.toml".to_string(), "x".to_string()),
            ("../escape.lua".to_string(), "y".to_string()),
        ];
        assert_eq!(
            create_files(&root, "alice.partial", &files),
            Err(NotCreated::Disk)
        );
        assert!(!dir.exists(), "no half-written folder is left");
        assert!(
            create(&root, "alice.partial", &[]).is_ok(),
            "a retry is not Exists"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn cleanup_deletes_only_what_was_written() {
        let root = tmp("cleanup");
        let scratch = root.join("scratch");
        std::fs::create_dir_all(scratch.join("a/b")).unwrap();
        std::fs::create_dir_all(scratch.join("keep")).unwrap();
        for f in [
            "top.lua",
            "a/b/deep.lua",
            "keep/mine.lua",
            "keep/theirs.txt",
        ] {
            std::fs::write(scratch.join(f), "x").unwrap();
        }
        let wrote: Vec<PathBuf> = ["top.lua", "a/b/deep.lua", "keep/mine.lua"]
            .iter()
            .map(|f| scratch.join(f))
            .chain([scratch.canonicalize().unwrap().join("a/b/gone.lua")])
            .collect();
        remove_written(&scratch, &wrote);
        assert!(
            !scratch.join("a").exists(),
            "emptied folders go, deepest first"
        );
        assert!(!scratch.join("top.lua").exists() && !scratch.join("keep/mine.lua").exists());
        assert!(
            scratch.join("keep/theirs.txt").exists(),
            "a file nobody listed stays, and so do its folders"
        );
        // A path outside the root is never touched.
        let outside = root.join("outside.txt");
        std::fs::write(&outside, "x").unwrap();
        remove_written(&scratch, std::slice::from_ref(&outside));
        assert!(outside.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_folder_without_a_manifest_is_not_a_plugin() {
        let root = tmp("none");
        assert_eq!(load(&root), Err(copy::NOT_A_PLUGIN));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn saving_replaces_the_file_whole_and_leaves_no_scratch() {
        let root = tmp("save");
        let p = root.join("plugin.toml");
        std::fs::write(&p, "old").unwrap();
        save_atomic(&p, "new text").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "new text");
        let left: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().collect();
        assert_eq!(left.len(), 1, "only the file itself is left");
        // A folder that is gone: an error, nothing written.
        assert!(save_atomic(&root.join("missing/x.lua"), "x").is_err());
        // The file keeps its bits; a new one is 0644 whatever the umask.
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        save_atomic(&p, "again").unwrap();
        assert_eq!(mode(&p), 0o755);
        let fresh = root.join("fresh.lua");
        save_atomic(&fresh, "x").unwrap();
        assert_eq!(mode(&fresh), 0o644);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn only_the_newest_edit_is_current() {
        let t = Turns::default();
        let a = t.next();
        let b = t.next();
        assert!(!t.is_current(a), "an older edit's timer does nothing");
        assert!(t.is_current(b));
        let shared = t.clone();
        let c = shared.next();
        assert!(
            t.is_current(c) && !t.is_current(b),
            "clones share the count"
        );
        assert_eq!(CHECK_AFTER, Duration::from_millis(400));
    }

    #[test]
    fn names_read_like_the_cli_makes_them() {
        assert_eq!(plugin_name("alice.weather"), "Weather");
        assert_eq!(plugin_name("alice.weather-now"), "Weather now");
    }
}
