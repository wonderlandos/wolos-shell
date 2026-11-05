// SPDX-License-Identifier: GPL-2.0-or-later
//! Quick Look (docs/modules/quicklook.md): what `wol-quicklook` shows for Space on a file in
//! Files, without any window code. Kind detection, bounded readers (text, folder, PDF page),
//! window sizing, the show/close rule, and the D-Bus service Files calls (`org.gnome.NautilusPreviewer`,
//! interface `NautilusPreviewer2`: confirmed against Nautilus 51 source and binary, see the module doc).
//! Nothing here executes a file or touches the network; every read has a limit.
use gtk::prelude::*;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zbus::object_server::SignalEmitter;

/// The bus name, object path and interface Files (Nautilus 45 and later) calls for Space.
pub const NAME: &str = "org.gnome.NautilusPreviewer";
pub const PATH: &str = "/org/gnome/NautilusPreviewer";
pub const IFACE: &str = "org.gnome.NautilusPreviewer2";

/// Text read from a file: the first 256 KiB.
pub const TEXT_LIMIT: usize = 256 * 1024;
/// A picture bigger than this on disk is not decoded (it gets the information card).
pub const IMAGE_FILE_LIMIT: u64 = 256 * 1024 * 1024;
/// A folder is counted up to this many entries.
pub const FOLDER_LIMIT: usize = 20_000;
/// The PDF page is rendered by `pdftocairo` for at most this long, into at most this many bytes.
pub const PDF_SECONDS: u64 = 8;
pub const PDF_BYTES: usize = 32 * 1024 * 1024;
/// The longest side of the rendered page and of a decoded picture (device pixels).
pub const RENDER_SIDE: i32 = 2400;
/// A picture of more than this many pixels is refused before it is decoded (a small file can
/// declare a huge canvas).
pub const IMAGE_PIXELS_LIMIT: i64 = 200_000_000;
/// `pdftocairo` may map at most this much memory (`prlimit --as`).
pub const PDF_MEMORY: u64 = 1 << 30;
/// `ParentHandle` keeps at most this many bytes of what Files sends.
pub const HANDLE_LIMIT: usize = 256;
/// The output size assumed when GTK cannot tell.
pub const FALLBACK_SCREEN: (i32, i32) = (1920, 1080);
/// The idle previewer quits after this long hidden; the next Space starts it again.
pub const IDLE_EXIT_SECS: u32 = 600;

/// `GtkDirectionType` values the arrow keys send back to Files (`SelectionEvent`).
pub const DIR_UP: u32 = 2;
pub const DIR_DOWN: u32 = 3;
pub const DIR_LEFT: u32 = 4;
pub const DIR_RIGHT: u32 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Image,
    Text,
    Audio,
    Video,
    Pdf,
    Folder,
    Other,
}

const IMAGE: &[&str] = &[
    "png", "jpg", "jpeg", "jpe", "gif", "webp", "bmp", "tif", "tiff", "svg", "svgz", "avif", "ico",
    "xpm", "tga", "heic", "heif", "jxl", "qoi",
];
const AUDIO: &[&str] = &[
    "mp3", "flac", "ogg", "oga", "opus", "wav", "m4a", "aac", "wma", "aif", "aiff", "mka", "mid",
    "midi",
];
const VIDEO: &[&str] = &[
    "mp4", "m4v", "mkv", "webm", "avi", "mov", "ogv", "wmv", "flv", "mpg", "mpeg", "3gp", "m2ts",
];
const TEXT: &[&str] = &[
    "txt",
    "md",
    "markdown",
    "rst",
    "log",
    "csv",
    "tsv",
    "json",
    "jsonc",
    "yaml",
    "yml",
    "toml",
    "ini",
    "conf",
    "cfg",
    "xml",
    "html",
    "htm",
    "css",
    "scss",
    "js",
    "mjs",
    "ts",
    "tsx",
    "jsx",
    "rs",
    "c",
    "h",
    "cpp",
    "hpp",
    "cc",
    "py",
    "rb",
    "go",
    "java",
    "kt",
    "swift",
    "sh",
    "bash",
    "zsh",
    "fish",
    "lua",
    "cr",
    "sql",
    "diff",
    "patch",
    "tex",
    "bib",
    "service",
    "desktop",
    "rules",
    "mk",
    "cmake",
    "gradle",
    "php",
    "pl",
    "el",
    "vim",
    "org",
    "adoc",
    "nfo",
    "srt",
    "vtt",
    "ics",
    "vcf",
    "gitignore",
    "env",
    "lock",
];
/// Whole file names (lower case) that are text without an extension.
const TEXT_NAMES: &[&str] = &[
    "makefile",
    "readme",
    "license",
    "copying",
    "dockerfile",
    "pkgbuild",
    "changelog",
    "authors",
    "notice",
    "todo",
];

/// What a file name says it is. `Other` when the name does not say: [`kind_for`] then looks inside.
pub fn kind_of(name: &str) -> Kind {
    let lower = name.to_lowercase();
    let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    let has = |t: &[&str]| t.contains(&ext);
    if has(&["pdf"]) {
        Kind::Pdf
    } else if has(IMAGE) {
        Kind::Image
    } else if has(AUDIO) {
        Kind::Audio
    } else if has(VIDEO) {
        Kind::Video
    } else if has(TEXT) || TEXT_NAMES.contains(&lower.as_str()) {
        Kind::Text
    } else {
        Kind::Other
    }
}

/// Plain text: valid UTF-8 (a cut at the end of the sample is allowed), no NUL, not empty.
pub fn looks_like_text(sample: &[u8]) -> bool {
    if sample.is_empty() || sample.contains(&0) {
        return false;
    }
    match std::str::from_utf8(sample) {
        Ok(_) => true,
        // A multi-byte character cut by the end of the sample is fine; bad bytes before it are not.
        Err(e) => e.error_len().is_none(),
    }
}

