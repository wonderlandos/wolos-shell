// SPDX-License-Identifier: GPL-2.0-or-later
//! Players libwol apps publish (`wol_media_publish`, libwol 1.16): the desktop owns one MPRIS name per
//! player on the session bus, `org.mpris.MediaPlayer2.wol.<program name>.instance<pid>`, on behalf of the app. The
//! app gives only the state (`shared::portal::MediaState`); the name, the icon and the bus are ours,
//! so no app can pose as another or as a player it is not. A remote control (media keys,
//! `playerctl`, Control Center) calls our object and goes back to wold as `media.key`, which tells
//! the one process that owns the player, only for what its abilities allow. No GTK: one bus
//! connection per player, one worker thread for the calls to wold.
use shared::events::Event;
use shared::portal::{
    CAN_NEXT, CAN_PREVIOUS, CAN_SEEK, Call, MAX_MEDIA_US, MediaKey, MediaState, MediaStatus,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

/// Every player's bus name starts with this: apps never choose a name outside it.
pub const NAME_PREFIX: &str = "org.mpris.MediaPlayer2.wol.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
/// The one track a player has at a time (`mpris:trackid`; `SetPosition` must name it).
const TRACK: &str = "/org/wolos/media/track";
/// Players the desktop serves for one account at once.
const MAX_PLAYERS: usize = 8;
/// The longest a picture may be to be shown.
const MAX_ART: u64 = 4 * 1024 * 1024;
/// How far the app's position may drift from the one we counted before it counts as a seek.
const SEEK_SLACK_US: i64 = 1_500_000;

/// The part of a bus name an app's executable gives: its file name, with every character outside
/// `A-Z a-z 0-9 _ -` made `_`, not starting with a digit, at most 48 bytes.
pub fn id_of(app: &str) -> String {
    let base = Path::new(app)
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let mut id: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(48)
        .collect();
    if id.is_empty() {
        id.push_str("app");
    }
    if id.starts_with(|c: char| c.is_ascii_digit()) {
        id.insert(0, '_');
    }
    id
}

/// The player's bus name: `org.mpris.MediaPlayer2.wol.<id>.instance<pid>`. The instance suffix is
/// MPRIS' own convention for one player per process, and it is how Memory Saver tells which process
/// is playing (`wol_realtime::memsaver::players` reads the pid from it), so a playing app is never
/// frozen. `playerctl -p wol.<id>` still finds it.
pub fn bus_name(app: &str, pid: i32) -> String {
    format!("{NAME_PREFIX}{}.instance{pid}", id_of(app))
}

/// A path with a hidden part (`.ssh`, `.cache`, `..`): nothing an app names for the desktop to show
/// or send is hidden, and a link into a hidden folder is followed before this is asked again.
pub fn hidden(path: &Path) -> bool {
    path.components()
        .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
}

/// The real path of the file `path` names, if it is inside one of `roots` and nothing in it is
/// hidden, both as named and once every link is followed. The name is judged first, without
/// touching the disk, so a path outside the roots or a hidden one is refused the same whether or
/// not it exists; then the real path is judged again, so a link from a visible folder into
/// `.ssh` is refused like a missing file. None says nothing about which.
pub fn canonical_within(path: &Path, roots: &[PathBuf]) -> Option<PathBuf> {
    let real_roots: Vec<PathBuf> = roots.iter().filter_map(|r| r.canonicalize().ok()).collect();
    let inside = |p: &Path| !hidden(p) && roots.iter().chain(&real_roots).any(|r| p.starts_with(r));
    if !inside(path) {
        return None;
    }
    let real = path.canonicalize().ok()?;
    inside(&real).then_some(real)
}

/// Where an app's picture may be: the person's home, mounted media, the system's shared files
/// (`/usr/share`) and the session's runtime folder (`$XDG_RUNTIME_DIR`, where a player writes a
/// cover it only needs for now).
pub fn art_roots() -> Vec<PathBuf> {
    let mut roots = vec![
        wol_core::widgets::home(),
        PathBuf::from("/usr/share"),
        PathBuf::from("/run/media"),
        PathBuf::from("/media"),
    ];
    roots.extend(crate::envv::var("XDG_RUNTIME_DIR").map(PathBuf::from));
    roots
}

/// `file://` + the real path, percent-encoded; None unless it is a small PNG, JPEG, WebP or GIF
/// file inside `roots` ([`canonical_within`]). Other schemes are never offered: a client that
/// fetches art would fetch what an app chose.
pub fn art_url(path: &str, roots: &[PathBuf]) -> Option<String> {
    use std::io::Read;
    let p = canonical_within(Path::new(path), roots)?;
    let meta = std::fs::metadata(&p).ok()?;
    if !meta.is_file() || meta.len() > MAX_ART || meta.len() < 12 {
        return None;
    }
    let mut head = [0u8; 12];
    std::fs::File::open(&p).ok()?.read_exact(&mut head).ok()?;
    let image = head.starts_with(b"\x89PNG\r\n\x1a\n")
        || head.starts_with(&[0xff, 0xd8, 0xff])
        || head.starts_with(b"GIF8")
        || (head.starts_with(b"RIFF") && &head[8..12] == b"WEBP");
    image.then(|| {
        let mut url = String::from("file://");
        for b in p.as_os_str().as_encoded_bytes() {
            if b.is_ascii_alphanumeric() || b"/-._~".contains(b) {
                url.push(char::from(*b));
            } else {
                url.push_str(&format!("%{b:02X}"));
            }
        }
        url
    })
}

/// The directories of `PATH`, to find the program a `.desktop` entry runs.
pub fn path_dirs() -> Vec<PathBuf> {
    crate::envv::var("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default()
}

/// The real path of the program an entry's `Exec` runs: an absolute path, or a name found in `dirs`.
fn resolve(prog: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    if prog.starts_with('/') {
        return Path::new(prog).canonicalize().ok();
    }
    if prog.contains('/') {
        return None;
    }
    dirs.iter().find_map(|d| d.join(prog).canonicalize().ok())
}

/// The entry whose program IS the app: the entry's `Exec` program, resolved to its real path,
/// equals the app's real executable. Never by file name alone, so a program of the same name
/// anywhere else (`/tmp/x/firefox`) borrows nothing.
pub fn entry_of<'a>(
    entries: &'a [crate::desktop::Entry],
    app: &str,
    dirs: &[PathBuf],
) -> Option<&'a crate::desktop::Entry> {
    let exe = Path::new(app).canonicalize().ok()?;
    entries
        .iter()
        .find(|e| e.exec.first().and_then(|p| resolve(p, dirs)).as_ref() == Some(&exe))
}

