// SPDX-License-Identifier: GPL-2.0-or-later
//! Share targets (E2E-SHARE, docs/modules/share.md): the apps that can take the thing a person
//! shares, found in the desktop entries and nowhere else.
//!
//! * **Mail**: an entry that handles `x-scheme-handler/mailto`. It is started with a `mailto:`
//!   link (subject, body, `attach` for a file), the way any app opens a mail link.
//! * **Declared**: an entry with `X-WolOS-Share=text;url;file` (what the app takes through Share).
//!   Text and links come as `--share-text TEXT` / `--share-url URL` (and `--share-title TITLE`),
//!   a file as a plain file argument.
//! * **Opens**: an entry whose `MimeType` holds the kind of the file (or whose scheme handler
//!   matches the link) and whose `Exec` takes a file or link.
//!
//! Pure rules on entry text: the folders are read by [`read_entries`], blocking, so only off the
//! GTK thread. What a person shared most often comes first ([`Usage`]).
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use wol_core::desktop::Entry as Raw;

/// Tiles the sheet shows; the rest stays behind Open With.
pub const MAX_TARGETS: usize = 8;
/// Bytes of text a `mailto:` body carries.
const MAX_BODY: usize = 4000;
const MAILTO: &str = "x-scheme-handler/mailto";
/// Entries kept in the usage file.
const MAX_USAGE: usize = 200;
/// Bytes read from the usage file: a bigger one is not ours.
const MAX_USAGE_FILE: u64 = 64 * 1024;
/// Text above this many bytes goes to an app that asks for a file in a file, not in its arguments
/// (the process list shows arguments).
pub const SPILL_AT: usize = 4096;

/// What is being shared.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    File { path: String, mime: String },
    Text { title: String, text: String },
    Url { url: String, title: String },
}

impl Item {
    /// A file, its kind guessed from its name and the shared-mime database.
    pub fn file(path: &Path) -> Self {
        let (kind, _) = gtk::gio::content_type_guess(Some(path), None);
        Item::File {
            path: path.to_string_lossy().into_owned(),
            mime: kind.to_string(),
        }
    }

    /// Text; a single web address on its own is a link.
    pub fn text(title: &str, text: &str) -> Self {
        let one = text.trim();
        if is_url(one) {
            return Item::Url {
                url: one.to_owned(),
                title: title.to_owned(),
            };
        }
        Item::Text {
            title: title.to_owned(),
            text: text.to_owned(),
        }
    }

    /// The word an entry's `X-WolOS-Share` uses for this.
    fn word(&self) -> &'static str {
        match self {
            Item::File { .. } => "file",
            Item::Text { .. } => "text",
            Item::Url { .. } => "url",
        }
    }
}

/// A web address on its own: `http` or `https`, `://`, something after it, no spaces.
pub fn is_url(s: &str) -> bool {
    scheme(s).is_some_and(|sc| {
        ["http", "https"].contains(&sc.as_str())
            && s[sc.len()..]
                .strip_prefix("://")
                .is_some_and(|rest| !rest.is_empty())
    }) && !s.contains(char::is_whitespace)
}

/// Order of tiles when nothing was shared before: mail, then apps that asked for it, then the
/// rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Mail,
    Declared,
    Opens,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// The desktop id (`org.gnome.TextEditor.desktop`).
    pub id: String,
    pub name: String,
    pub icon: String,
    pub role: Role,
    exec: String,
    /// The entry declares `url`: it takes `--share-url`; without it a link comes as text.
    url_ok: bool,
    /// The entry declares `share-file`: long text may come as `--share-file PATH`.
    share_file: bool,
}

impl Target {
    /// Whether the app reads long text from `--share-file PATH`.
    pub fn takes_share_file(&self) -> bool {
        self.share_file
    }
}

/// How many times each target was chosen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Usage(BTreeMap<String, u32>);

impl Usage {
    /// `id<TAB>count` per line; anything else is skipped.
    pub fn parse(text: &str) -> Self {
        let mut m = BTreeMap::new();
        for l in text.lines() {
            if let Some((id, n)) = l.split_once('\t')
                && let Ok(n) = n.trim().parse::<u32>()
                && id.ends_with(".desktop")
            {
                m.insert(id.to_owned(), n);
            }
        }
        let mut u = Usage(m);
        u.trim();
        u
    }

    /// Keeps the `MAX_USAGE` most used.
    fn trim(&mut self) {
        while self.0.len() > MAX_USAGE {
            let Some(least) = self
                .0
                .iter()
                .min_by_key(|(_, n)| **n)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            self.0.remove(&least);
        }
    }

