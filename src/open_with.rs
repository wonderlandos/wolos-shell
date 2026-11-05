// SPDX-License-Identifier: GPL-2.0-or-later
//! Default apps and Open With: the person's choices in `~/.config/mimeapps.list`, the apps that can
//! take a file, and how a `.desktop` `Exec` line starts one with files (field codes) or with one of
//! its own `[Desktop Action]` blocks.
//!
//! Three users: Settings > General > Default Apps ([`MimeApps::set_default`], one journaled write),
//! the Dock (a file dropped on a tile: [`expand_exec`]; the tile menu: [`Handler::jump_list`]) and
//! the Open With chooser ([`remember`]: the app picked last is offered first next time).
//!
//! The file is kept line by line: a change edits or adds its own line and leaves every comment, blank
//! line, other group and key as it was, so what the person or another program wrote survives.
//! Reading the disk is always off the GTK thread (`snapshot`, `handler_for` are `async` over
//! `gio::spawn_blocking`).
use crate::copy::settings as copy;
use crate::envv;
use gtk::gio;
use std::path::{Path, PathBuf};

/// Group of the app that opens a type.
pub const DEFAULTS: &str = "Default Applications";
/// Group of apps offered for a type besides the default (Open With lists them first).
pub const ADDED: &str = "Added Associations";
/// Group of apps that must not be offered for a type.
pub const REMOVED: &str = "Removed Associations";
/// The file, under `$XDG_CONFIG_HOME`.
pub const FILE: &str = "mimeapps.list";
/// What the journal calls a change made on the page.
pub const JOURNAL_WHAT: &str = "default apps";

/// Most app ids kept on one line: the choice and a few earlier ones as a fall-back if it goes.
const KEEP_DEFAULTS: usize = 4;
/// Most app ids kept on an Added Associations line (Open With shows at most a handful).
const KEEP_ADDED: usize = 8;

// ---- the file --------------------------------------------------------------------------------

/// A `mimeapps.list`, line by line. `render(parse(t)) == t` for any text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MimeApps {
    lines: Vec<String>,
    /// The text ended with a line break (kept; a change adds one).
    final_newline: bool,
    /// The text began with a byte order mark (kept; it is not part of the first line).
    bom: bool,
    /// The lines end with CR LF: a line a change writes does too.
    crlf: bool,
}

/// The group a header line opens (`[Name]`), if it is one.
fn header(line: &str) -> Option<&str> {
    let t = line.trim();
    t.strip_prefix('[')?.strip_suffix(']')
}

/// (key, value) of a `key=value` line; None for comments, blanks and headers.
fn key_value(line: &str) -> Option<(&str, &str)> {
    let t = line.trim();
    if t.is_empty() || t.starts_with('#') || t.starts_with('[') {
        return None;
    }
    let (k, v) = t.split_once('=')?;
    Some((k.trim(), v.trim()))
}

/// The ids of a `a.desktop;b.desktop;` value.
fn ids(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

impl MimeApps {
    pub fn parse(text: &str) -> Self {
        let (bom, text) = match text.strip_prefix('\u{FEFF}') {
            Some(t) => (true, t),
            None => (false, text),
        };
        let crlf = text.find('\n').is_some_and(|i| text[..i].ends_with('\r'));
        let final_newline = text.ends_with('\n');
        let body = text.strip_suffix('\n').unwrap_or(text);
        let lines = if text.is_empty() {
            Vec::new()
        } else {
            body.split('\n').map(String::from).collect()
        };
        Self {
            lines,
            final_newline,
            bom,
            crlf,
        }
    }

    /// What a line this type writes ends with, before the line break.
    fn eol(&self) -> &'static str {
        if self.crlf { "\r" } else { "" }
    }

    pub fn render(&self) -> String {
        let mut s = if self.bom {
            "\u{FEFF}".to_string()
        } else {
            String::new()
        };
        s.push_str(&self.lines.join("\n"));
        if self.final_newline && !self.lines.is_empty() {
            s.push('\n');
        }
        s
    }

    /// (header index, end index) of the first `[group]`: its lines are `header+1..end`.
    fn span(&self, group: &str) -> Option<(usize, usize)> {
        let start = self
            .lines
            .iter()
            .position(|l| header(l).is_some_and(|g| g == group))?;
        let end = self.lines[start + 1..]
            .iter()
            .position(|l| header(l).is_some())
            .map_or(self.lines.len(), |n| start + 1 + n);
        Some((start, end))
    }

    /// The ids `group` lists for `mime`, in order (empty when there is no such line). A key that
    /// appears twice counts as its last line, like GKeyFile, which GLib reads the file with.
    pub fn values(&self, group: &str, mime: &str) -> Vec<String> {
        let Some((s, e)) = self.span(group) else {
            return Vec::new();
        };
        self.lines[s + 1..e]
            .iter()
            .rev()
            .find_map(|l| {
                key_value(l)
                    .filter(|(k, _)| *k == mime)
                    .map(|(_, v)| ids(v))
            })
            .unwrap_or_default()
    }

    /// The app this file makes the default for `mime` (first of its list).
    pub fn default_for(&self, mime: &str) -> Option<String> {
        self.values(DEFAULTS, mime).into_iter().next()
    }

    /// Replaces the `mime` line of `group` with `list` (removes it when `list` is empty), adding
    /// the group at the end of the file when it is missing. The last line of a repeated key is the
    /// one that counts ([`MimeApps::values`]), so it is the one replaced and the earlier ones go.
    fn put(&mut self, group: &str, mime: &str, list: &[String]) {
        self.final_newline = true;
        let eol = self.eol();
        let line = format!(
            "{mime}={}{eol}",
            list.iter().map(|i| format!("{i};")).collect::<String>()
        );
        let Some((s, e)) = self.span(group) else {
            if list.is_empty() {
                return;
            }
            if self.lines.last().is_some_and(|l| !l.trim().is_empty()) {
                self.lines.push(eol.to_string());
            }
            self.lines.push(format!("[{group}]{eol}"));
            self.lines.push(line);
            return;
        };
        let hits: Vec<usize> = (s + 1..e)
            .filter(|&i| key_value(&self.lines[i]).is_some_and(|(k, _)| k == mime))
            .collect();
        for &i in hits.iter().rev().skip(1) {
            self.lines.remove(i);
        }
        let dropped = hits.len().saturating_sub(1);
        let at = hits.last().map(|a| a - dropped);
        let end = e - dropped;
        match (at, list.is_empty()) {
            (Some(i), true) => {
                self.lines.remove(i);
            }
            (Some(i), false) => self.lines[i] = line,
            (None, true) => {}
            (None, false) => {
                // After the group's last key line (comments that sit above the next header stay
                // with that header), or straight under the header of a group with no keys.
                let after = (s + 1..end)
                    .rev()
                    .find(|&i| key_value(&self.lines[i]).is_some())
                    .unwrap_or(s);
                self.lines.insert(after + 1, line);
            }
        }
    }

    fn put_first(&mut self, group: &str, mime: &str, id: &str, keep: usize) {
        let mut list = vec![id.to_string()];
        list.extend(self.values(group, mime).into_iter().filter(|i| i != id));
        list.truncate(keep);
        self.put(group, mime, &list);
    }

    /// `id` opens `mime` from now on: first of the Default Applications line (earlier choices
    /// stay behind it as fall-backs), offered first by Open With, and no longer removed.
    pub fn set_default(&mut self, mime: &str, id: &str) {
        self.put_first(DEFAULTS, mime, id, KEEP_DEFAULTS);
        self.put_first(ADDED, mime, id, KEEP_ADDED);
        let removed: Vec<String> = self
            .values(REMOVED, mime)
            .into_iter()
            .filter(|i| i != id)
            .collect();
        if removed.len() != self.values(REMOVED, mime).len() {
            self.put(REMOVED, mime, &removed);
        }
    }

    /// Open With picked `id` for `mime`: it comes first in the next chooser. The default stays.
    pub fn prefer(&mut self, mime: &str, id: &str) {
        self.put_first(ADDED, mime, id, KEEP_ADDED);
    }
}