/// The name and `.desktop` id an app is shown with: its entry's, else the program's own name and
/// no id. The one lookup for the player's Identity and the recent files' app name.
pub fn identity(
    entries: &[crate::desktop::Entry],
    app: &str,
    dirs: &[PathBuf],
) -> (String, String) {
    match entry_of(entries, app, dirs) {
        Some(e) => (
            e.name.clone(),
            e.id.strip_suffix(".desktop").unwrap_or(&e.id).to_owned(),
        ),
        None => (id_of(app), String::new()),
    }
}

/// What a remote control may do, given the player's abilities: play, pause, toggle and stop
/// always; skipping and seeking only if the app said it can.
pub fn allowed(key: MediaKey, can: u8) -> bool {
    match key {
        MediaKey::Play | MediaKey::Pause | MediaKey::Toggle | MediaKey::Stop => true,
        MediaKey::Next => can & CAN_NEXT != 0,
        MediaKey::Previous => can & CAN_PREVIOUS != 0,
        MediaKey::Seek | MediaKey::SetPosition => can & CAN_SEEK != 0,
    }
}

/// What the desktop knows of one player.
struct Model {
    identity: String,
    desktop_entry: String,
    state: MediaState,
    art: Option<String>,
    /// When `state.position_us` was true.
    at: Instant,
}