/// `O_NONBLOCK` on Linux (x86-64 and arm64): opening a FIFO must not wait for a writer.
const O_NONBLOCK: i32 = 0o4000;

/// Opens `path` for reading only if it is a regular file, as the opened descriptor says: a FIFO, a
/// socket, a device or anything else is refused without a read (and the open does not block, so a
/// FIFO swapped in after a check cannot hold the thread either).
pub fn open_regular(path: &Path) -> Option<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(O_NONBLOCK)
        .open(path)
        .ok()?;
    f.metadata().ok()?.is_file().then_some(f)
}

/// The kind of a regular file: the name says it, or the first bytes do.
pub fn kind_of_file(name: &str, f: &mut std::fs::File) -> Kind {
    use std::io::{Seek, SeekFrom};
    match kind_of(name) {
        Kind::Other => {
            let mut sample = vec![0u8; 4096];
            let n = f.read(&mut sample).unwrap_or(0);
            sample.truncate(n);
            let _ = f.seek(SeekFrom::Start(0));
            if looks_like_text(&sample) {
                Kind::Text
            } else {
                Kind::Other
            }
        }
        k => k,
    }
}

/// The kind of the thing at `path`: a folder, a regular file (by name or by a look at the first
/// bytes), or `Other` for anything that is not either.
pub fn kind_for(path: &Path) -> Kind {
    let Ok(meta) = std::fs::metadata(path) else {
        return Kind::Other;
    };
    if meta.is_dir() {
        return Kind::Folder;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    match open_regular(path) {
        Some(mut f) => kind_of_file(&name, &mut f),
        None => Kind::Other,
    }
}

/// The local path of a `file://` URI (percent-decoded); `None` for any other kind of address.
pub fn path_of(uri: &str) -> Option<PathBuf> {
    gtk::gio::File::for_uri(uri)
        .path()
        .filter(|_| uri.starts_with("file://"))
}

/// The text of the regular file at `path` (see [`read_text_from`]).
pub fn read_text(path: &Path, limit: usize) -> Option<(String, bool)> {
    read_text_from(open_regular(path)?, limit)
}

/// The text of `f`: at most `limit` bytes, cut at a character boundary, and whether more follows.
/// `None` when it cannot be read or is not text (a NUL byte, or not UTF-8).
pub fn read_text_from(f: std::fs::File, limit: usize) -> Option<(String, bool)> {
    let mut buf = Vec::with_capacity(limit.min(64 * 1024) + 4);
    // One byte more than asked: tells a file of exactly `limit` bytes from a longer one.
    f.take(limit as u64 + 1).read_to_end(&mut buf).ok()?;
    let more = buf.len() > limit;
    buf.truncate(limit);
    if buf.contains(&0) {
        return None;
    }
    match String::from_utf8(buf) {
        Ok(s) => Some((s, more)),
        Err(e) => {
            // Cut inside a character at the limit: drop the partial character. Bad bytes earlier: not text.
            let valid = e.utf8_error().valid_up_to();
            if e.utf8_error().error_len().is_none() && more {
                let mut b = e.into_bytes();
                b.truncate(valid);
                String::from_utf8(b).ok().map(|s| (s, true))
            } else {
                None
            }
        }
    }
}

/// What is directly inside a folder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderInfo {
    pub folders: usize,
    pub files: usize,
    /// Bytes of the files directly inside (not followed into subfolders).
    pub bytes: u64,
    /// More than [`FOLDER_LIMIT`] entries: the numbers are at least these.
    pub partial: bool,
}

impl FolderInfo {
    pub fn items(&self) -> usize {
        self.folders + self.files
    }
}

/// Counts the entries of one folder (links not followed), up to `limit`. The caller reads the
/// folder (`std::fs::read_dir`) off the GTK thread.
pub fn folder_info(
    entries: impl Iterator<Item = std::io::Result<std::fs::DirEntry>>,
    limit: usize,
) -> FolderInfo {
    let mut info = FolderInfo::default();
    for (seen, e) in entries.flatten().enumerate() {
        if seen >= limit {
            info.partial = true;
            break;
        }
        match e.file_type() {
            Ok(t) if t.is_dir() => info.folders += 1,
            Ok(_) => {
                info.files += 1;
                info.bytes += e.metadata().map(|m| m.len()).unwrap_or(0);
            }
            Err(_) => {}
        }
    }
    info
}

/// Seconds since the epoch of the file's change time, if the system tells.
pub fn modified(path: &Path) -> Option<i64> {
    let t = std::fs::metadata(path).ok()?.modified().ok()?;
    let s = t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    i64::try_from(s).ok()
}

/// `argv` run without a shell for at most `timeout`, its stdout kept up to `max` bytes. `None` when
/// it cannot start, does not finish in time (it is killed), exits with an error, or prints more.
pub fn run_bounded(argv: &[&str], timeout: Duration, max: usize) -> Option<Vec<u8>> {
    run_bounded_in(argv, None, timeout, max)
}

/// [`run_bounded`] with `stdin` as the program's standard input (none: closed).
pub fn run_bounded_in(
    argv: &[&str],
    stdin: Option<std::process::Stdio>,
    timeout: Duration,
    max: usize,
) -> Option<Vec<u8>> {
    use std::process::{Command, Stdio};
    let (prog, args) = argv.split_first()?;
    let mut child = Command::new(prog)
        .args(args)
        .stdin(stdin.unwrap_or_else(Stdio::null))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let out = child.stdout.take()?;
    // Reads on its own thread so a full pipe never stalls the wait below.
    let reader = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = out.take(max as u64 + 1).read_to_end(&mut v);
        v
    });
    let end = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let bytes = reader.join().ok()?;
    (status?.success() && bytes.len() <= max).then_some(bytes)
}