    pub fn render(&self) -> String {
        self.0.iter().map(|(k, n)| format!("{k}\t{n}\n")).collect()
    }

    pub fn get(&self, id: &str) -> u32 {
        self.0.get(id).copied().unwrap_or(0)
    }

    pub fn bump(&mut self, id: &str) {
        let n = self.0.entry(id.to_owned()).or_insert(0);
        *n = n.saturating_add(1);
        self.trim();
    }
}

/// Where the counts are kept; None without a state folder or a home (then nothing is counted).
pub fn usage_path() -> Option<PathBuf> {
    let base = crate::envv::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            crate::envv::var("HOME")
                .filter(|h| h.starts_with('/'))
                .map(|h| Path::new(&h).join(".local/state"))
        })?;
    Some(base.join("wolos/share-usage"))
}

/// The counts in `path`, at most 64 KiB read. Blocking: off the GTK thread.
pub fn load_usage(path: &Path) -> Usage {
    use std::io::Read;
    let mut bytes = Vec::new();
    let read =
        std::fs::File::open(path).and_then(|f| f.take(MAX_USAGE_FILE).read_to_end(&mut bytes));
    match read {
        Ok(_) => Usage::parse(&String::from_utf8_lossy(&bytes)),
        Err(_) => Usage::default(),
    }
}

/// One more share to `id`: written next to the file and renamed, so a crash keeps the old counts;
/// the list is trimmed on every write. Blocking: off the GTK thread. Best effort: ranking is not
/// worth a refusal.
pub fn record(path: &Path, id: &str) {
    let mut u = load_usage(path);
    u.bump(id);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("new");
    if std::fs::write(&tmp, u.render()).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

fn has_code(exec: &str, codes: &str) -> bool {
    let mut it = exec.chars();
    while let Some(c) = it.next() {
        if c == '%' {
            match it.next() {
                Some('%') | None => {}
                Some(k) if codes.contains(k) => return true,
                Some(_) => {}
            }
        }
    }
    false
}

fn handles(mimes: &[&str], mime: &str) -> bool {
    if mimes.contains(&mime) {
        return true;
    }
    if let Some((major, _)) = mime.split_once('/')
        && mimes.iter().any(|m| m.strip_suffix("/*") == Some(major))
    {
        return true;
    }
    // Every text kind opens in an app for plain text.
    mime.starts_with("text/") && mimes.contains(&"text/plain")
}

/// The scheme of a web address (`https`), lower case.
fn scheme(url: &str) -> Option<String> {
    let (s, _) = url.split_once(':')?;
    (!s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)))
    .then(|| s.to_ascii_lowercase())
}

/// The targets for `item` among `entries` (desktop id, file text), best first. Entries the app
/// list hides (NoDisplay, Hidden, another desktop, a missing program, terminal-only) never show.
pub fn discover(entries: &[(String, String)], item: &Item, usage: &Usage) -> Vec<Target> {
    let mut out: Vec<Target> = entries
        .iter()
        .filter_map(|(id, text)| target_of(id, text, item))
        .collect();
    out.sort_by(|a, b| {
        usage
            .get(&b.id)
            .cmp(&usage.get(&a.id))
            .then(a.role.cmp(&b.role))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.id.cmp(&b.id))
    });
    out.truncate(MAX_TARGETS);
    out
}

fn target_of(id: &str, text: &str, item: &Item) -> Option<Target> {
    // The app list's own filter: one rule for what counts as an app a person can open.
    let shown = crate::desktop::parse(id, text)?;
    let raw = Raw::parse(text);
    if raw.get("Terminal") == Some("true") {
        return None;
    }
    let exec = raw.exec()?;
    let mimes = raw.list("MimeType").unwrap_or_default();
    let declared = raw.list("X-WolOS-Share").unwrap_or_default();
    let role = if mimes.contains(&MAILTO) {
        Role::Mail
    } else if declared.contains(&item.word())
        || (matches!(item, Item::Url { .. }) && declared.contains(&"text"))
    {
        Role::Declared
    } else {
        let opens = match item {
            Item::File { mime, .. } => handles(&mimes, mime) && has_code(exec, "fFuU"),
            Item::Url { url, .. } => {
                scheme(url)
                    .is_some_and(|s| mimes.contains(&format!("x-scheme-handler/{s}").as_str()))
                    && has_code(exec, "uU")
            }
            Item::Text { .. } => false,
        };
        if !opens {
            return None;
        }
        Role::Opens
    };
    Some(Target {
        id: id.to_owned(),
        name: shown.name,
        icon: shown.icon,
        role,
        exec: exec.to_owned(),
        url_ok: declared.contains(&"url"),
        share_file: declared.contains(&"share-file"),
    })
}

