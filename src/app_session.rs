// SPDX-License-Identifier: GPL-2.0-or-later
//! What wold asks the desktop to do for an app whose call passed its check (ADR 0020). wold is a
//! system daemon, so anything of the person's session runs here, in the account's own shell. No
//! GTK: a short thread per ask, its reply a `session.answer` to wold.
use serde_json::{Value, json};
use shared::portal::{Ask, Call, MAX_CLIPBOARD, MAX_LINE};
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use wol_core::system::{Host, PosixHost};

const SINK: &str = "@DEFAULT_AUDIO_SINK@";
/// Characters of shared text the dialog shows.
const MAX_PREVIEW: usize = 400;
/// Rows a search hands an app.
const MAX_FOUND: usize = 20;
/// Apps the open-with chooser offers (the dialog's row of buttons stays readable).
const MAX_CHOICES: usize = 4;

/// The session, apart from the rules (fake in tests).
pub trait Session {
    /// Text on the clipboard, at most `max` + 1 bytes; None when it holds no text.
    fn clipboard(&self, max: usize) -> Option<Vec<u8>>;
    fn set_clipboard(&self, text: &str) -> bool;
    /// Where the person wants shared text to go; None when they cancelled. Blocks until then.
    fn share(&self, title: &str, text: &str) -> Option<ShareTo>;
    /// Titles and kinds of the apps and Settings pages found. Never the rows of provider apps:
    /// those are the person's, for Search alone.
    fn search(&self, text: &str) -> Vec<(String, String)>;
    /// Volume and mute of the default output.
    fn volume(&self) -> Option<(u8, bool)>;
    /// (title, artist, player, playing).
    fn now_playing(&self) -> Option<(String, String, String, bool)>;
    fn set_volume(&self, percent: u8) -> bool;
    fn set_muted(&self, muted: bool) -> bool;
    /// What the person picked; None when they cancelled. Blocks until then.
    fn panel(&self, save: bool, title: &str, name: &str) -> Option<PathBuf>;
    /// The person's home folder.
    fn home(&self) -> PathBuf;
    /// The person's Trash on the home drive, where the Files Trash looks.
    fn trash_dir(&self) -> PathBuf;
    /// Move `path` to the Trash as the person, the one way the system does it
    /// (the desktop's own Trash, as Settings > Malware's `gio trash` does). False when it failed.
    fn trash(&self, path: &Path) -> bool;
    /// Where the person wants the file called `name` to go; None when they cancelled. Blocks.
    fn share_file(&self, name: &str) -> Option<FileTo>;
    /// Put the file on the clipboard as a link to it (`text/uri-list`), so a paste in Files or
    /// another app takes the file itself.
    fn copy_file(&self, path: &Path) -> bool;
    /// The chooser of the apps that open `path`, then the chosen one opens it: false when the
    /// person cancelled, or the sentence to show. Blocks until then.
    fn open_with(&self, path: &Path) -> Result<bool, String>;
}

/// The running desktop.
pub struct Desktop;