/// First page of a PDF as PNG bytes, through `pdftocairo` when it is installed. The file reaches it
/// on its standard input (the descriptor already checked), and `prlimit` caps its memory when that
/// program is there.
pub fn pdf_first_page(f: std::fs::File) -> Option<Vec<u8>> {
    let side = RENDER_SIDE.to_string();
    let mem = format!("--as={PDF_MEMORY}");
    let mut argv: Vec<&str> = Vec::new();
    if ["/usr/bin/prlimit", "/bin/prlimit"]
        .iter()
        .any(|p| Path::new(p).exists())
    {
        argv.extend(["prlimit", &mem]);
    }
    argv.extend([
        "pdftocairo",
        "-png",
        "-f",
        "1",
        "-l",
        "1",
        "-singlefile",
        "-scale-to",
        &side,
        "-",
        "-",
    ]);
    run_bounded_in(
        &argv,
        Some(f.into()),
        Duration::from_secs(PDF_SECONDS),
        PDF_BYTES,
    )
}

/// The window size for content of `content` pixels on an output of `output`: the content's own
/// size, at most 80% of the output on each side (aspect kept), never below `min` (nor above the 80%).
pub fn fit(content: (i32, i32), output: (i32, i32), min: (i32, i32)) -> (i32, i32) {
    let (cw, ch) = (i64::from(content.0.max(1)), i64::from(content.1.max(1)));
    let (mw, mh) = (i64::from(output.0) * 4 / 5, i64::from(output.1) * 4 / 5);
    let (w, h) = if cw <= mw && ch <= mh {
        (cw, ch)
    } else if cw * mh >= ch * mw {
        (mw, ch * mw / cw)
    } else {
        (cw * mh / ch, mh)
    };
    let w = w.max(i64::from(min.0).min(mw));
    let h = h.max(i64::from(min.1).min(mh));
    (
        i32::try_from(w).unwrap_or(i32::MAX),
        i32::try_from(h).unwrap_or(i32::MAX),
    )
}

/// What Files asked for, in the order it arrives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// `ShowFile(uri, parent handle, close if already shown, activation token)`.
    Show {
        uri: String,
        handle: String,
        close_if_shown: bool,
    },
    Close,
}

/// What the window does about an [`Event`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Present(String),
    Hide,
    Nothing,
}

/// The show/close rule: Space again on the file in view closes it (`close_if_shown`), a different
/// file or an arrow-key move replaces it, and `Close` with nothing in view does nothing.
#[derive(Debug, Default)]
pub struct Shown {
    uri: Option<String>,
}

impl Shown {
    pub fn on(&mut self, e: &Event) -> Action {
        match e {
            Event::Show {
                uri,
                close_if_shown,
                ..
            } => {
                if *close_if_shown && self.uri.as_deref() == Some(uri.as_str()) {
                    self.uri = None;
                    Action::Hide
                } else {
                    self.uri = Some(uri.clone());
                    Action::Present(uri.clone())
                }
            }
            Event::Close => {
                if self.uri.take().is_some() {
                    Action::Hide
                } else {
                    Action::Nothing
                }
            }
        }
    }

    /// The window was closed some other way (Space or Esc in it, the close button).
    pub fn closed(&mut self) {
        self.uri = None;
    }

    pub fn is_visible(&self) -> bool {
        self.uri.is_some()
    }
}

/// A position in a recording, "3:41" or "1:02:09" (microseconds in, as GTK counts them).
pub fn clock(micros: i64) -> String {
    let secs = micros.max(0) / 1_000_000;
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// What the card and the window need to name a thing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Facts {
    pub name: String,
    pub kind: Kind,
    /// "4.2 MB"; none for a folder.
    pub size: Option<String>,
    /// "3 March 2026".
    pub modified: Option<String>,
}

/// A picture decoded and scaled down to at most [`RENDER_SIDE`] on its longest side.
#[derive(Clone, Debug)]
pub struct Raster {
    pub bytes: gtk::glib::Bytes,
    pub width: i32,
    pub height: i32,
    pub stride: usize,
    pub alpha: bool,
}

