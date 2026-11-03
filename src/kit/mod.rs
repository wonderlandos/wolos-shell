// SPDX-License-Identifier: GPL-2.0-or-later
//! Rust port of the React.Iara/IaraKit pieces the shell uses: layer surfaces, theme loading, glass widgets.
pub mod anim;
pub mod assistant;
pub mod check;
pub mod controls;
pub mod data;
pub mod fence;
pub mod form;
pub mod lifetime;
pub mod motion;
pub mod pickers;
pub mod reveal;
pub mod style;
pub mod surface;
pub mod toast;
pub mod widgets;

/// Installed WolOS icon dir, else the repo design tree (dev, tests).
pub const ICON_DIRS: [&str; 3] = [
    "/usr/share/icons/WolOS/scalable/apps",
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
    "/usr/share/wolos/store-icons",
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

/// Local wall clock as pure-logic fields. Without a usable local time zone (a broken TZ
/// database), UTC from the system clock: the bar never panics over the clock.
pub fn now() -> crate::clock::Stamp {
    if std::env::var_os("WOL_FAKE_NOW").is_some() {
        return FAKE_NOW;
    }
    let Ok(t) = glib::DateTime::now_local() else {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        return utc_stamp(secs);
    };
    crate::clock::Stamp {
        weekday: field(t.day_of_week()),
        day: field(t.day_of_month()),
        month: field(t.month()),
        hour: field(t.hour()),
        minute: field(t.minute()),
        second: field(t.second()),
    }
}

/// A calendar field (all below 60 by construction) as the clock's `u8`; 0 if one ever is not.
fn field(v: impl TryInto<u8>) -> u8 {
    v.try_into().unwrap_or(0)
}

/// UTC fields of a Unix time (civil-from-days, Howard Hinnant).
pub fn utc_stamp(secs: i64) -> crate::clock::Stamp {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    crate::clock::Stamp {
        // 1970-01-01 was a Thursday; `weekday` 1 = Monday.
        weekday: field((days + 3).rem_euclid(7) + 1),
        day: field(day),
        month: field(month),
        hour: field(rem / 3_600),
        minute: field(rem % 3_600 / 60),
        second: field(rem % 60),
    }
}

/// Calls `tick` now and then on every minute boundary (one wakeup a minute), for the session.
/// Work tied to a window uses [`lifetime::every_minute_while`].
pub fn every_minute(tick: impl Fn() + 'static) {
    lifetime::every_minute_while(&lifetime::Lifetime::new(), tick);
}

/// `wol shell <verb> <panel>` as the socket line the CLI would send; None for anything else.
pub fn shell_line(argv: &[String]) -> Option<String> {
    match argv {
        [wol, shell, rest @ ..] if wol == "wol" && shell == "shell" && !rest.is_empty() => {
            Some(rest.join(" "))
        }
        _ => None,
    }
}

/// Spawns argv detached; errors go to stderr. `wol shell ...` goes straight to the shell socket
/// (no fork/exec of the CLI: Control Center p95 was 102 ms with it).
pub fn spawn(argv: &[String]) {
    if let Some(line) = shell_line(argv) {
        use std::io::Write;
        let path = crate::protocol::socket_path(&runtime_dir());
        if std::os::unix::net::UnixStream::connect(path)
            .and_then(|mut s| s.write_all(format!("{line}\n").as_bytes()))
            .is_ok()
        {
            return;
        }
    }
    match command(argv, &[]).map(|mut c| c.spawn()) {
        // Reap off the main thread so a long-running shell never collects zombies.
        Some(Ok(mut c)) => {
            let _ = std::thread::Builder::new()
                .name("wol-reap".into())
                .spawn(move || c.wait());
        }
        Some(Err(e)) => eprintln!("wol: {}: {e}", argv[0]),
        None => {}
    }
}

/// Starts an app (Dock, Search, App Store): like [`spawn`], and the app gets the current X11
/// scale ([`user_scale_env`], read on a worker thread). Commands that are not apps (Control
/// Center toggles, `wol ...`) go through [`spawn`] and get nothing added.
pub fn spawn_app(argv: &[String]) {
    if shell_line(argv).is_some() {
        return spawn(argv);
    }
    let owned = argv.to_vec();
    let started = std::thread::Builder::new()
        .name("wol-launch".into())
        .spawn(move || {
            let env = user_scale_env();
            match command(&owned, &env).map(|mut c| c.spawn()) {
                Some(Ok(mut c)) => {
                    let _ = c.wait();
                }
                Some(Err(e)) => eprintln!("wol: {}: {e}", owned[0]),
                None => {}
            }
        });
    // No thread for the scale lookup: the app still starts, at the session's scale.
    if started.is_err() {
        spawn(argv);
    }
}

/// argv as a command with `env` added; None for an empty argv.
fn command(argv: &[String], env: &[(String, String)]) -> Option<std::process::Command> {
    let (p, a) = argv.split_first()?;
    let mut c = std::process::Command::new(p);
    c.args(a).envs(env.iter().map(|(k, v)| (k, v)));
    Some(c)
}

/// What `wol-x11-env` exports to the systemd user manager after every scale change (audit X1-2).
/// The shell's own environment is from session start and stays without them on purpose (GTK 4
/// layer surfaces), so every launch reads them fresh and hands them to the app.
pub const SCALE_ENV: [&str; 2] = ["GDK_SCALE", "STEAM_FORCE_DESKTOPUI_SCALING"];

/// The [`SCALE_ENV`] entries of `systemctl --user show-environment`; a value that is not a plain
/// number (`2`, `1.5`) is dropped.
pub fn scale_env(show_environment: &str) -> Vec<(String, String)> {
    show_environment
        .lines()
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            let number = !v.is_empty() && v.chars().all(|c| c.is_ascii_digit() || c == '.');
            (SCALE_ENV.contains(&k) && number).then(|| (k.to_string(), v.to_string()))
        })
        .collect()
}