impl Session for Desktop {
    fn clipboard(&self, max: usize) -> Option<Vec<u8>> {
        let mut c = Command::new("wl-paste")
            .args(["--no-newline", "--type", "text"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut out = Vec::new();
        let read = c
            .stdout
            .take()?
            .take(max as u64 + 1)
            .read_to_end(&mut out)
            .is_ok();
        let long = out.len() > max;
        if long {
            let _ = c.kill();
        }
        let done = c.wait().ok()?;
        // Killed for being long is still an answer: `answer` refuses the length.
        (read && (long || done.success())).then_some(out)
    }

    fn set_clipboard(&self, text: &str) -> bool {
        copy(text, None)
    }

    fn home(&self) -> PathBuf {
        wol_core::widgets::home()
    }

    fn trash_dir(&self) -> PathBuf {
        wol_core::malware::home_trash()
    }

    fn trash(&self, path: &Path) -> bool {
        // The desktop's own Trash, in this process: no `gio` program to find and wait for.
        use gtk::gio::prelude::FileExt;
        gtk::gio::File::for_path(path)
            .trash(gtk::gio::Cancellable::NONE)
            .is_ok()
    }

    fn share_file(&self, name: &str) -> Option<FileTo> {
        let name = name.to_owned();
        let (tx, rx) = std::sync::mpsc::channel();
        gtk::glib::MainContext::default().invoke(move || {
            let d = gtk::AlertDialog::builder()
                .modal(true)
                .message(name.as_str())
                .detail(crate::copy::FILE_SHEET_DETAIL)
                .buttons(["Cancel", "Open With\u{2026}", "Save a Copy\u{2026}", "Copy"])
                .cancel_button(0)
                .default_button(3)
                .build();
            let cancel = gtk::gio::Cancellable::NONE;
            d.choose(None::<&gtk::Window>, cancel, move |r| {
                let _ = tx.send(r.ok());
            });
        });
        match rx.recv().ok().flatten() {
            Some(1) => Some(FileTo::App),
            Some(2) => Some(FileTo::Copy),
            Some(3) => Some(FileTo::Clipboard),
            _ => None,
        }
    }

    fn copy_file(&self, path: &Path) -> bool {
        use gtk::gio::prelude::FileExt;
        let uri = gtk::gio::File::for_path(path).uri();
        copy(&format!("{uri}\r\n"), Some("text/uri-list"))
    }

    fn open_with(&self, path: &Path) -> Result<bool, String> {
        use gtk::gio::prelude::*;
        // Read off the main thread: X11 apps get the scale the session exported.
        let env = crate::kit::user_scale_env();
        let path = path.to_owned();
        let (tx, rx) = std::sync::mpsc::channel::<Result<bool, String>>();
        gtk::glib::MainContext::default().invoke(move || {
            let apps = apps_for(&path);
            if apps.is_empty() {
                let _ = tx.send(Err(crate::copy::OPEN_WITH_NONE.into()));
                return;
            }
            let name = path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            let mut labels = vec!["Cancel".to_owned()];
            labels.extend(apps.iter().map(|a| a.display_name().to_string()));
            let d = gtk::AlertDialog::builder()
                .modal(true)
                .message(format!("Open \u{201c}{name}\u{201d} With"))
                .detail(crate::copy::OPEN_WITH_DETAIL)
                .buttons(labels.iter().map(String::as_str).collect::<Vec<_>>())
                .cancel_button(0)
                .build();
            let cancel = gtk::gio::Cancellable::NONE;
            d.choose(None::<&gtk::Window>, cancel, move |r| {
                let picked = r.ok().and_then(|i| usize::try_from(i).ok()?.checked_sub(1));
                let got = match picked.and_then(|i| apps.get(i)) {
                    None => Ok(false),
                    Some(a) => {
                        let file = gtk::gio::File::for_path(&path);
                        let ctx = crate::kit::data::launch_context(&env);
                        a.launch(&[file], Some(&ctx))
                            .map(|()| true)
                            .map_err(|_| crate::copy::OPEN_WITH_NOT_OPENED.to_owned())
                    }
                };
                let _ = tx.send(got);
            });
        });
        rx.recv().unwrap_or(Ok(false))
    }

    fn search(&self, text: &str) -> Vec<(String, String)> {
        let items = crate::ui::searchbar::index(crate::desktop::load_all());
        crate::search::search(&items, text, MAX_FOUND)
            .into_iter()
            .map(|i| (i.title.clone(), i.kind.to_owned()))
            .collect()
    }

    fn volume(&self) -> Option<(u8, bool)> {
        let t = PosixHost.run(&["wpctl", "get-volume", SINK])?;
        wol_core::controls::parse_wpctl_volume(&t).map(|v| (v.percent, v.muted))
    }

    fn now_playing(&self) -> Option<(String, String, String, bool)> {
        crate::widgets::media::now().map(|p| (p.title, p.artist, p.player, p.playing))
    }

    fn set_volume(&self, percent: u8) -> bool {
        PosixHost
            .run(&["wpctl", "set-volume", SINK, &format!("{percent}%")])
            .is_some()
    }

    fn set_muted(&self, muted: bool) -> bool {
        let m = if muted { "1" } else { "0" };
        PosixHost.run(&["wpctl", "set-mute", SINK, m]).is_some()
    }

    fn share(&self, title: &str, text: &str) -> Option<ShareTo> {
        let (title, preview) = (title.to_owned(), preview(text));
        let (tx, rx) = std::sync::mpsc::channel();
        gtk::glib::MainContext::default().invoke(move || {
            let d = gtk::AlertDialog::builder()
                .modal(true)
                .message(title.as_str())
                .detail(preview.as_str())
                .buttons(["Cancel", "Save\u{2026}", "Copy"])
                .cancel_button(0)
                .default_button(2)
                .build();
            let cancel = gtk::gio::Cancellable::NONE;
            d.choose(None::<&gtk::Window>, cancel, move |r| {
                let _ = tx.send(r.ok());
            });
        });
        match rx.recv().ok().flatten() {
            Some(1) => Some(ShareTo::File),
            Some(2) => Some(ShareTo::Clipboard),
            _ => None,
        }
    }

    fn panel(&self, save: bool, title: &str, name: &str) -> Option<PathBuf> {
        let (title, name) = (title.to_owned(), name.to_owned());
        let (tx, rx) = std::sync::mpsc::channel();
        gtk::glib::MainContext::default().invoke(move || {
            crate::ui::app_host::pick_file(save, &title, &name, move |p| {
                let _ = tx.send(p);
            })
        });
        rx.recv().ok().flatten()
    }
}

/// The apps that open `path`'s kind of file, the recommended first, at most [`MAX_CHOICES`].
fn apps_for(path: &Path) -> Vec<gtk::gio::AppInfo> {
    use gtk::gio::prelude::*;
    let (kind, _) = gtk::gio::content_type_guess(Some(path), None);
    let mut apps: Vec<gtk::gio::AppInfo> = Vec::new();
    let found = gtk::gio::AppInfo::recommended_for_type(&kind)
        .into_iter()
        .chain(gtk::gio::AppInfo::all_for_type(&kind));
    for a in found {
        if a.should_show() && !apps.iter().any(|b| b.equal(&a)) {
            apps.push(a);
        }
    }
    apps.truncate(MAX_CHOICES);
    apps
}

/// Put `text` on the clipboard, as `kind` (plain text when None). On stdin: an argument would
/// show in every process listing.
fn copy(text: &str, kind: Option<&str>) -> bool {
    let mut cmd = Command::new("wl-copy");
    if let Some(k) = kind {
        cmd.args(["--type", k]);
    }
    let Ok(mut c) = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let wrote = c
        .stdin
        .take()
        .is_some_and(|mut i| i.write_all(text.as_bytes()).is_ok());
    wrote && c.wait().is_ok_and(|s| s.success())
}

/// Where shared text goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShareTo {
    Clipboard,
    File,
}

/// Where a shared file goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileTo {
    /// A link to it on the clipboard.
    Clipboard,
    /// A copy, where the person saves it.
    Copy,
    /// The app the person picks next opens it.
    App,
}

