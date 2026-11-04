// SPDX-License-Identifier: GPL-2.0-or-later
//! QA-PERF-1 (H17-01): the desktop widgets layer at idle commits nothing.
//!
//! The empty desktop board (`wol-desktop`, Layer::Bottom, anchored on all edges) used to render
//! nothing at all, so GTK never attached a buffer to its layer surface. Hyprland answers a commit
//! of an unmapped layer surface without a buffer with a new `configure` (and damage on the whole
//! output); GTK acks it and commits again: 60 configure/ack/commit a second for ever, 2.2-4.2 cores
//! of compositor on the 0.12.1 VMs. Standard (class RC-39 of the QA-100 root causes, no RH rule yet): a surface
//! commits only when its content changed; a layer surface the shell presents attaches a buffer in
//! its first frame, so an idle one commits nothing.
//!
//! What this runs: the shell's own `ui::widgets::build` (the main display's desktop, no widget in
//! widgets.json) in a child process with `WAYLAND_DEBUG=client`, under the compositor of
//! `WAYLAND_DISPLAY`, and counts what the child sends for the `wol-desktop` surface: it must
//! attach a buffer before it settles (2 s), then commit nothing for 5 s. With no Wayland session
//! (or without `--features layer-shell`, where the desktop is a plain window) it says so and passes,
//! unless `WOL_DESKTOP_IDLE_REQUIRE=1` (the lane and the VM journey set it).
//!
//!   WAYLAND_DISPLAY=wayland-1 WOL_DESKTOP_IDLE_REQUIRE=1 \
//!     cargo test -p shell --features layer-shell --test desktop_idle
use std::process::Command;
use std::time::Duration;

const CHILD: &str = "WOL_DESKTOP_IDLE_CHILD";
const REQUIRE: &str = "WOL_DESKTOP_IDLE_REQUIRE";
/// Time the surface gets to map (configure, first frame, the size probe's relayout).
const SETTLE: Duration = Duration::from_secs(2);
/// Idle window after the settle: commits counted here must be zero.
const IDLE: Duration = Duration::from_secs(5);

fn main() {
    if std::env::var_os(CHILD).is_some() {
        child();
        return;
    }
    // libtest-style arguments (`--nocapture`, a filter, `--list`) are accepted and ignored.
    if std::env::args().any(|a| a == "--list") {
        println!("desktop_idle: test");
        return;
    }
    let required = std::env::var_os(REQUIRE).is_some_and(|v| v == "1");
    if !cfg!(feature = "layer-shell") {
        skip(
            required,
            "built without --features layer-shell: the desktop is not a layer surface",
        );
        return;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        skip(
            required,
            "no WAYLAND_DISPLAY: no compositor to run the desktop under",
        );
        return;
    }
    let (desktop, catcher, load) = run_child();
    println!(
        "desktop_idle: child {}",
        load.as_deref().unwrap_or("load not reported")
    );
    // Both, then judge: the numbers of the second surface are printed even when the first fails.
    report("wol-desktop", &desktop);
    report("wol-catcher", &catcher);
    judge("wol-desktop", &desktop);
    judge("wol-catcher", &catcher);
    println!("desktop_idle: ok");
}

fn report(name: &str, c: &Census) {
    println!(
        "desktop_idle: {name}: {} attach, {} commit, {} ack_configure before the settle; \
         {} commit, {} attach, {} ack_configure in the {} s after it ({:.1} commits/s)",
        c.attach_settle,
        c.commit_settle,
        c.ack_settle,
        c.commit_idle,
        c.attach_idle,
        c.ack_idle,
        IDLE.as_secs(),
        c.commit_idle as f64 / IDLE.as_secs_f64(),
    );
}

fn judge(name: &str, c: &Census) {
    assert!(c.found, "the child never made a {name} layer surface");
    assert!(
        c.attach_settle >= 1,
        "{name} never attached a buffer: its layer surface stays unmapped, and Hyprland answers \
         each empty commit with a configure and a redraw of the whole output (QA-PERF-1)"
    );
    assert_eq!(
        c.commit_idle,
        0,
        "{name} still commits at idle ({} in {} s): a configure/ack/commit or redraw loop",
        c.commit_idle,
        IDLE.as_secs()
    );
}

fn skip(required: bool, why: &str) {
    assert!(!required, "desktop_idle: {REQUIRE}=1 but {why}");
    println!("desktop_idle: skipped ({why})");
}

#[derive(Debug, Default)]
struct Census {
    found: bool,
    attach_settle: usize,
    commit_settle: usize,
    ack_settle: usize,
    attach_idle: usize,
    commit_idle: usize,
    ack_idle: usize,
}

