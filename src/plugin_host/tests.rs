// SPDX-License-Identifier: GPL-2.0-or-later
//! Unit tests of the shell plugin host: the bar, desktop cards, tiles, hotkeys and the run grant.
use super::*;
use shared::layout::Edge;

const STATS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../plugins/shell/org.wolos.stats"
);
const SAMPLE: &str = r#"{"cpu_percent": 12.4, "memory": {"total": 8589934592, "used": 3521873182}, "uptime_secs": 7260}"#;

/// `wol sys stats --json` answers with a sample; every command is remembered.
#[derive(Default)]
struct Fake {
    ran: RefCell<Vec<String>>,
    fail: bool,
}
impl Services for Fake {
    fn now(&self) -> Stamp {
        Stamp {
            year: 2026,
            month: 11,
            day: 2,
            weekday: 1,
            hour: 9,
            minute: 41,
            second: 0,
        }
    }
    fn system(&self) -> wol_lua::SystemInfo {
        unreachable!()
    }
    fn notify(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn run(&self, _plugin: &str, argv: &[String], _t: u32) -> Result<String, String> {
        self.ran.borrow_mut().push(argv.join(" "));
        if self.fail {
            return Err("no wol here".into());
        }
        Ok(SAMPLE.into())
    }
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wol-plugin-host-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
    }
}

/// An extra plugin: (id, plugin.toml, other files as (name, text)).
type Extra<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)]);

/// A user plugin folder with the stats example (and `extra` plugins), all enabled.
fn setup(tag: &str, extra: &[Extra<'_>]) -> Paths {
    let root = tmp(tag);
    let user = root.join("data/wolos/plugins");
    std::fs::create_dir_all(&user).unwrap();
    // Shipped under the system folder: it may use org.wolos.*.
    copy_dir(Path::new(STATS), &root.join("system/org.wolos.stats"));
    let paths = Paths {
        dirs: vec![(user.clone(), false), (root.join("system"), true)],
        config_home: root.join("config"),
        state_home: root.join("state"),
    };
    let mut e = Enabled::default();
    for (id, toml, files) in extra {
        let d = user.join(id);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("plugin.toml"), toml).unwrap();
        for (f, text) in *files {
            std::fs::write(d.join(f), text).unwrap();
        }
    }
    for p in cp::load_all(&paths.dirs) {
        let m = p
            .manifest
            .as_ref()
            .unwrap_or_else(|e| panic!("{}: {e}", p.id));
        e.enable(m);
    }
    cp::save_enabled(&paths.config_home, &e).unwrap();
    paths
}

fn host(paths: &Paths, kinds: &'static [Kind], s: Rc<Fake>) -> Host {
    let mut h = Host::with_services(paths.clone(), kinds, s);
    assert!(h.apply(read_snapshot(paths, kinds)));
    h
}

fn manifest(id: &str, body: &str) -> String {
    format!(
        "schema = 1\nid = \"{id}\"\nname = \"{id}\"\nversion = \"1.0\"\nauthor = \"t\"\nlicense = \"MIT\"\ndescription = \"t\"\n{body}"
    )
}