/// The data folders' `applications`, the person's first. Same order as the app list.
pub fn applications_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(h) = crate::envv::var("XDG_DATA_HOME") {
        dirs.push(h.into());
    } else if let Some(h) = crate::envv::var("HOME") {
        dirs.push(Path::new(&h).join(".local/share"));
    }
    let sys = crate::envv::var("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/share".into());
    dirs.extend(sys.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    dirs.into_iter().map(|d| d.join("applications")).collect()
}

/// Every `.desktop` file of `dirs` (first folder wins an id). Blocking: off the GTK thread.
pub fn read_entries(dirs: &[PathBuf]) -> Vec<(String, String)> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d) else {
            continue;
        };
        let mut files: Vec<_> = rd.flatten().collect();
        files.sort_by_key(|f| f.file_name());
        for f in files {
            let id = f.file_name().to_string_lossy().into_owned();
            if id.ends_with(".desktop")
                && seen.insert(id.clone())
                && let Ok(t) = std::fs::read_to_string(f.path())
            {
                out.push((id, t));
            }
        }
    }
    out
}

/// The targets for `item` on this computer, best first. Blocking: off the GTK thread.
pub fn find(item: &Item) -> Vec<Target> {
    let entries = read_entries(&applications_dirs());
    let usage = usage_path().map(|p| load_usage(&p)).unwrap_or_default();
    discover(&entries, item, &usage)
}

/// The target `id` on this computer, if it still takes `item`. Blocking.
pub fn find_one(id: &str, item: &Item) -> Option<Target> {
    find_all_in(&read_entries(&applications_dirs()), id, item)
}

fn find_all_in(entries: &[(String, String)], id: &str, item: &Item) -> Option<Target> {
    entries
        .iter()
        .find(|(i, _)| i == id)
        .and_then(|(i, t)| target_of(i, t, item))
}

// ---- Starting the app ----