/// The child under `WAYLAND_DEBUG=client`, with a home of its own (no widgets.json: the empty
/// desktop every new account has).
fn run_child() -> (Census, Census, Option<String>) {
    let home = std::env::temp_dir().join(format!("wol-desktop-idle-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(home.join(".config")).unwrap();
    let out = Command::new(std::env::current_exe().unwrap())
        .env(CHILD, "1")
        .env("WAYLAND_DEBUG", "client")
        .env("GDK_BACKEND", "wayland")
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("GSETTINGS_BACKEND", "memory")
        .output()
        .expect("run the desktop child");
    let _ = std::fs::remove_dir_all(&home);
    let log = String::from_utf8_lossy(&out.stderr);
    // WOL_DESKTOP_IDLE_LOG=<file>: keep the child's protocol log.
    if let Some(f) = std::env::var_os("WOL_DESKTOP_IDLE_LOG") {
        let _ = std::fs::write(f, log.as_bytes());
    }
    assert!(
        out.status.success(),
        "the desktop child failed ({}):\n{}",
        out.status,
        tail(&log, 40)
    );
    let load = log
        .lines()
        .find_map(|l| l.strip_prefix("desktop_idle-child: "))
        .map(String::from);
    (
        census(&log, "wol-desktop"),
        census(&log, "wol-catcher"),
        load,
    )
}

fn tail(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// `[  12345.678] -> wl_surface#33.commit()`: (ms, the message without direction and queue).
fn line(l: &str) -> Option<(f64, &str)> {
    let l = l.trim_start();
    let rest = l.strip_prefix('[')?;
    let (ts, msg) = rest.split_once(']')?;
    // `[1234567.890]` (ms) or, from libwayland 1.24, `[18:05:55.669658]` (wall clock).
    let ts = ts.trim();
    let ms: f64 = match ts.split(':').collect::<Vec<_>>()[..] {
        [h, m, s] => {
            (h.parse::<f64>().ok()? * 3600.0
                + m.parse::<f64>().ok()? * 60.0
                + s.parse::<f64>().ok()?)
                * 1000.0
        }
        _ => ts.parse().ok()?,
    };
    let msg = msg.trim_start();
    // Newer libwayland prints the queue name first: `{Default Queue}  -> wl_surface#10.commit()`.
    let msg = match msg.strip_prefix('{') {
        Some(m) => m.split_once('}').map_or(m, |(_, r)| r).trim_start(),
        None => msg,
    };
    let msg = msg.strip_prefix("->").unwrap_or(msg).trim_start();
    Some((ms, msg))
}

/// Requests the child sent for the `wol-desktop` surface, split at the settle (from the
/// surface's creation).
fn census(log: &str, namespace: &str) -> Census {
    let mut c = Census::default();
    let mut surface = String::new();
    let mut layer = String::new();
    let mut start = 0.0;
    for (ms, msg) in log.lines().filter_map(line) {
        if !c.found {
            // zwlr_layer_shell_v1#N.get_layer_surface(new id zwlr_layer_surface_v1#M, wl_surface#S, ...
            // "wol-desktop") — the namespace is the last argument.
            if msg.contains(".get_layer_surface(") && msg.contains(&format!("\"{namespace}\"")) {
                let args = msg.split_once('(').map_or("", |(_, a)| a);
                let mut parts = args.split(", ");
                layer = parts
                    .next()
                    .and_then(|p| p.rsplit(' ').next())
                    .unwrap_or_default()
                    .to_string();
                surface = parts.next().unwrap_or_default().trim().to_string();
                start = ms;
                c.found = true;
            }
            continue;
        }
        let idle = ms - start >= SETTLE.as_millis() as f64;
        if msg.starts_with(&format!("{surface}.commit(")) {
            if idle {
                c.commit_idle += 1;
            } else {
                c.commit_settle += 1;
            }
        } else if msg.starts_with(&format!("{surface}.attach(")) && !msg.contains("nil") {
            if idle {
                c.attach_idle += 1;
            } else {
                c.attach_settle += 1;
            }
        } else if msg.starts_with(&format!("{layer}.ack_configure(")) {
            if idle {
                c.ack_idle += 1;
            } else {
                c.ack_settle += 1;
            }
        }
    }
    c
}

/// This process's CPU time (clock ticks, user + system, all threads) and voluntary context
/// switches of its main thread (each one a wakeup of the GTK loop).
fn own_load() -> (u64, u64) {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let after = stat.rsplit_once(')').map_or("", |(_, r)| r);
    let f: Vec<u64> = after
        .split_whitespace()
        .map(|x| x.parse().unwrap_or(0))
        .collect();
    let ticks = f.get(11).copied().unwrap_or(0) + f.get(12).copied().unwrap_or(0);
    let wakeups = std::fs::read_to_string("/proc/thread-self/status")
        .unwrap_or_default()
        .lines()
        .find_map(|l| l.strip_prefix("voluntary_ctxt_switches:"))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);
    (ticks, wakeups)
}

/// The shell's desktop as `run_shell` builds it: the kit's style, then `ui::widgets::build`
/// presented; quits after the settle and the idle window.
fn child() {
    use gtk::prelude::*;
    use std::rc::Rc;
    gtk::init().expect("gtk init");
    shell::kit::style::install("");
    let app = gtk::Application::builder()
        .application_id("org.wolos.test.DesktopIdle")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(|app| {
        let (win, board) = shell::ui::widgets::build(app, Rc::new(shell::system::Fake));
        assert!(board.widgets().is_empty(), "a fresh home has no widget");
        win.present();
        // A panel app's click-catcher (Search opens one under itself): a full-screen layer with
        // nothing in it, presented while the panel is open.
        let spec = shell::kit::surface::Spec::panel(
            "wol-panel-idle-test",
            [true, false, false, true],
            shell::kit::surface::Keyboard::None,
        );
        let panel = shell::ui::window(app, "panel", spec);
        panel.set_child(Some(&gtk::Label::new(Some("panel"))));
        panel.present();
        shell::ui::transient::close_outside(app, &panel, Rc::new(|| {}));
        let at_settle = Rc::new(std::cell::Cell::new((0, 0)));
        let a = at_settle.clone();
        gtk::glib::timeout_add_local_once(SETTLE, move || a.set(own_load()));
        let app = app.clone();
        gtk::glib::timeout_add_local_once(SETTLE + IDLE, move || {
            // Keep the board alive until the end.
            let _ = &board;
            let ((t0, w0), (t1, w1)) = (at_settle.get(), own_load());
            eprintln!(
                "desktop_idle-child: idle {} cpu ticks {} wakeups",
                t1 - t0,
                w1 - w0
            );
            app.quit();
        });
    });
    app.run_with_args(&[] as &[&str]);
}