#[test]
fn the_stats_example_fills_the_bar_and_a_card() {
    let paths = setup("stats", &[]);
    let fake = Rc::new(Fake::default());
    let mut h = host(&paths, BAR_KINDS, fake.clone());
    let [left, centre, right] = h.bar(&Layout::default());
    let item = BarItem::Plugin {
        plugin: "org.wolos.stats".into(),
        id: "bar".into(),
    };
    assert_eq!(
        right.first(),
        Some(&item),
        "side = right: before the system items"
    );
    assert_eq!((left, centre), (Layout::default().left, Vec::new()));
    assert_eq!(right.last(), Some(&BarItem::Clock));
    let r = h
        .render(Kind::Menubar, "org.wolos.stats", "bar")
        .unwrap()
        .unwrap();
    let text = format!("{r:?}");
    assert!(
        text.contains("CPU 12%") && text.contains("MEM 41%"),
        "{text}"
    );
    assert_eq!(fake.ran.borrow()[0], "wol sys stats --json");
    assert_eq!(h.refresh(Kind::Menubar, "org.wolos.stats", "bar"), Some(5));
    let pop = h
        .popover("org.wolos.stats", "bar")
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(format!("{pop:?}").contains("Open Top"), "{pop:?}");
    assert_eq!(
        h.render(Kind::Widget, "org.wolos.stats", "card"),
        None,
        "the bar host runs no cards"
    );

    let mut d = host(&paths, DESKTOP_KINDS, fake);
    let cards = d.cards();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].key, "plugin-org-wolos-stats-card");
    assert_eq!(
        d.card(&cards[0].key),
        Some(("org.wolos.stats".into(), "card".into()))
    );
    assert_eq!(cards[0].sizes, [Size::Small, Size::Medium]);
    assert!(
        d.render(Kind::Widget, "org.wolos.stats", "card")
            .unwrap()
            .is_ok()
    );
    assert_eq!(d.render(Kind::Menubar, "org.wolos.stats", "bar"), None);

    let keys = hotkeys_of(&read_snapshot(&paths, &[]));
    assert_eq!(keys.len(), 1, "{keys:?}");
    assert_eq!(keys[0].shortcut_id(), "plugin:org.wolos.stats/top");
    assert_eq!(keys[0].combo, Combo::parse("SUPER SHIFT", "t").unwrap());
    assert_eq!(keys[0].group, "Stats");
}

#[test]
fn three_failures_turn_a_plugin_off_and_the_rest_keep_working() {
    let bad = manifest(
        "test.broken",
        "[[menubar]]\nid = \"bar\"\nscript = \"bar.lua\"\nside = \"left\"\n",
    );
    let paths = setup(
        "health",
        &[(
            "test.broken",
            &bad,
            &[(
                "bar.lua",
                "return wol.menubar { render = function() error('boom') end }",
            )],
        )],
    );
    let mut h = host(&paths, BAR_KINDS, Rc::new(Fake::default()));
    let [left, ..] = h.bar(&Layout::default());
    assert_eq!(
        left.last(),
        Some(&BarItem::Plugin {
            plugin: "test.broken".into(),
            id: "bar".into()
        })
    );
    for n in 1..=2 {
        let r = h.render(Kind::Menubar, "test.broken", "bar").unwrap();
        assert!(r.unwrap_err().contains("boom"), "failure {n}");
        assert!(h.take_toasts().is_empty());
        assert!(
            h.render(Kind::Menubar, "org.wolos.stats", "bar")
                .unwrap()
                .is_ok(),
            "isolated"
        );
    }
    assert!(
        h.render(Kind::Menubar, "test.broken", "bar")
            .unwrap()
            .is_err()
    );
    let toasts = h.take_toasts();
    assert_eq!(toasts.len(), 1, "{toasts:?}");
    assert!(toasts[0].contains("turned off"), "{toasts:?}");
    assert!(!h.has(Kind::Menubar, "test.broken", "bar"));
    assert!(
        !cp::load_enabled(&paths.config_home).is_enabled("test.broken"),
        "saved"
    );
    assert!(
        h.render(Kind::Menubar, "org.wolos.stats", "bar")
            .unwrap()
            .is_ok()
    );
    let [left, ..] = h.bar(&Layout::default());
    assert_eq!(left, Layout::default().left, "gone from the bar");
}

/// R2C-3: a script that runs past its deadline is stopped and stays stopped; three stops
/// turn it off without slowing the bar down.
#[test]
fn a_runaway_item_is_stopped_three_times_then_turned_off() {
    let slow = manifest(
        "test.slow",
        "[[menubar]]\nid = \"bar\"\nscript = \"bar.lua\"\n",
    );
    let paths = setup(
        "slow",
        &[(
            "test.slow",
            &slow,
            &[(
                "bar.lua",
                "return wol.menubar { render = function() local t = {} while true do table.move(t, 1, 2^20 - 1, 2) end; end }",
            )],
        )],
    );
    let mut h = host(&paths, BAR_KINDS, Rc::new(Fake::default()));
    let start = std::time::Instant::now();
    for _ in 0..3 {
        assert_eq!(
            h.render(Kind::Menubar, "test.slow", "bar")
                .unwrap()
                .unwrap_err(),
            wol_lua::TOO_LONG
        );
    }
    assert!(
        start.elapsed() < 3 * DEADLINES.render,
        "{:?}",
        start.elapsed()
    );
    assert!(!h.has(Kind::Menubar, "test.slow", "bar"));
    assert_eq!(h.take_toasts().len(), 1);
}