/// Move `path` to the person's Trash: its name there (what the Files Trash shows and what puts
/// it back), or None when the Trash of another drive took it. Never the home folder, a folder
/// holding it, or something already in the Trash.
fn trash(s: &dyn Session, path: &Path) -> Result<Option<String>, String> {
    if path.symlink_metadata().is_err() {
        return Err(crate::copy::FILE_GONE.into());
    }
    let bin = s.trash_dir();
    if s.home().starts_with(path) || bin.starts_with(path) {
        return Err(crate::copy::TRASH_KEEPS.into());
    }
    if path.starts_with(&bin) {
        return Err(crate::copy::TRASH_ALREADY.into());
    }
    if !s.trash(path) {
        return Err(crate::copy::TRASH_NOT_MOVED.into());
    }
    let (bin, path) = (bin.to_string_lossy(), path.to_string_lossy());
    Ok(wol_core::malware::trashed_name(&PosixHost, &bin, &path))
}

/// A plain file the person may read: only that is shared or handed to another app.
fn shareable(path: &Path) -> Result<(), String> {
    match std::fs::metadata(path) {
        Ok(m) if m.is_file() => Ok(()),
        Ok(_) => Err(crate::copy::SHARE_ONLY_FILES.into()),
        Err(_) => Err(crate::copy::FILE_GONE.into()),
    }
}

/// Send the file at `path` where the person chose, as them.
fn share_file(s: &dyn Session, path: &Path) -> Result<&'static str, String> {
    shareable(path)?;
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    match s.share_file(&name) {
        None => Ok("cancelled"),
        Some(FileTo::Clipboard) => s
            .copy_file(path)
            .then_some("copied")
            .ok_or_else(|| crate::copy::CLIPBOARD_UNCHANGED.into()),
        Some(FileTo::App) => open_with(s, path),
        Some(FileTo::Copy) => {
            let Some(to) = s.panel(true, "Save a Copy", &name) else {
                return Ok("cancelled");
            };
            let same = |a: &Path, b: &Path| {
                a.canonicalize()
                    .ok()
                    .is_some_and(|a| b.canonicalize().is_ok_and(|b| a == b))
            };
            if same(path, &to) {
                return Ok("saved");
            }
            let copied = File::open(path).and_then(|mut from| {
                let mut out = open_pick(&to, true)?;
                std::io::copy(&mut from, &mut out)
            });
            copied
                .map(|_| "saved")
                .map_err(|_| crate::copy::COPY_NOT_SAVED.into())
        }
    }
}

/// The app the person picks opens the file at `path`.
fn open_with(s: &dyn Session, path: &Path) -> Result<&'static str, String> {
    shareable(path)?;
    s.open_with(path)
        .map(|opened| if opened { "opened" } else { "cancelled" })
}

/// The start of shared text, for the dialog.
fn preview(text: &str) -> String {
    let mut t: String = text.chars().take(MAX_PREVIEW).collect();
    if t.len() < text.len() {
        t.push('\u{2026}');
    }
    t
}

/// Send `text` where the person chose, as them.
fn share(s: &dyn Session, title: &str, text: &str) -> Result<&'static str, String> {
    match s.share(title, text) {
        None => Ok("cancelled"),
        Some(ShareTo::Clipboard) => s
            .set_clipboard(text)
            .then_some("copied")
            .ok_or_else(|| "The clipboard could not be changed.".into()),
        Some(ShareTo::File) => {
            let name = format!("{}.txt", title.replace('/', "-"));
            let Some(path) = s.panel(true, title, &name) else {
                return Ok("cancelled");
            };
            open_pick(&path, true)
                .and_then(|mut f| f.write_all(text.as_bytes()))
                .map(|()| "saved")
                .map_err(|_| "That file could not be saved.".into())
        }
    }
}