/// [`scale_env`] of the running user manager; empty without one. Blocking: off the main thread.
pub fn user_scale_env() -> Vec<(String, String)> {
    std::process::Command::new("systemctl")
        .args(["--user", "show-environment"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| scale_env(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The bar's clock must never panic: without a local zone the UTC fallback is a real calendar.
    #[test]
    fn utc_fallback_is_a_real_calendar() {
        let e = utc_stamp(0);
        assert_eq!(
            (e.weekday, e.day, e.month, e.hour, e.minute, e.second),
            (4, 1, 1, 0, 0, 0),
            "Thursday 1 January 1970"
        );
        // Mon 26 Oct 2026 14:05:00 UTC (the golden time).
        assert_eq!(utc_stamp(1_793_023_500), FAKE_NOW);
        let leap = utc_stamp(951_782_400); // Tue 29 Feb 2000 00:00:00
        assert_eq!((leap.day, leap.month, leap.weekday), (29, 2, 2));
    }

    #[test]
    fn shell_commands_skip_the_cli() {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            shell_line(&v(&["wol", "shell", "toggle", "control-center"])).as_deref(),
            Some("toggle control-center")
        );
        assert_eq!(shell_line(&v(&["wol", "shell"])), None);
        assert_eq!(shell_line(&v(&["wol", "theme", "toggle"])), None);
        assert_eq!(shell_line(&v(&["wol-search"])), None);
    }

    /// Launches hand the app the scale wol-x11-env exported, never anything else from the manager.
    #[test]
    fn launches_carry_only_the_x11_scale() {
        let text = "HOME=/home/ana\nGDK_SCALE=2\nPATH=/usr/bin\n\
                    STEAM_FORCE_DESKTOPUI_SCALING=1.5\nGDK_SCALE_X=9\n";
        assert_eq!(
            scale_env(text),
            [
                ("GDK_SCALE".to_string(), "2".to_string()),
                ("STEAM_FORCE_DESKTOPUI_SCALING".into(), "1.5".into()),
            ]
        );
        assert!(scale_env("GDK_SCALE=$(reboot)\nGDK_SCALE=\n").is_empty());
        assert!(scale_env("").is_empty());
    }

    /// Only app launches carry the scale: plain spawns (toggles, `wol ...`) add nothing.
    #[test]
    fn only_app_launches_add_the_scale() {
        let argv = ["foot".to_string()];
        let env = [("GDK_SCALE".to_string(), "2".to_string())];
        let envs = |c: std::process::Command| {
            c.get_envs()
                .map(|(k, v)| format!("{}={}", k.to_string_lossy(), v.unwrap().to_string_lossy()))
                .collect::<Vec<_>>()
        };
        assert_eq!(envs(command(&argv, &env).unwrap()), ["GDK_SCALE=2"]);
        assert!(
            envs(command(&argv, &[]).unwrap()).is_empty(),
            "spawn adds nothing"
        );
        assert!(command(&[], &env).is_none());
    }
}