#[test]
fn the_layout_places_items_and_drops_plugins_that_are_off() {
    let paths = setup("layout", &[]);
    let h = host(&paths, BAR_KINDS, Rc::new(Fake::default()));
    let item = BarItem::Plugin {
        plugin: "org.wolos.stats".into(),
        id: "bar".into(),
    };
    let gone = BarItem::Plugin {
        plugin: "test.gone".into(),
        id: "x".into(),
    };
    let mut l = Layout::taskbar();
    l.centre = vec![item.clone(), gone.clone()];
    let [_, centre, right] = h.bar(&l);
    assert_eq!(
        centre,
        std::slice::from_ref(&item),
        "placed once where the layout says"
    );
    assert!(!right.contains(&item));
    assert_eq!(l.bar, Edge::Bottom);
    let mut e = cp::load_enabled(&paths.config_home);
    e.disable("org.wolos.stats");
    cp::save_enabled(&paths.config_home, &e).unwrap();
    let mut h2 = Host::with_services(paths.clone(), BAR_KINDS, Rc::new(Fake::default()));
    h2.apply(read_snapshot(&paths, BAR_KINDS));
    let [_, centre, _] = h2.bar(&l);
    assert!(centre.is_empty());
}

/// The plugin tiles reach the real Control Center, not only the golden (audit5 S7-3).
#[test]
fn the_control_center_panel_installs_the_plugin_tile_source() {
    let src = include_str!("../ui/control_center/mod.rs");
    let body = &src[src.find("pub fn panel(").unwrap()..];
    let body = &body[..body
        .find(
            "
}
",
        )
        .unwrap()];
    assert!(
        body.contains("set_tile_source(Some("),
        "panel() installs a TileSource"
    );
    assert!(body.contains("TileCache::new("), "the plugin TileCache");
}

