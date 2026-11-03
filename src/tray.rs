// SPDX-License-Identifier: GPL-2.0-or-later
//! Status items of third-party apps, shown in the bar (SP-7). The shell is both the session's
//! status item watcher (apps register their item with it) and its only host (it reads the items
//! and draws them, `ui::tray_items`). Wire protocol: the status notifier item specification
//! (`org.kde.StatusNotifierWatcher`, `org.kde.StatusNotifierItem`) and its menu companion
//! (`com.canonical.dbusmenu`). An item whose app leaves the bus vanishes.
//!
//! No toolkit here: reads of items run on whatever executor [`Spawn`] hands futures to (the
//! toolkit's main context in the shell, plain threads in `tests/tray.rs`), one thread follows the
//! bus (`wol-tray-bus`), and every change is sent as a whole snapshot of the items, in the order
//! they registered.
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedValue, Value};

/// Bus name and interface of the watcher (specification identifier).
pub const WATCHER_NAME: &str = "org.kde.StatusNotifierWatcher";
pub const WATCHER_PATH: &str = "/StatusNotifierWatcher";
/// Interface of one item (specification identifier).
pub const ITEM_IFACE: &str = "org.kde.StatusNotifierItem";
/// Object path an item lives at when it registers a bus name only.
pub const ITEM_PATH: &str = "/StatusNotifierItem";
/// Interface of an item's menu (specification identifier of the menu protocol).
pub const MENU_IFACE: &str = "com.canonical.dbusmenu";
const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";

/// Most items the watcher takes; a further app is told the limit is reached.
pub const MAX_ITEMS: usize = 32;
/// Menu levels shown (the item's menu, a submenu, a submenu of that); deeper rows are cut.
pub const MAX_DEPTH: usize = 3;
/// Rows kept per menu level.
pub const MAX_ROWS: usize = 64;
/// Longest wait for an app's answer: an item that hangs is dropped (when never read) or keeps
/// its last state; a click on it is lost.
pub const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
/// Largest icon picture taken from an item, in pixels per side.
pub const MAX_PIXMAP: i32 = 256;

/// Hands a future to an executor (the shell: the toolkit's main context).
pub type Spawn = Arc<dyn Fn(Pin<Box<dyn Future<Output = ()> + Send>>) + Send + Sync>;

/// What an item says about itself (`Status`). Passive items are not drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    Passive,
    #[default]
    Active,
    NeedsAttention,
}

impl Status {
    pub fn parse(s: &str) -> Self {
        match s {
            "Passive" => Status::Passive,
            "NeedsAttention" => Status::NeedsAttention,
            _ => Status::Active,
        }
    }
}

/// An icon picture, RGBA with straight alpha, `width * height * 4` bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pixmap {
    pub width: i32,
    pub height: i32,
    pub rgba: Vec<u8>,
}

/// One item as the bar draws it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Item {
    /// `bus/path`, as registered (the watcher's `RegisteredStatusNotifierItems` entry).
    pub id: String,
    /// The app's own id for the item (`Id`).
    pub app: String,
    pub title: String,
    /// Tooltip title, else the title, else the app's id.
    pub tooltip: String,
    pub status: Status,
    /// Icon name for the current status (the attention icon while it needs attention), or a
    /// file path; "" = none.
    pub icon_name: String,
    /// Picture for the current status, used when the theme has no icon by that name.
    pub pixmap: Option<Pixmap>,
    /// Extra folder the app's icon names are found in (`IconThemePath`); "" = none.
    pub theme_path: String,
    /// Object path of the item's menu (`Menu`).
    pub menu: Option<String>,
    /// The item is only a menu: a click opens it instead of activating (`ItemIsMenu`).
    pub is_menu: bool,
}

impl Item {
    /// Drawn in the bar: not passive.
    pub fn shown(&self) -> bool {
        self.status != Status::Passive
    }
}

/// How a menu row toggles (`toggle-type`, `toggle-state`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Toggle {
    #[default]
    None,
    Check(bool),
    Radio(bool),
}

/// One row of an item's menu; the root's children are the menu's rows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MenuNode {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub separator: bool,
    pub toggle: Toggle,
    pub icon: String,
    pub children: Vec<MenuNode>,
}