impl Model {
    fn status(&self) -> &'static str {
        match self.state.status {
            MediaStatus::Playing => "Playing",
            MediaStatus::Paused => "Paused",
            MediaStatus::Stopped => "Stopped",
        }
    }

    /// Where the player is at `now`: the app's word, plus the time since while it plays.
    fn position(&self, now: Instant) -> i64 {
        let mut p = self.state.position_us;
        if self.state.status == MediaStatus::Playing {
            let since = now.saturating_duration_since(self.at).as_micros();
            p = p.saturating_add(i64::try_from(since).unwrap_or(i64::MAX));
        }
        let end = if self.state.length_us > 0 {
            self.state.length_us
        } else {
            MAX_MEDIA_US
        };
        p.clamp(0, end)
    }

    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let mut m: HashMap<String, OwnedValue> = HashMap::new();
        let mut put = |k: &str, v: Value<'_>| {
            if let Ok(v) = OwnedValue::try_from(v) {
                m.insert(k.to_owned(), v);
            }
        };
        if let Ok(p) = ObjectPath::try_from(TRACK) {
            put("mpris:trackid", Value::from(p));
        }
        let s = &self.state;
        if !s.title.is_empty() {
            put("xesam:title", Value::from(s.title.as_str()));
        }
        if !s.artist.is_empty() {
            put("xesam:artist", Value::from(vec![s.artist.clone()]));
        }
        if !s.album.is_empty() {
            put("xesam:album", Value::from(s.album.as_str()));
        }
        if s.length_us > 0 {
            put("mpris:length", Value::from(s.length_us));
        }
        if let Some(u) = &self.art {
            put("mpris:artUrl", Value::from(u.as_str()));
        }
        m
    }
}

type Shared = Arc<Mutex<Model>>;

fn lock(m: &Shared) -> std::sync::MutexGuard<'_, Model> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// `org.mpris.MediaPlayer2`: nothing the app could raise or quit from here.
struct RootObject {
    model: Shared,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl RootObject {
    fn raise(&self) {}
    fn quit(&self) {}
    #[zbus(property)]
    fn can_quit(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn can_raise(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn identity(&self) -> String {
        lock(&self.model).identity.clone()
    }
    #[zbus(property)]
    fn desktop_entry(&self) -> String {
        lock(&self.model).desktop_entry.clone()
    }
    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        Vec::new()
    }
    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        Vec::new()
    }
}

/// A remote control to hand to wold: (process, key, microseconds).
type Press = (i32, MediaKey, i64);

/// `org.mpris.MediaPlayer2.Player`, read from the model; a control only queues a [`Press`].
struct PlayerObject {
    model: Shared,
    pid: i32,
    presses: Sender<Press>,
}

impl PlayerObject {
    fn press(&self, key: MediaKey, value_us: i64) {
        let can = lock(&self.model).state.can;
        if allowed(key, can) {
            let _ = self.presses.send((self.pid, key, value_us));
        }
    }