/// Percent-encoding for a `mailto:` field (RFC 6068): line breaks as CRLF.
pub fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.replace("\r\n", "\n").replace('\n', "\r\n").bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn clip(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut n = max;
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    &s[..n]
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

/// `s` as one line (control characters become a space or go) or, with `lines`, with its line
/// breaks kept: what goes into a mail header or body never carries a control character.
fn plain(s: &str, lines: bool) -> String {
    s.replace("\r\n", "\n")
        .chars()
        .filter_map(|c| match c {
            '\n' if lines => Some('\n'),
            c if c.is_control() => (c == '\n' || c == '\t').then_some(' '),
            c => Some(c),
        })
        .collect()
}

/// The mail link for `item`: subject, body, and the file as `attach`.
pub fn mailto(item: &Item) -> String {
    let (subject, body, attach) = match item {
        Item::File { path, .. } => (file_name(path), String::new(), Some(path.as_str())),
        Item::Text { title, text } => (title.clone(), clip(text, MAX_BODY).to_owned(), None),
        Item::Url { url, title } => (
            if title.is_empty() {
                url.clone()
            } else {
                title.clone()
            },
            url.clone(),
            None,
        ),
    };
    let mut q = vec![format!("subject={}", encode(plain(&subject, false).trim()))];
    let body = plain(&body, true);
    if !body.trim().is_empty() {
        q.push(format!("body={}", encode(&body)));
    }
    if let Some(p) = attach {
        q.push(format!("attach={}", encode(p)));
    }
    format!("mailto:?{}", q.join("&"))
}

fn file_uri(path: &str) -> String {
    let mut out = String::from("file://");
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Words of an `Exec` line (quotes and backslashes as the specification has them), field codes
/// left in place.
fn words(exec: &str) -> Vec<String> {
    let (mut out, mut cur, mut quoted, mut has) = (Vec::new(), String::new(), false, false);
    let mut it = exec.chars();
    while let Some(c) = it.next() {
        match c {
            '"' => {
                quoted = !quoted;
                has = true;
            }
            '\\' if quoted => {
                if let Some(n) = it.next() {
                    cur.push(n);
                }
            }
            ' ' if !quoted => {
                if has || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            _ => cur.push(c),
        }
    }
    if has || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// `Exec` with its field codes filled: `%f %F` the files, `%u %U` the links (a file's own link
/// when none is given), `%c` the name, `%i` the icon, `%%` a percent; the rest is dropped. When
/// the line has no code that takes them, the files and links are added at the end.
pub fn expand_exec(
    exec: &str,
    name: &str,
    icon: &str,
    files: &[String],
    uris: &[String],
) -> Vec<String> {
    let file_uris: Vec<String> = files.iter().map(|f| file_uri(f)).collect();
    let links: &[String] = if uris.is_empty() { &file_uris } else { uris };
    let mut out = Vec::new();
    let mut taken = false;
    for w in words(exec) {
        match w.as_str() {
            "%f" => {
                if let Some(f) = files.first() {
                    out.push(f.clone());
                    taken = true;
                }
            }
            "%F" => {
                taken |= !files.is_empty();
                out.extend(files.iter().cloned());
            }
            "%u" => {
                if let Some(u) = links.first() {
                    out.push(u.clone());
                    taken = true;
                }
            }
            "%U" => {
                taken |= !links.is_empty();
                out.extend(links.iter().cloned());
            }
            "%i" => {
                if !icon.is_empty() {
                    out.extend(["--icon".to_owned(), icon.to_owned()]);
                }
            }
            "%c" => out.push(name.to_owned()),
            "%k" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => {}
            _ => {
                let mut s = String::new();
                let mut it = w.chars();
                while let Some(c) = it.next() {
                    if c != '%' {
                        s.push(c);
                        continue;
                    }
                    match it.next() {
                        Some('%') => s.push('%'),
                        Some('f') => s.push_str(files.first().map_or("", String::as_str)),
                        Some('u') => s.push_str(links.first().map_or("", String::as_str)),
                        Some('c') => s.push_str(name),
                        _ => {}
                    }
                }
                out.push(s);
            }
        }
    }
    if !taken {
        if !files.is_empty() {
            out.extend(files.iter().cloned());
        } else {
            out.extend(uris.iter().cloned());
        }
    }
    out
}

/// The text `target` should get in a file (`--share-file`) instead of its arguments: only an app
/// that declares `share-file`, and only text above [`SPILL_AT`] bytes.
pub fn spill_text<'a>(target: &Target, item: &'a Item) -> Option<&'a str> {
    match item {
        Item::Text { text, .. }
            if target.role == Role::Declared && target.share_file && text.len() > SPILL_AT =>
        {
            Some(text)
        }
        _ => None,
    }
}

/// The command that hands `item` to `target`. `spill` is the file holding the text when
/// [`spill_text`] said so.
pub fn launch_argv(target: &Target, item: &Item, spill: Option<&Path>) -> Vec<String> {
    let go = |files: &[String], uris: &[String]| {
        expand_exec(&target.exec, &target.name, &target.icon, files, uris)
    };
    let with_title = |mut a: Vec<String>, title: &str| {
        if !title.is_empty() {
            a.extend(["--share-title".to_owned(), title.to_owned()]);
        }
        a
    };
    match (target.role, item) {
        (Role::Mail, _) => go(&[], &[mailto(item)]),
        (Role::Declared, Item::File { path, .. }) | (Role::Opens, Item::File { path, .. }) => {
            go(std::slice::from_ref(path), &[])
        }
        (Role::Opens, Item::Url { url, .. }) => go(&[], std::slice::from_ref(url)),
        (_, Item::Url { url, title }) => {
            // An app that declared text only gets the address as text.
            let flag = if target.url_ok {
                "--share-url"
            } else {
                "--share-text"
            };
            let mut a = go(&[], &[]);
            a.extend([flag.to_owned(), url.clone()]);
            with_title(a, title)
        }
        (_, Item::Text { title, text }) => {
            let mut a = go(&[], &[]);
            match spill {
                Some(p) => a.extend(["--share-file".to_owned(), p.to_string_lossy().into_owned()]),
                None => a.extend(["--share-text".to_owned(), text.clone()]),
            }
            with_title(a, title)
        }
    }
}

/// Folder for `--share-file` texts, in the runtime folder (tmpfs, the person's own, 0700). None
/// without one: the text then stays in the arguments.
pub fn spill_dir() -> Option<PathBuf> {
    crate::envv::var("XDG_RUNTIME_DIR")
        .filter(|d| d.starts_with('/'))
        .map(|d| Path::new(&d).join("wolos/share-texts"))
}

/// Writes `text` into a new file of `dir` (0600; `dir` is made 0700) for `--share-file`, after
/// removing what an earlier share left there for more than ten minutes. Blocking.
pub fn write_spill(dir: &Path, text: &str) -> Option<PathBuf> {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .ok()?;
    if let Ok(rd) = std::fs::read_dir(dir) {
        let old = std::time::Duration::from_secs(600);
        for f in rd.flatten() {
            let stale = f
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > old);
            if stale {
                let _ = std::fs::remove_file(f.path());
            }
        }
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let path = dir.join(format!("{}-{stamp}.txt", std::process::id()));
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .ok()?;
    if f.write_all(text.as_bytes()).is_err() {
        let _ = std::fs::remove_file(&path);
        return None;
    }
    Some(path)
}

/// Shared text from standard input as `wol-share -` takes it: at most libwol's message size (an
/// app's shared text has the same limit), valid UTF-8, line breaks `\n` (tabs become spaces) and no
/// other control character. Err is the sentence to tell the person.
pub fn stdin_text(bytes: Vec<u8>) -> Result<String, String> {
    if bytes.len() > shared::portal::MAX_MESSAGE {
        return Err(crate::copy::SHARE_TEXT_LONG.to_owned());
    }
    let text = String::from_utf8(bytes).map_err(|_| crate::copy::SHARE_TEXT_ODD.to_owned())?;
    let text = text.replace("\r\n", "\n").replace('\t', " ");
    if text.chars().any(|c| c.is_control() && c != '\n') {
        return Err(crate::copy::SHARE_TEXT_ODD.to_owned());
    }
    Ok(text)
}

/// The most bytes `wol-share -` reads (one over the limit, to know it was exceeded).
pub fn stdin_limit() -> u64 {
    u64::try_from(shared::portal::MAX_MESSAGE + 1).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, exec: &str, extra: &str) -> String {
        format!(
            "[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\nIcon=x-{name}\n{extra}\n"
        )
    }

    /// Ten entries on disk, as an applications folder.
    fn fake_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wol-share-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let files: [(&str, String); 10] = [
            (
                "mail.desktop",
                entry("Mail", "mail %U", "MimeType=x-scheme-handler/mailto;"),
            ),
            (
                "notes.desktop",
                entry("Notes", "notes %F", "X-WolOS-Share=text;url;file"),
            ),
            (
                "editor.desktop",
                entry("Editor", "editor %U", "MimeType=text/plain;"),
            ),
            (
                "viewer.desktop",
                entry("Viewer", "viewer %F", "MimeType=image/png;image/jpeg;"),
            ),
            (
                "browser.desktop",
                entry(
                    "Browser",
                    "browser %U",
                    "MimeType=text/html;x-scheme-handler/http;x-scheme-handler/https;",
                ),
            ),
            (
                "hidden.desktop",
                entry(
                    "Secret",
                    "secret %f",
                    "NoDisplay=true\nMimeType=text/plain;",
                ),
            ),
            (
                "gone.desktop",
                entry("Gone", "gone", "Hidden=true\nMimeType=text/plain;"),
            ),
            (
                "shell.desktop",
                entry("Shell", "shell %f", "Terminal=true\nMimeType=text/plain;"),
            ),
            (
                "nocode.desktop",
                entry("Nocode", "nocode", "MimeType=text/plain;"),
            ),
            (
                "other.desktop",
                entry(
                    "Other",
                    "other %f",
                    "OnlyShowIn=GNOME;\nMimeType=text/plain;",
                ),
            ),
        ];
        for (n, t) in files {
            std::fs::write(d.join(n), t).unwrap();
        }
        d
    }

    fn names(t: &[Target]) -> Vec<&str> {
        t.iter().map(|t| t.name.as_str()).collect()
    }

    fn txt() -> Item {
        Item::File {
            path: "/home/a/note.txt".into(),
            mime: "text/plain".into(),
        }
    }

    #[test]
    fn a_text_file_finds_mail_the_declaring_app_and_the_editor_only() {
        let dir = fake_dir("discover");
        let entries = read_entries(std::slice::from_ref(&dir));
        assert_eq!(entries.len(), 10);
        let t = discover(&entries, &txt(), &Usage::default());
        // Hidden, Hidden=true, terminal, another desktop and an Exec that takes no file are out.
        assert_eq!(names(&t), ["Mail", "Notes", "Editor"]);
        assert_eq!(t[0].role, Role::Mail);
        assert_eq!(t[1].role, Role::Declared);
        assert_eq!(t[2].role, Role::Opens);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn each_kind_of_thing_finds_its_own_apps() {
        let dir = fake_dir("kinds");
        let entries = read_entries(std::slice::from_ref(&dir));
        let png = Item::File {
            path: "/a/p.png".into(),
            mime: "image/png".into(),
        };
        assert_eq!(
            names(&discover(&entries, &png, &Usage::default())),
            ["Mail", "Notes", "Viewer"]
        );
        let page = Item::File {
            path: "/a/p.html".into(),
            mime: "text/html".into(),
        };
        assert_eq!(
            names(&discover(&entries, &page, &Usage::default())),
            ["Mail", "Notes", "Browser", "Editor"]
        );
        let text = Item::text("Note", "some words");
        assert!(matches!(text, Item::Text { .. }));
        assert_eq!(
            names(&discover(&entries, &text, &Usage::default())),
            ["Mail", "Notes"]
        );
        let link = Item::text("Page", " https://example.org/a?b=1 ");
        assert_eq!(
            link,
            Item::Url {
                url: "https://example.org/a?b=1".into(),
                title: "Page".into()
            }
        );
        assert_eq!(
            names(&discover(&entries, &link, &Usage::default())),
            ["Mail", "Notes", "Browser"]
        );
        let nothing = Item::File {
            path: "/a/x.bin".into(),
            mime: "application/x-bin".into(),
        };
        assert_eq!(
            names(&discover(&entries, &nothing, &Usage::default())),
            ["Mail", "Notes"]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn what_was_shared_most_comes_first() {
        let dir = fake_dir("rank");
        let entries = read_entries(std::slice::from_ref(&dir));
        let mut u = Usage::default();
        u.bump("editor.desktop");
        u.bump("editor.desktop");
        u.bump("notes.desktop");
        assert_eq!(
            names(&discover(&entries, &txt(), &u)),
            ["Editor", "Notes", "Mail"]
        );
        let again = Usage::parse(&u.render());
        assert_eq!(again, u);
        assert_eq!(
            Usage::parse("bad\nx\t3\nok.desktop\tz\nfine.desktop\t4\n").get("fine.desktop"),
            4
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn at_most_eight_tiles_and_the_earlier_folder_wins_an_id() {
        let many: Vec<(String, String)> = (0..12)
            .map(|i| {
                (
                    format!("app{i}.desktop"),
                    entry(&format!("App{i:02}"), "app %f", "MimeType=text/plain;"),
                )
            })
            .collect();
        assert_eq!(
            discover(&many, &txt(), &Usage::default()).len(),
            MAX_TARGETS
        );
        let a = std::env::temp_dir().join(format!("wol-share-a-{}", std::process::id()));
        let b = std::env::temp_dir().join(format!("wol-share-b-{}", std::process::id()));
        for d in [&a, &b] {
            let _ = std::fs::remove_dir_all(d);
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(a.join("x.desktop"), "first").unwrap();
        std::fs::write(b.join("x.desktop"), "second").unwrap();
        assert_eq!(
            read_entries(&[a.clone(), b.clone()]),
            [("x.desktop".to_owned(), "first".to_owned())]
        );
        let _ = (std::fs::remove_dir_all(a), std::fs::remove_dir_all(b));
    }

    #[test]
    fn the_usage_file_is_kept_beside_the_state() {
        let d = std::env::temp_dir().join(format!("wol-share-usage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let p = d.join("wolos/share-usage");
        assert_eq!(load_usage(&p), Usage::default());
        record(&p, "a.desktop");
        record(&p, "a.desktop");
        record(&p, "b.desktop");
        let u = load_usage(&p);
        assert_eq!(
            (u.get("a.desktop"), u.get("b.desktop"), u.get("c.desktop")),
            (2, 1, 0)
        );
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn exec_lines_fill_their_field_codes() {
        let one = |s: &str| vec![s.to_owned()];
        let f = one("/a b/c.txt");
        assert_eq!(
            expand_exec("ed %F", "Ed", "", &f, &[]),
            ["ed", "/a b/c.txt"]
        );
        assert_eq!(
            expand_exec("ed %f", "Ed", "", &f, &[]),
            ["ed", "/a b/c.txt"]
        );
        assert_eq!(
            expand_exec("ed %U", "Ed", "", &f, &[]),
            ["ed", "file:///a%20b/c.txt"]
        );
        assert_eq!(
            expand_exec("ed --new %u", "Ed", "", &[], &one("https://x/y")),
            ["ed", "--new", "https://x/y"]
        );
        assert_eq!(
            expand_exec("\"/opt/my app/ed\" --name=%c %i %k", "Ed", "ico", &[], &[]),
            ["/opt/my app/ed", "--name=Ed", "--icon", "ico"]
        );
        // No code that takes the file: it is added at the end.
        assert_eq!(
            expand_exec("ed --gui", "Ed", "", &f, &[]),
            ["ed", "--gui", "/a b/c.txt"]
        );
        assert_eq!(expand_exec("ed 100%%", "Ed", "", &[], &[]), ["ed", "100%"]);
    }

    #[test]
    fn mail_carries_subject_body_and_the_file() {
        assert_eq!(
            mailto(&Item::File {
                path: "/h/a/Report 1.pdf".into(),
                mime: "application/pdf".into()
            }),
            "mailto:?subject=Report%201.pdf&attach=%2Fh%2Fa%2FReport%201.pdf"
        );
        assert_eq!(
            mailto(&Item::Text {
                title: "Idea".into(),
                text: "one\ntwo & three".into()
            }),
            "mailto:?subject=Idea&body=one%0D%0Atwo%20%26%20three"
        );
        assert_eq!(
            mailto(&Item::Url {
                url: "https://e.org/?a=1".into(),
                title: String::new()
            }),
            "mailto:?subject=https%3A%2F%2Fe.org%2F%3Fa%3D1&body=https%3A%2F%2Fe.org%2F%3Fa%3D1"
        );
        let long = Item::Text {
            title: "t".into(),
            text: "é".repeat(MAX_BODY),
        };
        assert!(mailto(&long).len() < MAX_BODY * 6 + 40);
    }

    #[test]
    fn each_role_starts_its_app_its_own_way() {
        let dir = fake_dir("argv");
        let entries = read_entries(std::slice::from_ref(&dir));
        let get = |id: &str, item: &Item| find_all_in(&entries, id, item).unwrap();
        let file = txt();
        let argv = |id: &str, item: &Item| launch_argv(&get(id, item), item, None);
        assert_eq!(
            argv("mail.desktop", &file),
            [
                "mail",
                "mailto:?subject=note.txt&attach=%2Fhome%2Fa%2Fnote.txt"
            ]
        );
        assert_eq!(argv("notes.desktop", &file), ["notes", "/home/a/note.txt"]);
        assert_eq!(
            argv("editor.desktop", &file),
            ["editor", "file:///home/a/note.txt"]
        );
        let text = Item::text("Idea", "a b");
        assert_eq!(
            argv("notes.desktop", &text),
            ["notes", "--share-text", "a b", "--share-title", "Idea"]
        );
        let link = Item::text("", "https://e.org/x");
        assert_eq!(
            argv("notes.desktop", &link),
            ["notes", "--share-url", "https://e.org/x"]
        );
        assert_eq!(
            argv("browser.desktop", &link),
            ["browser", "https://e.org/x"]
        );
        assert!(
            find_all_in(&entries, "viewer.desktop", &file).is_none(),
            "a viewer does not take text"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    fn declared(extra: &str, item: &Item) -> Target {
        let text = entry("Notes", "notes", &format!("X-WolOS-Share={extra}"));
        target_of("n.desktop", &text, item).unwrap()
    }

    #[test]
    fn an_app_that_declared_text_only_never_gets_share_url() {
        let link = Item::text("Page", "https://e.org/x");
        let text_only = declared("text", &link);
        assert_eq!(
            launch_argv(&text_only, &link, None),
            [
                "notes",
                "--share-text",
                "https://e.org/x",
                "--share-title",
                "Page"
            ]
        );
        let url_too = declared("text;url", &link);
        assert_eq!(
            launch_argv(&url_too, &link, None),
            [
                "notes",
                "--share-url",
                "https://e.org/x",
                "--share-title",
                "Page"
            ]
        );
        // An app that declared only file is not a target for a link at all.
        assert!(target_of("n.desktop", &entry("N", "n", "X-WolOS-Share=file"), &link).is_none());
    }

    #[test]
    fn long_text_goes_in_a_file_only_to_an_app_that_asks_for_one() {
        let long = Item::Text {
            title: "t".into(),
            text: "x".repeat(SPILL_AT + 1),
        };
        let short = Item::text("t", "short");
        let asks = declared("text;share-file", &long);
        let plain = declared("text", &long);
        assert!(asks.takes_share_file() && !plain.takes_share_file());
        assert!(spill_text(&asks, &long).is_some());
        assert!(
            spill_text(&asks, &short).is_none(),
            "short text stays in the arguments"
        );
        assert!(
            spill_text(&plain, &long).is_none(),
            "an app that did not ask keeps the arguments"
        );
        let path = Path::new("/run/user/1000/wolos/share-texts/1.txt");
        assert_eq!(
            launch_argv(&asks, &long, Some(path)),
            [
                "notes",
                "--share-file",
                "/run/user/1000/wolos/share-texts/1.txt",
                "--share-title",
                "t"
            ]
        );
    }

    #[test]
    fn the_text_file_is_private_and_old_ones_are_swept() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("wol-share-spill-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("wolos/share-texts");
        std::fs::create_dir_all(&dir).unwrap();
        let old = dir.join("old.txt");
        let f = std::fs::File::create(&old).unwrap();
        f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
            .unwrap();
        let p = write_spill(&dir, "secret words").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "secret words");
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&p), 0o600);
        assert!(!old.exists(), "an hour old file is swept");
        let fresh = base.join("fresh");
        let q = write_spill(&fresh.join("wolos/share-texts"), "x").unwrap();
        assert_eq!(mode(q.parent().unwrap()), 0o700);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_mail_never_carries_a_control_character_in_its_subject_or_body() {
        let file = Item::File {
            path: "/h/a/bad\nname\r.txt".into(),
            mime: "text/plain".into(),
        };
        let m = mailto(&file);
        assert!(
            m.starts_with("mailto:?subject=bad%20name.txt&attach="),
            "{m}"
        );
        let subject = m.split('&').next().unwrap();
        assert!(!subject.contains("%0A") && !subject.contains("%0D"), "{m}");
        let text = Item::Text {
            title: "Hi\x07\nthere".into(),
            text: "one\ttwo\x1b[31m\nthree\r\nfour".into(),
        };
        assert_eq!(
            mailto(&text),
            "mailto:?subject=Hi%20there&body=one%20two%5B31m%0D%0Athree%0D%0Afour"
        );
    }

    #[test]
    fn text_on_standard_input_has_the_size_and_characters_libwol_allows() {
        let max = shared::portal::MAX_MESSAGE;
        assert_eq!(stdin_limit(), u64::try_from(max).unwrap() + 1);
        assert_eq!(
            stdin_text(b"a\r\nb\tc\n".to_vec()),
            Ok("a\nb c\n".to_owned())
        );
        assert!(stdin_text("é".repeat(max / 2).into_bytes()).is_ok());
        assert_eq!(
            stdin_text(vec![b'x'; max + 1]),
            Err(crate::copy::SHARE_TEXT_LONG.to_owned())
        );
        // A multi-byte letter that straddles the limit is refused whole, never cut in two.
        let mut straddle = vec![b'x'; max - 1];
        straddle.extend("é".as_bytes());
        assert_eq!(
            stdin_text(straddle),
            Err(crate::copy::SHARE_TEXT_LONG.to_owned())
        );
        for bad in [vec![0xff, 0xfe], b"a\x00b".to_vec(), b"esc\x1b[0m".to_vec()] {
            assert_eq!(stdin_text(bad), Err(crate::copy::SHARE_TEXT_ODD.to_owned()));
        }
        assert!(crate::copy::lint(crate::copy::SHARE_TEXT_LONG).is_empty());
        assert!(crate::copy::lint(crate::copy::SHARE_TEXT_ODD).is_empty());
    }

    #[test]
    fn the_usage_file_is_bounded_in_what_is_read_and_what_is_kept() {
        let many: String = (0..500).map(|i| format!("app{i}.desktop\t{i}\n")).collect();
        let u = Usage::parse(&many);
        assert!(u.0.len() <= MAX_USAGE);
        assert_eq!(u.get("app499.desktop"), 499, "the most used are kept");
        assert_eq!(u.get("app0.desktop"), 0);
        let d = std::env::temp_dir().join(format!("wol-share-big-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("share-usage");
        let big: String = (0..40_000).map(|i| format!("a{i}.desktop\t1\n")).collect();
        std::fs::write(&p, &big).unwrap();
        assert!(u64::try_from(big.len()).unwrap() > MAX_USAGE_FILE);
        record(&p, "z.desktop");
        let after = std::fs::read_to_string(&p).unwrap();
        assert!(
            after.lines().count() <= MAX_USAGE,
            "trimmed on write: {}",
            after.lines().count()
        );
        let _ = std::fs::remove_dir_all(d);
    }
}
