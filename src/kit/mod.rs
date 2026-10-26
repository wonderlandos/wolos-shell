//! Rust port of the React.Iara/IaraKit pieces the shell uses: layer surfaces, theme loading, glass widgets.
pub mod check;
pub mod style;
pub mod surface;
pub mod widgets;

/// Installed WolOS icon dir, else the repo design tree (dev, tests).
pub const ICON_DIRS: [&str; 3] = [
    "/usr/local/share/wol/icons",
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../design/icons/apps"),
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../design/brand"),
];

/// Full-colour SVG for an app icon name.
pub fn icon_file(name: &str) -> Option<std::path::PathBuf> {
    ICON_DIRS
        .iter()
        .map(|d| std::path::Path::new(d).join(format!("{name}.svg")))
        .find(|p| p.exists())
}

/// Installed store icons (`image/build.sh`), else the repo tree (dev, tests).
pub const STORE_ICON_DIRS: [&str; 2] = [
    "/usr/local/share/wolos/store-icons",
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../design/icons/store"),
];

/// Official icon for a store catalog id.
pub fn store_icon_file(id: &str) -> Option<std::path::PathBuf> {
    STORE_ICON_DIRS
        .iter()
        .map(|d| std::path::Path::new(d).join(format!("{id}.svg")))
        .find(|p| p.exists())
}

use gtk::glib;

/// Fixed time for golden renders (`WOL_FAKE_NOW=1`): Mon 26 Oct 14:05.
pub const FAKE_NOW: crate::clock::Stamp = crate::clock::Stamp {
    weekday: 1,
    day: 26,
    month: 10,
    hour: 14,
    minute: 5,
    second: 0,
};

/// Local wall clock as pure-logic fields.
pub fn now() -> crate::clock::Stamp {
    if std::env::var_os("WOL_FAKE_NOW").is_some() {
        return FAKE_NOW;
    }
    let t = glib::DateTime::now_local().expect("local time");
    crate::clock::Stamp {
        weekday: t.day_of_week() as u8,
        day: t.day_of_month() as u8,
        month: t.month() as u8,
        hour: t.hour() as u8,
        minute: t.minute() as u8,
        second: t.second() as u8,
    }
}

/// Calls `tick` now and then on every minute boundary (one wakeup a minute).
pub fn every_minute(tick: impl Fn() + 'static) {
    let tick = std::rc::Rc::new(tick);
    fn arm(tick: std::rc::Rc<dyn Fn()>) {
        let secs = crate::clock::secs_to_next_minute(now().second);
        glib::timeout_add_seconds_local_once(secs, move || {
            tick();
            arm(tick);
        });
    }
    tick();
    arm(tick);
}

/// Spawns argv detached; errors go to stderr.
pub fn spawn(argv: &[String]) {
    let Some((p, a)) = argv.split_first() else {
        return;
    };
    match std::process::Command::new(p).args(a).spawn() {
        // Reap off the main thread so a long-running shell never collects zombies.
        Ok(mut c) => {
            std::thread::spawn(move || c.wait());
        }
        Err(e) => eprintln!("wol: {p}: {e}"),
    }
}

pub fn runtime_dir() -> String {
    std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into())
}

pub fn config_home() -> String {
    std::env::var("XDG_CONFIG_HOME")
        .unwrap_or_else(|_| format!("{}/.config", std::env::var("HOME").unwrap_or_default()))
}

/// Dev aid (no grim needed): `WOL_SNAPSHOT=<dir>` renders `win` to `<dir>/<name>.png` 700 ms after it maps.
pub fn snapshot_on_map(win: &impl gtk::prelude::IsA<gtk::Window>, name: &'static str) {
    use gtk::prelude::*;
    let Some(dir) = std::env::var_os("WOL_SNAPSHOT") else {
        return;
    };
    let win = win.as_ref().clone();
    win.connect_map(move |w| {
        let (w, dir) = (w.clone(), dir.clone());
        glib::timeout_add_local_once(std::time::Duration::from_millis(700), move || {
            let p = gtk::WidgetPaintable::new(Some(&w));
            let (wd, ht) = (w.width().max(1), w.height().max(1));
            let snap = gtk::Snapshot::new();
            p.snapshot(&snap, wd as f64, ht as f64);
            let Some(node) = snap.to_node() else {
                eprintln!("snapshot {name}: empty");
                return;
            };
            let Some(r) = w.native().and_then(|n| n.renderer()) else {
                eprintln!("snapshot {name}: no renderer");
                return;
            };
            let tex = r.render_texture(
                node,
                Some(&gtk::graphene::Rect::new(0.0, 0.0, wd as f32, ht as f32)),
            );
            let out = std::path::Path::new(&dir).join(format!("{name}.png"));
            if let Err(e) = tex.save_to_png(&out) {
                eprintln!("snapshot {name}: {e}");
            }
        });
    });
}