/// What was read for the preview. Everything here is `Send`: it crosses from the worker to the GTK thread.
#[derive(Clone, Debug)]
pub enum Loaded {
    Picture(Raster),
    /// The first page of a PDF, as PNG bytes.
    Page(Vec<u8>),
    Text {
        text: String,
        cut: bool,
    },
    /// Audio or video: the player opens the file itself.
    Media {
        video: bool,
        path: PathBuf,
    },
    Folder(FolderInfo),
    /// Nothing to draw: the sentence says why.
    Card(&'static str),
}

/// "3 March 2026" in the person's time zone and language.
pub fn date_text(secs: i64) -> Option<String> {
    gtk::glib::DateTime::from_unix_local(secs)
        .ok()?
        .format("%-d %B %Y")
        .ok()
        .map(|s| s.to_string())
}

fn name_of(uri: &str, path: Option<&Path>) -> String {
    match path {
        Some(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()),
        None => gtk::gio::File::for_uri(uri)
            .basename()
            .map(|n| n.to_string_lossy().into_owned()),
    }
    .filter(|n| !n.is_empty())
    .unwrap_or_else(|| uri.to_owned())
}

/// Reads what the preview of `uri` shows. Blocking (disk, `pdftocairo`): runs in [`load_async`].
pub fn load(uri: &str) -> (Facts, Loaded) {
    let path = path_of(uri);
    let name = name_of(uri, path.as_deref());
    let none = |kind, card| {
        (
            Facts {
                name: name.clone(),
                kind,
                size: None,
                modified: None,
            },
            Loaded::Card(card),
        )
    };
    let Some(path) = path else {
        return none(Kind::Other, crate::copy::quicklook::NO_PREVIEW);
    };
    // Stat first: only a folder or a regular file is ever opened. A FIFO, a socket or a device
    // (a .txt that is really /dev/zero, say) gets the card without a single read.
    let Ok(meta) = std::fs::metadata(&path) else {
        return none(Kind::Other, crate::copy::quicklook::NOT_READ);
    };
    let mut facts = Facts {
        name: name.clone(),
        kind: if meta.is_dir() {
            Kind::Folder
        } else {
            kind_of(&name)
        },
        size: meta.is_file().then(|| wol_core::storage::human(meta.len())),
        modified: modified(&path).and_then(date_text),
    };
    if meta.is_dir() {
        return match std::fs::read_dir(&path) {
            Ok(rd) => (facts, Loaded::Folder(folder_info(rd, FOLDER_LIMIT))),
            Err(_) => (facts, Loaded::Card(crate::copy::quicklook::NOT_READ)),
        };
    }
    // Opened without blocking and checked again on the descriptor (the path may have changed since the stat).
    let Some(mut file) = meta.is_file().then(|| open_regular(&path)).flatten() else {
        facts.kind = Kind::Other;
        facts.size = None;
        return (facts, Loaded::Card(crate::copy::quicklook::NO_PREVIEW));
    };
    let kind = kind_of_file(&name, &mut file);
    facts.kind = kind;
    let card = |facts: Facts, c| (facts, Loaded::Card(c));
    let loaded = match kind {
        Kind::Folder | Kind::Other => return card(facts, crate::copy::quicklook::NO_PREVIEW),
        Kind::Text => match read_text_from(file, TEXT_LIMIT) {
            Some((text, cut)) => Loaded::Text { text, cut },
            None => return card(facts, crate::copy::quicklook::NO_PREVIEW),
        },
        Kind::Pdf => match pdf_first_page(file) {
            Some(png) => Loaded::Page(png),
            None => return card(facts, crate::copy::quicklook::PDF_NOT_SHOWN),
        },
        // The player opens the path itself: it is a regular file now; a later swap is the player's to survive.
        Kind::Audio | Kind::Video => Loaded::Media {
            video: kind == Kind::Video,
            path,
        },
        Kind::Image if meta.len() > IMAGE_FILE_LIMIT => {
            return card(facts, crate::copy::quicklook::PICTURE_TOO_BIG);
        }
        Kind::Image => match decode_picture(file) {
            Ok(r) => Loaded::Picture(r),
            Err(Refused::TooLarge) => return card(facts, crate::copy::quicklook::PICTURE_TOO_BIG),
            Err(Refused::Unreadable) => {
                return card(facts, crate::copy::quicklook::PICTURE_NOT_SHOWN);
            }
        },
    };
    (facts, loaded)
}

/// Why a picture was not decoded.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// More than [`IMAGE_PIXELS_LIMIT`] pixels declared.
    TooLarge,
    Unreadable,
}

/// Decodes the picture in `f` through the loader, which reports the declared size before it
/// allocates anything: a canvas over [`IMAGE_PIXELS_LIMIT`] stops there. Only ever scaled down to
/// [`RENDER_SIDE`]; the embedded orientation is applied.
pub fn decode_picture(mut f: std::fs::File) -> Result<Raster, Refused> {
    use gtk::gdk_pixbuf::PixbufLoader;
    use std::cell::Cell;
    use std::rc::Rc;
    let loader = PixbufLoader::new();
    let too_large = Rc::new(Cell::new(false));
    {
        let too_large = too_large.clone();
        loader.connect_size_prepared(move |l, w, h| {
            if i64::from(w) * i64::from(h) > IMAGE_PIXELS_LIMIT {
                too_large.set(true);
                l.set_size(1, 1);
            } else if w.max(h) > RENDER_SIDE {
                let long = i64::from(w.max(h));
                let scaled = |v: i32| {
                    i32::try_from((i64::from(v) * i64::from(RENDER_SIDE) / long).max(1))
                        .unwrap_or(RENDER_SIDE)
                };
                l.set_size(scaled(w), scaled(h));
            }
        });
    }
    let mut chunk = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    let fed = loop {
        let n = match f.read(&mut chunk) {
            Ok(0) => break true,
            Ok(n) => n,
            Err(_) => break false,
        };
        total += n as u64;
        if total > IMAGE_FILE_LIMIT || loader.write(&chunk[..n]).is_err() || too_large.get() {
            break false;
        }
    };
    let closed = loader.close();
    if too_large.get() {
        return Err(Refused::TooLarge);
    }
    if !fed || closed.is_err() {
        return Err(Refused::Unreadable);
    }
    let pb = loader.pixbuf().ok_or(Refused::Unreadable)?;
    let pb = pb.apply_embedded_orientation().unwrap_or(pb);
    Ok(Raster {
        bytes: pb.read_pixel_bytes(),
        width: pb.width(),
        height: pb.height(),
        stride: usize::try_from(pb.rowstride()).unwrap_or(0),
        alpha: pb.has_alpha(),
    })
}

/// [`load`] on a worker thread.
pub async fn load_async(uri: String) -> (Facts, Loaded) {
    match gtk::gio::spawn_blocking(move || load(&uri)).await {
        Ok(v) => v,
        Err(_) => (
            Facts {
                name: String::new(),
                kind: Kind::Other,
                size: None,
                modified: None,
            },
            Loaded::Card(crate::copy::quicklook::NOT_READ),
        ),
    }
}

/// One load at a time, and a load nobody wants any more is dropped: a new ShowFile replaces the
/// one waiting, Close or Space cancels, and the result of an older load is never shown.
#[derive(Debug, Default)]
pub struct Loads {
    epoch: u64,
    running: bool,
    next: Option<String>,
}

