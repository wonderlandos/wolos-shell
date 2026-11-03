// SPDX-License-Identifier: GPL-2.0-or-later
//! org.freedesktop.Notifications server (zbus) + banners (top right, 5 s) + Notification Center panel.
use crate::copy;
use crate::kit::{
    surface::{Keyboard, Spec},
    widgets as w,
};
use crate::offers::{self, Offer};
use gtk::{gio, glib, prelude::*};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedValue;

const PATH: &str = "/org/freedesktop/Notifications";
const NAME: &str = "org.freedesktop.Notifications";

#[derive(Clone, Debug)]
pub struct Note {
    pub id: u32,
    pub app: String,
    pub icon: String,
    pub summary: String,
    pub body: String,
    pub critical: bool,
    pub timeout_ms: i32,
    /// A picture file shown as the banner's thumbnail (`image-path` hint), or empty.
    pub image: String,
    /// (key, label) pairs from the client; see [`buttons`] and [`click_action`].
    pub actions: Vec<(String, String)>,
}

/// Answers note `id`: with an action key (ActionInvoked), or None (dismissed).
type Answer = dyn Fn(u32, Option<&str>);

/// Notices from the system itself (`notify-send -a WolOS`) carry the WolOS mark.
const SYSTEM_APP: &str = "WolOS";
const SYSTEM_ICON: &str = "wolos-mark";

/// The action a click on the banner runs (spec: key "default").
pub const DEFAULT_ACTION: &str = "default";
/// Buttons a banner shows at most.
pub const MAX_BUTTONS: usize = 2;

/// Notify's flat action list (key, label, key, label…) as pairs; a dangling key is dropped.
pub fn parse_actions(flat: &[String]) -> Vec<(String, String)> {
    flat.as_chunks::<2>()
        .0
        .iter()
        .map(|[key, label]| (key.clone(), label.clone()))
        .collect()
}

/// The banner's buttons: the first [`MAX_BUTTONS`] actions other than the default one.
pub fn buttons(n: &Note) -> Vec<&(String, String)> {
    n.actions
        .iter()
        .filter(|a| a.0 != DEFAULT_ACTION)
        .take(MAX_BUTTONS)
        .collect()
}

/// What a click on the banner does: the default action when the client gave one, else dismiss.
pub fn click_action(n: &Note) -> Option<&'static str> {
    n.actions
        .iter()
        .any(|a| a.0 == DEFAULT_ACTION)
        .then_some(DEFAULT_ACTION)
}

/// Server -> UI.
#[derive(Clone, Debug)]
pub enum Event {
    Show(Note),
    /// CloseNotification from a client: drop the banner (signal already sent).
    Close(u32),
    /// Esc (`wol shell hide banners`): every banner on screen leaves, dismissed.
    DismissAll,
    /// A banner held while a window was in full screen, shown now (already in the center).
    Replay(Note),
}

/// Horizontal travel (px) that swipes a banner away: a drag toward the screen edge, or a
/// two-finger swipe either way (natural scrolling flips the sign).
const SWIPE: f64 = 64.0;

/// A drag of `dx` px (right = toward the edge) or a touchpad swipe of `scroll` px dismisses.
pub fn swiped(dx: f64, scroll: f64) -> bool {
    dx >= SWIPE || scroll.abs() >= SWIPE
}

/// The banner follows a drag to the right and leaves past [`SWIPE`]; a two-finger swipe too.
fn swipe_away(rev: &gtk::Revealer, done: impl Fn() + 'static) {
    let done = Rc::new(done);
    let drag = gtk::GestureDrag::new();
    {
        let r = rev.clone();
        drag.connect_drag_update(move |_, dx, _| r.set_margin_start(dx.max(0.0) as i32));
    }
    {
        let (r, done) = (rev.clone(), done.clone());
        drag.connect_drag_end(move |_, dx, _| {
            r.set_margin_start(0);
            if swiped(dx, 0.0) {
                done();
            }
        });
    }
    rev.add_controller(drag);
    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::HORIZONTAL);
    let travel = Rc::new(std::cell::Cell::new(0.0));
    {
        let travel = travel.clone();
        scroll.connect_scroll_begin(move |_| travel.set(0.0));
    }
    scroll.connect_scroll(move |_, dx, _| {
        travel.set(travel.get() + dx);
        if swiped(0.0, travel.get()) {
            travel.set(0.0);
            done();
        }
        glib::Propagation::Stop
    });
    rev.add_controller(scroll);
}

/// NotificationClosed reasons (Desktop Notifications spec 1.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum CloseReason {
    Expired = 1,
    Dismissed = 2,
    Closed = 3,
}

type Open = Arc<Mutex<HashSet<u32>>>;

/// Takes `id` out of the open set; true when it was open (signal it exactly once).
fn take(open: &Open, id: u32) -> bool {
    open.lock().map(|mut o| o.remove(&id)).unwrap_or(false)
}

struct Server {
    tx: async_channel::Sender<Event>,
    next: AtomicU32,
    open: Open,
}

/// The live bus connection: kept for the process lifetime to emit NotificationClosed.
#[derive(Clone)]
pub struct Bus {
    conn: zbus::Connection,
    open: Open,
}

impl Bus {
    /// Owns `org.freedesktop.Notifications` on `address` (None: session bus).
    pub async fn serve(
        address: Option<&str>,
        tx: async_channel::Sender<Event>,
    ) -> zbus::Result<Bus> {
        let open = Open::default();
        let server = Server {
            tx,
            next: AtomicU32::new(1),
            open: open.clone(),
        };
        let b = match address {
            Some(a) => zbus::connection::Builder::address(a)?,
            None => zbus::connection::Builder::session()?,
        };
        let conn = b.name(NAME)?.serve_at(PATH, server)?.build().await?;
        Ok(Bus { conn, open })
    }

    /// Emits NotificationClosed once per notification; false when it was already closed.
    pub async fn close(&self, id: u32, reason: CloseReason) -> zbus::Result<bool> {
        if !take(&self.open, id) {
            return Ok(false);
        }
        let e = SignalEmitter::new(&self.conn, PATH)?;
        Server::notification_closed(&e, id, reason as u32).await?;
        Ok(true)
    }