#[test]
fn dock_tiles_and_hotkeys_of_a_demo_plugin() {
    let demo = manifest(
        "user.demo",
        "permissions = [\"toggles\", \"run\"]\n\
         [[dock]]\nid = \"term\"\nname = \"Terminal\"\nicon = \"utilities-terminal\"\nexec = [\"foot\"]\n\
         [[tile]]\nid = \"awake-tile\"\nicon = \"weather-clear-symbolic\"\nlabel = \"Stay Awake\"\ntoggle = \"awake\"\n\
         [[toggle]]\nid = \"awake\"\non = [\"true\"]\noff = [\"true\"]\n\
         [[hotkey]]\nid = \"top\"\ndefault = \"super+shift+y\"\nexec = [\"foot\", \"-e\", \"btop it's\"]\n\
         [[hotkey]]\nid = \"clash\"\ndefault = \"super+space\"\nexec = [\"foot\"]\n",
    );
    let paths = setup("demo", &[("user.demo", &demo, &[])]);
    let h = host(&paths, BAR_KINDS, Rc::new(Fake::default()));
    let items = h.dock(&Layout::default());
    let term = DockItem::Plugin {
        plugin: "user.demo".into(),
        id: "term".into(),
    };
    assert_eq!(items[..3], [DockItem::Apps, term, DockItem::Separator]);
    assert_eq!(h.dock_tiles()[0].exec, ["foot"]);
    assert!(
        h.dock(&Layout::taskbar()).len() == 1,
        "no Dock edge: only the plugin item"
    );

    let t = tiles(&paths);
    assert_eq!(t.len(), 1);
    assert_eq!((t[0].label.as_str(), t[0].on), ("Stay Awake", false));
    set_tile(&paths, "user.demo", "awake-tile", true).unwrap();
    assert!(tiles(&paths)[0].on);
    set_tile(&paths, "user.demo", "awake-tile", false).unwrap();
    assert!(!tiles(&paths)[0].on);
    assert!(set_tile(&paths, "user.demo", "nope", true).is_err());

    // The Control Center source over the same folder lists the tile (audit5 S7-3).
    use crate::ui::control_center::tiles::TileSource;
    let cache = TileCache::new(paths.clone());
    assert!(cache.tiles().is_empty(), "nothing before the first read");
    cache.fill(tiles(&paths));
    let cc = cache.tiles();
    assert_eq!(cc.len(), 1);
    assert_eq!(
        (cc[0].id.as_str(), cc[0].label.as_str(), cc[0].on),
        ("user.demo.awake-tile", "Stay Awake", false)
    );
    assert_eq!(
        cache.find("user.demo.awake-tile"),
        Some(("user.demo".into(), "awake-tile".into()))
    );

    let all = hotkeys(&paths);
    assert_eq!(all.len(), 3, "the demo's two and the stats example's ⇧⌘T");
    let keys: Vec<Hotkey> = all
        .into_iter()
        .filter(|k| k.plugin == "user.demo")
        .collect();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0].shortcut_id(), "plugin:user.demo/top");
    assert_eq!(keys[0].group, "user.demo", "no group: the plugin's name");
    let space = Shortcut {
        id: "exec wol-search".into(),
        name: "Search".into(),
        group: Group::Custom,
        default: Combo::parse("SUPER", "space"),
        combo: Combo::parse("SUPER", "space"),
        bind: Bind {
            flags: String::new(),
            combo: Combo::parse("SUPER", "space").unwrap(),
            dispatcher: "exec".into(),
            args: "wol-search".into(),
        },
        editable: true,
    };
    let b = binds(&keys, &Overrides::default(), std::slice::from_ref(&space));
    assert_eq!(b.len(), 1, "the system's ⌘Space wins: {b:?}");
    assert_eq!(b[0].1.args, "foot -e 'btop it'\\''s'");
    assert_eq!(b[0].0, Combo::parse("SUPER SHIFT", "y").unwrap());
    let mut o = Overrides::default();
    o.changed.insert("plugin:user.demo/top".into(), None);
    assert!(
        binds(&keys, &o, &[])
            .iter()
            .all(|(k, _)| !k.same(&keys[0].combo)),
        "turned off"
    );
}

/// Audit C8-4: no more than MAX_NOTIFY posts wait on wold at once.
#[test]
fn notify_slots_are_bounded() {
    let took = (0..MAX_NOTIFY * 3).filter(|_| notify_slot()).count();
    assert_eq!(took, MAX_NOTIFY);
    NOTIFY_IN_FLIGHT.fetch_sub(took, Ordering::AcqRel);
    assert!(notify_slot());
    NOTIFY_IN_FLIGHT.fetch_sub(1, Ordering::AcqRel);
}