/// The person opened a row an app provided. Waits for wold: Search closes next.
pub fn open_row(provider: i32, key: &str) {
    let c = Call::SearchOpen {
        provider,
        key: key.to_owned(),
    };
    let a = crate::app_notices::call(&crate::app_notices::wold_socket(), &c);
    if !a.ok {
        eprintln!("wol-shell: open a search row: {:?}", a.message);
    }
}

/// A row from an app that provides search results.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub struct ProviderRow {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub detail: String,
    /// The app's name.
    pub app: String,
    pub provider: i32,
}

/// What the apps providing search results have for `text`, within wold's short wait.
pub fn provider_rows(text: &str) -> Vec<ProviderRow> {
    let c = Call::SearchCollect {
        text: text.to_owned(),
    };
    if c.validate().is_err() {
        return Vec::new();
    }
    let a = crate::app_notices::call(&crate::app_notices::wold_socket(), &c);
    parse_rows(a.data)
}

fn parse_rows(data: Option<Value>) -> Vec<ProviderRow> {
    let rows = data.and_then(|d| d.get("results").cloned());
    let mut rows: Vec<ProviderRow> = rows
        .and_then(|r| serde_json::from_value(r).ok())
        .unwrap_or_default();
    for r in &mut rows {
        if let Some(name) = r.app.rsplit('/').next().filter(|n| !n.is_empty()) {
            r.app = name.to_owned();
        }
    }
    rows
}

fn done(ok: bool) -> Result<Option<Value>, String> {
    if ok {
        Ok(None)
    } else {
        Err("Sound is not available.".into())
    }
}

/// One ask -> what goes back, or a sentence to show.
pub fn answer(s: &dyn Session, ask: Ask) -> Result<Option<Value>, String> {
    match ask {
        Ask::ClipboardRead => {
            let bytes = s
                .clipboard(MAX_CLIPBOARD)
                .ok_or("The clipboard holds no text.")?;
            if bytes.len() > MAX_CLIPBOARD {
                return Err("The text on the clipboard is too long to hand to an app.".into());
            }
            let text = String::from_utf8(bytes).map_err(|_| "The clipboard holds no text.")?;
            Ok(Some(json!({ "text": text })))
        }
        Ask::Panel { .. } => Err("Panels are answered with a file.".into()),
        Ask::Approval { .. } => Err("Approvals are answered on the approval sheet.".into()),
        Ask::Share { title, text } => {
            let done = share(s, &title, &text)?;
            Ok(Some(json!({ "done": done })))
        }
        Ask::Trash { path } => Ok(Some(match trash(s, Path::new(&path))? {
            Some(name) => json!({ "name": name }),
            None => json!({}),
        })),
        Ask::ShareFile { path } => {
            let done = share_file(s, Path::new(&path))?;
            Ok(Some(json!({ "done": done })))
        }
        Ask::OpenWith { path } => {
            let done = open_with(s, Path::new(&path))?;
            Ok(Some(json!({ "done": done })))
        }
        Ask::Search { text } => {
            let rows: Vec<Value> = s
                .search(&text)
                .into_iter()
                .map(|(title, kind)| json!({ "title": title, "kind": kind }))
                .collect();
            Ok(Some(json!({ "results": rows })))
        }
        Ask::AudioInfo => {
            let (volume, muted) = s.volume().ok_or("Sound is not available.")?;
            let mut v = json!({ "volume": volume, "muted": muted });
            if let Some((title, artist, player, playing)) = s.now_playing() {
                v["now_playing"] = json!({
                    "title": title, "artist": artist, "player": player, "playing": playing
                });
            }
            Ok(Some(v))
        }
        Ask::AudioSetVolume { percent } => done(s.set_volume(percent.min(100))),
        Ask::AudioSetMuted { muted } => done(s.set_muted(muted)),
        Ask::ClipboardWrite { text } => {
            if s.set_clipboard(&text) {
                Ok(None)
            } else {
                Err("The clipboard could not be changed.".into())
            }
        }
    }
}

/// The `session.answer` for one ask; never longer than wold reads.
pub fn reply_of(s: &dyn Session, id: u32, ask: Ask) -> Call {
    let (error, data) = match answer(s, ask) {
        Ok(data) => (None, data),
        Err(m) => (Some(m), None),
    };
    let c = Call::SessionAnswer { id, error, data };
    if c.encode().len() < MAX_LINE {
        return c;
    }
    Call::SessionAnswer {
        id,
        error: Some("The answer is too long to hand to an app.".into()),
        data: None,
    }
}