    /// A button or the banner was clicked: ActionInvoked, then NotificationClosed (dismissed).
    /// False when the notification was already closed.
    pub async fn invoke(&self, id: u32, key: &str) -> zbus::Result<bool> {
        if !take(&self.open, id) {
            return Ok(false);
        }
        let e = SignalEmitter::new(&self.conn, PATH)?;
        Server::action_invoked(&e, id, key).await?;
        Server::notification_closed(&e, id, CloseReason::Dismissed as u32).await?;
        Ok(true)
    }
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Server {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let id = if replaces_id != 0 {
            replaces_id
        } else {
            self.next.fetch_add(1, Ordering::Relaxed)
        };
        let critical = hints.get("urgency").and_then(|v| u8::try_from(v).ok()) == Some(2);
        if let Ok(mut o) = self.open.lock() {
            o.insert(id);
        }
        let _ = self.tx.try_send(Event::Show(Note {
            id,
            app: app_name,
            icon: app_icon,
            summary,
            body,
            critical,
            timeout_ms: expire_timeout,
            image: image_hint(&hints),
            actions: parse_actions(&actions),
        }));
        id
    }
    async fn close_notification(
        &self,
        id: u32,
        #[zbus(signal_emitter)] e: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        if take(&self.open, id) {
            Self::notification_closed(&e, id, CloseReason::Closed as u32).await?;
            let _ = self.tx.try_send(Event::Close(id));
        }
        Ok(())
    }
    #[zbus(signal)]
    async fn notification_closed(e: &SignalEmitter<'_>, id: u32, reason: u32) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn action_invoked(e: &SignalEmitter<'_>, id: u32, action_key: &str) -> zbus::Result<()>;
    fn get_capabilities(&self) -> Vec<String> {
        vec!["actions".into(), "body".into(), "persistence".into()]
    }
    fn get_server_information(&self) -> (String, String, String, String) {
        (
            "wol-shell".into(),
            "WolOS".into(),
            env!("CARGO_PKG_VERSION").into(),
            "1.2".into(),
        )
    }
}

/// Banner lifetime: server default 5 s, -1 = default, 0 = until clicked; critical and banners
/// with buttons stay until answered or dismissed (macOS alerts).
pub fn banner_ms(n: &Note) -> Option<u32> {
    match (n.critical || !buttons(n).is_empty(), n.timeout_ms) {
        (true, _) | (_, 0) => None,
        (_, t) if t > 0 => Some(t as u32),
        _ => Some(5000),
    }
}

/// Focus (Do Not Disturb) keeps the banner away; the note still lands in Notification Center.
pub fn quiet(n: &Note, focus: crate::focus::Focus) -> bool {
    focus.active() && !n.critical
}

/// Whether `n`'s banner waits (audit3 R2A-9): the active workspace is in real full screen (a
/// game, a video) and the note is not critical. A banner mapped then draws over the game and
/// stops its direct scanout; the note is in Notification Center already and its banner shows
/// when full screen ends. A zoomed window holds nothing.
pub fn held(mode: crate::hypr::Fullscreen, n: &Note) -> bool {
    mode.holds_banners() && !n.critical
}

/// Most banners kept for after full screen (the oldest go; they stay in the center).
pub const HELD_MAX: usize = 8;

/// The banners [`held`] back, oldest first, replayed when full screen ends.
#[derive(Default)]
pub struct Held {
    notes: Vec<Note>,
}

impl Held {
    pub fn hold(&mut self, n: Note) {
        self.notes.retain(|x| x.id != n.id);
        self.notes.push(n);
        if self.notes.len() > HELD_MAX {
            self.notes.remove(0);
        }
    }

    /// The app withdrew note `id`: it never shows.
    pub fn forget(&mut self, id: u32) {
        self.notes.retain(|x| x.id != id);
    }

    pub fn take(&mut self) -> Vec<Note> {
        std::mem::take(&mut self.notes)
    }

    /// Dismiss all or the Center's Clear: nothing comes back after full screen (audit5 S6-1).
    pub fn clear(&mut self) {
        self.notes.clear();
    }

    /// The banners to replay now that the mode is `mode`: all of them once full screen ended,
    /// none while it lasts.
    pub fn release(&mut self, mode: crate::hypr::Fullscreen) -> Vec<Note> {
        if mode.holds_banners() {
            return Vec::new();
        }
        self.take()
    }
}

/// One app's banner group: its notes oldest first, the newest on screen. A timed note's expiry
/// takes the timed notes only: a sticky one (buttons, critical, no timeout) stays and shows again
/// (audit4 S3-4: it was closed as Expired with the plain note stacked over it).
#[derive(Default, Debug)]
pub struct GroupNotes {
    notes: Vec<Note>,
}

impl GroupNotes {
    /// Puts `n` on screen (a replaced id moves to the top with its new text).
    pub fn push(&mut self, n: Note) {
        self.notes.retain(|x| x.id != n.id);
        self.notes.push(n);
    }

    pub fn contains(&self, id: u32) -> bool {
        self.notes.iter().any(|n| n.id == id)
    }

    pub fn ids(&self) -> Vec<u32> {
        self.notes.iter().map(|n| n.id).collect()
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    /// The note on screen.
    pub fn shown(&self) -> Option<&Note> {
        self.notes.last()
    }

    /// The shown note's timer ran out: the notes with a timeout leave (returned, to close as
    /// Expired); the sticky ones stay, the newest of them on screen.
    pub fn expire(&mut self) -> Vec<u32> {
        let (gone, keep): (Vec<Note>, Vec<Note>) = std::mem::take(&mut self.notes)
            .into_iter()
            .partition(|n| banner_ms(n).is_some());
        self.notes = keep;
        gone.iter().map(|n| n.id).collect()
    }

    /// The app withdrew `id`; true when it was the one on screen (show the next one).
    pub fn remove(&mut self, id: u32) -> bool {
        let was_shown = self.shown().is_some_and(|n| n.id == id);
        self.notes.retain(|n| n.id != id);
        was_shown
    }

    /// The shown note's banner time, when it has one.
    pub fn shown_ms(&self) -> Option<u32> {
        self.shown().and_then(banner_ms)
    }

    /// The app withdrew `id`. `Some(timer)` when the withdrawn one was on screen and the group
    /// shows another note now: `timer` is that note's banner time (None = it stays until
    /// answered). The note re-shown needs its own timer, the one it had ran out unseen
    /// (audit6 S6-1).
    pub fn withdraw(&mut self, id: u32) -> Option<Option<u32>> {
        let reshown = self.remove(id) && !self.is_empty();
        reshown.then(|| self.shown_ms())
    }
}

/// Where a new banner goes, given the banners on screen as (app, stacked count), top first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// New group at the top.
    Top,
    /// Same app already showing: replace that banner, stack count becomes `count`.
    Stack { index: usize, count: u32 },
}

pub fn place(shown: &[(String, u32)], app: &str) -> Place {
    match shown
        .iter()
        .position(|(a, _)| !app.is_empty() && a.eq_ignore_ascii_case(app))
    {
        Some(index) => Place::Stack {
            index,
            count: shown[index].1 + 1,
        },
        None => Place::Top,
    }
}

/// Notification Center row (same layout as a banner, no stack). No app buttons: the banner with
/// buttons stays until answered, and an answered note takes no second answer. The offers stay
/// ([`crate::offers`]: a code can still be copied from the list); the card never closes itself.
fn card(n: &Note) -> gtk::Box {
    banner_offering(
        &Note {
            actions: vec![],
            ..n.clone()
        },
        1,
        Rc::new(|_: &str| {}),
        Rc::new(|o: &Offer, b: &gtk::Button, _| take_offer(o, b, None)),
    )
}