/// The app that opens `mime`, by the specification: `files` in reading order, a Default
/// Applications entry that is installed and not removed (by its file or a higher one) wins;
/// with none, the first Added Association that is installed and not removed. A lower file cannot
/// bring back an app a higher one removed.
pub fn effective_default(
    mime: &str,
    files: &[MimeApps],
    installed: &dyn Fn(&str) -> bool,
) -> Option<String> {
    let mut removed: Vec<Vec<String>> = Vec::new();
    let mut gone: Vec<String> = Vec::new();
    for f in files {
        gone.extend(f.values(REMOVED, mime));
        removed.push(gone.clone());
    }
    for group in [DEFAULTS, ADDED] {
        for (f, gone) in files.iter().zip(&removed) {
            if let Some(id) = f
                .values(group, mime)
                .into_iter()
                .find(|id| installed(id) && !gone.contains(id))
            {
                return Some(id);
            }
        }
    }
    None
}

/// The files `mimeapps.list` is read from, in the order of the specification: in the person's
/// config, the system config, the person's data and the system data, each first as
/// `<desktop>-mimeapps.list` for every desktop of `XDG_CURRENT_DESKTOP` (lowercase), then plain.
pub fn mimeapps_paths(
    config_home: &Path,
    config_dirs: &[PathBuf],
    data_home: &Path,
    data_dirs: &[PathBuf],
    desktops: &[String],
) -> Vec<PathBuf> {
    let dirs: Vec<PathBuf> = std::iter::once(config_home.to_path_buf())
        .chain(config_dirs.iter().cloned())
        .chain(std::iter::once(data_home.join("applications")))
        .chain(data_dirs.iter().map(|d| d.join("applications")))
        .collect();
    dirs.iter()
        .flat_map(|d| {
            desktops
                .iter()
                .map(|n| d.join(format!("{}-{FILE}", n.to_lowercase())))
                .chain(std::iter::once(d.join(FILE)))
        })
        .collect()
}

// ---- the kinds of thing the page offers --------------------------------------------------------

/// One row of Default Apps: a kind of thing and every type that belongs to it.
pub struct Category {
    pub title: &'static str,
    pub subtitle: &'static str,
    /// The first is the one apps are listed by; the rest are set together with it.
    pub mimes: &'static [&'static str],
}

pub static CATEGORIES: [Category; 9] = [
    Category {
        title: copy::DEFAULT_APPS_WEB,
        subtitle: copy::DEFAULT_APPS_WEB_SUB,
        mimes: &[
            "x-scheme-handler/http",
            "x-scheme-handler/https",
            "text/html",
            "application/xhtml+xml",
        ],
    },
    Category {
        title: copy::DEFAULT_APPS_MAIL,
        subtitle: copy::DEFAULT_APPS_MAIL_SUB,
        mimes: &["x-scheme-handler/mailto"],
    },
    Category {
        title: copy::DEFAULT_APPS_FILES,
        subtitle: copy::DEFAULT_APPS_FILES_SUB,
        mimes: &["inode/directory"],
    },
    Category {
        title: copy::DEFAULT_APPS_IMAGES,
        subtitle: copy::DEFAULT_APPS_IMAGES_SUB,
        mimes: &[
            "image/png",
            "image/jpeg",
            "image/gif",
            "image/webp",
            "image/svg+xml",
        ],
    },
    Category {
        title: copy::DEFAULT_APPS_MUSIC,
        subtitle: copy::DEFAULT_APPS_MUSIC_SUB,
        mimes: &["audio/mpeg", "audio/flac", "audio/ogg", "audio/x-wav"],
    },
    Category {
        title: copy::DEFAULT_APPS_VIDEO,
        subtitle: copy::DEFAULT_APPS_VIDEO_SUB,
        mimes: &["video/mp4", "video/webm", "video/x-matroska"],
    },
    Category {
        title: copy::DEFAULT_APPS_PDF,
        subtitle: copy::DEFAULT_APPS_PDF_SUB,
        mimes: &["application/pdf"],
    },
    Category {
        title: copy::DEFAULT_APPS_TEXT,
        subtitle: copy::DEFAULT_APPS_TEXT_SUB,
        mimes: &["text/plain"],
    },
    Category {
        title: copy::DEFAULT_APPS_CALENDAR,
        subtitle: copy::DEFAULT_APPS_CALENDAR_SUB,
        mimes: &["text/calendar"],
    },
];

impl Category {
    /// The type that decides which apps the row offers.
    pub fn primary(&self) -> &'static str {
        self.mimes[0]
    }

    /// The types to set when `app` is chosen: every one of the category the app says it opens,
    /// and always the first (a browser that lists only `x-scheme-handler/http` still gets it).
    pub fn mimes_for(&self, app: &Handler) -> Vec<&'static str> {
        self.mimes
            .iter()
            .enumerate()
            .filter(|(i, m)| *i == 0 || app.mimes.iter().any(|x| x == **m))
            .map(|(_, m)| *m)
            .collect()
    }
}

/// The text of `mimeapps` after the person chose `app` for `cat`.
pub fn choose(mimeapps: &str, cat: &Category, app: &Handler) -> String {
    let mut f = MimeApps::parse(mimeapps);
    for m in cat.mimes_for(app) {
        f.set_default(m, &app.id);
    }
    f.render()
}

// ---- desktop entries --------------------------------------------------------------------------

/// One `[Desktop Action]` the app lists in `Actions=` (New Window, New Private Window).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    pub name: String,
    pub exec: String,
}

/// An installed app as Open With and the Dock need it. Unlike `desktop::Entry`, the `Exec` line
/// keeps its field codes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Handler {
    /// The file name, `org.gnome.Nautilus.desktop`.
    pub id: String,
    pub name: String,
    pub icon: String,
    pub mimes: Vec<String>,
    pub exec: String,
    /// Where the file was read (the `%k` code).
    pub path: String,
    pub actions: Vec<Action>,
}

