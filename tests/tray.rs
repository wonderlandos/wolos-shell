// SPDX-License-Identifier: GPL-2.0-or-later
//! The tray watcher and host (`shell::tray`, SP-7) against a fake app on a private dbus-daemon:
//! an item registers and shows with its icon, title and picture, the host's clicks, wheel and
//! menu reach the app, a changed icon is read again, an app that leaves the bus loses its item,
//! a second watcher is refused, and a stopped watcher frees the name. Plus the pure parts
//! (ids, the 32-item limit, pictures, menu labels and depth, wheel steps).
use shell::tray::{self, Item, MenuNode, Registry, Status, Toggle, Tray};
use std::collections::HashMap;
use std::io::BufRead;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Structure, Value};

// ---------------------------------------------------------------- pure parts

#[test]
fn ids_name_the_bus_and_the_path() {
    assert_eq!(
        tray::item_id(":1.7", "/org/app/Item").as_deref(),
        Some(":1.7/org/app/Item")
    );
    assert_eq!(
        tray::item_id(":1.7", "org.app.Tray-1").as_deref(),
        Some("org.app.Tray-1/StatusNotifierItem")
    );
    assert_eq!(
        tray::item_id(":1.7", ":1.9/Item").as_deref(),
        Some(":1.9/Item")
    );
    assert_eq!(tray::item_id(":1.7", " "), None);
    assert_eq!(tray::item_id("", "/Item"), None);
    assert_eq!(
        tray::split_id(":1.7/org/app/Item"),
        (":1.7", "/org/app/Item")
    );
    assert_eq!(tray::split_id("org.app"), ("org.app", tray::ITEM_PATH));
}

#[test]
fn the_registry_keeps_order_and_stops_at_32() {
    let mut r = Registry::default();
    for n in 0..tray::MAX_ITEMS {
        assert_eq!(r.register(&format!(":1.{n}/Item")), Ok(true));
    }
    assert_eq!(
        r.register(":1.0/Item"),
        Ok(false),
        "a second register is no new item"
    );
    assert_eq!(r.register(":1.99/Item"), Err(tray::Full));
    assert_eq!(r.ids().len(), 32);
    assert_eq!(r.ids()[3], ":1.3/Item");
    assert_eq!(r.drop_bus(":1.3"), vec![":1.3/Item".to_string()]);
    assert!(!r.contains(":1.3/Item"));
    assert_eq!(r.register(":1.99/Item"), Ok(true), "room again");
}

#[test]
fn pictures_turn_rgba_and_the_fitting_size_wins() {
    // ARGB in network order: opaque red, half-transparent blue.
    let p = tray::argb_to_rgba(2, 1, &[255, 255, 0, 0, 128, 0, 0, 255]).unwrap();
    assert_eq!(p.rgba, vec![255, 0, 0, 255, 0, 0, 255, 128]);
    assert_eq!(
        tray::argb_to_rgba(2, 2, &[0; 4]),
        None,
        "bytes short of the size"
    );
    assert_eq!(tray::argb_to_rgba(0, 1, &[]), None);
    assert_eq!(tray::argb_to_rgba(512, 512, &vec![0; 512 * 512 * 4]), None);
    let pic = |s: i32| (s, s, vec![0u8; (s * s * 4) as usize]);
    let list = vec![pic(16), pic(64), pic(48), (24, 24, vec![0u8; 3])];
    assert_eq!(tray::pick_pixmap(&list, 32).unwrap().width, 48);
    assert_eq!(tray::pick_pixmap(&list, 128).unwrap().width, 64);
    assert_eq!(tray::pick_pixmap(&[], 32), None);
}

#[test]
fn menu_labels_lose_their_access_marks() {
    assert_eq!(tray::menu_label("_Open"), "Open");
    assert_eq!(tray::menu_label("Save __as"), "Save _as");
    assert_eq!(tray::menu_label("Plain"), "Plain");
}