/// Notes the Notification Center keeps; older ones fall off the bottom (audit2 A2-8: the list
/// grew for the whole session).
pub const CENTER_MAX: usize = 50;

/// Notification Center rows by note id, newest first, at most [`CENTER_MAX`]. `T` is the row
/// widget (a plain value in tests).
#[derive(Debug)]
pub struct Center<T> {
    rows: Vec<(u32, T)>,
}

impl<T> Default for Center<T> {
    fn default() -> Self {
        Center { rows: Vec::new() }
    }
}

impl<T> Center<T> {
    /// Puts note `id` on top. Returns the rows to take off the list: the note it replaces
    /// (`replaces_id`) and the oldest beyond [`CENTER_MAX`].
    pub fn push(&mut self, id: u32, row: T) -> Vec<T> {
        let mut gone: Vec<T> = self.remove(id).into_iter().collect();
        self.rows.insert(0, (id, row));
        while self.rows.len() > CENTER_MAX {
            if let Some((_, r)) = self.rows.pop() {
                gone.push(r);
            }
        }
        gone
    }

    /// Note `id` was answered or withdrawn by its app: its row, to take off the list.
    pub fn remove(&mut self, id: u32) -> Option<T> {
        let i = self.rows.iter().position(|(x, _)| *x == id)?;
        Some(self.rows.remove(i).1)
    }