/// The groups of a `.desktop` text: (name, lines).
fn groups(text: &str) -> Vec<(&str, Vec<&str>)> {
    let mut out: Vec<(&str, Vec<&str>)> = Vec::new();
    for line in text.lines() {
        if let Some(g) = header(line) {
            out.push((g, Vec::new()));
        } else if let Some((_, lines)) = out.last_mut() {
            lines.push(line);
        }
    }
    out
}

/// First non-empty value of `key` in `lines`.
fn value<'a>(lines: &[&'a str], key: &str) -> Option<&'a str> {
    lines
        .iter()
        .filter_map(|l| key_value(l))
        .find(|(k, v)| *k == key && !v.is_empty())
        .map(|(_, v)| v)
}

impl Handler {
    /// None for a file the app lists leave out: not an app, no `Exec`, hidden (`NoDisplay`), for
    /// another desktop or with its program missing (the one reader of `desktop::parse`).
    pub fn parse(id: &str, path: &str, text: &str) -> Option<Self> {
        crate::desktop::parse(id, text)?;
        let g = groups(text);
        let main = &g.iter().find(|(n, _)| *n == "Desktop Entry")?.1;
        let exec = value(main, "Exec")?.to_string();
        let actions = value(main, "Actions")
            .map(ids)
            .unwrap_or_default()
            .iter()
            .filter_map(|a| {
                let lines = &g
                    .iter()
                    .find(|(n, _)| *n == format!("Desktop Action {a}"))?
                    .1;
                Some(Action {
                    name: value(lines, "Name")?.to_string(),
                    exec: value(lines, "Exec")?.to_string(),
                })
            })
            .collect();
        Some(Self {
            id: id.to_string(),
            name: value(main, "Name").unwrap_or(id).to_string(),
            icon: value(main, "Icon").unwrap_or_default().to_string(),
            mimes: value(main, "MimeType").map(ids).unwrap_or_default(),
            exec,
            path: path.to_string(),
            actions,
        })
    }

    /// Whether a file dropped on the app's Dock tile means something to it.
    pub fn takes_files(&self) -> bool {
        takes_files(&self.exec)
    }

    /// Whether `files` mean something to the app: its `Exec` takes files and at least one of them
    /// is usable for its codes (`%f`/`%F` need a path on this computer, `%u`/`%U` any address).
    pub fn usable(&self, files: &[Target]) -> bool {
        let cs: Vec<char> = split_exec(&self.exec)
            .iter()
            .flat_map(|w| codes(w))
            .collect();
        let by_address = cs.iter().any(|c| *c == 'u' || *c == 'U');
        let by_path = cs.iter().any(|c| *c == 'f' || *c == 'F');
        files
            .iter()
            .any(|t| (by_address && !t.uri.is_empty()) || (by_path && t.path.is_some()))
    }

    /// The menu of the app's Dock tile: each action's name and the programs that start it.
    pub fn jump_list(&self) -> Vec<(String, Vec<String>)> {
        self.actions
            .iter()
            .filter_map(|a| {
                let argv = expand_exec(&a.exec, self, &[]).into_iter().next()?;
                Some((a.name.clone(), argv))
            })
            .collect()
    }

    /// The programs that start the app with `files`: one for `%F`/`%U`, one per file for `%f`/`%u`.
    pub fn launch(&self, files: &[Target]) -> Vec<Vec<String>> {
        expand_exec(&self.exec, self, files)
    }
}

/// A file that is dropped or opened: where it is on this computer (if it is) and its address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub path: Option<String>,
    pub uri: String,
}

impl Target {
    pub fn local(path: &str) -> Self {
        Self {
            path: Some(path.to_string()),
            uri: file_uri(path),
        }
    }
}