#[test]
fn wheel_steps_are_notches_up_positive() {
    for (d, n) in [
        (0.0, 0),
        (0.4, 0),
        (0.5, 1),
        (-0.5, -1),
        (1.6, 2),
        (-120.4, -120),
        (1e9, 0),
        (f64::NAN, 0),
    ] {
        assert_eq!(tray::whole(d), n, "{d}");
    }
    assert_eq!(tray::scroll_args(0.0, -1.0), Some((120, "vertical")));
    assert_eq!(tray::scroll_args(0.0, 1.0), Some((-120, "vertical")));
    assert_eq!(tray::scroll_args(1.0, 0.0), Some((-120, "horizontal")));
    assert_eq!(tray::scroll_args(0.0, 0.0), None);
}

#[test]
fn status_reads_as_the_protocol_spells_it() {
    assert_eq!(Status::parse("Passive"), Status::Passive);
    assert_eq!(Status::parse("NeedsAttention"), Status::NeedsAttention);
    assert_eq!(Status::parse("Active"), Status::Active);
    assert_eq!(Status::parse("whatever"), Status::Active);
}

/// One `(i a{sv} av)` menu row.
fn node(id: i32, props: &[(&str, Value<'static>)], kids: Vec<Value<'static>>) -> Value<'static> {
    let map: HashMap<String, Value<'static>> = props
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    Value::Structure(Structure::from((id, map, kids)))
}

fn label(t: &str) -> (&'static str, Value<'static>) {
    ("label", Value::from(t.to_string()))
}

/// The fake app's menu: Open, a line, Mute (checked), More > Level 2 > Level 3 > Level 4, and a
/// hidden row.
fn fake_menu() -> Value<'static> {
    let l4 = node(7, &[label("Level 4")], vec![]);
    let l3 = node(6, &[label("Level 3")], vec![l4]);
    let l2 = node(5, &[label("Level 2")], vec![l3]);
    node(
        0,
        &[],
        vec![
            node(1, &[label("_Open")], vec![]),
            node(2, &[("type", Value::from("separator"))], vec![]),
            node(
                3,
                &[
                    label("Mute"),
                    ("toggle-type", Value::from("checkmark")),
                    ("toggle-state", Value::from(1i32)),
                ],
                vec![],
            ),
            node(
                4,
                &[label("More"), ("children-display", Value::from("submenu"))],
                vec![l2],
            ),
            node(
                8,
                &[label("Hidden"), ("visible", Value::from(false))],
                vec![],
            ),
            node(
                9,
                &[label("Later"), ("enabled", Value::from(false))],
                vec![],
            ),
        ],
    )
}

fn labels(n: &MenuNode) -> Vec<&str> {
    n.children.iter().map(|c| c.label.as_str()).collect()
}

/// The parsed fake menu: hidden rows gone, three levels, the fourth cut.
fn check_menu(root: &MenuNode) {
    assert_eq!(labels(root), ["Open", "", "Mute", "More", "Later"]);
    assert!(root.children[1].separator);
    assert_eq!(root.children[2].toggle, Toggle::Check(true));
    assert!(!root.children[4].enabled);
    let more = &root.children[3];
    assert_eq!(labels(more), ["Level 2"]);
    let l3 = &more.children[0].children[0];
    assert_eq!(l3.label, "Level 3");
    assert!(
        l3.children.is_empty(),
        "past {} levels rows are cut",
        tray::MAX_DEPTH
    );
}

#[test]
fn menus_parse_three_levels_deep() {
    let root = tray::menu_node(&fake_menu(), 0).expect("root");
    check_menu(&root);
}

// ---------------------------------------------------------------- on a bus

struct Daemon(std::process::Child, String);
// Drop: kills and reaps the private bus daemon.
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn daemon() -> Option<Daemon> {
    let mut c = std::process::Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--print-address=1"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .ok()?;
    let mut line = String::new();
    std::io::BufReader::new(c.stdout.take()?)
        .read_line(&mut line)
        .ok()?;
    Some(Daemon(c, line.trim().to_string()))
}

/// Futures on threads of their own (the shell uses the toolkit's main context).
fn spawn() -> tray::Spawn {
    Arc::new(|f| {
        std::thread::spawn(move || async_io::block_on(f));
    })
}

type Calls = async_channel::Sender<String>;
/// `ToolTip`: icon name, pictures, title, description.
type ToolTip = (String, Vec<(i32, i32, Vec<u8>)>, String, String);
/// A menu row as `GetLayout` returns it: id, properties, children.
type Row = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