impl Loads {
    /// A file was asked for. `Some((token, uri))`: start loading it now; `None`: a load is still
    /// running, this one starts when that finishes (a later request replaces it).
    pub fn request(&mut self, uri: &str) -> Option<(u64, String)> {
        self.epoch += 1;
        if self.running {
            self.next = Some(uri.to_owned());
            None
        } else {
            self.running = true;
            self.next = None;
            Some((self.epoch, uri.to_owned()))
        }
    }

    /// Hidden, closed or replaced from outside: whatever is loading or waiting is not wanted.
    pub fn cancel(&mut self) {
        self.epoch += 1;
        self.next = None;
    }

    /// The load of `token` ended. `(show it, the load to start next)`.
    pub fn finished(&mut self, token: u64) -> (bool, Option<(u64, String)>) {
        self.running = false;
        let next = self.next.take().map(|u| {
            self.running = true;
            (self.epoch, u)
        });
        (token == self.epoch, next)
    }
}

/// `handle` cut to [`HANDLE_LIMIT`] bytes at a character boundary.
pub fn cap_handle(handle: &str) -> String {
    let mut end = handle.len().min(HANDLE_LIMIT);
    while !handle.is_char_boundary(end) {
        end -= 1;
    }
    handle[..end].to_owned()
}

/// What the previewer shares with the bus thread: the two properties Files reads.
#[derive(Clone, Default)]
pub struct Props {
    visible: Arc<AtomicBool>,
    parent: Arc<Mutex<String>>,
}

struct Server {
    tx: async_channel::Sender<Event>,
    props: Props,
}