    fn can(&self, flag: u8) -> bool {
        lock(&self.model).state.can & flag != 0
    }
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl PlayerObject {
    fn next(&self) {
        self.press(MediaKey::Next, 0);
    }
    fn previous(&self) {
        self.press(MediaKey::Previous, 0);
    }
    fn pause(&self) {
        self.press(MediaKey::Pause, 0);
    }
    fn play_pause(&self) {
        self.press(MediaKey::Toggle, 0);
    }
    fn stop(&self) {
        self.press(MediaKey::Stop, 0);
    }
    fn play(&self) {
        self.press(MediaKey::Play, 0);
    }
    fn seek(&self, offset: i64) {
        if offset.abs() <= MAX_MEDIA_US {
            self.press(MediaKey::Seek, offset);
        }
    }
    fn set_position(&self, track: ObjectPath<'_>, position: i64) {
        if track.as_str() == TRACK && (0..=MAX_MEDIA_US).contains(&position) {
            self.press(MediaKey::SetPosition, position);
        }
    }
    /// Nothing is opened from outside: a player plays what its app chose.
    fn open_uri(&self, _uri: String) {}
    #[zbus(signal)]
    async fn seeked(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        position: i64,
    ) -> zbus::Result<()>;
    #[zbus(property)]
    fn playback_status(&self) -> String {
        lock(&self.model).status().to_owned()
    }
    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn volume(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn set_volume(&self, _volume: f64) {}
    #[zbus(property)]
    fn position(&self) -> i64 {
        lock(&self.model).position(Instant::now())
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        lock(&self.model).metadata()
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        self.can(CAN_NEXT)
    }
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.can(CAN_PREVIOUS)
    }
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        self.can(CAN_SEEK)
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

/// One published player: its bus connection (the name goes when this drops) and its model.
struct Live {
    conn: zbus::blocking::Connection,
    model: Shared,
}

/// The players of the account, by process.
pub struct Players {
    dbus: Option<String>,
    live: HashMap<i32, Live>,
    presses: Sender<Press>,
    entries: Option<Vec<crate::desktop::Entry>>,
    art_roots: Vec<PathBuf>,
}

impl Players {
    /// `wold`: the system service to hand remote controls to. `dbus`: bus address (None: the
    /// session bus). The worker thread ends when this drops.
    pub fn new(wold: PathBuf, dbus: Option<String>) -> Self {
        let (presses, queue) = channel::<Press>();
        let worker = std::thread::Builder::new()
            .name("wol-media-keys".into())
            .spawn(move || {
                for (player, key, value_us) in queue {
                    let c = Call::MediaKey {
                        player,
                        key,
                        value_us,
                    };
                    let a = crate::app_notices::call(&wold, &c);
                    if !a.ok {
                        crate::diag::say!("wol-shell: media key: {:?}", a.message);
                    }
                }
            });
        if let Err(e) = worker {
            crate::diag::say!("wol-shell: media keys: {e}");
        }
        Self {
            dbus,
            live: HashMap::new(),
            presses,
            entries: None,
            art_roots: art_roots(),
        }
    }

    /// Pictures may come from `roots` ([`art_roots`] by default).
    pub fn with_art_roots(mut self, roots: Vec<PathBuf>) -> Self {
        self.art_roots = roots;
        self
    }

    /// Handle a wold event of account `uid`: true when it was about a player.
    pub fn event(&mut self, e: &Event, uid: u32) -> bool {
        match e {
            Event::MediaState {
                uid: u,
                pid,
                app,
                state,
            } => {
                if *u == uid {
                    self.update(*pid, app, state);
                }
                true
            }
            Event::MediaGone { uid: u, pid } => {
                if *u == uid {
                    self.live.remove(pid);
                }
                true
            }
            _ => false,
        }
    }

    /// How many players are up.
    pub fn len(&self) -> usize {
        self.live.len()
    }

    /// True when no player is up.
    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    fn update(&mut self, pid: i32, app: &str, state: &MediaState) {
        if state.check().is_err() {
            return;
        }
        let art = (!state.art.is_empty())
            .then(|| art_url(&state.art, &self.art_roots))
            .flatten();
        if let Some(l) = self.live.get(&pid) {
            changed(l, state, art);
            return;
        }
        if self.live.len() >= MAX_PLAYERS {
            return;
        }
        let (identity, desktop_entry) = self.who(app);
        let model: Shared = Arc::new(Mutex::new(Model {
            identity,
            desktop_entry,
            state: state.clone(),
            art,
            at: Instant::now(),
        }));
        match self.serve(pid, app, &model) {
            Ok(conn) => {
                self.live.insert(pid, Live { conn, model });
            }
            Err(e) => crate::diag::say!("wol-shell: a player for {app}: {e}"),
        }
    }

    /// The name and entry of the app ([`identity`]).
    fn who(&mut self, app: &str) -> (String, String) {
        let entries = self.entries.get_or_insert_with(crate::desktop::load_all);
        identity(entries, app, &path_dirs())
    }

    /// One bus connection with the player's objects, owning the player's name (never queued behind
    /// another owner: the process id is in the name).
    fn serve(
        &self,
        pid: i32,
        app: &str,
        model: &Shared,
    ) -> zbus::Result<zbus::blocking::Connection> {
        let b = match &self.dbus {
            Some(a) => zbus::blocking::connection::Builder::address(a.as_str())?,
            None => zbus::blocking::connection::Builder::session()?,
        };
        let conn = b
            .serve_at(
                PATH,
                RootObject {
                    model: model.clone(),
                },
            )?
            .serve_at(
                PATH,
                PlayerObject {
                    model: model.clone(),
                    pid,
                    presses: self.presses.clone(),
                },
            )?
            .build()?;
        let flags = zbus::fdo::RequestNameFlags::DoNotQueue.into();
        match conn.request_name_with_flags(bus_name(app, pid), flags)? {
            zbus::fdo::RequestNameReply::PrimaryOwner
            | zbus::fdo::RequestNameReply::AlreadyOwner => Ok(conn),
            _ => Err(zbus::Error::NameTaken),
        }
    }
}

/// The app's new state: keep it and tell the bus what changed.
fn changed(l: &Live, state: &MediaState, art: Option<String>) {
    let now = Instant::now();
    let (props, seeked) = {
        let mut m = lock(&l.model);
        let before = (m.status(), m.metadata(), m.state.can);
        let counted = m.position(now);
        m.state = state.clone();
        m.art = art;
        m.at = now;
        let after = (m.status(), m.metadata(), m.state.can);
        let mut props: HashMap<&str, Value<'_>> = HashMap::new();
        if before.0 != after.0 {
            props.insert("PlaybackStatus", Value::from(after.0));
        }
        if before.1 != after.1 {
            props.insert("Metadata", Value::from(after.1));
        }
        if before.2 != after.2 {
            for (name, flag) in [
                ("CanGoNext", CAN_NEXT),
                ("CanGoPrevious", CAN_PREVIOUS),
                ("CanSeek", CAN_SEEK),
            ] {
                if (before.2 ^ after.2) & flag != 0 {
                    props.insert(name, Value::from(after.2 & flag != 0));
                }
            }
        }
        let jumped = (state.position_us - counted).abs() > SEEK_SLACK_US;
        (props, jumped.then_some(state.position_us))
    };
    let invalidated: Vec<&str> = Vec::new();
    if !props.is_empty() {
        let _ = l.conn.emit_signal(
            None::<&str>,
            PATH,
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            &(PLAYER, props, invalidated),
        );
    }
    if let Some(p) = seeked {
        let _ = l
            .conn
            .emit_signal(None::<&str>, PATH, PLAYER, "Seeked", &(p,));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_comes_from_the_kernels_word_and_is_always_ours() {
        for (app, want) in [
            ("/usr/bin/toy", "toy"),
            ("/usr/bin/my player", "my_player"),
            ("/opt/1app/2go", "_2go"),
            ("/home/a/.local/bin/Quod-Libet", "Quod-Libet"),
            ("/usr/bin/", "bin"),
            ("/", "app"),
            ("/usr/bin/é.x", "__x"),
        ] {
            assert_eq!(id_of(app), want, "{app}");
        }
        let long = format!("/usr/bin/{}", "x".repeat(200));
        assert_eq!(id_of(&long).len(), 48);
        let n = bus_name("/usr/bin/toy", 77);
        assert_eq!(n, "org.mpris.MediaPlayer2.wol.toy.instance77");
        assert!(
            zbus::names::WellKnownName::try_from(n.as_str()).is_ok(),
            "{n}"
        );
        assert!(n.starts_with(NAME_PREFIX));
        assert_ne!(n, bus_name("/usr/bin/toy", 78), "a process of its own");
        // Memory Saver reads the process from the instance suffix.
        let pid = n
            .rsplit_once(".instance")
            .and_then(|(_, p)| p.parse::<i32>().ok());
        assert_eq!(pid, Some(77));
    }

    #[test]
    fn remote_controls_follow_the_abilities() {
        for k in [
            MediaKey::Play,
            MediaKey::Pause,
            MediaKey::Toggle,
            MediaKey::Stop,
        ] {
            assert!(allowed(k, 0), "{k:?}");
        }
        assert!(!allowed(MediaKey::Next, 0) && allowed(MediaKey::Next, CAN_NEXT));
        assert!(
            !allowed(MediaKey::Previous, CAN_NEXT) && allowed(MediaKey::Previous, CAN_PREVIOUS)
        );
        for k in [MediaKey::Seek, MediaKey::SetPosition] {
            assert!(
                !allowed(k, CAN_NEXT | CAN_PREVIOUS) && allowed(k, CAN_SEEK),
                "{k:?}"
            );
        }
    }

    fn model(status: MediaStatus, position_us: i64, length_us: i64) -> Model {
        Model {
            identity: "Toy".into(),
            desktop_entry: String::new(),
            state: MediaState {
                status,
                title: "Song".into(),
                artist: "Band".into(),
                album: String::new(),
                art: String::new(),
                length_us,
                position_us,
                can: 0,
            },
            art: None,
            at: Instant::now(),
        }
    }

    #[test]
    fn the_desktop_counts_the_seconds_while_it_plays() {
        let later = |m: &Model, s: u64| m.at + std::time::Duration::from_secs(s);
        let playing = model(MediaStatus::Playing, 10_000_000, 100_000_000);
        assert_eq!(playing.position(later(&playing, 5)), 15_000_000);
        assert_eq!(
            playing.position(later(&playing, 500)),
            100_000_000,
            "not past the end"
        );
        let paused = model(MediaStatus::Paused, 10_000_000, 100_000_000);
        assert_eq!(
            paused.position(later(&paused, 5)),
            10_000_000,
            "paused counts nothing"
        );
        let stream = model(MediaStatus::Playing, 0, 0);
        assert_eq!(
            stream.position(later(&stream, 7)),
            7_000_000,
            "no end known"
        );
    }

    #[test]
    fn metadata_holds_only_what_the_app_gave() {
        let m = model(MediaStatus::Stopped, 0, 90_000_000).metadata();
        let mut keys: Vec<&str> = m.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "mpris:length",
                "mpris:trackid",
                "xesam:artist",
                "xesam:title"
            ]
        );
        assert_eq!(
            model(MediaStatus::Stopped, 0, 0).metadata().len(),
            3,
            "no length, no album, no art"
        );
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wol-media-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn art_is_a_small_picture_file_and_never_anything_else() {
        let d = scratch("art");
        let roots = vec![d.clone()];
        let art = |p: &str| art_url(p, &roots);
        let write = |name: &str, bytes: &[u8]| {
            let p = d.join(name);
            std::fs::write(&p, bytes).unwrap();
            p.display().to_string()
        };
        let png = write("cover art.png", b"\x89PNG\r\n\x1a\n\0\0\0\0\0\0");
        let url = art(&png).expect("a PNG");
        assert!(
            url.starts_with("file://") && url.ends_with("cover%20art.png"),
            "{url}"
        );
        assert_eq!(
            crate::widgets::media::art_path(&url),
            Some(PathBuf::from(&png).canonicalize().unwrap()),
            "the Music widget reads back the same path"
        );
        assert!(art(&write("a.jpg", b"\xff\xd8\xff\xe0\0\0\0\0\0\0\0\0")).is_some());
        assert!(art(&write("a.gif", b"GIF89a\0\0\0\0\0\0")).is_some());
        assert!(art(&write("a.webp", b"RIFF\0\0\0\0WEBP")).is_some());
        // A text file named like a picture, a secret, a folder, a huge file, a gone file.
        assert_eq!(art(&write("fake.png", b"password=hunter2 ......")), None);
        assert_eq!(art(&write("tiny.png", b"\x89PNG")), None);
        assert_eq!(art(&d.display().to_string()), None);
        assert_eq!(art("/etc/shadow"), None);
        assert_eq!(art(&d.join("gone.png").display().to_string()), None);
        let big = d.join("big.png");
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.resize(usize::try_from(MAX_ART).unwrap() + 1, 0);
        std::fs::write(&big, bytes).unwrap();
        assert_eq!(art(&big.display().to_string()), None);
    }

    #[test]
    fn art_stays_inside_its_roots_and_out_of_hidden_places() {
        let d = scratch("art-roots");
        let png = b"\x89PNG\r\n\x1a\n\0\0\0\0\0\0";
        let (home, elsewhere) = (d.join("home"), d.join("elsewhere"));
        for dir in [
            home.join("Music"),
            home.join(".ssh"),
            home.join(".cache/toy"),
            elsewhere.clone(),
        ] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(home.join("Music/cover.png"), png).unwrap();
        std::fs::write(home.join(".ssh/looks-like.png"), png).unwrap();
        std::fs::write(home.join(".cache/toy/cover.png"), png).unwrap();
        std::fs::write(elsewhere.join("cover.png"), png).unwrap();
        std::os::unix::fs::symlink(
            home.join(".ssh/looks-like.png"),
            home.join("Music/link.png"),
        )
        .unwrap();
        std::os::unix::fs::symlink(elsewhere.join("cover.png"), home.join("Music/out.png"))
            .unwrap();
        std::os::unix::fs::symlink(home.join(".ssh"), home.join("Music/dir")).unwrap();
        let roots = vec![home.clone()];
        let art = |p: PathBuf| art_url(&p.display().to_string(), &roots);
        assert!(art(home.join("Music/cover.png")).is_some());
        // Outside the roots, in a hidden folder (a cache too), and through a link to either.
        assert_eq!(art(elsewhere.join("cover.png")), None);
        assert_eq!(art(home.join(".ssh/looks-like.png")), None);
        assert_eq!(art(home.join(".cache/toy/cover.png")), None);
        assert_eq!(art(home.join("Music/link.png")), None, "a link into .ssh");
        assert_eq!(
            art(home.join("Music/out.png")),
            None,
            "a link out of the roots"
        );
        assert_eq!(art(home.join("Music/dir/looks-like.png")), None);
        assert_eq!(art(home.join("Music/../.ssh/looks-like.png")), None);
        // Refused alike whether it exists or not.
        assert_eq!(art(home.join(".ssh/nothing.png")), None);
        assert_eq!(
            canonical_within(&home.join("Music/cover.png"), &roots).unwrap(),
            home.join("Music/cover.png").canonicalize().unwrap()
        );
    }

    #[test]
    fn a_program_borrows_an_identity_only_by_its_real_path() {
        use crate::desktop::Entry;
        let d = scratch("who");
        let (bin, evil) = (d.join("usr/bin"), d.join("tmp/x"));
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&evil).unwrap();
        for p in [bin.join("firefox"), evil.join("firefox")] {
            std::fs::write(p, b"#!/bin/sh\n").unwrap();
        }
        std::os::unix::fs::symlink(bin.join("firefox"), d.join("ff-link")).unwrap();
        let entry = |id: &str, name: &str, exec: &str| Entry {
            id: id.into(),
            name: name.into(),
            exec: vec![exec.into()],
            ..Entry::default()
        };
        let entries = vec![
            entry("org.mozilla.firefox.desktop", "Firefox", "firefox"),
            entry(
                "other.desktop",
                "Other",
                &evil.join("nothing").display().to_string(),
            ),
        ];
        let dirs = vec![bin.clone()];
        let who = |app: &Path| identity(&entries, &app.display().to_string(), &dirs);
        // The real one, by PATH and through a link to it.
        let right = ("Firefox".to_owned(), "org.mozilla.firefox".to_owned());
        assert_eq!(who(&bin.join("firefox")), right);
        assert_eq!(who(&d.join("ff-link")), right);
        // Same file name elsewhere: no borrowed name, entry or icon.
        let fake = who(&evil.join("firefox"));
        assert_eq!(fake, ("firefox".to_owned(), String::new()));
        // An entry with a relative program, or none that exists, matches nothing.
        let relative = vec![entry("r.desktop", "R", "./firefox")];
        assert!(entry_of(&relative, &bin.join("firefox").display().to_string(), &dirs).is_none());
        assert!(entry_of(&entries, "/no/such/program", &dirs).is_none());
    }

    #[test]
    fn a_hidden_part_is_found_anywhere_in_a_path() {
        for (p, want) in [
            ("/home/a/Documents/r.pdf", false),
            ("/home/a/.cache/x", true),
            ("/home/a/Documents/.r.pdf", true),
            ("/home/a/my.docs/r.pdf", false),
            ("/home/.a/r.pdf", true),
            ("/home/a/Documents/../.ssh/k", true),
        ] {
            assert_eq!(hidden(Path::new(p)), want, "{p}");
        }
    }
}