    /// Clear: every row.
    pub fn clear(&mut self) -> Vec<T> {
        self.rows.drain(..).map(|(_, r)| r).collect()
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// The Notification Center's content: title, Clear, and the list `rows` fills (no golden yet).
/// `on_clear` runs after Clear (the held banners go too, audit5 S6-1).
pub fn center_panel(
    rows: Rc<RefCell<Center<gtk::Box>>>,
    on_clear: impl Fn() + 'static,
) -> (gtk::Box, gtk::Box) {
    let list = w::vbox(0);
    let g = w::glass_panel(w::S2);
    let head = w::hbox(w::S2);
    head.append(&w::label("Notifications", "title"));
    head.append(&w::spacer());
    let l2 = list.clone();
    head.append(&w::button("Clear", w::Kind::Default, move || {
        for r in rows.borrow_mut().clear() {
            l2.remove(&r);
        }
        on_clear();
    }));
    g.append(&head);
    g.append(
        &gtk::ScrolledWindow::builder()
            .child(&list)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build(),
    );
    (g, list)
}

/// The `image-path` hint (older `image_path`) when it names a picture file: the thumbnail.
pub fn image_hint(hints: &HashMap<String, OwnedValue>) -> String {
    ["image-path", "image_path"]
        .iter()
        .filter_map(|k| hints.get(*k).and_then(|v| <&str>::try_from(v).ok()))
        .find_map(|p| match p {
            p if p.starts_with('/') => Some(p.to_string()),
            p if p.starts_with("file://") => glib::filename_from_uri(p)
                .ok()
                .map(|(f, _)| f.to_string_lossy().into_owned()),
            _ => None,
        })
        .unwrap_or_default()
}

/// Shorter side (px) a banner picture or a file icon is decoded at: 2x the thumbnail, for HiDPI.
pub const THUMB_PX: i32 = 2 * w::ICON_L;
/// Biggest picture file a banner decodes (audit4 S3-3: any app can name any file).
pub const THUMB_FILE_MAX: u64 = 64 * 1024 * 1024;
/// Decoded thumbnails kept: every Notification Center row's picture, and a few more.
const THUMBS_KEPT: usize = CENTER_MAX + 8;

/// Small textures by file, shared by a banner and its Notification Center card (audit4 S3-3:
/// each decoded the full-size picture on the UI thread and kept it).
static THUMBS: Mutex<Vec<(String, gtk::gdk::Texture)>> = Mutex::new(Vec::new());

/// Decodes the picture at `path` with its shorter side at most [`THUMB_PX`] (the banner covers
/// its box with it) and its longer side at most [`THUMB_LONG_MAX`]; None for a file that
/// is missing, not a regular file, bigger than [`THUMB_FILE_MAX`] or not a picture. Blocking:
/// called off the UI thread ([`preload_thumbs`]), or for a golden render.
pub fn decode_thumb(path: &str) -> Option<gtk::gdk::Texture> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > THUMB_FILE_MAX {
        return None;
    }
    let (_, w, h) = gtk::gdk_pixbuf::Pixbuf::file_info(path)?;
    let (box_w, box_h) = thumb_box(w, h);
    let pb = gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(path, box_w, box_h, true).ok()?;
    Some(gtk::gdk::Texture::for_pixbuf(&pb))
}

/// Longest side of a decoded thumbnail (a panorama keeps a short side under [`THUMB_PX`]).
pub const THUMB_LONG_MAX: i32 = 4 * THUMB_PX;

/// The box a `w`x`h` picture is scaled into (aspect kept): its shorter side [`THUMB_PX`].
fn thumb_box(w: i32, h: i32) -> (i32, i32) {
    let (w, h) = (i64::from(w.max(1)), i64::from(h.max(1)));
    let px = i64::from(THUMB_PX);
    let long = i64::from(THUMB_LONG_MAX);
    let side = |a: i64, b: i64| i32::try_from((px * a / b).clamp(1, long)).unwrap_or(THUMB_PX);
    if w >= h {
        (side(w, h), THUMB_PX)
    } else {
        (THUMB_PX, side(h, w))
    }
}

fn cached_thumb(path: &str) -> Option<gtk::gdk::Texture> {
    let t = THUMBS.lock().ok()?;
    t.iter().find(|(p, _)| p == path).map(|(_, t)| t.clone())
}

fn keep_thumb(path: &str, t: gtk::gdk::Texture) {
    if let Ok(mut c) = THUMBS.lock() {
        c.retain(|(p, _)| p != path);
        if c.len() >= THUMBS_KEPT {
            c.remove(0);
        }
        c.push((path.to_string(), t));
    }
}

/// The files `n` shows as pictures: its `image-path` and an icon given as a file.
fn thumb_files(n: &Note) -> Vec<String> {
    [&n.image, &n.icon]
        .into_iter()
        .filter(|p| p.starts_with('/'))
        .cloned()
        .collect()
}

/// Decodes `n`'s pictures off the UI thread, once per file, before its banner and card are built.
async fn preload_thumbs(n: &Note) {
    for p in thumb_files(n) {
        if cached_thumb(&p).is_some() {
            continue;
        }
        let q = p.clone();
        if let Ok(Some(t)) = gtk::gio::spawn_blocking(move || decode_thumb(&q)).await {
            keep_thumb(&p, t);
        }
    }
}

/// The thumbnail of `path` from the cache. A golden render (WOL_FAKE_NOW) decodes it here; live
/// banners were preloaded, and a picture that could not be decoded stays empty.
fn thumb(path: &str) -> Option<gtk::gdk::Texture> {
    if let Some(t) = cached_thumb(path) {
        return Some(t);
    }
    // Only a golden render gets past here.
    std::env::var_os("WOL_FAKE_NOW")?;
    let t = decode_thumb(path)?;
    keep_thumb(path, t.clone());
    Some(t)
}

/// The icon of a note without `app_icon`: the dock pin, else the installed app whose entry, name
/// or program is `app` (audit4 S3-8), else the app name as an icon name.
pub fn note_icon(app: &str, installed: &[crate::desktop::Entry]) -> String {
    if app == SYSTEM_APP {
        return SYSTEM_ICON.into();
    }
    let icon = crate::apps::icon_for(app, crate::apps::here(), installed);
    if icon != app.to_lowercase() {
        return icon;
    }
    // Notify's app_name is what people read ("Signal"), not the entry id (signal-desktop).
    installed
        .iter()
        .find(|e| !e.icon.is_empty() && e.name.eq_ignore_ascii_case(app))
        .map_or(icon, |e| e.icon.clone())
}

/// macOS banner: 344 px glass card, app icon, app name + "now", bold title, body up to 2 lines.
/// `count` > 1 draws the group's stacked edges under the card. Buttons do nothing here (golden,
/// Notification Center); [`banner_with`] wires them.
pub fn banner(n: &Note, count: u32) -> gtk::Box {
    banner_with(n, count, Rc::new(|_: &str| {}))
}

/// [`banner`] whose buttons run `act(key)`; offer buttons draw but do nothing.
pub fn banner_with(n: &Note, count: u32, act: Rc<dyn Fn(&str)>) -> gtk::Box {
    banner_offering(n, count, act, Rc::new(|_: &Offer, _: &gtk::Button, _| {}))
}

/// What an offer button runs: the offer, the button (for its "Copied" label) and whether
/// copying it finishes the note ([`crate::offers::Offers::closes_after_copy`]).
pub type Take = dyn Fn(&Offer, &gtk::Button, bool);

/// [`banner_with`] plus one row of offer buttons ([`crate::offers::scan`] of the title and body:
/// Copy Code, Open Link, Copy Number) that run `take`.
pub fn banner_offering(n: &Note, count: u32, act: Rc<dyn Fn(&str)>, take: Rc<Take>) -> gtk::Box {
    let outer = w::vbox(0);
    outer.add_css_class("banner-group");
    let c = w::glass_panel(0);
    c.add_css_class("notification");
    if n.critical {
        c.add_css_class("critical");
    }
    let row = w::hbox(w::S3);
    // Live notes come with the icon resolved against the installed apps ([`note_icon`]).
    let icon = if n.icon.is_empty() {
        note_icon(&n.app, &[])
    } else {
        n.icon.clone()
    };
    let img = if icon.starts_with('/') {
        let i = gtk::Image::new();
        i.set_paintable(thumb(&icon).as_ref());
        i.set_pixel_size(w::ICON_M);
        i
    } else {
        w::app_icon(&icon, w::ICON_M)
    };
    row.append(&img);
    let text = w::vbox(0);
    let head = w::hbox(w::S2);
    let app = w::label(&n.app.to_uppercase(), "banner-app");
    app.set_hexpand(true);
    app.set_max_width_chars(1);
    app.set_ellipsize(gtk::pango::EllipsizeMode::End);
    app.add_css_class("may-ellipsize");
    head.append(&app);
    head.append(&w::label("now", "banner-time"));
    text.append(&head);
    let title = w::label(&n.summary, "banner-title");
    title.add_css_class("may-ellipsize");
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_max_width_chars(1);
    text.append(&title);
    if !n.body.is_empty() {
        let b = w::label(&n.body, "banner-body");
        b.set_wrap(true);
        b.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        b.set_lines(2);
        b.set_ellipsize(gtk::pango::EllipsizeMode::End);
        b.add_css_class("may-ellipsize");
        b.set_max_width_chars(1);
        text.append(&b);
    }
    if count > 1 {
        text.append(&w::label(
            &format!("{} more notifications", count - 1),
            "banner-more",
        ));
    }
    let found = crate::offers::scan(&n.summary, &n.body);
    let offered = found.buttons();
    if !offered.is_empty() {
        let bar = w::hbox(w::S2);
        bar.add_css_class("banner-actions");
        bar.set_halign(gtk::Align::End);
        for o in offered {
            let b = w::button(
                &copy::offer_label(&o, found.codes.len() > 1),
                w::Kind::Default,
                || {},
            );
            b.set_tooltip_text(Some(&copy::offer_tip(&o)));
            let closes = matches!(&o, Offer::Code(c) if found.closes_after_copy(c));
            let take = take.clone();
            b.connect_clicked(move |b| take(&o, b, closes));
            bar.append(&b);
        }
        text.append(&bar);
    }
    let actions = buttons(n);
    if !actions.is_empty() {
        let bar = w::hbox(w::S2);
        bar.add_css_class("banner-actions");
        bar.set_halign(gtk::Align::End);
        for (key, label) in actions {
            let (act, key) = (act.clone(), key.clone());
            bar.append(&w::button(label, w::Kind::Default, move || act(&key)));
        }
        text.append(&bar);
    }
    row.append(&text);
    if !n.image.is_empty() {
        // A screenshot's banner shows the picture, as on macOS: a small copy decoded off the UI
        // thread once, shared with the Notification Center card (audit4 S3-3).
        let pic = gtk::Picture::new();
        pic.set_paintable(thumb(&n.image).as_ref());
        pic.set_content_fit(gtk::ContentFit::Cover);
        pic.set_size_request(w::ICON_L, w::ICON_L);
        pic.set_valign(gtk::Align::Center);
        pic.set_overflow(gtk::Overflow::Hidden);
        pic.add_css_class("banner-thumb");
        row.append(&pic);
    }
    c.append(&row);
    outer.append(&c);
    for edge in ["banner-edge-1", "banner-edge-2"]
        .iter()
        .take((count as usize).saturating_sub(1).min(2))
    {
        let e = w::vbox(0);
        e.add_css_class("banner-edge");
        e.add_css_class(edge);
        outer.append(&e);
    }
    outer
}

/// Runs an offer button. A code is copied as sensitive off the GTK thread and cleared after
/// [`offers::CLEAR_AFTER`] if still on the clipboard; `close` (the banner whose only code it
/// was) runs [`offers::CLOSE_AFTER`] after the copy, so the "Copied" label is seen. A number is
/// a plain copy; a link opens in the browser after one more [`offers::safe_https_url`] check.
fn take_offer(o: &Offer, b: &gtk::Button, close: Option<Rc<dyn Fn()>>) {
    let b = b.clone();
    match o.clone() {
        Offer::Code(code) => {
            glib::spawn_future_local(async move {
                let copied = gio::spawn_blocking(move || {
                    offers::copy_code(&mut offers::Wayland::default(), &offers::SECRETS, &code)
                })
                .await;
                match copied {
                    Ok(Ok(ticket)) => {
                        b.set_label(copy::OFFER_COPIED);
                        glib::timeout_add_local_once(offers::CLEAR_AFTER, move || {
                            gio::spawn_blocking(move || {
                                let mut clip = offers::Wayland::default();
                                if let Err(e) = offers::expire(&mut clip, &offers::SECRETS, &ticket)
                                {
                                    eprintln!("wol-shell: clearing a copied code: {e}");
                                }
                            });
                        });
                        if let Some(close) = close {
                            glib::timeout_add_local_once(offers::CLOSE_AFTER, move || close());
                        }
                    }
                    Ok(Err(e)) => {
                        eprintln!("wol-shell: copying a code: {e}");
                        b.set_label(copy::OFFER_NOT_COPIED);
                    }
                    Err(_) => b.set_label(copy::OFFER_NOT_COPIED),
                }
            });
        }
        Offer::Phone(number) => {
            glib::spawn_future_local(async move {
                let copied = gio::spawn_blocking(move || {
                    offers::copy_number(&mut offers::Wayland::default(), &number)
                })
                .await;
                match copied {
                    Ok(Ok(())) => b.set_label(copy::OFFER_COPIED),
                    Ok(Err(e)) => {
                        eprintln!("wol-shell: copying a number: {e}");
                        b.set_label(copy::OFFER_NOT_COPIED);
                    }
                    Err(_) => b.set_label(copy::OFFER_NOT_COPIED),
                }
            });
        }
        Offer::Link(url) => {
            if let Some(url) = offers::safe_https_url(&url)
                && let Err(e) =
                    gio::AppInfo::launch_default_for_uri(&url, None::<&gio::AppLaunchContext>)
            {
                eprintln!("wol-shell: opening a link: {e}");
            }
        }
    }
}

/// Starts the D-Bus server; returns the Notification Center panel and "dismiss every banner".
pub fn start(app: &gtk::Application) -> (gtk::Window, Rc<dyn Fn()>) {
    let (tx, rx) = async_channel::unbounded::<Event>();
    let dismiss = {
        let tx = tx.clone();
        Rc::new(move || {
            let _ = tx.try_send(Event::DismissAll);
        })
    };
    // Held while full screen; replayed (in order) when it ends.
    let held_back: Rc<RefCell<Held>> = Default::default();
    {
        let (held_back, tx) = (held_back.clone(), tx.clone());
        crate::hypr::on_fullscreen_change(move |mode| {
            for n in held_back.borrow_mut().release(mode) {
                let _ = tx.try_send(Event::Replay(n));
            }
        });
    }
    let bus: Rc<std::cell::OnceCell<Bus>> = Default::default();
    let b2 = bus.clone();
    glib::spawn_future_local(async move {
        match Bus::serve(None, tx).await {
            Ok(b) => {
                let _ = b2.set(b);
            }
            Err(e) => eprintln!("wol-shell: notifications: {e}"),
        }
    });
    let invoke = {
        let bus = bus.clone();
        move |id: u32, key: String| {
            if let Some(b) = bus.get().cloned() {
                glib::spawn_future_local(async move {
                    if let Err(e) = b.invoke(id, &key).await {
                        eprintln!("wol-shell: ActionInvoked: {e}");
                    }
                });
            }
        }
    };
    let closed = move |id: u32, reason: CloseReason| {
        if let Some(b) = bus.get().cloned() {
            glib::spawn_future_local(async move {
                if let Err(e) = b.close(id, reason).await {
                    eprintln!("wol-shell: NotificationClosed: {e}");
                }
            });
        }
    };

    // Top layer (kit::surface::Spec::banners); held while a window is in full screen ([`held`]).
    let banners = super::window(app, "banners", Spec::banners());
    let stack = w::vbox(0);
    crate::kit::surface::survive_output_loss(&banners, Spec::banners());
    stack.add_css_class("banner-stack");
    // Width first: a Fill-height stack would ask the 2-line bodies for width-for-height and widen.
    stack.set_valign(gtk::Align::Start);
    stack.set_halign(gtk::Align::End);
    banners.set_child(Some(&stack));
    let rows: Rc<RefCell<Center<gtk::Box>>> = Default::default();
    let (g, center_list) = {
        let held_back = held_back.clone();
        center_panel(rows.clone(), move || held_back.borrow_mut().clear())
    };
    // Takes note `id`'s row off the Notification Center (answered, or withdrawn by its app).
    let unlist = {
        let (rows, center_list, held_back) = (rows.clone(), center_list.clone(), held_back.clone());
        move |id: u32| {
            // Off the center: never replayed either (audit5 S6-1).
            held_back.borrow_mut().forget(id);
            if let Some(r) = rows.borrow_mut().remove(id) {
                center_list.remove(&r);
            }
        }
    };
    let center = super::window(
        app,
        "panel",
        Spec::panel(
            "wol-panel-notifications",
            [true, true, false, true],
            Keyboard::OnDemand,
        ),
    );
    crate::kit::surface::survive_output_loss(
        &center,
        Spec::panel(
            "wol-panel-notifications",
            [true, true, false, true],
            Keyboard::OnDemand,
        ),
    );
    center.add_css_class("notifications");
    center.set_child(Some(&g));
    // Esc closes Notification Center, like every other panel.
    let esc = gtk::EventControllerKey::new();
    {
        let c = center.clone();
        esc.connect_key_pressed(move |_, k, _, _| {
            if k == gtk::gdk::Key::Escape {
                c.set_visible(false);
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    center.add_controller(esc);

    glib::spawn_future_local(async move {
        // One banner per app, newest group on top; a group's notes leave together, except the
        // sticky ones a timed note's expiry leaves on screen ([`GroupNotes::expire`]).
        struct Group {
            app: String,
            notes: GroupNotes,
            rev: gtk::Revealer,
            generation: u32,
            /// The note on screen and what a click on it runs ([`click_action`]).
            last: (u32, Option<&'static str>),
        }
        let groups: Rc<RefCell<Vec<Group>>> = Default::default();
        let next_gen = Rc::new(std::cell::Cell::new(0u32));
        // Removes the group holding `rev`; returns its ids.
        let drop_group = {
            let (groups, stack, banners) = (groups.clone(), stack.clone(), banners.clone());
            move |rev: &gtk::Revealer| -> Vec<u32> {
                let mut g = groups.borrow_mut();
                let Some(i) = g.iter().position(|x| &x.rev == rev) else {
                    return vec![];
                };
                let gr = g.remove(i);
                stack.remove(&gr.rev);
                if g.is_empty() {
                    banners.set_visible(false);
                }
                gr.notes.ids()
            }
        };
        // Answers note `id`'s group: `key` goes to `id` (ActionInvoked), the rest are dismissed.
        let answer: Rc<Answer> = {
            let (groups, drop_group, closed, unlist) = (
                groups.clone(),
                drop_group.clone(),
                closed.clone(),
                unlist.clone(),
            );
            Rc::new(move |id, key| {
                let rev = groups
                    .borrow()
                    .iter()
                    .find(|g| g.notes.contains(id))
                    .map(|g| g.rev.clone());
                let Some(rev) = rev else { return };
                for i in drop_group(&rev) {
                    match key {
                        Some(k) if i == id => {
                            // Answered: not left in the center as if unread.
                            unlist(i);
                            invoke(i, k.to_string())
                        }
                        _ => closed(i, CloseReason::Dismissed),
                    }
                }
            })
        };
        // Draws the group's newest note in its banner, with the stack count and its buttons.
        let render = {
            let answer = answer.clone();
            Rc::new(move |gr: &mut Group| {
                let Some(n) = gr.notes.shown() else { return };
                let (id, a) = (n.id, answer.clone());
                let act: Rc<dyn Fn(&str)> = Rc::new(move |k: &str| a(id, Some(k)));
                let a = answer.clone();
                let take: Rc<Take> = Rc::new(move |o: &Offer, b: &gtk::Button, closes: bool| {
                    let a = a.clone();
                    let close: Option<Rc<dyn Fn()>> =
                        closes.then(|| Rc::new(move || a(id, None)) as Rc<dyn Fn()>);
                    take_offer(o, b, close)
                });
                gr.rev
                    .set_child(Some(&banner_offering(n, gr.notes.len() as u32, act, take)));
                gr.last = (id, click_action(n));
            })
        };
        // Starts the banner timer of the group holding `rev`: when it runs out and `generation`
        // is still the group's, the timed notes expire (audit6 S6-1: one place for every arm).
        let arm = {
            let (groups, drop_group, closed, render) = (
                groups.clone(),
                drop_group.clone(),
                closed.clone(),
                render.clone(),
            );
            move |rev: gtk::Revealer, generation: u32, ms: u32| {
                let (groups, drop_group, closed, render) = (
                    groups.clone(),
                    drop_group.clone(),
                    closed.clone(),
                    render.clone(),
                );
                glib::timeout_add_local_once(
                    std::time::Duration::from_millis(ms as u64),
                    move || {
                        // Only the group's latest note expires it (a newer note has its own
                        // timer); its sticky notes stay and show again (audit4 S3-4).
                        let (gone, empty) = {
                            let mut g = groups.borrow_mut();
                            let Some(gr) = g
                                .iter_mut()
                                .find(|g| g.rev == rev && g.generation == generation)
                            else {
                                return;
                            };
                            let gone = gr.notes.expire();
                            if !gr.notes.is_empty() {
                                render(gr);
                            }
                            (gone, gr.notes.is_empty())
                        };
                        if empty {
                            drop_group(&rev);
                        }
                        for id in gone {
                            closed(id, CloseReason::Expired);
                        }
                    },
                );
            }
        };
        while let Ok(ev) = rx.recv().await {
            let (mut n, fresh) = match ev {
                Event::Show(n) => (n, true),
                Event::Replay(n) => (n, false),
                Event::DismissAll => {
                    // Held ones are dismissed too: none replays after full screen (audit5 S6-1).
                    held_back.borrow_mut().clear();
                    let all: Vec<gtk::Revealer> =
                        groups.borrow().iter().map(|g| g.rev.clone()).collect();
                    for rev in all {
                        for id in drop_group(&rev) {
                            closed(id, CloseReason::Dismissed);
                        }
                    }
                    continue;
                }
                Event::Close(id) => {
                    // The app withdrew it (`CloseNotification`): gone from the center too. The
                    // banner shows the group's next note, or leaves with the last one.
                    unlist(id);
                    let rev = {
                        let mut g = groups.borrow_mut();
                        let Some(gr) = g.iter_mut().find(|x| x.notes.contains(id)) else {
                            continue;
                        };
                        let reshown = gr.notes.withdraw(id);
                        if gr.notes.is_empty() {
                            Some(gr.rev.clone())
                        } else {
                            if let Some(ms) = reshown {
                                render(gr);
                                // The note shown again may be a timed one that ran its timer
                                // out of sight: it gets a fresh one (audit6 S6-1).
                                if let Some(ms) = ms {
                                    let generation = next_gen.get().wrapping_add(1);
                                    next_gen.set(generation);
                                    gr.generation = generation;
                                    arm(gr.rev.clone(), generation, ms);
                                }
                            }
                            None
                        }
                    };
                    if let Some(rev) = rev {
                        drop_group(&rev);
                    }
                    continue;
                }
            };
            // The icon from the installed apps (audit4 S3-8); the pictures decoded off the UI
            // thread before the banner and the card are built (audit4 S3-3). Events wait here
            // in order, so a Close never overtakes its Show.
            if n.icon.is_empty() {
                n.icon = note_icon(&n.app, &super::ipc::installed().await);
            }
            preload_thumbs(&n).await;
            if fresh {
                let row = card(&n);
                for gone in rows.borrow_mut().push(n.id, row.clone()) {
                    center_list.remove(&gone);
                }
                center_list.prepend(&row);
            }
            if quiet(&n, crate::focus::read()) {
                continue;
            }
            if held(crate::hypr::fullscreen_state(), &n) {
                held_back.borrow_mut().hold(n);
                continue;
            }
            let generation = next_gen.get().wrapping_add(1);
            next_gen.set(generation);
            let shown: Vec<(String, u32)> = groups
                .borrow()
                .iter()
                .map(|g| (g.app.clone(), g.notes.len() as u32))
                .collect();
            let rev = match place(&shown, &n.app) {
                Place::Stack { index, count } => {
                    let mut g = groups.borrow_mut();
                    let gr = &mut g[index];
                    gr.notes.push(n.clone());
                    gr.generation = generation;
                    debug_assert!(count >= gr.notes.len() as u32);
                    render(gr);
                    gr.rev.clone()
                }
                Place::Top => {
                    let rev = gtk::Revealer::builder()
                        .transition_type(gtk::RevealerTransitionType::SlideLeft)
                        .build();
                    // A click runs the default action when there is one, else dismisses.
                    let click = gtk::GestureClick::new();
                    let (r2, groups2, answer2) = (rev.clone(), groups.clone(), answer.clone());
                    click.connect_released(move |_, _, _, _| {
                        let last = groups2
                            .borrow()
                            .iter()
                            .find(|g| g.rev == r2)
                            .map(|g| g.last);
                        if let Some((id, key)) = last {
                            answer2(id, key);
                        }
                    });
                    rev.add_controller(click);
                    let (r2, drop2, closed2) = (rev.clone(), drop_group.clone(), closed.clone());
                    swipe_away(&rev, move || {
                        for id in drop2(&r2) {
                            closed2(id, CloseReason::Dismissed);
                        }
                    });
                    stack.prepend(&rev);
                    let mut gr = Group {
                        app: n.app.clone(),
                        notes: GroupNotes::default(),
                        rev: rev.clone(),
                        generation,
                        last: (n.id, click_action(&n)),
                    };
                    gr.notes.push(n.clone());
                    render(&mut gr);
                    groups.borrow_mut().insert(0, gr);
                    banners.set_visible(true);
                    let r2 = rev.clone();
                    glib::idle_add_local_once(move || r2.set_reveal_child(true));
                    rev
                }
            };
            if let Some(ms) = banner_ms(&n) {
                arm(rev, generation, ms);
            }
        }
    });
    (center.upcast(), dismiss)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn n(critical: bool, t: i32) -> Note {
        Note {
            id: 1,
            app: "".into(),
            icon: "".into(),
            summary: "".into(),
            body: "".into(),
            critical,
            timeout_ms: t,
            image: "".into(),
            actions: vec![],
        }
    }

    /// Actions arrive flat (key, label, key, label…); a dangling key is dropped.
    #[test]
    fn actions_pair_up() {
        let flat: Vec<String> = ["default", "Open", "restart", "Restart", "later"]
            .map(String::from)
            .into();
        assert_eq!(
            parse_actions(&flat),
            [
                ("default".to_string(), "Open".to_string()),
                ("restart".to_string(), "Restart".to_string())
            ]
        );
    }

    /// The banner shows at most two buttons; "default" is a click on the banner, not a button.
    #[test]
    fn two_buttons_at_most_and_default_is_the_banner() {
        let a = |pairs: &[(&str, &str)]| {
            let mut x = n(false, -1);
            x.actions = pairs
                .iter()
                .map(|(k, l)| (k.to_string(), l.to_string()))
                .collect();
            x
        };
        let x = a(&[
            ("default", "Open"),
            ("restart", "Restart"),
            ("later", "Later"),
            ("x", "X"),
        ]);
        let keys: Vec<&str> = buttons(&x).iter().map(|b| b.0.as_str()).collect();
        assert_eq!(keys, ["restart", "later"]);
        assert_eq!(click_action(&x), Some("default"));
        let y = a(&[("restart", "Restart")]);
        assert_eq!(buttons(&y).len(), 1);
        assert_eq!(click_action(&y), None, "a plain click dismisses");
        // An action keeps the banner up until answered or dismissed.
        assert_eq!(banner_ms(&y), None);
        assert_eq!(banner_ms(&n(false, -1)), Some(5000));
    }

    /// wol-screenshot passes the picture as `image-path`: the banner shows it as a thumbnail.
    #[test]
    fn image_hint_is_a_file_thumbnail() {
        let hint = |k: &str, v: &str| {
            HashMap::from([(
                k.to_string(),
                OwnedValue::try_from(zbus::zvariant::Value::from(v)).unwrap(),
            )])
        };
        for (k, v, want) in [
            (
                "image-path",
                "/home/a/Desktop/Screenshot.png",
                "/home/a/Desktop/Screenshot.png",
            ),
            ("image_path", "/tmp/b.png", "/tmp/b.png"),
            ("image-path", "file:///tmp/c%20d.png", "/tmp/c d.png"),
            ("image-path", "dialog-information", ""),
            ("urgency", "/tmp/e.png", ""),
        ] {
            assert_eq!(image_hint(&hint(k, v)), want, "{k} {v}");
        }
    }

    /// A banner leaves when dragged toward the screen edge or swiped with two fingers (J13).
    #[test]
    fn banners_swipe_away() {
        for (dx, scroll, gone) in [
            (SWIPE, 0.0, true),
            (SWIPE - 1.0, 0.0, false),
            (-SWIPE * 2.0, 0.0, false),
            (0.0, SWIPE, true),
            (0.0, -SWIPE, true),
            (0.0, SWIPE / 2.0, false),
        ] {
            assert_eq!(swiped(dx, scroll), gone, "{dx} {scroll}");
        }
    }

    #[test]
    fn banners_group_per_app() {
        let shown = vec![("Mail".to_string(), 1), ("Music".to_string(), 3)];
        for (app, want) in [
            ("Mail", Place::Stack { index: 0, count: 2 }),
            ("music", Place::Stack { index: 1, count: 4 }),
            ("Calendar", Place::Top),
            ("", Place::Top),
        ] {
            assert_eq!(place(&shown, app), want, "{app}");
        }
        assert_eq!(place(&[], "Mail"), Place::Top);
    }

    #[test]
    fn banner_lifetimes() {
        for (c, t, want) in [
            (false, -1, Some(5000)),
            (false, 2000, Some(2000)),
            (false, 0, None),
            (true, 3000, None),
        ] {
            assert_eq!(banner_ms(&n(c, t)), want);
        }
    }

    /// Focus on: the note goes to Notification Center without a banner; critical ones still show.
    #[test]
    fn focus_silences_banners() {
        use crate::focus::Focus;
        assert!(quiet(&n(false, -1), Focus::On));
        assert!(quiet(&n(false, -1), Focus::Until(9)));
        assert!(!quiet(&n(true, -1), Focus::On));
        assert!(!quiet(&n(false, -1), Focus::Off));
    }

    /// Audit5 S6-1: dismiss all and the Center's Clear empty the held banners.
    #[test]
    fn cleared_held_banners_never_replay() {
        use crate::hypr::Fullscreen;
        let note = |id| Note { id, ..n(false, -1) };
        let ids = |v: Vec<Note>| v.iter().map(|n| n.id).collect::<Vec<u32>>();
        let mut h = Held::default();
        h.hold(note(1));
        h.hold(note(2));
        assert!(
            h.release(Fullscreen::Full).is_empty(),
            "still in full screen"
        );
        assert_eq!(
            ids(h.release(Fullscreen::None)),
            [1, 2],
            "untouched: they replay"
        );
        // Dismiss all (Esc, `wol shell hide banners`) or the Center's Clear while held.
        h.hold(note(1));
        h.hold(note(2));
        h.clear();
        assert!(
            h.release(Fullscreen::None).is_empty(),
            "nothing replays after full screen"
        );
        // A row taken off the Center (answered or withdrawn) never replays either.
        h.hold(note(1));
        h.hold(note(2));
        h.forget(1);
        assert_eq!(ids(h.release(Fullscreen::Maximized)), [2]);
    }

    /// Audit3 R2A-9: over a game in full screen banners wait (critical ones still show); a
    /// zoomed window holds nothing; held ones replay in order, withdrawn ones never.
    #[test]
    fn banners_wait_while_a_window_is_in_full_screen() {
        use crate::hypr::Fullscreen;
        assert!(held(Fullscreen::Full, &n(false, -1)));
        assert!(
            !held(Fullscreen::Full, &n(true, -1)),
            "critical still shows"
        );
        assert!(!held(Fullscreen::Maximized, &n(false, -1)));
        assert!(!held(Fullscreen::None, &n(false, -1)));
        let note = |id| Note { id, ..n(false, -1) };
        let mut h = Held::default();
        for id in 1..=3 {
            h.hold(note(id));
        }
        h.hold(note(2));
        h.forget(1);
        let ids: Vec<u32> = h.take().iter().map(|n| n.id).collect();
        assert_eq!(
            ids,
            [3, 2],
            "replaced notes move to the end, withdrawn ones go"
        );
        assert!(h.take().is_empty());
        for id in 0..(HELD_MAX as u32 + 3) {
            h.hold(note(id));
        }
        let kept = h.take();
        assert_eq!(kept.len(), HELD_MAX);
        assert_eq!(kept[0].id, 3, "the oldest go");
    }

    /// Audit2 A2-8: the Notification Center kept every note of the session, had no Clear, and
    /// kept answered or withdrawn notes as if unread.
    #[test]
    fn the_center_is_capped_cleared_and_drops_answered_notes() {
        let mut c: Center<u32> = Center::default();
        for id in 1..=CENTER_MAX as u32 {
            assert!(c.push(id, id).is_empty());
        }
        assert_eq!(c.push(1000, 1000), [1], "the oldest falls off");
        assert_eq!(c.len(), CENTER_MAX);
        assert_eq!(c.push(1000, 1001), [1000], "replaces_id: the old row goes");
        assert_eq!(c.len(), CENTER_MAX);
        assert_eq!(c.remove(7), Some(7), "answered or withdrawn");
        assert_eq!(c.remove(7), None);
        assert_eq!(c.clear().len(), CENTER_MAX - 1);
        assert!(c.is_empty());
    }

    /// Audit6 S6-1: a timed note under a sticky one lost its timer when the sticky one was
    /// withdrawn (or replaced the shown note by id) and then stayed on screen for good.
    #[test]
    fn a_timed_note_shown_again_after_a_withdrawal_gets_a_timer() {
        let sticky = |id| Note {
            id,
            actions: vec![("open".into(), "Open".into())],
            ..n(false, -1)
        };
        let timed = |id| Note {
            id,
            ..n(false, 3000)
        };
        let mut g = GroupNotes::default();
        g.push(timed(2));
        g.push(sticky(3));
        assert_eq!(g.shown_ms(), None, "the sticky one has no timer");
        assert_eq!(
            g.withdraw(3),
            Some(Some(3000)),
            "note 2 is back, with a fresh timer"
        );
        g.push(sticky(4));
        assert_eq!(
            g.withdraw(2),
            None,
            "a hidden note goes quietly: nothing to re-arm"
        );
        assert_eq!(g.withdraw(9), None, "an unknown id changes nothing");
        assert_eq!(
            g.withdraw(4),
            None,
            "the last one leaves: no group left to time"
        );
        g.push(sticky(5));
        g.push(timed(6));
        g.push(sticky(7));
        assert_eq!(g.withdraw(7), Some(Some(3000)));
        assert_eq!(
            g.withdraw(6),
            Some(None),
            "a sticky note comes back: no timer"
        );
    }

    /// Audit4 S3-4: a plain note stacked over an unanswered one with buttons expired both; the
    /// one with buttons stays and shows again. Withdrawing the shown note shows the previous.
    #[test]
    fn a_timed_note_expires_alone_and_sticky_ones_stay() {
        let sticky = |id| Note {
            id,
            actions: vec![("open".into(), "Open".into())],
            ..n(false, -1)
        };
        let timed = |id| Note { id, ..n(false, -1) };
        let mut g = GroupNotes::default();
        g.push(sticky(1));
        g.push(timed(2));
        assert_eq!(g.shown().map(|n| n.id), Some(2));
        assert_eq!(g.expire(), [2]);
        assert_eq!(
            g.shown().map(|n| n.id),
            Some(1),
            "the note with buttons is back"
        );
        assert_eq!(g.ids(), [1]);
        assert!(g.expire().is_empty(), "sticky notes never expire");
        g.push(timed(3));
        g.push(timed(4));
        assert!(g.remove(4), "the shown one was withdrawn");
        assert_eq!(g.shown().map(|n| n.id), Some(3));
        assert!(!g.remove(1), "an older one goes quietly");
        g.push(timed(3));
        assert_eq!(g.len(), 1, "a replaced note is not counted twice");
        assert_eq!(g.expire(), [3]);
        assert!(g.is_empty());
    }

    /// Audit4 S3-3: a banner picture is decoded small (2x the thumbnail), once; a directory or
    /// a missing file gives none.
    #[test]
    fn banner_pictures_are_small_and_checked() {
        let svg = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../design/wallpapers/wonderland-day.svg"
        );
        let t = decode_thumb(svg).expect("the wallpaper decodes (the goldens need the loader)");
        assert_eq!(
            t.width().min(t.height()),
            THUMB_PX,
            "{}x{}",
            t.width(),
            t.height()
        );
        assert!(t.width().max(t.height()) <= THUMB_LONG_MAX);
        assert_eq!(
            thumb_box(3840, 2160),
            (170, THUMB_PX),
            "4K: small, covers the box"
        );
        assert_eq!(thumb_box(1080, 1920), (THUMB_PX, 170));
        assert_eq!(
            thumb_box(100_000, 10),
            (THUMB_LONG_MAX, THUMB_PX),
            "panorama capped"
        );
        assert_eq!(thumb_box(0, 0), (THUMB_PX, THUMB_PX));
        assert!(
            decode_thumb(env!("CARGO_MANIFEST_DIR")).is_none(),
            "a directory"
        );
        assert!(decode_thumb("/nonexistent/wol-fs3.png").is_none());
        keep_thumb(
            "/a.png",
            gtk::gdk::MemoryTexture::new(
                1,
                1,
                gtk::gdk::MemoryFormat::R8g8b8,
                &glib::Bytes::from_static(&[0, 0, 0]),
                3,
            )
            .upcast(),
        );
        assert!(cached_thumb("/a.png").is_some(), "kept for the center card");
        for i in 0..THUMBS_KEPT {
            keep_thumb(&format!("/{i}.png"), cached_thumb("/a.png").unwrap());
        }
        assert!(cached_thumb("/a.png").is_none(), "bounded: the oldest goes");
        let note = Note {
            image: "/s.png".into(),
            icon: "/i.svg".into(),
            ..n(false, -1)
        };
        assert_eq!(thumb_files(&note), ["/s.png", "/i.svg"]);
        assert!(
            thumb_files(&Note {
                icon: "mail".into(),
                ..n(false, -1)
            })
            .is_empty()
        );
    }

    /// Audit4 S3-8: a note from an app outside the dock takes its installed entry's icon, found
    /// by the name Notify carries.
    #[test]
    fn note_icons_come_from_installed_apps() {
        let e = crate::desktop::Entry {
            id: "signal-desktop.desktop".into(),
            name: "Signal".into(),
            exec: vec!["signal-desktop".into()],
            icon: "signal-desktop".into(),
        };
        assert_eq!(
            note_icon("Signal", std::slice::from_ref(&e)),
            "signal-desktop"
        );
        assert_eq!(
            note_icon("signal-desktop", std::slice::from_ref(&e)),
            "signal-desktop"
        );
        assert_eq!(note_icon(SYSTEM_APP, &[]), SYSTEM_ICON);
        assert_eq!(
            note_icon("Zed", &[]),
            "zed",
            "unknown: the name as an icon name"
        );
    }
}