#[zbus::interface(name = "org.gnome.NautilusPreviewer2")]
impl Server {
    async fn show_file(
        &self,
        uri: String,
        window_handle: String,
        close_if_already_shown: bool,
        _token: String,
        #[zbus(signal_emitter)] e: SignalEmitter<'_>,
    ) {
        let window_handle = cap_handle(&window_handle);
        if let Ok(mut p) = self.props.parent.lock() {
            p.clone_from(&window_handle);
        }
        let _ = self.parent_handle_changed(&e).await;
        let _ = self.tx.try_send(Event::Show {
            uri,
            handle: window_handle,
            close_if_shown: close_if_already_shown,
        });
    }
    fn close(&self) {
        let _ = self.tx.try_send(Event::Close);
    }
    #[zbus(property)]
    fn visible(&self) -> bool {
        self.props.visible.load(Ordering::Relaxed)
    }
    #[zbus(property)]
    fn parent_handle(&self) -> String {
        self.props
            .parent
            .lock()
            .map(|p| p.clone())
            .unwrap_or_default()
    }
    /// An arrow key in the preview: Files moves its selection and shows the new file.
    #[zbus(signal)]
    async fn selection_event(e: &SignalEmitter<'_>, direction: u32) -> zbus::Result<()>;
    /// Return on a folder: Files opens it.
    #[zbus(signal)]
    async fn navigate_to(e: &SignalEmitter<'_>, uri: &str) -> zbus::Result<()>;
}

/// The live bus connection.
#[derive(Clone)]
pub struct Bus {
    conn: zbus::Connection,
    props: Props,
}

impl Bus {
    /// Owns [`NAME`] on `address` (None: the session bus) and forwards what Files asks to `tx`.
    pub async fn serve(
        address: Option<&str>,
        tx: async_channel::Sender<Event>,
    ) -> zbus::Result<Bus> {
        let props = Props::default();
        let b = match address {
            Some(a) => zbus::connection::Builder::address(a)?,
            None => zbus::connection::Builder::session()?,
        };
        let conn = b
            .name(NAME)?
            .serve_at(
                PATH,
                Server {
                    tx,
                    props: props.clone(),
                },
            )?
            .build()
            .await?;
        Ok(Bus { conn, props })
    }

    /// Files reads `Visible` to know whether to push the next file after an arrow key.
    pub async fn set_visible(&self, on: bool) -> zbus::Result<()> {
        if self.props.visible.swap(on, Ordering::Relaxed) == on {
            return Ok(());
        }
        let r = self
            .conn
            .object_server()
            .interface::<_, Server>(PATH)
            .await?;
        r.get().await.visible_changed(r.signal_emitter()).await
    }

    pub async fn selection(&self, direction: u32) -> zbus::Result<()> {
        let e = SignalEmitter::new(&self.conn, PATH)?;
        Server::selection_event(&e, direction).await
    }

    pub async fn navigate(&self, uri: &str) -> zbus::Result<()> {
        let e = SignalEmitter::new(&self.conn, PATH)?;
        Server::navigate_to(&e, uri).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wol-quicklook-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn thirty_names_say_what_they_are() {
        use Kind::*;
        let table = [
            ("holiday.JPG", Image),
            ("scan.png", Image),
            ("logo.svg", Image),
            ("anim.gif", Image),
            ("pic.webp", Image),
            ("song.mp3", Audio),
            ("album.FLAC", Audio),
            ("voice.opus", Audio),
            ("clip.wav", Audio),
            ("film.mkv", Video),
            ("film.mp4", Video),
            ("talk.webm", Video),
            ("report.pdf", Pdf),
            ("REPORT.PDF", Pdf),
            ("main.rs", Text),
            ("script.py", Text),
            ("notes.md", Text),
            ("data.json", Text),
            ("page.html", Text),
            ("build.sh", Text),
            ("Makefile", Text),
            ("README", Text),
            ("PKGBUILD", Text),
            ("config.toml", Text),
            ("app.service", Text),
            ("archive.tar.gz", Other),
            ("setup.exe", Other),
            ("blob.bin", Other),
            ("noextension", Other),
            ("types.ts", Text),
            (".gitignore", Text),
        ];
        assert_eq!(table.len(), 31);
        for (name, want) in table {
            assert_eq!(kind_of(name), want, "{name}");
        }
    }

    #[test]
    fn text_is_told_from_binary_by_the_first_bytes() {
        assert!(looks_like_text(b"hello\nworld\n"));
        assert!(looks_like_text("naïve café".as_bytes()));
        // A character cut by the end of the sample is still text.
        assert!(looks_like_text(&"é".as_bytes()[..1]));
        assert!(!looks_like_text(b""));
        assert!(!looks_like_text(b"abc\0def"));
        assert!(!looks_like_text(&[0xff, 0xfe, b'a']));
    }

    #[test]
    fn a_folder_and_an_unnamed_text_file_are_found_by_looking() {
        let d = scratch("kind");
        std::fs::write(d.join("LICENSEFILE"), "words words\n").unwrap();
        std::fs::write(d.join("blob"), [0u8, 1, 2, 3]).unwrap();
        assert_eq!(kind_for(&d), Kind::Folder);
        assert_eq!(kind_for(&d.join("LICENSEFILE")), Kind::Text);
        assert_eq!(kind_for(&d.join("blob")), Kind::Other);
        assert_eq!(kind_for(&d.join("missing")), Kind::Other);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn the_text_reader_stops_at_its_limit_on_a_character_boundary() {
        let d = scratch("text");
        let f = d.join("a.txt");
        std::fs::write(&f, "abcdef").unwrap();
        assert_eq!(read_text(&f, 6), Some(("abcdef".into(), false)));
        assert_eq!(read_text(&f, 4), Some(("abcd".into(), true)));
        // "é" is two bytes: a limit of 3 lands inside the second one.
        std::fs::write(&f, "aéé").unwrap();
        assert_eq!(read_text(&f, 3), Some(("aé".into(), true)));
        assert_eq!(read_text(&f, 2), Some(("a".into(), true)));
        // A big file is read only as far as the limit.
        std::fs::write(&f, "x".repeat(3 * TEXT_LIMIT)).unwrap();
        let (t, more) = read_text(&f, TEXT_LIMIT).unwrap();
        assert_eq!((t.len(), more), (TEXT_LIMIT, true));
        // Binary and missing files are not text.
        std::fs::write(&f, b"ab\0cd").unwrap();
        assert_eq!(read_text(&f, 100), None);
        std::fs::write(&f, [b'a', 0xff, b'b']).unwrap();
        assert_eq!(read_text(&f, 100), None);
        assert_eq!(read_text(&d.join("missing"), 100), None);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_folder_is_counted_one_level_deep_with_a_cap() {
        let d = scratch("folder");
        std::fs::create_dir(d.join("sub")).unwrap();
        std::fs::write(d.join("sub").join("deep"), vec![0u8; 1000]).unwrap();
        std::fs::write(d.join("a"), vec![0u8; 10]).unwrap();
        std::fs::write(d.join("b"), vec![0u8; 20]).unwrap();
        let i = folder_info(std::fs::read_dir(&d).unwrap(), FOLDER_LIMIT);
        assert_eq!(
            i,
            FolderInfo {
                folders: 1,
                files: 2,
                bytes: 30,
                partial: false
            }
        );
        assert_eq!(i.items(), 3);
        let cut = folder_info(std::fs::read_dir(&d).unwrap(), 2);
        assert!(cut.partial);
        assert_eq!(cut.items(), 2);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn only_a_file_address_is_a_local_path() {
        assert_eq!(
            path_of("file:///home/a/My%20Pictures/x.png"),
            Some(PathBuf::from("/home/a/My Pictures/x.png"))
        );
        assert_eq!(path_of("trash:///x"), None);
        assert_eq!(path_of("sftp://host/x"), None);
        assert_eq!(path_of("/plain/path"), None);
    }

    #[test]
    fn a_program_that_hangs_is_killed_and_one_that_floods_is_refused() {
        let ok = run_bounded(&["printf", "abc"], Duration::from_secs(5), 100);
        assert_eq!(ok, Some(b"abc".to_vec()));
        let t = Instant::now();
        assert_eq!(
            run_bounded(&["sleep", "30"], Duration::from_millis(200), 100),
            None
        );
        assert!(t.elapsed() < Duration::from_secs(5));
        assert_eq!(
            run_bounded(&["printf", "abcdef"], Duration::from_secs(5), 3),
            None
        );
        assert_eq!(run_bounded(&["false"], Duration::from_secs(5), 3), None);
        assert_eq!(
            run_bounded(&["wol-no-such-program"], Duration::from_secs(5), 3),
            None
        );
    }

    fn png(path: &Path, w: i32, h: i32) {
        use gtk::gdk_pixbuf::{Colorspace, Pixbuf};
        let pb = Pixbuf::new(Colorspace::Rgb, false, 8, w, h).unwrap();
        pb.fill(0xff0000ff);
        pb.savev(path, "png", &[]).unwrap();
    }

    fn uri(p: &Path) -> String {
        format!("file://{}", p.display())
    }

    #[test]
    fn load_reads_each_kind_within_its_limits() {
        let d = scratch("load");
        // Text: the facts and the cut flag.
        let t = d.join("note.txt");
        std::fs::write(&t, "hello\n").unwrap();
        let (f, l) = load(&uri(&t));
        assert_eq!((f.name.as_str(), f.kind), ("note.txt", Kind::Text));
        assert_eq!(f.size.as_deref(), Some("6 bytes"));
        assert!(matches!(l, Loaded::Text { ref text, cut: false } if text == "hello\n"));
        std::fs::write(&t, "x".repeat(TEXT_LIMIT + 10)).unwrap();
        assert!(
            matches!(load(&uri(&t)).1, Loaded::Text { cut: true, ref text } if text.len() == TEXT_LIMIT)
        );
        // A .txt that is really binary has no text preview.
        std::fs::write(&t, [0u8, 1, 2]).unwrap();
        assert!(
            matches!(load(&uri(&t)).1, Loaded::Card(c) if c == crate::copy::quicklook::NO_PREVIEW)
        );
        // Pictures: a small one keeps its pixels, a big one is scaled down to RENDER_SIDE.
        let small = d.join("small.png");
        png(&small, 40, 20);
        match load(&uri(&small)).1 {
            Loaded::Picture(r) => assert_eq!((r.width, r.height), (40, 20)),
            other => panic!("{other:?}"),
        }
        let big = d.join("big.png");
        png(&big, RENDER_SIDE * 2, RENDER_SIDE);
        match load(&uri(&big)).1 {
            Loaded::Picture(r) => assert_eq!((r.width, r.height), (RENDER_SIDE, RENDER_SIDE / 2)),
            other => panic!("{other:?}"),
        }
        // A picture that is not one.
        let bad = d.join("bad.png");
        std::fs::write(&bad, "not a picture").unwrap();
        assert!(
            matches!(load(&uri(&bad)).1, Loaded::Card(c) if c == crate::copy::quicklook::PICTURE_NOT_SHOWN)
        );
        // Folders, media, unknown files, missing files and addresses that are not local.
        assert!(
            matches!(load(&uri(&d)).1, Loaded::Folder(ref i) if i.files == 4 && i.folders == 0)
        );
        assert_eq!(load(&uri(&d)).0.size, None);
        let m = d.join("song.flac");
        std::fs::write(&m, "x").unwrap();
        assert!(matches!(
            load(&uri(&m)).1,
            Loaded::Media { video: false, .. }
        ));
        let v = d.join("film.mkv");
        std::fs::write(&v, "x").unwrap();
        assert!(matches!(
            load(&uri(&v)).1,
            Loaded::Media { video: true, .. }
        ));
        let o = d.join("setup.exe");
        std::fs::write(&o, [0u8, 1]).unwrap();
        let (f, l) = load(&uri(&o));
        assert_eq!(f.kind, Kind::Other);
        assert!(matches!(l, Loaded::Card(c) if c == crate::copy::quicklook::NO_PREVIEW));
        let (f, l) = load(&uri(&d.join("gone.txt")));
        assert_eq!(f.name, "gone.txt");
        assert!(matches!(l, Loaded::Card(c) if c == crate::copy::quicklook::NOT_READ));
        let (f, l) = load("trash:///thing%20one.txt");
        assert_eq!(f.name, "thing one.txt");
        assert!(matches!(l, Loaded::Card(_)));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_pdf_gets_its_first_page_when_the_renderer_is_installed() {
        let d = scratch("pdf");
        let f = d.join("a.pdf");
        std::fs::write(
            &f,
            "%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 100]>>endobj\ntrailer<</Root 1 0 R>>\n%%EOF\n",
        )
        .unwrap();
        let have = run_bounded(&["pdftocairo", "-v"], Duration::from_secs(5), 1 << 16).is_some()
            || std::process::Command::new("pdftocairo")
                .arg("-v")
                .output()
                .is_ok();
        match load(&uri(&f)).1 {
            Loaded::Page(png) => assert_eq!(&png[..4], b"\x89PNG"),
            Loaded::Card(c) => {
                assert!(!have, "pdftocairo is installed but the page was not drawn");
                assert_eq!(c, crate::copy::quicklook::PDF_NOT_SHOWN);
            }
            other => panic!("{other:?}"),
        }
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn positions_read_as_minutes_and_seconds() {
        assert_eq!(clock(0), "0:00");
        assert_eq!(clock(-5), "0:00");
        assert_eq!(clock(23_900_000), "0:23");
        assert_eq!(clock(221_000_000), "3:41");
        assert_eq!(clock(3_729_000_000), "1:02:09");
    }

    #[test]
    fn a_slow_load_that_was_cancelled_or_replaced_is_never_shown() {
        let mut l = Loads::default();
        // A file is asked for and starts loading.
        let (a, uri) = l.request("file:///a").unwrap();
        assert_eq!(uri, "file:///a");
        // Close or Space while it loads: when it ends it is not shown.
        l.cancel();
        assert_eq!(l.finished(a), (false, None));
        // Two asks during one slow load: only the last one starts, and only after the first ended.
        let (b, _) = l.request("file:///b").unwrap();
        assert_eq!(l.request("file:///c"), None);
        assert_eq!(l.request("file:///d"), None);
        let (show_b, next) = l.finished(b);
        assert!(!show_b, "b was replaced");
        let (d, uri) = next.unwrap();
        assert_eq!(uri, "file:///d");
        assert_eq!(l.finished(d), (true, None));
        // Cancel drops the one waiting too.
        let (e, _) = l.request("file:///e").unwrap();
        assert_eq!(l.request("file:///f"), None);
        l.cancel();
        assert_eq!(l.finished(e), (false, None));
        // Idle again: the next ask starts at once.
        assert!(l.request("file:///g").is_some());
    }

    #[test]
    fn the_parent_handle_is_capped_on_a_character_boundary() {
        assert_eq!(cap_handle("wayland:abc"), "wayland:abc");
        let long = "x".repeat(10_000);
        assert_eq!(cap_handle(&long).len(), HANDLE_LIMIT);
        // 'é' is two bytes: a cap landing between them backs up.
        let odd = format!("{}é", "x".repeat(HANDLE_LIMIT - 1));
        assert_eq!(cap_handle(&odd).len(), HANDLE_LIMIT - 1);
    }

    /// Runs `load` on a thread and gives up after one second: a blocked read fails the test.
    fn load_in_time(uri: String) -> Option<(Facts, Loaded)> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(load(&uri));
        });
        rx.recv_timeout(Duration::from_secs(1)).ok()
    }

    #[test]
    fn a_fifo_a_device_or_a_socket_gets_the_card_without_being_read() {
        let d = scratch("special");
        for name in ["pipe.txt", "pipe.png", "pipe.pdf", "pipe.flac", "pipe"] {
            let ok = std::process::Command::new("mkfifo")
                .arg(d.join(name))
                .status()
                .is_ok_and(|s| s.success());
            assert!(ok, "mkfifo");
            let (f, l) =
                load_in_time(uri(&d.join(name))).unwrap_or_else(|| panic!("{name}: load blocked"));
            assert!(
                matches!(l, Loaded::Card(c) if c == crate::copy::quicklook::NO_PREVIEW),
                "{name}: {l:?}"
            );
            assert_eq!(f.kind, Kind::Other, "{name}");
        }
        for dev in ["/dev/zero", "/dev/null", "/dev/urandom"] {
            let (_, l) = load_in_time(format!("file://{dev}"))
                .unwrap_or_else(|| panic!("{dev}: load blocked"));
            assert!(matches!(l, Loaded::Card(_)), "{dev}: {l:?}");
        }
        // A named copy of a device: a .txt link to /dev/zero is followed like Files does, and is refused.
        std::os::unix::fs::symlink("/dev/zero", d.join("zero.txt")).unwrap();
        let (_, l) = load_in_time(uri(&d.join("zero.txt"))).expect("zero.txt blocked");
        assert!(matches!(l, Loaded::Card(_)), "{l:?}");
        // A socket.
        let _sock = std::os::unix::net::UnixListener::bind(d.join("s.txt")).unwrap();
        let (_, l) = load_in_time(uri(&d.join("s.txt"))).expect("socket blocked");
        assert!(matches!(l, Loaded::Card(_)), "{l:?}");
        // open_regular refuses them itself (the descriptor check, not only the stat).
        assert!(open_regular(&d.join("pipe.txt")).is_none());
        assert!(open_regular(Path::new("/dev/zero")).is_none());
        assert!(open_regular(&d.join("s.txt")).is_none());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_picture_that_declares_a_huge_canvas_is_refused_before_it_is_decoded() {
        let d = scratch("bomb");
        // PNG signature and an IHDR for 20000 x 20000 (400 megapixels), then the start of the image data: 53 bytes.
        let ihdr = [
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0xc3, 0x50, 0x00, 0x00, 0xc3, 0x50, 0x08, 0x02, 0x00, 0x00,
            0x00, 0xc4, 0xcd, 0xaa, 0x9d, 0x00, 0x00, 0x00, 0x08, 0x49, 0x44, 0x41, 0x54, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xee, 0x48, 0x5d, 0x87,
        ];
        let f = d.join("bomb.png");
        std::fs::write(&f, ihdr).unwrap();
        assert_eq!(
            decode_picture(open_regular(&f).unwrap()).unwrap_err(),
            Refused::TooLarge
        );
        assert!(matches!(
            load(&uri(&f)).1,
            Loaded::Card(c) if c == crate::copy::quicklook::PICTURE_TOO_BIG
        ));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_pdf_renderer_that_wants_too_much_memory_is_stopped() {
        // The cap is real: a program under prlimit --as cannot map more.
        if !Path::new("/usr/bin/prlimit").exists() {
            return;
        }
        let ok = run_bounded(
            &["prlimit", "--as=1073741824", "true"],
            Duration::from_secs(5),
            10,
        );
        assert_eq!(ok, Some(Vec::new()));
        let low = run_bounded(
            &["prlimit", "--as=1048576", "true"],
            Duration::from_secs(5),
            10,
        );
        assert_eq!(
            low, None,
            "a 1 MiB address space cannot even start a program"
        );
    }

    #[test]
    fn the_window_is_the_content_up_to_eighty_percent_of_the_output() {
        let min = (480, 360);
        let out = (1920, 1080);
        // Small content gets the minimum.
        assert_eq!(fit((100, 80), out, min), (480, 360));
        // Content that fits keeps its size.
        assert_eq!(fit((900, 600), out, min), (900, 600));
        // A big picture scales down to 80% of the height, aspect kept.
        assert_eq!(fit((4000, 3000), out, min), (1152, 864));
        // A wide panorama is limited by the width.
        assert_eq!(fit((8000, 1000), out, min), (1536, 360));
        // A small output wins over the minimum.
        assert_eq!(fit((100, 100), (400, 300), min), (320, 240));
    }

    #[test]
    fn space_again_closes_and_another_file_replaces() {
        let show = |u: &str, c: bool| Event::Show {
            uri: u.into(),
            handle: String::new(),
            close_if_shown: c,
        };
        let mut s = Shown::default();
        assert!(!s.is_visible());
        assert_eq!(s.on(&Event::Close), Action::Nothing);
        assert_eq!(
            s.on(&show("file:///a", true)),
            Action::Present("file:///a".into())
        );
        assert!(s.is_visible());
        // Space on the same file closes it.
        assert_eq!(s.on(&show("file:///a", true)), Action::Hide);
        assert!(!s.is_visible());
        // After an arrow key Files sends the next file with close_if_shown false.
        assert_eq!(
            s.on(&show("file:///a", false)),
            Action::Present("file:///a".into())
        );
        assert_eq!(
            s.on(&show("file:///b", false)),
            Action::Present("file:///b".into())
        );
        // The same file again without the flag stays shown.
        assert_eq!(
            s.on(&show("file:///b", false)),
            Action::Present("file:///b".into())
        );
        // Space on a different file while one is shown shows that one.
        assert_eq!(
            s.on(&show("file:///c", true)),
            Action::Present("file:///c".into())
        );
        assert_eq!(s.on(&Event::Close), Action::Hide);
        assert_eq!(s.on(&Event::Close), Action::Nothing);
        // Closed from inside the window.
        s.on(&show("file:///d", false));
        s.closed();
        assert_eq!(
            s.on(&show("file:///d", true)),
            Action::Present("file:///d".into())
        );
    }
}