/// Audit C8-1: toggle, Dock and hotkey commands that are not `wol` verbs run only with the
/// `run` permission granted, not merely declared.
#[test]
fn commands_need_the_run_grant() {
    let paths = setup("run-grant", &[]);
    let mark = paths.state_home.join("ran");
    let demo = manifest(
        "user.cmds",
        &format!(
            "permissions = [\"toggles\", \"run\"]\n\
             [[dock]]\nid = \"term\"\nname = \"T\"\nicon = \"utilities-terminal\"\nexec = [\"foot\"]\n\
             [[tile]]\nid = \"t\"\nicon = \"x\"\nlabel = \"T\"\ntoggle = \"go\"\n\
             [[toggle]]\nid = \"go\"\non = [\"touch\", \"{}\"]\noff = [\"true\"]\n\
             [[hotkey]]\nid = \"k\"\ndefault = \"super+shift+y\"\nexec = [\"foot\"]\n\
             [[hotkey]]\nid = \"w\"\ndefault = \"super+shift+u\"\nexec = [\"wol\", \"focus\", \"on\"]\n",
            mark.display()
        ),
    );
    let dir = paths.dirs[0].0.join("user.cmds");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("plugin.toml"), demo).unwrap();
    // Enabled with everything but `run` (a grant from before `run` was declared).
    let mut e = cp::load_enabled(&paths.config_home);
    e.enabled.insert(
        "user.cmds".into(),
        cp::Grant {
            permissions: vec![Permission::Toggles],
            hosts: Vec::new(),
        },
    );
    cp::save_enabled(&paths.config_home, &e).unwrap();

    let err = set_tile(&paths, "user.cmds", "t", true).unwrap_err();
    assert!(err.contains(NOT_ALLOWED_TO_RUN), "{err}");
    assert!(!mark.exists(), "nothing ran");
    let t = tiles(&paths);
    assert!(t.iter().any(|t| t.plugin == "user.cmds" && !t.on), "{t:?}");
    let h = host(&paths, BAR_KINDS, Rc::new(Fake::default()));
    assert!(
        h.dock_tiles().iter().all(|t| t.plugin != "user.cmds"),
        "no Dock entry without run"
    );
    let keys: Vec<String> = hotkeys(&paths)
        .into_iter()
        .filter(|k| k.plugin == "user.cmds")
        .map(|k| k.id)
        .collect();
    assert_eq!(keys, ["w"], "only the wol verb is bound");

    // Granted: the toggle runs.
    e.enabled.get_mut("user.cmds").unwrap().permissions =
        vec![Permission::Toggles, Permission::Run];
    cp::save_enabled(&paths.config_home, &e).unwrap();
    std::fs::create_dir_all(&paths.state_home).unwrap();
    set_tile(&paths, "user.cmds", "t", true).unwrap();
    assert!(mark.exists());
}

/// audit5 S9-4: after `hyprctl reload` the diff against `last` sent nothing, so plugin
/// hotkeys never came back. A replay sends every bind.
#[test]
fn a_config_reload_replays_every_plugin_bind() {
    let bind = |k: &str, a: &str| {
        let combo = Combo::parse("SUPER SHIFT", k).unwrap();
        (
            combo.clone(),
            Bind {
                flags: String::new(),
                combo,
                dispatcher: "exec".into(),
                args: a.into(),
            },
        )
    };
    let last = vec![bind("y", "foot"), bind("u", "wol-search")];
    assert!(
        shortcuts::live(&last, &last).is_empty(),
        "the diff alone sends nothing"
    );
    let again = replay_commands(&last);
    assert_eq!(again.len(), 2, "{again:?}");
    assert_eq!(
        again,
        [
            "keyword bind SUPER SHIFT,y,exec,foot",
            "keyword bind SUPER SHIFT,u,exec,wol-search"
        ]
    );
}

#[test]
fn a_reload_sees_a_new_plugin_and_an_equal_read_does_nothing() {
    let paths = setup("reload", &[]);
    let mut h = host(&paths, BAR_KINDS, Rc::new(Fake::default()));
    let g = h.generation();
    let same = h.snap.borrow().clone();
    assert!(!h.apply(same), "nothing changed");
    assert_eq!(h.generation(), g);
    let toml = manifest(
        "user.clock",
        "[[menubar]]\nid = \"t\"\nscript = \"t.lua\"\nside = \"centre\"\n",
    );
    let d = paths.dirs[0].0.join("user.clock");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("plugin.toml"), toml).unwrap();
    std::fs::write(
        d.join("t.lua"),
        "return wol.menubar { render = function() return wol.label { 'hi' } end }",
    )
    .unwrap();
    let mut e = cp::load_enabled(&paths.config_home);
    e.enable(&cp::load(&d, false).manifest.unwrap());
    cp::save_enabled(&paths.config_home, &e).unwrap();
    assert!(h.apply(read_snapshot(&paths, BAR_KINDS)));
    assert!(h.generation() > g);
    assert!(h.has(Kind::Menubar, "user.clock", "t"));
    let [_, centre, _] = h.bar(&Layout::default());
    assert_eq!(centre.len(), 1);
}

