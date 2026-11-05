// SPDX-License-Identifier: GPL-2.0-or-later
//! Surface fences: a panic while one part of the shell is built (a panel, the widgets board, the
//! notification server) stops that part only. It shows a small "this part stopped" card in its
//! place, the journal gets one line naming it, and the rest of the shell keeps running. A panic
//! later, inside a toolkit callback, cannot unwind through C: the process ends and the supervisor
//! (`crate::supervise`) brings the shell back.
use super::surface::{Keyboard, Spec};
use super::widgets as w;
use gtk::prelude::*;
use std::cell::RefCell;

/// The card's title and sentence (HIG §3: what happened, what to do).
pub const TITLE: &str = "This part stopped";
pub const HINT: &str = crate::copy::PART_STOPPED;
/// Layer namespace of the stand-in window for a panel that stopped.
pub const NAMESPACE: &str = "wol-panel-stopped";
/// Fault injection for tests and VM journeys: `WOL_SHELL_BREAK=clipboard,themes` makes those
/// parts stop as they are built (as if they had panicked), so the card and the rest of the shell
/// can be checked.
pub const BREAK_ENV: &str = "WOL_SHELL_BREAK";

/// Is part `name` in the [`BREAK_ENV`] list `list`?
pub fn broken(list: Option<&str>, name: &str) -> bool {
    list.is_some_and(|l| l.split(',').any(|p| p.trim() == name))
}

thread_local! {
    /// The fences this thread is inside, innermost last (the panic hook names it).
    static INSIDE: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Parts that stopped, in order.
    static FAILED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// The panic message: the `&str` or `String` payload, else a fixed word.
pub fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".into())
}

/// Journal line for a panic: inside fence `part`, or outside every fence (the process ends).
pub fn journal_line(part: Option<&str>, msg: &str) -> String {
    match part {
        Some(p) => format!("{p} stopped (the rest keeps running): {msg}"),
        None => format!("panic outside every part: {msg}"),
    }
}

/// Installs (once) a panic hook that logs which part panicked: stderr as before, and the journal
/// (`journalctl -t wol-shell`) when the supervisor runs this shell.
pub fn install_hook() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let part = INSIDE
                .try_with(|s| s.try_borrow().ok().and_then(|s| s.last().cloned()))
                .ok()
                .flatten();
            let at = info
                .location()
                .map(|l| format!(" at {}:{}", l.file(), l.line()))
                .unwrap_or_default();
            let line = journal_line(
                part.as_deref(),
                &format!("{}{at}", panic_text(info.payload())),
            );
            if crate::envv::is_set(crate::supervise::UI_ENV) {
                // Not waited for: the hook runs on the toolkit thread, and an abort may follow.
                let _ = std::process::Command::new("logger")
                    .args(["-t", "wol-shell", "-p", "user.err", "--", &line])
                    .spawn();
            }
            prev(info);
        }));
    });
}

/// Runs `f` as part `name`: its value, or None when it panicked or was asked to stop by
/// [`BREAK_ENV`] (the part is then marked).
pub fn guard<T>(name: &str, f: impl FnOnce() -> T) -> Option<T> {
    INSIDE.with(|s| s.borrow_mut().push(name.to_string()));
    let asked = broken(crate::envv::var(BREAK_ENV).as_deref(), name);
    // An injected stop ends the part without unwinding (no panic is made for it): the same card,
    // the same record, the same list of stopped parts.
    let r = if asked {
        crate::diag::say!(
            "{}",
            journal_line(Some(name), &format!("{BREAK_ENV} asked {name} to stop"))
        );
        Err(())
    } else {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(|_| ())
    };
    INSIDE.with(|s| s.borrow_mut().pop());
    match r {
        Ok(v) => Some(v),
        Err(_) => {
            FAILED.with(|f| f.borrow_mut().push(name.to_string()));
            None
        }
    }
}

/// The parts that stopped on this thread, in order.
pub fn failed() -> Vec<String> {
    FAILED.with(|f| f.borrow().clone())
}

/// `build` as part `name`: its widget, or the "this part stopped" card when it panicked.
pub fn fence(name: &str, build: impl FnOnce() -> gtk::Widget) -> gtk::Widget {
    guard(name, build).unwrap_or_else(|| card(name).upcast())
}

/// `build` for a panel window: the window, or a stand-in panel holding the card (so `wol shell
/// show <panel>` still answers).
pub fn fence_window(
    app: &gtk::Application,
    name: &str,
    build: impl FnOnce() -> gtk::Window,
) -> gtk::Window {
    guard(name, build).unwrap_or_else(|| stand_in(app, name))
}

/// The card shown where part `name` would be.
pub fn card(name: &str) -> gtk::Box {
    let g = w::glass_panel(w::S2);
    g.set_widget_name(name);
    g.append(&w::empty_state(
        "dialog-warning-symbolic",
        TITLE,
        HINT,
        None,
    ));
    g
}

/// A panel window holding [`card`], hidden until shown.
pub fn stand_in(app: &gtk::Application, name: &str) -> gtk::Window {
    let win = gtk::ApplicationWindow::new(app);
    win.add_css_class("panel");
    win.set_title(Some(TITLE));
    super::surface::place(
        &win,
        Spec::panel(NAMESPACE, [true, false, false, false], Keyboard::OnDemand),
    );
    win.set_child(Some(&card(name)));
    win.upcast()
}

/// QA-CRASH-2: GIO's shared session-bus connection ends the whole process (it raises SIGTERM)
/// when the bus goes away, because `exit-on-close` is on by default. A lost user bus then took
/// the bar, Dock and wallpaper down with no log line, and the lock screen with them. Our surfaces
/// are Wayland, not D-Bus, so every toolkit process keeps running without the bus instead. The
/// journal gets one line. Call it right after the `Application` is built, before `run`.
pub fn survive_bus_loss(app: &impl IsA<gtk::gio::Application>) {
    app.connect_startup(|app| {
        if let Some(c) = app.dbus_connection() {
            keep_on_bus_loss(&c);
        }
    });
}

/// [`survive_bus_loss`] for one connection: no exit on close, and one journal line when the bus
/// end closed it (a close we asked for says nothing).
pub fn keep_on_bus_loss(c: &gtk::gio::DBusConnection) {
    c.set_exit_on_close(false);
    c.connect_closed(|_, remote, _| {
        if remote {
            let me = std::env::args()
                .next()
                .and_then(|a| a.rsplit('/').next().map(str::to_owned))
                .unwrap_or_else(|| "wolos".into());
            crate::diag::say!("{}", bus_lost_line(&me));
        }
    });
}

/// The journal line for a lost session bus.
pub fn bus_lost_line(me: &str) -> String {
    format!("{me}: the session bus went away; keeping on without it")
}