/// Open what the person picked, as them: wold never opens a path for an app. Only a plain file,
/// so a pipe cannot stall the app or the desktop.
fn open_pick(path: &Path, save: bool) -> std::io::Result<File> {
    let mut o = File::options();
    o.custom_flags(shared::sys::OPEN_QUIET);
    if save {
        o.write(true).create(true).truncate(false);
    } else {
        o.read(true);
    }
    let f = o.open(path)?;
    if !f.metadata()?.is_file() {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    if save {
        f.set_len(0)?;
    }
    Ok(f)
}

/// The answer to a panel, and the file to attach.
pub fn panel_reply(
    s: &dyn Session,
    id: u32,
    save: bool,
    title: &str,
    name: &str,
) -> (Call, Option<File>) {
    let answer = |error: Option<&str>, data| Call::SessionAnswer {
        id,
        error: error.map(str::to_owned),
        data,
    };
    let Some(path) = s.panel(save, title, name) else {
        return (answer(None, Some(json!({"cancelled": true}))), None);
    };
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    match (open_pick(&path, save), name) {
        (Ok(f), Some(name)) => (answer(None, Some(json!({ "name": name }))), Some(f)),
        _ => (answer(Some("That file could not be opened."), None), None),
    }
}

/// Shows wold's approval sheet and blocks until the person answers it (set by
/// `ui::approval::handler`, which owns the GTK sheet). None when no sheet can be shown.
pub type SheetFn = dyn Fn(shared::shell::Approval) -> Option<shared::shell::Answer> + Send + Sync;

static SHEET: std::sync::OnceLock<Box<SheetFn>> = std::sync::OnceLock::new();

/// Where wold's approval asks go (audit S1): the sheet of this session. Set once, by the UI.
pub fn set_sheet(f: Box<SheetFn>) {
    let _ = SHEET.set(f);
}

/// The answer to wold's approval ask `id`: the sheet's answer as data, or why nobody could answer.
pub fn approval_reply(sheet: Option<&SheetFn>, id: u32, a: shared::shell::Approval) -> Call {
    let answer = sheet.and_then(|f| f(a));
    match answer.and_then(|x| serde_json::to_value(x).ok()) {
        Some(data) => Call::SessionAnswer {
            id,
            error: None,
            data: Some(data),
        },
        None => Call::SessionAnswer {
            id,
            error: Some("No approval sheet can be shown.".into()),
            data: None,
        },
    }
}

fn reply(wold: &Path, s: &dyn Session, id: u32, ask: Ask) {
    let (call, file) = match ask {
        Ask::Panel { save, title, name } => panel_reply(s, id, save, &title, &name),
        Ask::Approval { approval } => (
            approval_reply(SHEET.get().map(|b| &**b), id, approval),
            None,
        ),
        ask => (reply_of(s, id, ask), None),
    };
    let a = crate::app_notices::call_with(wold, &call, file.as_ref());
    if !a.ok {
        eprintln!("wol-shell: answer to wold: {:?}", a.message);
    }
}

/// Answers being worked on at once; more are refused at once (R2C-18: a flood of asks never
/// becomes a flood of threads). Above wold's own per-account cap (8 asks).
pub const ANSWERS_IN_FLIGHT: usize = 16;

static IN_FLIGHT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A place among [`ANSWERS_IN_FLIGHT`]; dropping it frees it.
struct Busy;

impl Busy {
    fn take() -> Option<Self> {
        use std::sync::atomic::Ordering::SeqCst;
        let mut n = IN_FLIGHT.load(SeqCst);
        loop {
            if n >= ANSWERS_IN_FLIGHT {
                return None;
            }
            match IN_FLIGHT.compare_exchange_weak(n, n + 1, SeqCst, SeqCst) {
                Ok(_) => return Some(Busy),
                Err(now) => n = now,
            }
        }
    }
}

// Gives the answer slot back to IN_FLIGHT.
impl Drop for Busy {
    fn drop(&mut self) {
        IN_FLIGHT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Answer `ask` on its own thread, so a slow one holds up nothing else. Past
/// [`ANSWERS_IN_FLIGHT`] the ask is answered "busy" right away.
pub fn answer_later(wold: PathBuf, id: u32, ask: Ask) {
    let Some(slot) = Busy::take() else {
        let call = Call::SessionAnswer {
            id,
            error: Some(crate::copy::DESKTOP_BUSY.into()),
            data: None,
        };
        let _ = crate::app_notices::call(&wold, &call);
        return;
    };
    std::thread::spawn(move || {
        let _slot = slot;
        reply(&wold, &Desktop, id, ask)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        clip: Option<Vec<u8>>,
        set: RefCell<Vec<String>>,
        refuse: bool,
        pick: Option<PathBuf>,
        sound: bool,
        share_to: Option<ShareTo>,
        file_to: Option<FileTo>,
        dir: PathBuf,
        /// What the chooser does: Some(true) opens, Some(false) cancels, None has no app.
        opens: Option<bool>,
    }

    impl Session for Fake {
        fn clipboard(&self, _: usize) -> Option<Vec<u8>> {
            self.clip.clone()
        }
        fn set_clipboard(&self, text: &str) -> bool {
            self.set.borrow_mut().push(text.into());
            !self.refuse
        }
        fn panel(&self, _: bool, _: &str, _: &str) -> Option<PathBuf> {
            self.pick.clone()
        }
        fn volume(&self) -> Option<(u8, bool)> {
            self.sound.then_some((40, false))
        }
        fn share(&self, _: &str, _: &str) -> Option<ShareTo> {
            self.share_to
        }
        fn search(&self, text: &str) -> Vec<(String, String)> {
            vec![(text.to_uppercase(), "Application".into())]
        }
        fn now_playing(&self) -> Option<(String, String, String, bool)> {
            self.sound
                .then(|| ("Song".into(), "Band".into(), "Player".into(), true))
        }
        fn set_volume(&self, percent: u8) -> bool {
            self.set.borrow_mut().push(format!("volume {percent}"));
            self.sound
        }
        fn set_muted(&self, muted: bool) -> bool {
            self.set.borrow_mut().push(format!("muted {muted}"));
            self.sound
        }
        fn home(&self) -> PathBuf {
            self.dir.join("home")
        }
        fn trash_dir(&self) -> PathBuf {
            self.dir.join("home/.local/share/Trash")
        }
        /// The standard move: the file into `files/`, its note into `info/`.
        fn trash(&self, path: &Path) -> bool {
            self.set
                .borrow_mut()
                .push(format!("trash {}", path.display()));
            let bin = self.trash_dir();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let note = format!(
                "[Trash Info]\nPath={}\nDeletionDate=2026-11-03T12:00:00\n",
                path.display()
            );
            !self.refuse
                && std::fs::create_dir_all(bin.join("files")).is_ok()
                && std::fs::create_dir_all(bin.join("info")).is_ok()
                && std::fs::write(bin.join("info").join(format!("{name}.trashinfo")), note).is_ok()
                && std::fs::rename(path, bin.join("files").join(&name)).is_ok()
        }
        fn share_file(&self, name: &str) -> Option<FileTo> {
            self.set.borrow_mut().push(format!("sheet {name}"));
            self.file_to
        }
        fn copy_file(&self, path: &Path) -> bool {
            self.set
                .borrow_mut()
                .push(format!("copy {}", path.display()));
            !self.refuse
        }
        fn open_with(&self, path: &Path) -> Result<bool, String> {
            self.set
                .borrow_mut()
                .push(format!("open {}", path.display()));
            self.opens
                .ok_or_else(|| "No app on this computer opens this kind of file.".into())
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wol-app-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("home/Pictures")).unwrap();
        d
    }

    #[test]
    fn trash_moves_with_the_note_and_answers_the_name() {
        let dir = scratch("trash");
        let f = Fake {
            dir: dir.clone(),
            ..Fake::default()
        };
        let pic = dir.join("home/Pictures/a.jpg");
        std::fs::write(&pic, b"x").unwrap();
        let ask = |p: &Path| Ask::Trash {
            path: p.display().to_string(),
        };
        assert_eq!(answer(&f, ask(&pic)), Ok(Some(json!({ "name": "a.jpg" }))));
        assert!(!pic.exists());
        assert!(f.trash_dir().join("files/a.jpg").exists());
        assert_eq!(
            answer(&f, ask(&pic)),
            Err("That file is not there any more.".into())
        );
        let home = dir.join("home");
        for keep in [home.clone(), dir.clone(), home.join(".local")] {
            assert_eq!(
                answer(&f, ask(&keep)),
                Err("That folder cannot go to the Trash.".into())
            );
        }
        let inside = f.trash_dir().join("files/a.jpg");
        assert_eq!(
            answer(&f, ask(&inside)),
            Err("That is in the Trash already.".into())
        );
        assert_eq!(
            f.set.borrow().len(),
            1,
            "only the first ask reached the Trash"
        );
        // A move that fails says so (the name for no name is wol_core::malware's test).
        let other = dir.join("b.jpg");
        std::fs::write(&other, b"x").unwrap();
        let f2 = Fake {
            dir: dir.join("nowhere"),
            ..Fake::default()
        };
        std::fs::create_dir_all(f2.home()).unwrap();
        let none = Fake { refuse: true, ..f2 };
        assert_eq!(
            answer(&none, ask(&other)),
            Err("That could not be moved to the Trash.".into())
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_shared_file_goes_where_the_person_chose() {
        let dir = scratch("share-file");
        let pic = dir.join("home/Pictures/a.jpg");
        std::fs::write(&pic, b"picture").unwrap();
        let ask = || Ask::ShareFile {
            path: pic.display().to_string(),
        };
        let done = |v: &str| Ok(Some(json!({ "done": v })));
        let to = |file_to, pick, opens| Fake {
            file_to,
            pick,
            opens,
            dir: dir.clone(),
            ..Fake::default()
        };
        let f = to(None, None, None);
        assert_eq!(answer(&f, ask()), done("cancelled"));
        assert_eq!(*f.set.borrow(), ["sheet a.jpg"]);
        let f = to(Some(FileTo::Clipboard), None, None);
        assert_eq!(answer(&f, ask()), done("copied"));
        assert_eq!(f.set.borrow()[1], format!("copy {}", pic.display()));
        assert_eq!(
            answer(&to(Some(FileTo::App), None, Some(true)), ask()),
            done("opened")
        );
        assert_eq!(
            answer(&to(Some(FileTo::App), None, Some(false)), ask()),
            done("cancelled")
        );
        assert_eq!(
            answer(&to(Some(FileTo::Copy), None, None), ask()),
            done("cancelled")
        );
        let copy = dir.join("copy.jpg");
        assert_eq!(
            answer(&to(Some(FileTo::Copy), Some(copy.clone()), None), ask()),
            done("saved")
        );
        assert_eq!(std::fs::read(&copy).unwrap(), b"picture");
        // Saving over itself keeps the picture.
        assert_eq!(
            answer(&to(Some(FileTo::Copy), Some(pic.clone()), None), ask()),
            done("saved")
        );
        assert_eq!(std::fs::read(&pic).unwrap(), b"picture");
        for bad in [dir.join("home"), dir.join("missing.jpg")] {
            let f = to(Some(FileTo::Clipboard), None, None);
            let a = Ask::ShareFile {
                path: bad.display().to_string(),
            };
            assert!(answer(&f, a).is_err());
            assert!(f.set.borrow().is_empty(), "no sheet for {bad:?}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn open_with_answers_opened_or_cancelled() {
        let dir = scratch("open-with");
        let pic = dir.join("home/Pictures/a.jpg");
        std::fs::write(&pic, b"x").unwrap();
        let ask = |p: &Path| Ask::OpenWith {
            path: p.display().to_string(),
        };
        let with = |opens| Fake {
            opens,
            dir: dir.clone(),
            ..Fake::default()
        };
        let done = |v: &str| Ok(Some(json!({ "done": v })));
        assert_eq!(answer(&with(Some(true)), ask(&pic)), done("opened"));
        assert_eq!(answer(&with(Some(false)), ask(&pic)), done("cancelled"));
        assert_eq!(
            answer(&with(None), ask(&pic)),
            Err("No app on this computer opens this kind of file.".into())
        );
        assert_eq!(
            answer(&with(Some(true)), ask(&dir)),
            Err("Only a file can be shared.".into())
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Audit S1: wold's approval ask is answered with what the person chose on the sheet; with
    /// no sheet, an error (wold then treats it as nobody answering).
    #[test]
    fn approval_asks_answer_with_the_sheet() {
        use shared::shell::{Answer, Approval, ApprovalRequest};
        let a = Approval {
            helper: "/usr/bin/wopkg".into(),
            tool: "wopkg packages-update".into(),
            request: ApprovalRequest::Change {
                capability: "packages-update".into(),
                what: "undo the last system update".into(),
            },
        };
        let deny: &SheetFn = &|_| Some(Answer::Deny);
        let Call::SessionAnswer { id, error, data } = approval_reply(Some(deny), 7, a.clone())
        else {
            panic!()
        };
        assert_eq!((id, error), (7, None));
        assert_eq!(data, Some(json!({ "answer": "deny" })));
        let Call::SessionAnswer { error, data, .. } = approval_reply(None, 7, a) else {
            panic!()
        };
        assert!(error.is_some() && data.is_none());
    }

    #[test]
    fn sharing_goes_where_the_person_chose() {
        let ask = || Ask::Share {
            title: "A note".into(),
            text: "text".into(),
        };
        let done = |v: &str| Ok(Some(json!({ "done": v })));
        assert_eq!(answer(&Fake::default(), ask()), done("cancelled"));
        let f = Fake {
            share_to: Some(ShareTo::Clipboard),
            ..Fake::default()
        };
        assert_eq!(answer(&f, ask()), done("copied"));
        assert_eq!(*f.set.borrow(), ["text"]);

        let dir = std::env::temp_dir().join(format!("wol-app-share-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.txt");
        let to_file = |pick| Fake {
            share_to: Some(ShareTo::File),
            pick,
            ..Fake::default()
        };
        assert_eq!(answer(&to_file(None), ask()), done("cancelled"));
        assert_eq!(answer(&to_file(Some(file.clone())), ask()), done("saved"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "text");
        assert!(answer(&to_file(Some(dir.join("no/a.txt"))), ask()).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_dialog_shows_the_start_of_long_text() {
        assert_eq!(preview("short"), "short");
        let p = preview(&"\u{e9}".repeat(MAX_PREVIEW + 1));
        assert_eq!(p.chars().count(), MAX_PREVIEW + 1);
        assert!(p.ends_with('\u{2026}'));
    }

    #[test]
    fn a_search_answers_titles_and_kinds_only() {
        let v = answer(&Fake::default(), Ask::Search { text: "ab".into() });
        assert_eq!(
            v,
            Ok(Some(
                json!({"results": [{"title": "AB", "kind": "Application"}]})
            ))
        );
    }

    #[test]
    fn provider_rows_name_the_app_and_garbage_is_nothing() {
        let got = parse_rows(Some(json!({"results": [
            {"key": "k", "title": "T", "detail": "", "app": "/usr/bin/notes", "provider": 7}
        ]})));
        assert_eq!(got[0].app, "notes");
        assert_eq!(got[0].provider, 7);
        assert!(parse_rows(None).is_empty());
        assert!(parse_rows(Some(json!({"results": 3}))).is_empty());
    }

    #[test]
    fn audio_reads_and_sets() {
        let f = Fake {
            sound: true,
            ..Fake::default()
        };
        let v = answer(&f, Ask::AudioInfo).unwrap().unwrap();
        assert_eq!(v["volume"], 40);
        assert_eq!(v["now_playing"]["title"], "Song");
        assert_eq!(answer(&f, Ask::AudioSetVolume { percent: 200 }), Ok(None));
        assert_eq!(answer(&f, Ask::AudioSetMuted { muted: true }), Ok(None));
        assert_eq!(*f.set.borrow(), ["volume 100", "muted true"]);
        let none = Fake::default();
        assert!(answer(&none, Ask::AudioInfo).is_err());
        assert!(answer(&none, Ask::AudioSetMuted { muted: false }).is_err());
    }

    #[test]
    fn a_panel_gives_the_picked_file_and_nothing_else() {
        let dir = std::env::temp_dir().join(format!("wol-app-panel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let picked = |p: Option<PathBuf>| Fake {
            pick: p,
            ..Fake::default()
        };
        let data = |c: Call| match c {
            Call::SessionAnswer { error, data, .. } => (error, data),
            _ => panic!(),
        };

        let (c, f) = panel_reply(&picked(None), 1, false, "Open", "");
        assert_eq!(data(c), (None, Some(json!({"cancelled": true}))));
        assert!(f.is_none());

        let file = dir.join("a.txt");
        std::fs::write(&file, "text").unwrap();
        let (c, f) = panel_reply(&picked(Some(file.clone())), 2, false, "Open", "");
        assert_eq!(data(c), (None, Some(json!({"name": "a.txt"}))));
        let mut got = String::new();
        f.unwrap().read_to_string(&mut got).unwrap();
        assert_eq!(got, "text");

        let (c, f) = panel_reply(&picked(Some(file.clone())), 3, true, "Save", "a.txt");
        assert_eq!(data(c), (None, Some(json!({"name": "a.txt"}))));
        f.unwrap().write_all(b"new").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"new");

        for bad in [dir.clone(), dir.join("missing")] {
            let (c, f) = panel_reply(&picked(Some(bad)), 4, false, "Open", "");
            assert!(data(c).0.is_some());
            assert!(f.is_none());
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    fn holding(clip: Option<&[u8]>) -> Fake {
        Fake {
            clip: clip.map(<[u8]>::to_vec),
            ..Fake::default()
        }
    }

    #[test]
    fn clipboard_reads() {
        assert_eq!(
            answer(&holding(Some(b"hi")), Ask::ClipboardRead),
            Ok(Some(json!({"text": "hi"})))
        );
        assert!(answer(&holding(None), Ask::ClipboardRead).is_err());
        assert!(answer(&holding(Some(&[0xff])), Ask::ClipboardRead).is_err());
        let long = vec![b'x'; MAX_CLIPBOARD + 1];
        assert!(answer(&holding(Some(&long)), Ask::ClipboardRead).is_err());
    }

    #[test]
    fn clipboard_writes() {
        let f = Fake::default();
        let ask = || Ask::ClipboardWrite { text: "hi".into() };
        assert_eq!(answer(&f, ask()), Ok(None));
        assert_eq!(*f.set.borrow(), ["hi"]);
        let no = Fake {
            refuse: true,
            ..Fake::default()
        };
        assert!(answer(&no, ask()).is_err());
    }

    #[test]
    fn an_answer_that_would_not_fit_a_line_says_so() {
        // Newlines double in JSON: legal text, too long once encoded.
        let text = vec![b'\n'; MAX_CLIPBOARD];
        let Call::SessionAnswer { error, data, .. } =
            reply_of(&holding(Some(&text)), 1, Ask::ClipboardRead)
        else {
            panic!()
        };
        assert!(error.is_some() && data.is_none());
        let c = reply_of(&holding(Some(b"short")), 1, Ask::ClipboardRead);
        assert!(c.validate().is_ok() && c.encode().len() < MAX_LINE);
    }

    /// R2C-18: answers in flight are capped; a freed place is taken again.
    #[test]
    fn answers_in_flight_are_capped() {
        let held: Vec<Busy> = (0..ANSWERS_IN_FLIGHT).map_while(|_| Busy::take()).collect();
        assert_eq!(held.len(), ANSWERS_IN_FLIGHT);
        assert!(Busy::take().is_none());
        drop(held);
        assert!(Busy::take().is_some());
    }
}