#[test]
fn runner_answers_later_without_blocking() {
    let r = Runner::default();
    let gate = Arc::new(Mutex::new(()));
    let held = gate.lock().unwrap();
    let g = gate.clone();
    let started = std::time::Instant::now();
    let first = r.ask("k".into(), move || {
        let _g = g.lock();
        Ok("out".into())
    });
    assert_eq!(first, Err(RUNNING.into()));
    assert!(started.elapsed() < Duration::from_millis(50));
    assert!(r.busy());
    drop(held);
    while r.busy() {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(r.take_fresh());
    assert!(!r.take_fresh());
    assert_eq!(
        r.ask("k".into(), || Ok("new".into())),
        Ok("out".into()),
        "the last answer"
    );
}

/// The plan's acceptance path without a display: `wol plugin new demo --kinds
/// menubar,widget,tile,hotkey,theme` (the same `scaffold`), installed in the user folder and
/// enabled, shows up in the bar, on the desktop, as a Control Center tile and as a shortcut.
#[test]
fn a_scaffolded_demo_reaches_bar_desktop_tiles_and_shortcuts() {
    let paths = setup("scaffold", &[]);
    let kinds = [
        Kind::Menubar,
        Kind::Widget,
        Kind::Tile,
        Kind::Hotkey,
        Kind::Theme,
    ];
    let files = cp::scaffold("user.demo", "Demo", &kinds).unwrap();
    let d = paths.dirs[0].0.join("user.demo");
    for (f, text) in files {
        let p = d.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    let r = cp::check(&d, false);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    let mut e = cp::load_enabled(&paths.config_home);
    e.enable(r.manifest.as_ref().unwrap());
    cp::save_enabled(&paths.config_home, &e).unwrap();

    let mut bar = Host::with_services(paths.clone(), BAR_KINDS, Rc::new(Fake::default()));
    bar.apply(read_snapshot(&paths, BAR_KINDS));
    let entry = bar.snap.borrow().manifest("user.demo").unwrap().1.menubar[0]
        .id
        .clone();
    let placed = bar.bar(&Layout::default()).concat();
    assert!(placed.contains(&BarItem::Plugin {
        plugin: "user.demo".into(),
        id: entry.clone()
    }));
    assert!(
        bar.render(Kind::Menubar, "user.demo", &entry)
            .unwrap()
            .is_ok()
    );

    let mut desk = Host::with_services(paths.clone(), DESKTOP_KINDS, Rc::new(Fake::default()));
    desk.apply(read_snapshot(&paths, DESKTOP_KINDS));
    let card = desk
        .cards()
        .into_iter()
        .find(|c| c.plugin == "user.demo")
        .unwrap();
    assert!(
        desk.render(Kind::Widget, "user.demo", &card.id)
            .unwrap()
            .is_ok()
    );

    assert!(tiles(&paths).iter().any(|t| t.plugin == "user.demo"));
    let keys = hotkeys(&paths);
    let k: Vec<&Hotkey> = keys.iter().filter(|k| k.plugin == "user.demo").collect();
    assert_eq!(k.len(), 1);
    assert_eq!(
        shortcuts(&keys, &Overrides::default())
            .iter()
            .filter(|s| s.id.starts_with("plugin:user.demo/"))
            .count(),
        1
    );
}

#[test]
fn widget_keys_are_widget_ids() {
    for (p, id) in [
        ("org.wolos.stats", "card"),
        ("a.b", "x_y"),
        (&"a".repeat(60)[..], "card"),
    ] {
        let k = widget_key(&format!("{p}.z"), id);
        assert!(shared::widgets::is_package_id(&k), "{k}");
        assert!(k.starts_with(WIDGET_PREFIX));
    }
    assert_eq!(sh_word("foot"), "foot");
    assert_eq!(sh_word("a b"), "'a b'");
    assert_eq!(sh_word(""), "''");
}