// ---------------------------------------------------------------- pure parts

/// The registered id for a `RegisterStatusNotifierItem(arg)` call from `sender`: `sender/path`
/// for an object path, `name/StatusNotifierItem` for a bus name, `name/path` as given. None for
/// an empty argument.
pub fn item_id(sender: &str, arg: &str) -> Option<String> {
    let arg = arg.trim();
    if arg.is_empty() || (arg.starts_with('/') && sender.is_empty()) {
        return None;
    }
    Some(if arg.starts_with('/') {
        format!("{sender}{arg}")
    } else if arg.contains('/') {
        arg.to_string()
    } else {
        format!("{arg}{ITEM_PATH}")
    })
}

/// Bus name and object path of an item id.
pub fn split_id(id: &str) -> (&str, &str) {
    match id.find('/') {
        Some(i) => (&id[..i], &id[i..]),
        None => (id, ITEM_PATH),
    }
}

/// The watcher's list: registration order, no duplicates, at most [`MAX_ITEMS`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Registry {
    ids: Vec<String>,
}

/// Registering failed: the list is full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full;

impl Registry {
    /// True when `id` is new.
    pub fn register(&mut self, id: &str) -> Result<bool, Full> {
        if self.ids.iter().any(|x| x == id) {
            return Ok(false);
        }
        if self.ids.len() >= MAX_ITEMS {
            return Err(Full);
        }
        self.ids.push(id.to_string());
        Ok(true)
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let n = self.ids.len();
        self.ids.retain(|x| x != id);
        self.ids.len() != n
    }

    /// Drops the items on bus name `name` (it left the bus); the ids dropped.
    pub fn drop_bus(&mut self, name: &str) -> Vec<String> {
        let (gone, kept): (Vec<String>, Vec<String>) = std::mem::take(&mut self.ids)
            .into_iter()
            .partition(|id| split_id(id).0 == name);
        self.ids = kept;
        gone
    }

    pub fn contains(&self, id: &str) -> bool {
        self.ids.iter().any(|x| x == id)
    }

    pub fn ids(&self) -> &[String] {
        &self.ids
    }
}

/// A variant's inner value (`v` wraps another value).
fn inner<'a>(v: &'a Value<'a>) -> &'a Value<'a> {
    match v {
        Value::Value(b) => inner(b),
        v => v,
    }
}

fn str_of(v: &Value) -> Option<String> {
    match inner(v) {
        Value::Str(s) => Some(s.to_string()),
        Value::ObjectPath(p) => Some(p.to_string()),
        _ => None,
    }
}

fn bool_of(v: &Value) -> Option<bool> {
    match inner(v) {
        Value::Bool(b) => Some(*b),
        _ => None,
    }
}

fn i32_of(v: &Value) -> Option<i32> {
    match inner(v) {
        Value::I32(n) => Some(*n),
        Value::U32(n) => i32::try_from(*n).ok(),
        _ => None,
    }
}

/// `ARGB32` in network byte order (the protocol's pictures) to straight RGBA; None when the size
/// is out of range or the bytes do not fill it.
pub fn argb_to_rgba(width: i32, height: i32, argb: &[u8]) -> Option<Pixmap> {
    if !(1..=MAX_PIXMAP).contains(&width) || !(1..=MAX_PIXMAP).contains(&height) {
        return None;
    }
    if usize::try_from(width * height * 4) != Ok(argb.len()) {
        return None;
    }
    let rgba = argb
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[a, r, g, b]| [r, g, b, a])
        .collect();
    Some(Pixmap {
        width,
        height,
        rgba,
    })
}

/// The picture to draw at `want` pixels from an item's list (`a(iiay)`): the smallest that is
/// at least `want` wide, else the largest. Pictures that do not add up are skipped.
pub fn pick_pixmap(list: &[(i32, i32, Vec<u8>)], want: i32) -> Option<Pixmap> {
    let ok: Vec<&(i32, i32, Vec<u8>)> = list
        .iter()
        .filter(|(w, h, d)| argb_to_rgba(*w, *h, d).is_some())
        .collect();
    let best = ok
        .iter()
        .filter(|(w, _, _)| *w >= want)
        .min_by_key(|(w, _, _)| *w)
        .or_else(|| ok.iter().max_by_key(|(w, _, _)| *w))?;
    argb_to_rgba(best.0, best.1, &best.2)
}

