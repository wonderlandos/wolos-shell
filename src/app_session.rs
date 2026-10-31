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

/// The session, apart from the rules (fake in tests).
pub trait Session {
    /// Text on the clipboard, at most `max` + 1 bytes; None when it holds no text.
    fn clipboard(&self, max: usize) -> Option<Vec<u8>>;
    fn set_clipboard(&self, text: &str) -> bool;
    /// Where the person wants shared text to go; None when they cancelled. Blocks until then.
    fn share(&self, title: &str, text: &str) -> Option<ShareTo>;
    /// Titles and kinds of the apps and Settings pages found. Never the rows of provider apps:
    /// those are the person's, for Spotlight alone.
    fn search(&self, text: &str) -> Vec<(String, String)>;
    /// Volume and mute of the default output.
    fn volume(&self) -> Option<(u8, bool)>;
    /// (title, artist, player, playing).
    fn now_playing(&self) -> Option<(String, String, String, bool)>;
    fn set_volume(&self, percent: u8) -> bool;
    fn set_muted(&self, muted: bool) -> bool;
    /// What the person picked; None when they cancelled. Blocks until then.
    fn panel(&self, save: bool, title: &str, name: &str) -> Option<PathBuf>;
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
        // On stdin: an argument would show in every process listing.
        let Ok(mut c) = Command::new("wl-copy")
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

    fn search(&self, text: &str) -> Vec<(String, String)> {
        let items = crate::ui::spotlight::index(crate::desktop::load_all());
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

/// Where shared text goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShareTo {
    Clipboard,
    File,
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

/// The person opened a row an app provided. Waits for wold: Spotlight closes next.
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
        Ask::Share { title, text } => {
            let done = share(s, &title, &text)?;
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

fn reply(wold: &Path, s: &dyn Session, id: u32, ask: Ask) {
    let (call, file) = match ask {
        Ask::Panel { save, title, name } => panel_reply(s, id, save, &title, &name),
        ask => (reply_of(s, id, ask), None),
    };
    let a = crate::app_notices::call_with(wold, &call, file.as_ref());
    if !a.ok {
        eprintln!("wol-shell: answer to wold: {:?}", a.message);
    }
}

/// Answer `ask` on its own thread, so a slow one holds up nothing else.
pub fn answer_later(wold: PathBuf, id: u32, ask: Ask) {
    std::thread::spawn(move || reply(&wold, &Desktop, id, ask));
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
}