/// `file:///a%20b` for `/a b`.
pub fn file_uri(path: &str) -> String {
    let mut s = String::from("file://");
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

/// The words of an `Exec` value: spaces separate, a double-quoted part is one word, and inside
/// quotes a backslash escapes `"`, `` ` ``, `$` and `\`.
pub fn split_exec(exec: &str) -> Vec<String> {
    let (mut out, mut cur) = (Vec::new(), String::new());
    let (mut quoted, mut any) = (false, false);
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            '\\' if quoted && chars.peek().is_some_and(|n| "\"`$\\".contains(*n)) => {
                cur.extend(chars.next());
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The field codes of one word: each `%x` that is not `%%`.
fn codes(word: &str) -> Vec<char> {
    let mut v = Vec::new();
    let mut it = word.chars();
    while let Some(c) = it.next() {
        if c == '%' {
            match it.next() {
                Some('%') | None => {}
                Some(x) => v.push(x),
            }
        }
    }
    v
}

/// The `Exec` line has a code that takes a file or an address (`%f %F %u %U`).
pub fn takes_files(exec: &str) -> bool {
    split_exec(exec)
        .iter()
        .flat_map(|w| codes(w))
        .any(|c| "fFuU".contains(c))
}

/// The programs that start `app` from `exec` with `files`, field codes expanded:
/// `%F`/`%U` take every file in one start; `%f`/`%u` start the program once per file (the
/// specification's rule for a program that opens one); with no file the code vanishes. `%i` is
/// `--icon <Icon>`, `%c` the name, `%k` the file, `%%` a percent sign, the removed codes nothing.
/// A path-less file (not on this computer) is passed to `%u`/`%U` only.
pub fn expand_exec(exec: &str, app: &Handler, files: &[Target]) -> Vec<Vec<String>> {
    let words = split_exec(exec);
    let multi = words
        .iter()
        .any(|w| codes(w).iter().any(|c| *c == 'F' || *c == 'U'));
    let single = words
        .iter()
        .any(|w| codes(w).iter().any(|c| *c == 'f' || *c == 'u'));
    let one = |chosen: &[&Target]| -> Vec<String> {
        let mut argv = Vec::new();
        for w in &words {
            let cs = codes(w);
            // A word that is just one code: it may turn into several words or none.
            if w.len() == 2 && w.starts_with('%') && !w.starts_with("%%") {
                match cs.first() {
                    Some('F') => argv.extend(chosen.iter().filter_map(|t| t.path.clone())),
                    Some('U') => argv.extend(chosen.iter().map(|t| t.uri.clone())),
                    Some('f') => argv.extend(chosen.first().and_then(|t| t.path.clone())),
                    Some('u') => argv.extend(chosen.first().map(|t| t.uri.clone())),
                    Some('i') => {
                        if !app.icon.is_empty() {
                            argv.extend(["--icon".to_string(), app.icon.clone()]);
                        }
                    }
                    Some('c') => argv.push(app.name.clone()),
                    Some('k') => argv.push(app.path.clone()),
                    _ => {}
                }
                continue;
            }
            // A code inside a word (`--file=%f`): the word goes when a file code has no file.
            let has_file_code = cs.iter().any(|c| "fFuU".contains(*c));
            let first = chosen.first();
            let mut out = String::new();
            let mut it = w.chars();
            let mut missing = false;
            while let Some(c) = it.next() {
                if c != '%' {
                    out.push(c);
                    continue;
                }
                match it.next() {
                    Some('%') => out.push('%'),
                    Some('f') => match first.and_then(|t| t.path.as_ref()) {
                        Some(p) => out.push_str(p),
                        None => missing = true,
                    },
                    Some('F') => {
                        let p: Vec<_> = chosen.iter().filter_map(|t| t.path.clone()).collect();
                        missing |= p.is_empty();
                        out.push_str(&p.join(" "));
                    }
                    Some('u') => match first {
                        Some(t) => out.push_str(&t.uri),
                        None => missing = true,
                    },
                    Some('U') => {
                        missing |= chosen.is_empty();
                        out.push_str(
                            &chosen
                                .iter()
                                .map(|t| t.uri.as_str())
                                .collect::<Vec<_>>()
                                .join(" "),
                        );
                    }
                    Some('c') => out.push_str(&app.name),
                    Some('k') => out.push_str(&app.path),
                    Some(_) | None => {}
                }
            }
            if !(has_file_code && missing) {
                argv.push(out);
            }
        }
        argv
    };
    let refs: Vec<&Target> = files.iter().collect();
    let runs: Vec<Vec<String>> = if multi {
        vec![one(&refs)]
    } else if single && !files.is_empty() {
        let by_address = words.iter().any(|w| codes(w).contains(&'u'));
        refs.iter()
            .filter(|t| by_address || t.path.is_some())
            .map(|t| one(&[*t]))
            .collect()
    } else {
        vec![one(&[])]
    };
    runs.into_iter().filter(|a| !a.is_empty()).collect()
}

// ---- reading the disk (off the GTK thread) -------------------------------------------------------

/// Where `.desktop` files live, first wins: the person's, then the system's.
fn app_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(h) = envv::var("XDG_DATA_HOME") {
        v.push(PathBuf::from(h));
    } else if let Some(h) = envv::var("HOME") {
        v.push(Path::new(&h).join(".local/share"));
    }
    let sys = envv::var("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    v.extend(sys.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    v
}

/// The `mimeapps.list` files that apply to this session, as [`mimeapps_paths`] orders them.
fn chain() -> Vec<PathBuf> {
    let var = |k: &str, d: &str| envv::var(k).unwrap_or_else(|| d.to_string());
    let dirs = |s: String| -> Vec<PathBuf> {
        s.split(':')
            .filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .collect()
    };
    let data = app_dirs();
    let (data_home, data_dirs) = data
        .split_first()
        .map_or((PathBuf::new(), Vec::new()), |(h, r)| {
            (h.clone(), r.to_vec())
        });
    mimeapps_paths(
        Path::new(&crate::kit::config_home()),
        &dirs(var("XDG_CONFIG_DIRS", "/etc/xdg")),
        &data_home,
        &data_dirs,
        &desktops(),
    )
}

/// The desktops of `XDG_CURRENT_DESKTOP` (`Hyprland`, or `GNOME:Unity`), in order.
fn desktops() -> Vec<String> {
    envv::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|d| !d.is_empty())
        .map(String::from)
        .collect()
}

/// Everything Default Apps shows: the apps and the `mimeapps.list` files, the person's first.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub apps: Vec<Handler>,
    pub files: Vec<MimeApps>,
    /// The person's file as text (what a change is applied to).
    pub user_text: String,
}

impl Snapshot {
    pub fn installed(&self, id: &str) -> bool {
        self.apps.iter().any(|a| a.id == id)
    }

    /// The apps that can open the category's first type, by name.
    pub fn candidates(&self, cat: &Category) -> Vec<&Handler> {
        let mut v: Vec<&Handler> = self
            .apps
            .iter()
            .filter(|a| a.mimes.iter().any(|m| m == cat.primary()))
            .collect();
        v.sort_by_key(|a| a.name.to_lowercase());
        v
    }

    /// The app that opens the category now, by id.
    pub fn current(&self, cat: &Category) -> Option<String> {
        effective_default(cat.primary(), &self.files, &|id| self.installed(id))
    }
}

/// Every `.desktop` in `dirs` (`<dir>/applications`), first id wins.
fn read_apps(dirs: &[PathBuf]) -> Vec<Handler> {
    let mut seen = std::collections::HashSet::new();
    let mut files = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d.join("applications")) else {
            continue;
        };
        for f in rd.flatten() {
            let id = f.file_name().to_string_lossy().into_owned();
            if id.ends_with(".desktop") && seen.insert(id.clone()) {
                files.push((id, f.path()));
            }
        }
    }
    crate::desktop::par_map(&files, |(id, p)| {
        let text = std::fs::read_to_string(p).ok()?;
        Handler::parse(id, &p.to_string_lossy(), &text)
    })
    .into_iter()
    .flatten()
    .collect()
}

fn read_snapshot(apps: Vec<Handler>) -> Snapshot {
    let texts: Vec<String> = chain()
        .iter()
        .map(|p| std::fs::read_to_string(p).unwrap_or_default())
        .collect();
    Snapshot {
        apps,
        user_text: read_user_text(),
        files: texts.iter().map(|t| MimeApps::parse(t)).collect(),
    }
}

/// The apps and files Default Apps shows. Reads off the GTK thread.
pub async fn snapshot() -> Snapshot {
    gio::spawn_blocking(|| read_snapshot(read_apps(&app_dirs())))
        .await
        .unwrap_or_default()
}

/// The person's `mimeapps.list` as it is now (empty when there is none). Reads off the GTK thread.
pub async fn user_text() -> String {
    gio::spawn_blocking(read_user_text)
        .await
        .unwrap_or_default()
}

fn read_user_text() -> String {
    std::fs::read_to_string(Path::new(&crate::kit::config_home()).join(FILE)).unwrap_or_default()
}

/// The names of the files a pin's desktop entry may have: its app id, then its program.
pub fn ids_for_pin(id: &str, program: &str) -> Vec<String> {
    let prog = program.rsplit('/').next().unwrap_or(program);
    let mut v = vec![format!("{id}.desktop")];
    let p = format!("{prog}.desktop");
    if !prog.is_empty() && !v.contains(&p) {
        v.push(p);
    }
    v
}

fn read_handler(wanted: &[String]) -> Option<Handler> {
    let dirs = app_dirs();
    wanted.iter().find_map(|id| {
        dirs.iter().find_map(|d| {
            let p = d.join("applications").join(id);
            let text = std::fs::read_to_string(&p).ok()?;
            Handler::parse(id, &p.to_string_lossy(), &text)
        })
    })
}

/// The desktop entry of a Dock pin. Reads off the GTK thread.
pub async fn handler_for(id: &str, program: &str) -> Option<Handler> {
    let wanted = ids_for_pin(id, program);
    gio::spawn_blocking(move || read_handler(&wanted))
        .await
        .ok()
        .flatten()
}

// ---- Open With remembers ------------------------------------------------------------------------

/// Writes `text` over `path` with the shared atomic writer (a new file beside it, then a rename),
/// creating the folder.
fn write_over(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    shared::atomic::atomic_write(path, 0o644, text.as_bytes())
}