/// The `a(iiay)` pictures of a property value.
fn pixmaps_of(v: &Value) -> Vec<(i32, i32, Vec<u8>)> {
    let Value::Array(a) = inner(v) else {
        return Vec::new();
    };
    a.iter()
        .filter_map(|e| {
            let Value::Structure(s) = inner(e) else {
                return None;
            };
            let f = s.fields();
            let (w, h) = (i32_of(f.first()?)?, i32_of(f.get(1)?)?);
            if !(1..=MAX_PIXMAP).contains(&w) || !(1..=MAX_PIXMAP).contains(&h) {
                return None;
            }
            let bytes = match inner(f.get(2)?) {
                Value::Array(b) => b
                    .iter()
                    .map(|x| match x {
                        Value::U8(n) => Some(*n),
                        _ => None,
                    })
                    .collect::<Option<Vec<u8>>>()?,
                _ => return None,
            };
            Some((w, h, bytes))
        })
        .collect()
}

/// Picture size asked of an item: the bar's 16 px icon at twice the density.
pub const PIXMAP_WANT: i32 = 32;

/// An [`Item`] from its properties (`GetAll` on [`ITEM_IFACE`]).
pub fn item_from_props(id: &str, p: &HashMap<String, OwnedValue>) -> Item {
    let s = |k: &str| p.get(k).and_then(|v| str_of(v)).unwrap_or_default();
    let status = Status::parse(&s("Status"));
    let attention = status == Status::NeedsAttention;
    let (name_key, pix_key) = if attention {
        ("AttentionIconName", "AttentionIconPixmap")
    } else {
        ("IconName", "IconPixmap")
    };
    let mut icon_name = s(name_key);
    let mut pixmap = p
        .get(pix_key)
        .and_then(|v| pick_pixmap(&pixmaps_of(v), PIXMAP_WANT));
    if attention && icon_name.is_empty() && pixmap.is_none() {
        icon_name = s("IconName");
        pixmap = p
            .get("IconPixmap")
            .and_then(|v| pick_pixmap(&pixmaps_of(v), PIXMAP_WANT));
    }
    // ToolTip: (s icon, a(iiay) pictures, s title, s description).
    let tip_title = p
        .get("ToolTip")
        .and_then(|v| match inner(v) {
            Value::Structure(t) => t.fields().get(2).and_then(str_of),
            _ => None,
        })
        .unwrap_or_default();
    let (app, title) = (s("Id"), s("Title"));
    let tooltip = [&tip_title, &title, &app]
        .into_iter()
        .find(|t| !t.trim().is_empty())
        .cloned()
        .unwrap_or_default();
    let menu = p
        .get("Menu")
        .and_then(|v| str_of(v))
        .filter(|m| m.starts_with('/') && m != "/");
    Item {
        id: id.to_string(),
        app,
        title,
        tooltip,
        status,
        icon_name,
        pixmap,
        theme_path: s("IconThemePath"),
        menu,
        is_menu: p
            .get("ItemIsMenu")
            .and_then(|v| bool_of(v))
            .unwrap_or(false),
    }
}