struct FakeItem {
    icon: Arc<Mutex<String>>,
    calls: Calls,
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl FakeItem {
    #[zbus(property)]
    fn id(&self) -> String {
        "fake-app".into()
    }
    #[zbus(property)]
    fn title(&self) -> String {
        "Fake".into()
    }
    #[zbus(property)]
    fn status(&self) -> String {
        "Active".into()
    }
    #[zbus(property)]
    fn icon_name(&self) -> String {
        self.icon.lock().unwrap().clone()
    }
    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
        let px = |s: i32| (s, s, [255u8, 255, 0, 0].repeat((s * s) as usize));
        vec![px(16), px(32)]
    }
    #[zbus(property)]
    fn tool_tip(&self) -> ToolTip {
        (String::new(), Vec::new(), "Fake tip".into(), "body".into())
    }
    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn menu(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from("/Menu").unwrap()
    }
    fn activate(&self, x: i32, y: i32) {
        let _ = self.calls.try_send(format!("Activate {x} {y}"));
    }
    fn secondary_activate(&self, _x: i32, _y: i32) {
        let _ = self.calls.try_send("SecondaryActivate".into());
    }
    fn context_menu(&self, _x: i32, _y: i32) {
        let _ = self.calls.try_send("ContextMenu".into());
    }
    fn scroll(&self, delta: i32, orientation: &str) {
        let _ = self.calls.try_send(format!("Scroll {delta} {orientation}"));
    }
    #[zbus(signal)]
    async fn new_icon(e: &SignalEmitter<'_>) -> zbus::Result<()>;
}

/// A menu row in the reply's typed form.
fn typed(v: &Value) -> Row {
    let Value::Structure(s) = v else {
        unreachable!()
    };
    let f = s.fields();
    let Value::I32(id) = f[0] else { unreachable!() };
    let unwrap = |v: &Value| match v {
        Value::Value(b) => b.try_to_owned().unwrap(),
        v => v.try_to_owned().unwrap(),
    };
    let mut props = HashMap::new();
    if let Value::Dict(d) = &f[1] {
        for (k, v) in d.iter() {
            let Value::Str(k) = k else { unreachable!() };
            props.insert(k.to_string(), unwrap(v));
        }
    }
    let kids = match &f[2] {
        Value::Array(a) => a.iter().map(unwrap).collect(),
        _ => Vec::new(),
    };
    (id, props, kids)
}

struct FakeMenu {
    calls: Calls,
}

#[zbus::interface(name = "com.canonical.dbusmenu")]
impl FakeMenu {
    fn get_layout(&self, _parent: i32, _depth: i32, _props: Vec<String>) -> (u32, Row) {
        (1, typed(&fake_menu()))
    }
    fn about_to_show(&self, id: i32) -> bool {
        let _ = self.calls.try_send(format!("AboutToShow {id}"));
        false
    }
    fn event(&self, id: i32, event: &str, _data: Value<'_>, _ts: u32) {
        let _ = self.calls.try_send(format!("Event {id} {event}"));
    }
}

/// Waits (5 s at most) for a list from the host that `ok` accepts.
async fn until(
    rx: &async_channel::Receiver<Vec<Item>>,
    what: &str,
    ok: impl Fn(&[Item]) -> bool,
) -> Vec<Item> {
    let wait = async {
        loop {
            let items = rx.recv().await.expect("the host stopped");
            if ok(&items) {
                return Some(items);
            }
        }
    };
    let timeout = async {
        async_io::Timer::after(Duration::from_secs(5)).await;
        None
    };
    futures_lite::future::or(wait, timeout)
        .await
        .unwrap_or_else(|| panic!("no list where {what}"))
}

async fn call(calls: &async_channel::Receiver<String>) -> String {
    let timeout = async {
        async_io::Timer::after(Duration::from_secs(5)).await;
        "(nothing)".to_string()
    };
    futures_lite::future::or(async { calls.recv().await.unwrap() }, timeout).await
}

async fn has_owner(conn: &zbus::Connection, name: &str) -> bool {
    zbus::fdo::DBusProxy::new(conn)
        .await
        .unwrap()
        .name_has_owner(name.try_into().unwrap())
        .await
        .unwrap()
}

#[test]
fn an_app_item_shows_answers_changes_and_vanishes() {
    let Some(d) = daemon() else {
        eprintln!("skipped: no dbus-daemon");
        return;
    };
    async_io::block_on(async {
        let (tray, rx) = Tray::serve(Some(&d.1), spawn()).await.expect("serve");
        assert!(
            Tray::serve(Some(&d.1), spawn()).await.is_err(),
            "a second watcher is refused"
        );

        // The app: an item and its menu on a connection of its own.
        let (ctx, crx) = async_channel::unbounded();
        let icon = Arc::new(Mutex::new("fake-app-symbolic".to_string()));
        let app = zbus::connection::Builder::address(d.1.as_str())
            .unwrap()
            .serve_at(
                tray::ITEM_PATH,
                FakeItem {
                    icon: icon.clone(),
                    calls: ctx.clone(),
                },
            )
            .unwrap()
            .serve_at("/Menu", FakeMenu { calls: ctx })
            .unwrap()
            .build()
            .await
            .unwrap();
        let me = app.unique_name().unwrap().to_string();
        let id = format!("{me}{}", tray::ITEM_PATH);

        // Someone else watching the watcher, as other apps do.
        let observer = zbus::connection::Builder::address(d.1.as_str())
            .unwrap()
            .build()
            .await
            .unwrap();
        let watcher = zbus::Proxy::new(
            &observer,
            tray::WATCHER_NAME,
            tray::WATCHER_PATH,
            tray::WATCHER_NAME,
        )
        .await
        .unwrap();
        let mut gone = watcher
            .receive_signal("StatusNotifierItemUnregistered")
            .await
            .unwrap();
        assert!(
            watcher
                .get_property::<bool>("IsStatusNotifierHostRegistered")
                .await
                .unwrap()
        );

        let () = app
            .call_method(
                Some(tray::WATCHER_NAME),
                tray::WATCHER_PATH,
                Some(tray::WATCHER_NAME),
                "RegisterStatusNotifierItem",
                &(tray::ITEM_PATH,),
            )
            .await
            .unwrap()
            .body()
            .deserialize()
            .unwrap();
        let items = until(&rx, "the item shows", |i| i.len() == 1).await;
        let it = &items[0];
        assert_eq!(it.id, id);
        assert_eq!(it.app, "fake-app");
        assert_eq!(it.title, "Fake");
        assert_eq!(it.tooltip, "Fake tip");
        assert_eq!(it.status, Status::Active);
        assert!(it.shown());
        assert_eq!(it.icon_name, "fake-app-symbolic");
        let px = it.pixmap.as_ref().expect("a picture");
        assert_eq!(
            (px.width, px.height),
            (32, 32),
            "the size that fits the bar"
        );
        assert_eq!(&px.rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(it.menu.as_deref(), Some("/Menu"));
        assert!(!it.is_menu);
        assert_eq!(tray.items(), items);
        let listed: Vec<String> = watcher
            .get_property("RegisteredStatusNotifierItems")
            .await
            .unwrap();
        assert_eq!(listed, vec![id.clone()]);

        // Clicks, wheel and menu reach the app.
        tray.activate(&id).await.unwrap();
        assert_eq!(call(&crx).await, "Activate 0 0");
        tray.secondary_activate(&id).await.unwrap();
        assert_eq!(call(&crx).await, "SecondaryActivate");
        let (delta, dir) = tray::scroll_args(0.0, -1.0).unwrap();
        tray.scroll(&id, delta, dir).await.unwrap();
        assert_eq!(call(&crx).await, "Scroll 120 vertical");
        let menu = tray.menu(&id).await.unwrap().expect("a menu");
        assert_eq!(call(&crx).await, "AboutToShow 0");
        check_menu(&menu);
        tray.menu_clicked(&id, 1).await.unwrap();
        assert_eq!(call(&crx).await, "Event 1 clicked");

        // A new icon is read again.
        *icon.lock().unwrap() = "fake-app-busy-symbolic".into();
        FakeItem::new_icon(&SignalEmitter::new(&app, tray::ITEM_PATH).unwrap())
            .await
            .unwrap();
        until(&rx, "the new icon shows", |i| {
            i.first()
                .is_some_and(|i| i.icon_name == "fake-app-busy-symbolic")
        })
        .await;

        // The app leaves the bus: its item goes, and the watcher says so.
        app.close().await.unwrap();
        until(&rx, "the item is gone", |i| i.is_empty()).await;
        let m = futures_lite::StreamExt::next(&mut gone).await.unwrap();
        assert_eq!(m.body().deserialize::<String>().unwrap(), id);
        assert!(tray.items().is_empty());

        // Stopped: the name is free and a new watcher may start.
        assert!(has_owner(&observer, tray::WATCHER_NAME).await);
        tray.stop().await;
        let mut free = false;
        for _ in 0..50 {
            if !has_owner(&observer, tray::WATCHER_NAME).await {
                free = true;
                break;
            }
            async_io::Timer::after(Duration::from_millis(20)).await;
        }
        assert!(free, "the watcher name is given up");
        let (again, _rx) = Tray::serve(Some(&d.1), spawn()).await.expect("serve again");
        again.stop().await;
    });
}

#[test]
fn an_item_that_cannot_be_read_does_not_stay() {
    let Some(d) = daemon() else {
        eprintln!("skipped: no dbus-daemon");
        return;
    };
    async_io::block_on(async {
        let (tray, _rx) = Tray::serve(Some(&d.1), spawn()).await.expect("serve");
        let app = zbus::connection::Builder::address(d.1.as_str())
            .unwrap()
            .build()
            .await
            .unwrap();
        // Registered, but nothing answers at that path.
        let () = app
            .call_method(
                Some(tray::WATCHER_NAME),
                tray::WATCHER_PATH,
                Some(tray::WATCHER_NAME),
                "RegisterStatusNotifierItem",
                &("/Nothing",),
            )
            .await
            .unwrap()
            .body()
            .deserialize()
            .unwrap();
        let mut left = Vec::new();
        // An app that does not answer is given `CALL_TIMEOUT` before its item goes.
        for _ in 0..250 {
            // Uncached: the property is read from the watcher each time.
            let reply = app
                .call_method(
                    Some(tray::WATCHER_NAME),
                    tray::WATCHER_PATH,
                    Some("org.freedesktop.DBus.Properties"),
                    "Get",
                    &(tray::WATCHER_NAME, "RegisteredStatusNotifierItems"),
                )
                .await
                .unwrap();
            let v: zbus::zvariant::OwnedValue = reply.body().deserialize().unwrap();
            left = Vec::<String>::try_from(v).unwrap();
            if left.is_empty() {
                break;
            }
            async_io::Timer::after(Duration::from_millis(20)).await;
        }
        assert!(left.is_empty(), "still listed: {left:?}");
        assert!(tray.items().is_empty());
        tray.stop().await;
    });
}

/// Stability S5: the tray turned off and on within one debounce. The new watcher asks for the
/// name while the old one is still giving it up: it is refused with `NameTaken`, tries again
/// [`tray::SERVE_GAP`] later, and gets it.
#[test]
fn a_watcher_started_while_the_last_one_stops_gets_the_name() {
    let Some(d) = daemon() else {
        eprintln!("skipped: no dbus-daemon");
        return;
    };
    let other = zbus::Error::Failure("no bus".into());
    assert!(
        !tray::serve_again(&other, 1),
        "only a taken name is tried again"
    );
    assert!(!tray::serve_again(
        &zbus::Error::NameTaken,
        tray::SERVE_TRIES
    ));
    async_io::block_on(async {
        let (old, _rx) = Tray::serve(Some(&d.1), spawn()).await.expect("serve");
        let e = Tray::serve(Some(&d.1), spawn()).await.err().expect("taken");
        assert!(matches!(e, zbus::Error::NameTaken), "{e}");
        assert!(tray::serve_again(&e, 1));
        let stopping = async {
            async_io::Timer::after(Duration::from_millis(250)).await;
            old.stop().await;
        };
        let start = async {
            let mut tries = 1;
            loop {
                match Tray::serve(Some(&d.1), spawn()).await {
                    Ok(x) => break (x, tries),
                    Err(e) if tray::serve_again(&e, tries) => {
                        tries += 1;
                        async_io::Timer::after(tray::SERVE_GAP).await;
                    }
                    Err(e) => panic!("not served after {tries} tries: {e}"),
                }
            }
        };
        let ((), ((new, _rx), tries)) = futures_lite::future::zip(stopping, start).await;
        assert!(tries > 1, "the first try met the old watcher");
        new.stop().await;
    });
}