/// The apps the person's file offers first for the kind of `path` (its Added Associations, in
/// order). Reads off the GTK thread.
pub async fn added_for(path: &Path) -> Vec<String> {
    let (mime, _) = gio::content_type_guess(Some(path), None);
    let mime = mime.to_string();
    gio::spawn_blocking(move || added_in(&read_user_text(), &mime))
        .await
        .unwrap_or_default()
}

/// The Added Associations of `mime` in `mimeapps`.
pub fn added_in(mimeapps: &str, mime: &str) -> Vec<String> {
    MimeApps::parse(mimeapps).values(ADDED, mime)
}

/// `items` with those whose id is in `first` moved to the front, in the order of `first`; the
/// rest keep their order (the chooser's recommended apps stay as they were).
pub fn promote<T>(items: Vec<T>, id_of: impl Fn(&T) -> String, first: &[String]) -> Vec<T> {
    let mut items = items;
    items.sort_by_key(|t| {
        let id = id_of(t);
        first.iter().position(|f| *f == id).unwrap_or(usize::MAX)
    });
    items
}

/// `mimeapps` after Open With picked `app_id` for `mime`; None when nothing changes.
pub fn remembered(mimeapps: &str, mime: &str, app_id: &str) -> Option<String> {
    let mut f = MimeApps::parse(mimeapps);
    f.prefer(mime, app_id);
    let out = f.render();
    (out != mimeapps).then_some(out)
}