/// A menu label without its access-key marks: `_File` -> `File`, `__` -> `_`.
pub fn menu_label(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            if chars.peek() == Some(&'_') {
                chars.next();
                out.push('_');
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// One menu row from its `(i a{sv} av)` layout, at level `depth` (the root is 0). Hidden rows
/// are None; children past [`MAX_DEPTH`] and past [`MAX_ROWS`] are cut.
pub fn menu_node(v: &Value, depth: usize) -> Option<MenuNode> {
    let Value::Structure(s) = inner(v) else {
        return None;
    };
    let f = s.fields();
    let id = i32_of(f.first()?)?;
    let mut props: HashMap<String, &Value> = HashMap::new();
    if let Value::Dict(d) = inner(f.get(1)?) {
        for (k, v) in d.iter() {
            if let Some(k) = str_of(k) {
                props.insert(k, v);
            }
        }
    }
    let kids: Vec<&Value> = match f.get(2).map(inner) {
        Some(Value::Array(a)) => a.iter().collect(),
        _ => Vec::new(),
    };
    build_node(id, &props, &kids, depth)
}

/// [`menu_node`] from a layout's parts as the reply carries them.
pub fn menu_root(
    id: i32,
    props: &HashMap<String, OwnedValue>,
    kids: &[OwnedValue],
) -> Option<MenuNode> {
    let props: HashMap<String, &Value> = props.iter().map(|(k, v)| (k.clone(), &**v)).collect();
    let kids: Vec<&Value> = kids.iter().map(|v| &**v).collect();
    build_node(id, &props, &kids, 0)
}

fn build_node(
    id: i32,
    props: &HashMap<String, &Value>,
    kids: &[&Value],
    depth: usize,
) -> Option<MenuNode> {
    let s = |k: &str| props.get(k).and_then(|v| str_of(v)).unwrap_or_default();
    let b = |k: &str, d: bool| props.get(k).and_then(|v| bool_of(v)).unwrap_or(d);
    if !b("visible", true) {
        return None;
    }
    let on = props.get("toggle-state").and_then(|v| i32_of(v)) == Some(1);
    let toggle = match s("toggle-type").as_str() {
        "checkmark" => Toggle::Check(on),
        "radio" => Toggle::Radio(on),
        _ => Toggle::None,
    };
    let children = if depth < MAX_DEPTH {
        kids.iter()
            .filter_map(|c| menu_node(c, depth + 1))
            .take(MAX_ROWS)
            .collect()
    } else {
        Vec::new()
    };
    Some(MenuNode {
        id,
        label: menu_label(&s("label")),
        enabled: b("enabled", true),
        separator: s("type") == "separator",
        toggle,
        icon: s("icon-name"),
        children,
    })
}

/// `d` rounded to a whole number (halves away from zero); 0 when it is not a number or is outside
/// plus or minus 2^20, far beyond any wheel. Found by halving the range: a float is never cast.
pub fn whole(d: f64) -> i32 {
    const LIMIT: i32 = 1 << 20;
    if !(-f64::from(LIMIT)..=f64::from(LIMIT)).contains(&d) {
        return 0;
    }
    let r = d.round();
    let (mut lo, mut hi) = (-LIMIT, LIMIT);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if f64::from(mid) < r {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

/// `Scroll(delta, orientation)` for a wheel step `(dx, dy)` as the toolkit reports it (down and
/// right positive): one notch is 120, up and left positive as the protocol's hosts send it.
pub fn scroll_args(dx: f64, dy: f64) -> Option<(i32, &'static str)> {
    let notch = |d: f64| -whole(d * 120.0);
    if dy != 0.0 && notch(dy) != 0 {
        Some((notch(dy), "vertical"))
    } else if dx != 0.0 && notch(dx) != 0 {
        Some((notch(dx), "horizontal"))
    } else {
        None
    }
}

// ---------------------------------------------------------------- the bus side

#[derive(Default)]
struct Model {
    reg: Registry,
    /// Fetched items by id.
    items: HashMap<String, Item>,
    /// Unique bus name of each item's owner (an item may register a well-known name).
    owners: HashMap<String, String>,
    /// Ids being fetched, and ids that changed again while being fetched.
    busy: HashSet<String>,
    dirty: HashSet<String>,
}

struct Shared {
    model: Mutex<Model>,
    tx: async_channel::Sender<Vec<Item>>,
    spawn: Spawn,
    stopped: AtomicBool,
}

impl Shared {
    fn with<T>(&self, f: impl FnOnce(&mut Model) -> T) -> T {
        let mut m = self.model.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut m)
    }

    /// Sends the items, in registration order, to the bar.
    fn publish(&self) {
        let snap = self.with(|m| {
            m.reg
                .ids()
                .iter()
                .filter_map(|id| m.items.get(id).cloned())
                .collect::<Vec<_>>()
        });
        let _ = self.tx.try_send(snap);
    }
}

/// The watcher object apps call.
struct Watcher {
    shared: Arc<Shared>,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    async fn register_status_notifier_item(
        &self,
        service: &str,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] e: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        let sender = hdr.sender().map(|s| s.to_string()).unwrap_or_default();
        let id = item_id(&sender, service)
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("no item named".into()))?;
        let new = self
            .shared
            .with(|m| m.reg.register(&id))
            .map_err(|Full| zbus::fdo::Error::LimitsExceeded(format!("{MAX_ITEMS} items")))?;
        if new {
            Self::status_notifier_item_registered(&e, &id).await?;
            self.registered_status_notifier_items_changed(&e).await?;
        }
        refresh(&self.shared, conn, id);
        Ok(())
    }

    /// The shell is the session's host; another one is noted and not needed.
    async fn register_status_notifier_host(&self, _service: &str) {}

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.shared.with(|m| m.reg.ids().to_vec())
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    async fn status_notifier_item_registered(e: &SignalEmitter<'_>, id: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_item_unregistered(e: &SignalEmitter<'_>, id: &str)
    -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_host_registered(e: &SignalEmitter<'_>) -> zbus::Result<()>;
}

/// Reads item `id` again (it registered or said something changed). One read per item at a
/// time; a change during a read reads once more. An item that cannot be read at all goes.
fn refresh(shared: &Arc<Shared>, conn: &zbus::Connection, id: String) {
    let start = shared.with(|m| {
        if !m.reg.contains(&id) {
            return false;
        }
        if m.busy.insert(id.clone()) {
            true
        } else {
            m.dirty.insert(id.clone());
            false
        }
    });
    if !start {
        return;
    }
    let (s, conn) = (shared.clone(), conn.clone());
    (shared.spawn)(Box::pin(async move {
        let mut dropped = false;
        loop {
            let got = fetch(&conn, &id).await;
            let again = s.with(|m| {
                match got {
                    Ok((item, owner)) if m.reg.contains(&id) => {
                        m.items.insert(id.clone(), item);
                        m.owners.insert(id.clone(), owner);
                    }
                    Ok(_) => {}
                    Err(_) if !m.items.contains_key(&id) => {
                        dropped = m.reg.remove(&id);
                    }
                    Err(_) => {}
                }
                if m.dirty.remove(&id) && m.reg.contains(&id) {
                    true
                } else {
                    m.busy.remove(&id);
                    false
                }
            });
            if !again {
                break;
            }
        }
        s.publish();
        if dropped {
            unregistered(&conn, &[id]).await;
        }
    }));
}

/// Tells the bus items `gone` left the watcher's list.
async fn unregistered(conn: &zbus::Connection, gone: &[String]) {
    let Ok(w) = conn
        .object_server()
        .interface::<_, Watcher>(WATCHER_PATH)
        .await
    else {
        return;
    };
    let e = w.signal_emitter();
    for id in gone {
        let _ = Watcher::status_notifier_item_unregistered(e, id).await;
    }
    let _ = w
        .get()
        .await
        .registered_status_notifier_items_changed(e)
        .await;
}

/// The item's properties and its owner's unique bus name.
async fn fetch(conn: &zbus::Connection, id: &str) -> zbus::Result<(Item, String)> {
    let (bus, path) = split_id(id);
    let reply = conn
        .call_method(Some(bus), path, Some(PROPS_IFACE), "GetAll", &(ITEM_IFACE,))
        .await?;
    let props: HashMap<String, OwnedValue> = reply.body().deserialize()?;
    let owner = if bus.starts_with(':') {
        bus.to_string()
    } else {
        zbus::fdo::DBusProxy::new(conn)
            .await?
            .get_name_owner(zbus::names::BusName::try_from(bus)?)
            .await?
            .to_string()
    };
    Ok((item_from_props(id, &props), owner))
}

/// Tries of a watcher start while the name is still taken (a watcher just stopped, or the last
/// shell's connection is not gone yet), [`SERVE_GAP`] apart.
pub const SERVE_TRIES: u32 = 5;
pub const SERVE_GAP: std::time::Duration = std::time::Duration::from_millis(200);

/// Whether a watcher start that failed with `e` after `tries` tries is tried again: only while
/// the name is taken, at most [`SERVE_TRIES`] in all.
pub fn serve_again(e: &zbus::Error, tries: u32) -> bool {
    matches!(e, zbus::Error::NameTaken) && tries < SERVE_TRIES
}

/// The running watcher and host. Clones share it; [`Tray::stop`] ends it for all.
#[derive(Clone)]
pub struct Tray {
    conn: zbus::Connection,
    shared: Arc<Shared>,
}

impl Tray {
    /// Owns the watcher name on `address` (None: the session bus) and starts following the
    /// items. Every change sends the whole list on the returned channel. Fails when another
    /// watcher owns the name.
    pub async fn serve(
        address: Option<&str>,
        spawn: Spawn,
    ) -> zbus::Result<(Tray, async_channel::Receiver<Vec<Item>>)> {
        let (tx, rx) = async_channel::unbounded();
        let shared = Arc::new(Shared {
            model: Mutex::default(),
            tx,
            spawn,
            stopped: AtomicBool::new(false),
        });
        let b = match address {
            Some(a) => zbus::connection::Builder::address(a)?,
            None => zbus::connection::Builder::session()?,
        };
        let conn = b
            .method_timeout(CALL_TIMEOUT)
            .serve_at(
                WATCHER_PATH,
                Watcher {
                    shared: shared.clone(),
                },
            )?
            .build()
            .await?;
        // Follow first, so no change between the name and the first read is missed: one
        // iterator over every message the connection gets, fed by two match rules.
        let iter =
            zbus::blocking::MessageIterator::from(zbus::blocking::Connection::from(conn.clone()));
        let dbus = zbus::fdo::DBusProxy::new(&conn).await?;
        dbus.add_match_rule(
            zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .sender("org.freedesktop.DBus")?
                .member("NameOwnerChanged")?
                .build(),
        )
        .await?;
        dbus.add_match_rule(
            zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .interface(ITEM_IFACE)?
                .build(),
        )
        .await?;
        let flags = zbus::fdo::RequestNameFlags::DoNotQueue.into();
        match conn.request_name_with_flags(WATCHER_NAME, flags).await? {
            zbus::fdo::RequestNameReply::PrimaryOwner
            | zbus::fdo::RequestNameReply::AlreadyOwner => {}
            _ => return Err(zbus::Error::NameTaken),
        }
        let tray = Tray { conn, shared };
        let t = tray.clone();
        std::thread::Builder::new()
            .name("wol-tray-bus".into())
            .spawn(move || t.follow(iter))
            .map_err(|e| zbus::Error::InputOutput(Arc::new(e)))?;
        if let Ok(e) = SignalEmitter::new(&tray.conn, WATCHER_PATH) {
            let _ = Watcher::status_notifier_host_registered(&e).await;
        }
        Ok((tray, rx))
    }

    /// Runs on its own thread until [`Tray::stop`]: items whose app left the bus go (and the
    /// watcher says so); `NewIcon`, `NewStatus`, `NewTitle`, ... from an item read it again.
    fn follow(&self, iter: zbus::blocking::MessageIterator) {
        for msg in iter {
            if self.shared.stopped.load(Ordering::Acquire) {
                break;
            }
            let Ok(msg) = msg else { continue };
            let h = msg.header();
            if h.message_type() != zbus::message::Type::Signal {
                continue;
            }
            let (Some(sender), Some(path), Some(iface), Some(member)) =
                (h.sender(), h.path(), h.interface(), h.member())
            else {
                continue;
            };
            if iface.as_str() == ITEM_IFACE {
                self.changed(sender.as_str(), path.as_str());
            } else if member.as_str() == "NameOwnerChanged"
                && sender.as_str() == "org.freedesktop.DBus"
                && let Ok((name, _old, new)) = msg.body().deserialize::<(String, String, String)>()
                && new.is_empty()
            {
                self.left(&name);
            }
        }
    }

    /// Bus name `name` has no owner any more: its items go.
    fn left(&self, name: &str) {
        let gone = self.shared.with(|m| {
            let mut gone = m.reg.drop_bus(name);
            let by_owner: Vec<String> = m
                .owners
                .iter()
                .filter(|(_, o)| o.as_str() == name)
                .map(|(id, _)| id.clone())
                .collect();
            for id in by_owner {
                if m.reg.remove(&id) {
                    gone.push(id);
                }
            }
            for id in &gone {
                m.items.remove(id);
                m.owners.remove(id);
            }
            gone
        });
        if gone.is_empty() {
            return;
        }
        self.shared.publish();
        let conn = self.conn.clone();
        (self.shared.spawn)(Box::pin(async move { unregistered(&conn, &gone).await }));
    }

    /// Item signal from `sender` at `path`: the items it names are read again.
    fn changed(&self, sender: &str, path: &str) {
        let ids: Vec<String> = self.shared.with(|m| {
            m.reg
                .ids()
                .iter()
                .filter(|id| {
                    let (bus, p) = split_id(id);
                    p == path && (bus == sender || m.owners.get(*id).is_some_and(|o| o == sender))
                })
                .cloned()
                .collect()
        });
        for id in ids {
            refresh(&self.shared, &self.conn, id);
        }
    }

    /// The items now, in registration order.
    pub fn items(&self) -> Vec<Item> {
        self.shared.with(|m| {
            m.reg
                .ids()
                .iter()
                .filter_map(|id| m.items.get(id).cloned())
                .collect()
        })
    }

    async fn call<B>(
        &self,
        id: &str,
        iface: &str,
        path: Option<&str>,
        method: &str,
        body: &B,
    ) -> zbus::Result<zbus::Message>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
    {
        let (bus, item_path) = split_id(id);
        self.conn
            .call_method(
                Some(bus),
                path.unwrap_or(item_path),
                Some(iface),
                method,
                body,
            )
            .await
    }

    /// The primary click (`Activate`).
    pub async fn activate(&self, id: &str) -> zbus::Result<()> {
        self.call(id, ITEM_IFACE, None, "Activate", &(0i32, 0i32))
            .await
            .map(drop)
    }

    /// The middle click (`SecondaryActivate`).
    pub async fn secondary_activate(&self, id: &str) -> zbus::Result<()> {
        self.call(id, ITEM_IFACE, None, "SecondaryActivate", &(0i32, 0i32))
            .await
            .map(drop)
    }

    /// The app draws its own menu (`ContextMenu`): for items without a menu object.
    pub async fn context_menu(&self, id: &str) -> zbus::Result<()> {
        self.call(id, ITEM_IFACE, None, "ContextMenu", &(0i32, 0i32))
            .await
            .map(drop)
    }

    /// A wheel step ([`scroll_args`]).
    pub async fn scroll(&self, id: &str, delta: i32, orientation: &str) -> zbus::Result<()> {
        self.call(id, ITEM_IFACE, None, "Scroll", &(delta, orientation))
            .await
            .map(drop)
    }

    /// The item's menu as it is now (None: it has none).
    pub async fn menu(&self, id: &str) -> zbus::Result<Option<MenuNode>> {
        let Some(path) = self
            .shared
            .with(|m| m.items.get(id).and_then(|i| i.menu.clone()))
        else {
            return Ok(None);
        };
        // Lets the app fill the menu in; an app without it is fine.
        let _ = self
            .call(id, MENU_IFACE, Some(&path), "AboutToShow", &(0i32,))
            .await;
        let reply = self
            .call(
                id,
                MENU_IFACE,
                Some(&path),
                "GetLayout",
                &(0i32, -1i32, Vec::<&str>::new()),
            )
            .await?;
        type Layout = (u32, (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>));
        let (_rev, (root, props, kids)): Layout = reply.body().deserialize()?;
        Ok(menu_root(root, &props, &kids))
    }

    /// Row `row` of the item's menu was chosen (`Event(row, "clicked")`).
    pub async fn menu_clicked(&self, id: &str, row: i32) -> zbus::Result<()> {
        let Some(path) = self
            .shared
            .with(|m| m.items.get(id).and_then(|i| i.menu.clone()))
        else {
            return Ok(());
        };
        self.call(
            id,
            MENU_IFACE,
            Some(&path),
            "Event",
            &(row, "clicked", Value::I32(0), 0u32),
        )
        .await
        .map(drop)
    }

    /// Gives the watcher name up and stops following the items (apps see no tray any more).
    pub async fn stop(&self) {
        self.shared.stopped.store(true, Ordering::Release);
        let _ = self.conn.release_name(WATCHER_NAME).await;
        self.shared.with(|m| *m = Model::default());
        self.shared.tx.close();
        let _ = self.conn.clone().close().await;
    }
}
