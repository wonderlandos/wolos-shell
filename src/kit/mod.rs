//! Rust port of the React.Iara/IaraKit pieces the shell uses: layer surfaces, theme loading, glass widgets.
pub mod style;
pub mod surface;
pub mod widgets;

use gtk::glib;

/// Local wall clock as pure-logic fields.
pub fn now() -> crate::clock::Stamp {
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
    if let Err(e) = std::process::Command::new(p).args(a).spawn() {
        eprintln!("wol: {p}: {e}");
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
            eprintln!("snapshot {name}");
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