/// Open With picked `app_id` for `path`: the app comes first in the next chooser for files of that
/// kind (`[Added Associations]`; the default app is not touched). Quiet and best effort.
pub fn remember(path: &Path, app_id: &str) {
    let (mime, _) = gio::content_type_guess(Some(path), None);
    let (mime, app_id) = (mime.to_string(), app_id.to_string());
    let _ = std::thread::Builder::new()
        .name("wol-open-with".into())
        .spawn(move || {
            let file = Path::new(&crate::kit::config_home()).join(FILE);
            let now = std::fs::read_to_string(&file).unwrap_or_default();
            if let Some(next) = remembered(&now, &mime, &app_id) {
                let _ = write_over(&file, &next);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(exec: &str) -> Handler {
        Handler {
            id: "a.desktop".into(),
            name: "App".into(),
            icon: "app-icon".into(),
            exec: exec.into(),
            path: "/usr/share/applications/a.desktop".into(),
            ..Handler::default()
        }
    }

    fn argv(exec: &str, files: &[&str]) -> Vec<Vec<String>> {
        let t: Vec<Target> = files.iter().map(|f| Target::local(f)).collect();
        expand_exec(exec, &app(exec), &t)
    }

    fn strs(v: &[&[&str]]) -> Vec<Vec<String>> {
        v.iter()
            .map(|a| a.iter().map(|s| s.to_string()).collect())
            .collect()
    }

    // ---- field codes

    /// `%F` and `%U` take every dropped file in one start; `%f` and `%u` start once per file.
    #[test]
    fn field_codes_make_one_start_or_one_per_file() {
        let two = ["/h/a b.png", "/h/c.png"];
        assert_eq!(
            argv("viewer %F", &two),
            strs(&[&["viewer", "/h/a b.png", "/h/c.png"]])
        );
        assert_eq!(
            argv("viewer %U", &two),
            strs(&[&["viewer", "file:///h/a%20b.png", "file:///h/c.png"]])
        );
        assert_eq!(
            argv("viewer %f", &two),
            strs(&[&["viewer", "/h/a b.png"], &["viewer", "/h/c.png"]])
        );
        assert_eq!(
            argv("viewer %u", &two),
            strs(&[
                &["viewer", "file:///h/a%20b.png"],
                &["viewer", "file:///h/c.png"]
            ])
        );
        // One file: the same either way.
        assert_eq!(argv("viewer %f", &["/h/x"]), argv("viewer %F", &["/h/x"]));
    }

    /// With no file the file codes vanish (a click on the tile), and an Exec without one ignores files.
    #[test]
    fn no_file_no_code_and_no_code_no_file() {
        assert_eq!(argv("viewer %U", &[]), strs(&[&["viewer"]]));
        assert_eq!(argv("viewer --open=%f", &[]), strs(&[&["viewer"]]));
        assert_eq!(argv("viewer", &["/h/x"]), strs(&[&["viewer"]]));
        assert!(!takes_files("viewer"));
        assert!(!takes_files("viewer 100%% %c"));
        for c in ["%f", "%F", "%u", "%U"] {
            assert!(takes_files(&format!("viewer {c}")), "{c}");
        }
    }

    #[test]
    fn the_other_codes_and_quotes() {
        assert_eq!(
            argv(r#""/opt/My App/run" %i %c --k=%k 100%% %F"#, &["/h/x"]),
            strs(&[&[
                "/opt/My App/run",
                "--icon",
                "app-icon",
                "App",
                "--k=/usr/share/applications/a.desktop",
                "100%",
                "/h/x"
            ]])
        );
        // A removed code is nothing; a code inside a word is replaced in place.
        assert_eq!(
            argv("v %d %n --file=%f", &["/h/x"]),
            strs(&[&["v", "--file=/h/x"]])
        );
        assert_eq!(
            split_exec(r#"sh -c "echo \"hi\" \$HOME" tail"#),
            ["sh", "-c", r#"echo "hi" $HOME"#, "tail"]
        );
        assert_eq!(split_exec(r#"a "" b"#), ["a", "", "b"]);
    }

    /// A file that is not on this computer has an address but no path: `%u`/`%U` take it, `%f`/`%F`
    /// cannot, so the drop is refused (`Handler::usable`) and an empty drop is too.
    #[test]
    fn a_remote_file_has_an_address_only() {
        let t = [Target {
            path: None,
            uri: "smb://nas/pic.png".into(),
        }];
        assert_eq!(
            expand_exec("v %U", &app("v %U"), &t),
            strs(&[&["v", "smb://nas/pic.png"]])
        );
        assert!(app("v %U").usable(&t) && app("v %u").usable(&t));
        for exec in ["v %F", "v %f"] {
            assert!(!app(exec).usable(&t), "{exec}: no path, nothing to pass");
        }
        // %f with a remote and a local file: one start, for the local one.
        let mixed = [t[0].clone(), Target::local("/h/a.png")];
        assert_eq!(
            expand_exec("v %f", &app("v %f"), &mixed),
            strs(&[&["v", "/h/a.png"]])
        );
        for exec in ["v %U", "v %F", "v %f", "v"] {
            assert!(!app(exec).usable(&[]), "{exec}: an empty drop is refused");
        }
        assert!(!app("v").usable(&[Target::local("/h/x")]), "no file code");
    }

    // ---- desktop entries

    const BROWSER: &str = "# comment\n[Desktop Entry]\nType=Application\nName=Atlas\nExec=atlas %U\n\
        Icon=atlas\nMimeType=x-scheme-handler/http;x-scheme-handler/https;text/html;\n\
        Actions=new-window;private;gone;\n\n\
        [Desktop Action private]\nName=New Private Window\nExec=atlas --private\n\
        [Desktop Action new-window]\nName=New Window\nExec=atlas --new-window\n\
        [Desktop Action unlisted]\nName=Not listed\nExec=atlas --x\n";

    /// The tile menu: the names in `Actions=` order; an action with no block or no Exec is left out.
    #[test]
    fn the_jump_list_is_the_listed_actions_in_order() {
        let h = Handler::parse("atlas.desktop", "/p/atlas.desktop", BROWSER).unwrap();
        assert_eq!(
            h.jump_list(),
            vec![
                (
                    "New Window".to_string(),
                    vec!["atlas".into(), "--new-window".into()]
                ),
                (
                    "New Private Window".to_string(),
                    vec!["atlas".into(), "--private".into()]
                ),
            ]
        );
        assert!(h.takes_files());
        assert_eq!(h.mimes.len(), 3);
        assert_eq!(
            h.launch(&[Target::local("/h/a.html")]),
            strs(&[&["atlas", "file:///h/a.html"]])
        );
        // No Actions= key: no menu.
        let plain = "[Desktop Entry]\nType=Application\nName=P\nExec=p\n[Desktop Action x]\nName=X\nExec=p x\n";
        assert!(
            Handler::parse("p.desktop", "/p", plain)
                .unwrap()
                .jump_list()
                .is_empty()
        );
        assert!(
            Handler::parse(
                "l.desktop",
                "/p",
                "[Desktop Entry]\nType=Link\nName=L\nExec=l\n"
            )
            .is_none()
        );
    }

    /// What the app lists leave out, Open With and Default Apps leave out too.
    #[test]
    fn hidden_entries_are_not_handlers() {
        let e =
            |extra: &str| format!("[Desktop Entry]\nType=Application\nName=H\nExec=h %U\n{extra}");
        assert!(Handler::parse("h.desktop", "/p", &e("")).is_some());
        assert!(Handler::parse("h.desktop", "/p", &e("NoDisplay=true\n")).is_none());
        assert!(Handler::parse("h.desktop", "/p", &e("OnlyShowIn=GNOME;\n")).is_none());
        assert!(Handler::parse("h.desktop", "/p", "[Desktop Entry]\nName=H\nExec=h\n").is_none());
    }

    #[test]
    fn a_pin_finds_its_entry_by_app_id_then_program() {
        assert_eq!(
            ids_for_pin("org.gnome.Nautilus", "nautilus"),
            ["org.gnome.Nautilus.desktop", "nautilus.desktop"]
        );
        assert_eq!(ids_for_pin("helium", "/usr/bin/helium"), ["helium.desktop"]);
    }

    // ---- the file

    const USER: &str = "# my choices\n[Default Applications]\nimage/png=old.desktop;\n\
        # keep me\ntext/plain = editor.desktop;\n\n[Added Associations]\nimage/png=old.desktop;\n\n\
        [Other]\nx=y\n";

    #[test]
    fn choosing_edits_only_its_lines() {
        let mut f = MimeApps::parse(USER);
        f.set_default("image/png", "new.desktop");
        let out = f.render();
        assert_eq!(
            out,
            "# my choices\n[Default Applications]\nimage/png=new.desktop;old.desktop;\n\
             # keep me\ntext/plain = editor.desktop;\n\n[Added Associations]\n\
             image/png=new.desktop;old.desktop;\n\n[Other]\nx=y\n"
        );
        assert_eq!(f.default_for("image/png").as_deref(), Some("new.desktop"));
        // A type with no line: added under the last key of its group, before the comment gap.
        f.set_default("image/gif", "new.desktop");
        let out = f.render();
        assert!(
            out.contains("text/plain = editor.desktop;\nimage/gif=new.desktop;\n\n[Added"),
            "{out}"
        );
        // Choosing the same app again changes nothing.
        let again = {
            let mut g = MimeApps::parse(&out);
            g.set_default("image/gif", "new.desktop");
            g.render()
        };
        assert_eq!(again, out);
    }

    #[test]
    fn a_missing_group_or_file_is_made() {
        let mut f = MimeApps::default();
        f.set_default("text/plain", "e.desktop");
        assert_eq!(
            f.render(),
            "[Default Applications]\ntext/plain=e.desktop;\n\n[Added Associations]\ntext/plain=e.desktop;\n"
        );
        let mut g = MimeApps::parse("# only a comment");
        g.set_default("a/b", "x.desktop");
        assert!(
            g.render()
                .starts_with("# only a comment\n\n[Default Applications]\n")
        );
        // No trailing break: the one we write ends with one.
        assert!(g.render().ends_with('\n'));
    }

    #[test]
    fn a_removed_association_is_lifted_and_duplicates_collapse() {
        let t = "[Default Applications]\na/b=x.desktop;\na/b=y.desktop;\n[Removed Associations]\na/b=n.desktop;z.desktop;\n";
        let mut f = MimeApps::parse(t);
        f.set_default("a/b", "n.desktop");
        // The repeated key counts as its last line (y), like GKeyFile.
        assert_eq!(MimeApps::parse(t).values(DEFAULTS, "a/b"), ["y.desktop"]);
        assert_eq!(f.values(DEFAULTS, "a/b"), ["n.desktop", "y.desktop"]);
        assert_eq!(f.values(REMOVED, "a/b"), ["z.desktop"]);
        assert_eq!(
            f.render().matches("a/b=").count(),
            3,
            "one default line (the duplicate went), one added, one removed: {}",
            f.render()
        );
    }

    #[test]
    fn open_with_prefers_without_touching_the_default() {
        let t = "[Default Applications]\na/b=d.desktop;\n";
        let out = remembered(t, "a/b", "p.desktop").unwrap();
        let f = MimeApps::parse(&out);
        assert_eq!(f.default_for("a/b").as_deref(), Some("d.desktop"));
        assert_eq!(f.values(ADDED, "a/b"), ["p.desktop"]);
        assert_eq!(remembered(&out, "a/b", "p.desktop"), None, "already first");
    }

    /// The person's file wins; an app that is not installed is skipped for the next one.
    #[test]
    fn the_current_app_follows_the_file_order() {
        let user =
            MimeApps::parse("[Default Applications]\nimage/png=gone.desktop;mine.desktop;\n");
        let system = MimeApps::parse(
            "[Default Applications]\nimage/png=sys.desktop;\ntext/plain=sys.desktop;\n",
        );
        let have = |id: &str| id == "mine.desktop" || id == "sys.desktop";
        let files = [user, system];
        assert_eq!(
            effective_default("image/png", &files, &have).as_deref(),
            Some("mine.desktop")
        );
        assert_eq!(
            effective_default("text/plain", &files, &have).as_deref(),
            Some("sys.desktop")
        );
        assert_eq!(effective_default("video/mp4", &files, &have), None);
    }

    #[test]
    fn the_files_are_read_in_the_specified_order() {
        let paths = |desktops: &[&str]| {
            let d: Vec<String> = desktops.iter().map(|s| s.to_string()).collect();
            mimeapps_paths(
                Path::new("/h/.config"),
                &[PathBuf::from("/etc/xdg")],
                Path::new("/h/.local/share"),
                &[PathBuf::from("/usr/share")],
                &d,
            )
        };
        assert_eq!(
            paths(&[]),
            [
                "/h/.config/mimeapps.list",
                "/etc/xdg/mimeapps.list",
                "/h/.local/share/applications/mimeapps.list",
                "/usr/share/applications/mimeapps.list"
            ]
            .map(PathBuf::from)
        );
        // XDG_CURRENT_DESKTOP=Hyprland: the desktop's own file comes first in every directory.
        assert_eq!(
            paths(&["Hyprland"]),
            [
                "/h/.config/hyprland-mimeapps.list",
                "/h/.config/mimeapps.list",
                "/etc/xdg/hyprland-mimeapps.list",
                "/etc/xdg/mimeapps.list",
                "/h/.local/share/applications/hyprland-mimeapps.list",
                "/h/.local/share/applications/mimeapps.list",
                "/usr/share/applications/hyprland-mimeapps.list",
                "/usr/share/applications/mimeapps.list"
            ]
            .map(PathBuf::from)
        );
        assert_eq!(
            paths(&["GNOME", "Unity"]).len(),
            12,
            "two desktops: two names each"
        );
    }

    /// The specification's lookup: Default Applications first, then Added Associations; an app a
    /// file removes is not brought back by a lower file; a repeated key is its last line.
    #[test]
    fn the_current_app_honours_added_and_removed_associations() {
        let f = |t: &str| MimeApps::parse(t);
        let have = |id: &str| ["a.desktop", "b.desktop", "c.desktop"].contains(&id);
        // Added is consulted after every Default Applications entry.
        let files = [
            f("[Added Associations]\nx/y=a.desktop;\n"),
            f("[Default Applications]\nx/y=b.desktop;\n"),
        ];
        assert_eq!(
            effective_default("x/y", &files, &have).as_deref(),
            Some("b.desktop")
        );
        // With no default anywhere, the first Added Association that is installed.
        let added = [f(
            "[Added Associations]\nx/y=gone.desktop;c.desktop;a.desktop;\n",
        )];
        assert_eq!(
            effective_default("x/y", &added, &have).as_deref(),
            Some("c.desktop")
        );
        // A higher file removes what a lower one defaults to; the next entry takes over.
        let files = [
            f("[Removed Associations]\nx/y=a.desktop;\n"),
            f("[Default Applications]\nx/y=a.desktop;b.desktop;\n"),
        ];
        assert_eq!(
            effective_default("x/y", &files, &have).as_deref(),
            Some("b.desktop")
        );
        // Removed in a LOWER file does not touch a higher file's default.
        let files = [
            f("[Default Applications]\nx/y=a.desktop;\n"),
            f("[Removed Associations]\nx/y=a.desktop;\n"),
        ];
        assert_eq!(
            effective_default("x/y", &files, &have).as_deref(),
            Some("a.desktop")
        );
        // A file removes its own entry too, and everything removed: nothing.
        let all = [f(
            "[Default Applications]\nx/y=a.desktop;\n[Removed Associations]\nx/y=a.desktop;\n",
        )];
        assert_eq!(effective_default("x/y", &all, &have), None);
        // The last of a repeated key counts.
        let dup = [f(
            "[Default Applications]\nx/y=a.desktop;\nx/y=c.desktop;\n",
        )];
        assert_eq!(
            effective_default("x/y", &dup, &have).as_deref(),
            Some("c.desktop")
        );
    }

    /// A byte order mark is not part of the first header, and is kept; a CR LF file stays CR LF in
    /// every line a change writes.
    #[test]
    fn a_bom_and_crlf_are_kept() {
        let bom = "\u{FEFF}[Default Applications]\ntext/plain=a.desktop;\n";
        let mut f = MimeApps::parse(bom);
        assert_eq!(
            f.default_for("text/plain").as_deref(),
            Some("a.desktop"),
            "header found"
        );
        assert_eq!(f.render(), bom);
        f.set_default("text/plain", "b.desktop");
        assert!(f.render().starts_with("\u{FEFF}[Default Applications]\n"));
        assert_eq!(f.default_for("text/plain").as_deref(), Some("b.desktop"));

        let crlf = "# c\r\n[Default Applications]\r\ntext/plain=a.desktop;\r\n";
        let mut g = MimeApps::parse(crlf);
        assert_eq!(g.default_for("text/plain").as_deref(), Some("a.desktop"));
        assert_eq!(g.render(), crlf);
        g.set_default("text/plain", "b.desktop");
        g.set_default("image/png", "b.desktop");
        let out = g.render();
        assert!(
            !out.replace("\r\n", "").contains('\n'),
            "no bare line feed: {out:?}"
        );
        assert!(out.contains("# c\r\n"), "{out:?}");
        assert_eq!(g.default_for("image/png").as_deref(), Some("b.desktop"));
        // A file with no groups at all: the new group and its blank line end in CR LF too.
        let mut h = MimeApps::parse("# only\r\n");
        h.set_default("a/b", "x.desktop");
        assert!(!h.render().replace("\r\n", "").contains('\n'));
        // LF files stay LF.
        let mut l = MimeApps::parse("[Default Applications]\n");
        l.set_default("a/b", "x.desktop");
        assert!(!l.render().contains('\r'));
    }

    /// Open With remembers: the app picked is the first Added Association, and the chooser puts
    /// it before the recommended ones, which keep their order.
    #[test]
    fn a_remembered_app_comes_first_in_the_chooser() {
        let apps = || {
            vec![
                "rec1.desktop",
                "rec2.desktop",
                "picked.desktop",
                "other.desktop",
            ]
        };
        let id = |s: &&str| s.to_string();
        let none: Vec<String> = Vec::new();
        assert_eq!(
            promote(apps(), id, &none),
            apps(),
            "nothing remembered: as it was"
        );
        let text = remembered("", "image/png", "picked.desktop").unwrap();
        let first = added_in(&text, "image/png");
        assert_eq!(first, ["picked.desktop"]);
        assert_eq!(
            promote(apps(), id, &first),
            [
                "picked.desktop",
                "rec1.desktop",
                "rec2.desktop",
                "other.desktop"
            ]
        );
        // Two picks: the newest first.
        let text = remembered(&text, "image/png", "other.desktop").unwrap();
        assert_eq!(
            promote(apps(), id, &added_in(&text, "image/png")),
            [
                "other.desktop",
                "picked.desktop",
                "rec1.desktop",
                "rec2.desktop"
            ]
        );
        assert!(added_in(&text, "text/plain").is_empty(), "per kind");
    }

    /// A category sets every type the app opens, always its first type, never one it does not open.
    #[test]
    fn a_category_sets_the_types_the_app_opens() {
        let images = &CATEGORIES[3];
        let viewer = Handler {
            id: "v.desktop".into(),
            mimes: vec!["image/jpeg".into(), "image/png".into(), "image/tiff".into()],
            ..Handler::default()
        };
        assert_eq!(images.mimes_for(&viewer), ["image/png", "image/jpeg"]);
        let browser = Handler {
            id: "b.desktop".into(),
            mimes: vec!["x-scheme-handler/http".into()],
            ..Handler::default()
        };
        assert_eq!(CATEGORIES[0].mimes_for(&browser), ["x-scheme-handler/http"]);
        let out = choose("", images, &viewer);
        let f = MimeApps::parse(&out);
        assert_eq!(f.default_for("image/jpeg").as_deref(), Some("v.desktop"));
        assert_eq!(f.default_for("image/gif"), None);
    }

    #[test]
    fn nine_categories_with_distinct_types() {
        let mut seen = std::collections::HashSet::new();
        for c in &CATEGORIES {
            assert!(!c.title.is_empty() && !c.subtitle.is_empty());
            for m in c.mimes {
                assert!(seen.insert(*m), "{m} in two rows");
            }
        }
        assert_eq!(CATEGORIES.len(), 9);
    }

    // ---- the property: any file round-trips and a change keeps what it does not touch

    /// A small deterministic generator (no new dependency): xorshift64.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
            &v[(self.next() % v.len() as u64) as usize]
        }
    }

    fn generated(seed: u64) -> String {
        let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mimes = [
            "image/png",
            "text/plain",
            "x-scheme-handler/http",
            "a/b",
            "video/mp4",
        ];
        let apps = ["a.desktop", "b.desktop", "c.d.desktop"];
        let groups = [DEFAULTS, ADDED, REMOVED, "Other Group"];
        let mut s = String::new();
        for _ in 0..(r.next() % 14) {
            match r.next() % 6 {
                0 => s.push_str(&format!("# note {}\n", r.next() % 100)),
                1 => s.push('\n'),
                2 => s.push_str(&format!("[{}]\n", r.pick(&groups))),
                3 => s.push_str(&format!(
                    "{} = {};{};\n",
                    r.pick(&mimes),
                    r.pick(&apps),
                    r.pick(&apps)
                )),
                4 => s.push_str(&format!("{}={}\n", r.pick(&mimes), r.pick(&apps))),
                _ => s.push_str("junk without equals\n"),
            }
        }
        if r.next().is_multiple_of(3) {
            s.pop(); // no final line break
        }
        if seed % 4 == 1 {
            s = s.replace('\n', "\r\n");
        }
        if seed.is_multiple_of(5) {
            s.insert(0, '\u{FEFF}');
        }
        s
    }

    fn comments(t: &str) -> Vec<String> {
        t.lines()
            .filter(|l| l.trim_start().starts_with('#'))
            .map(String::from)
            .collect()
    }

    /// Lines that sit in a group other than the two the page writes, or that are not key lines.
    fn untouched(t: &str) -> Vec<String> {
        let mut group = String::new();
        let mut out = Vec::new();
        for l in t.trim_start_matches('\u{FEFF}').lines() {
            if let Some(g) = header(l) {
                group = g.to_string();
                continue;
            }
            let page_group = group == DEFAULTS || group == ADDED || group == REMOVED;
            if (!page_group || key_value(l).is_none()) && !l.trim().is_empty() {
                out.push(l.to_string());
            }
        }
        out
    }

    #[test]
    fn fifty_generated_files_round_trip_and_keep_their_comments() {
        let mut changed = 0;
        for seed in 1..=50u64 {
            let t = generated(seed);
            assert_eq!(
                MimeApps::parse(&t).render(),
                t,
                "round trip, seed {seed}: {t:?}"
            );
            let mut f = MimeApps::parse(&t);
            let before: Vec<(String, Vec<String>)> = ["image/png", "text/plain", "video/mp4"]
                .iter()
                .map(|m| (m.to_string(), f.values(DEFAULTS, m)))
                .collect();
            f.set_default("a/b", "new.desktop");
            let out = f.render();
            assert_eq!(
                f.default_for("a/b").as_deref(),
                Some("new.desktop"),
                "seed {seed}"
            );
            assert_eq!(
                f.values(ADDED, "a/b").first().map(String::as_str),
                Some("new.desktop")
            );
            assert_eq!(
                comments(&out),
                comments(&t),
                "comments, seed {seed}\n{t}\n--\n{out}"
            );
            assert_eq!(
                untouched(&out),
                untouched(&t),
                "other groups, seed {seed}\n{t}\n--\n{out}"
            );
            for (m, v) in &before {
                assert_eq!(&f.values(DEFAULTS, m), v, "{m} untouched, seed {seed}");
            }
            // Idempotent, and what we wrote reads back the same.
            let mut g = MimeApps::parse(&out);
            g.set_default("a/b", "new.desktop");
            assert_eq!(g.render(), out, "idempotent, seed {seed}");
            changed += usize::from(out != t);
        }
        assert_eq!(changed, 50, "every file got the new line");
    }

    // ---- the disk

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wol-open-with-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn apps_are_read_from_the_first_directory_that_has_them() {
        let (a, b) = (scratch("a"), scratch("b"));
        for (d, name) in [(&a, "First"), (&b, "Second")] {
            std::fs::create_dir_all(d.join("applications")).unwrap();
            std::fs::write(
                d.join("applications/x.desktop"),
                format!(
                    "[Desktop Entry]\nType=Application\nName={name}\nExec=x %F\nMimeType=a/b;\n"
                ),
            )
            .unwrap();
        }
        std::fs::write(
            b.join("applications/y.desktop"),
            "[Desktop Entry]\nType=Application\nName=Y\nExec=y\nNoDisplay=true\n",
        )
        .unwrap();
        let apps = read_apps(&[a.clone(), b.clone()]);
        let x = apps.iter().find(|h| h.id == "x.desktop").unwrap();
        assert_eq!(x.name, "First");
        assert!(
            !apps.iter().any(|h| h.id == "y.desktop"),
            "NoDisplay stays out of the lists"
        );
        let snap = Snapshot {
            apps,
            ..Snapshot::default()
        };
        let cat = Category {
            title: "t",
            subtitle: "s",
            mimes: &["a/b"],
        };
        assert_eq!(snap.candidates(&cat).len(), 1);
        let _ = std::fs::remove_dir_all(a);
        let _ = std::fs::remove_dir_all(b);
    }

    #[test]
    fn a_write_replaces_the_file_whole() {
        let d = scratch("w");
        let f = d.join("sub/mimeapps.list");
        write_over(&f, "one\n").unwrap();
        write_over(&f, "two\n").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "two\n");
        assert_eq!(
            std::fs::read_dir(d.join("sub")).unwrap().count(),
            1,
            "no temporary file is left"
        );
        // Writers at once: each has a temporary name of its own; the file is one of the texts.
        let writers: Vec<_> = (0..8)
            .map(|i| {
                let f = f.clone();
                std::thread::spawn(move || write_over(&f, &format!("writer {i}\n")).unwrap())
            })
            .collect();
        writers.into_iter().for_each(|w| w.join().unwrap());
        assert!(std::fs::read_to_string(&f).unwrap().starts_with("writer "));
        assert_eq!(std::fs::read_dir(d.join("sub")).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(d);
    }
}
