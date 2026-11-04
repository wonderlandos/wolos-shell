// SPDX-License-Identifier: GPL-2.0-or-later
//! Transient panels (Control Center, Notification Center, clipboard, session menu): one at a
//! time, and gone on a click outside, Esc, another app in front or a workspace switch.
//!
//! They fade in with `ease` when they map and out with `in` when the shell dismisses them
//! ([`Presence`], on the window's own clock, `docs/modules/hig.md` section 7). A hide that comes
//! straight from the socket (`wol shell hide|toggle`) stays immediate: the panel's visibility
//! is what `protocol::apply` says, and the compositor fades the layer out.
use crate::hypr;
use crate::kit::anim::{Animated, Clock, Motion};
use crate::kit::motion;
use gtk::prelude::*;
use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// A surface coming and going: one opacity on its window's clock. [`Presence::opened`] fades it
/// in from nothing, [`Presence::close`] fades it out and then hides it; a [`Presence::keep`]
/// during the fade-out turns it around (from where it is) and nothing hides. Under reduce
/// motion both jump, and the hide is at once.
pub struct Presence {
    fade: Animated,
    closing: Rc<Cell<bool>>,
}

impl Presence {
    /// `apply` gets the opacity every frame; `hide` runs once a [`Presence::close`] got to 0.
    pub fn on_clock(
        clock: &Clock,
        apply: impl Fn(f32) + 'static,
        hide: impl Fn() + 'static,
    ) -> Rc<Presence> {
        let closing = Rc::new(Cell::new(false));
        let fade = Animated::on_clock(clock, 1.0, apply);
        let c = closing.clone();
        fade.connect_settled(move |v| {
            if v == 0.0 && c.replace(false) {
                hide()
            }
        });
        Rc::new(Presence { fade, closing })
    }

    /// The window's own: its opacity, `set_visible(false)` at the end of a close.
    pub fn of_window(win: &gtk::Window) -> Rc<Presence> {
        let (a, h) = (win.downgrade(), win.downgrade());
        Presence::on_clock(
            &Clock::of(win),
            move |v| {
                if let Some(w) = a.upgrade() {
                    w.set_opacity(v.into())
                }
            },
            move || {
                if let Some(w) = h.upgrade() {
                    w.set_visible(false)
                }
            },
        )
    }

    /// Just mapped: from nothing to full in `d` with `ease`.
    pub fn opened(&self, d: motion::Duration) {
        self.closing.set(false);
        self.fade.set(0.0);
        self.fade.retarget(1.0, Motion::enter(d));
    }

    /// Shown again while it may be fading out: back to full from where it is, no hide.
    pub fn keep(&self) {
        self.closing.set(false);
        self.fade
            .retarget(1.0, Motion::enter(motion::Duration::Normal));
    }

    /// Fade out with `in`, one step faster than the entrance, then hide.
    pub fn close(&self) {
        self.closing.set(true);
        if self.fade.value() == 0.0 && !self.fade.is_moving() {
            // Already at nothing (a close before the first frame): nothing to wait for.
            return self.fade.set(0.0);
        }
        self.fade
            .retarget(0.0, Motion::leave(motion::Duration::Fast));
    }

    pub fn is_closing(&self) -> bool {
        self.closing.get()
    }

    /// The opacity now.
    pub fn value(&self) -> f32 {
        self.fade.value()
    }
}

thread_local! {
    /// Windows that fade in and out, with their [`Presence`].
    static PRESENCE: std::cell::RefCell<Vec<(gtk::glib::WeakRef<gtk::Window>, Rc<Presence>)>> =
        Default::default();
}

/// `win`'s [`Presence`], when it has one.
pub fn presence(win: &gtk::Window) -> Option<Rc<Presence>> {
    PRESENCE.with_borrow(|p| {
        p.iter()
            .find(|(w, _)| w.upgrade().as_ref() == Some(win))
            .map(|(_, p)| p.clone())
    })
}

/// `win` fades in for `d` each time it maps (once per window; the first call's `d` holds).
pub fn fade_on_map(win: &gtk::Window, d: motion::Duration) -> Rc<Presence> {
    if let Some(p) = presence(win) {
        return p;
    }
    let p = Presence::of_window(win);
    {
        let p = p.clone();
        win.connect_map(move |_| p.opened(d));
    }
    PRESENCE.with_borrow_mut(|v| {
        v.retain(|(w, _)| w.upgrade().is_some());
        v.push((win.downgrade(), p.clone()));
    });
    p
}

/// The shell dismisses `win`: it fades out and hides (at once without a [`Presence`]).
pub fn dismiss(win: &impl IsA<gtk::Window>) {
    let win = win.upcast_ref::<gtk::Window>();
    if !win.is_visible() {
        return;
    }
    match presence(win) {
        Some(p) if win.is_mapped() => p.close(),
        _ => win.set_visible(false),
    }
}

/// Shell panels that close like menus. Not the OSD, Overview, the screenshot bar or
/// Force Quit (a window that stays until dismissed).
pub const PANELS: &[&str] = &[
    "control-center",
    "notifications",
    "clipboard",
    "session",
    "shortcuts",
];

pub fn is_transient(name: &str) -> bool {
    PANELS.contains(&name)
}

/// Showing `name` closes every other transient panel.
pub fn others(name: &str) -> Vec<&'static str> {
    if !is_transient(name) {
        return vec![];
    }
    PANELS.iter().copied().filter(|p| *p != name).collect()
}

/// A compositor event that closes transient panels: an app window came to the front, or the
/// workspace changed. `activewindow>>,` (a layer surface took the keyboard, e.g. the panel itself)
/// does not.
pub fn dismisses(e: &hypr::Event) -> bool {
    match e {
        hypr::Event::ActiveWindow { class, .. } => !class.is_empty(),
        hypr::Event::Workspace(_) => true,
        _ => false,
    }
}

/// A window address as the compositor spells it in events: lowercase hex without `0x` (`j/activewindow`
/// and `j/clients` add the prefix). Empty for "no window".
pub fn address(s: &str) -> String {
    s.trim().trim_start_matches("0x").to_ascii_lowercase()
}

/// `activewindowv2>>ADDRESS` -> the address (empty = no window); None for any other line.
pub fn active_address(line: &str) -> Option<String> {
    line.trim_end()
        .strip_prefix("activewindowv2>>")
        .map(|a| address(a.trim_end_matches(',')))
}

/// Decides, line by line from `.socket2.sock`, whether the shell panels close. When a focus grab
/// ends, the compositor hands the keyboard back to the window that had it (`activewindow` then
/// `activewindowv2` for the same window): that is not "another app came to the front", so the
/// window in front when the panel opened never dismisses it. A click on that window still closes
/// the panel: the grab clears first (`focus_grab`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dismisser {
    /// The window in front now (from `activewindowv2`, or the seed from `j/activewindow`).
    active: Option<String>,
    /// The window that was in front when the panel opened.
    opened_on: Option<String>,
    /// An `activewindow` with a class arrived; its `activewindowv2` line decides.
    pending: bool,
    /// The shell destroyed its focus grab: until then, the next `activewindow`/`activewindowv2`
    /// pair is the compositor handing the keyboard back (to the window under the pointer), not another
    /// app coming to the front (audit3 R2A-1).
    refocus_until: Option<Instant>,
}

/// How long a destroyed grab's refocus is waited for: past it, an app switch counts again.
pub const REFOCUS_WINDOW: Duration = Duration::from_millis(500);

impl Dismisser {
    /// The window in front (from `j/activewindow`, any spelling) before events flow.
    pub fn seed(&mut self, addr: Option<&str>) {
        self.active = addr.map(address).filter(|a| !a.is_empty());
    }

    /// A panel opened: remember whose keyboard the grab takes. While another panel holds the
    /// keyboard no window is active (`activewindowv2>>,`): a panel replacing it keeps the window
    /// the first one recorded, or that window getting its keys back (the grab is recreated for
    /// the new panel) would close the new one (audit2 A2-1).
    pub fn opened(&mut self) {
        if self.active.is_some() {
            self.opened_on = self.active.clone();
        }
        self.pending = false;
    }

    /// The last panel closed.
    pub fn closed(&mut self) {
        self.opened_on = None;
    }

    /// The shell destroyed its focus grab at `now`: the refocus pair that follows is swallowed.
    pub fn expect_refocus(&mut self, now: Instant) {
        self.refocus_until = Some(now + REFOCUS_WINDOW);
    }

    /// One event line; true when it closes the panels.
    pub fn feed(&mut self, line: &str) -> bool {
        self.feed_at(line, Instant::now())
    }

    /// [`Dismisser::feed`] at `now`.
    pub fn feed_at(&mut self, line: &str, now: Instant) -> bool {
        let pending = std::mem::take(&mut self.pending);
        let refocus = self.refocus_until.is_some_and(|t| now < t);
        if let Some(addr) = active_address(line) {
            self.active = Some(addr.clone()).filter(|a| !a.is_empty());
            if refocus {
                // The pair is complete: the next one is the user's again.
                self.refocus_until = None;
                return false;
            }
            return pending && !addr.is_empty() && self.opened_on.as_deref() != Some(addr.as_str());
        }
        match hypr::parse_event(line) {
            Some(hypr::Event::Workspace(_)) => true,
            Some(hypr::Event::ActiveWindow { class, .. }) if !class.is_empty() && refocus => false,
            Some(hypr::Event::ActiveWindow { class, .. }) if !class.is_empty() => {
                // Decided by the `activewindowv2` line that follows it.
                self.pending = true;
                false
            }
            // An old compositor without `activewindowv2`: the class alone decides.
            _ => pending,
        }
    }
}

thread_local! {
    static DISMISSER: std::cell::RefCell<Dismisser> = Default::default();
}

/// [`Dismisser::seed`] for the shell's one instance.
pub fn seed_active(address: Option<&str>) {
    DISMISSER.with_borrow_mut(|d| d.seed(address));
}

/// [`Dismisser::feed`] for the shell's one instance.
pub fn feed(line: &str) -> bool {
    DISMISSER.with_borrow_mut(|d| d.feed(line))
}

/// Whether a panel's hide resyncs the focus grab now, from the panels' `(visible, mapped)`. Not
/// while one is visible and not yet mapped (a panel switch): a grab without it would be cleared
/// by its map (the compositor drops a seat grab that does not accept a newly mapped on-demand layer)
/// and close it. Its own map syncs (audit3 R2A-1).
pub fn sync_on_hide(panels: impl IntoIterator<Item = (bool, bool)>) -> bool {
    !panels
        .into_iter()
        .any(|(visible, mapped)| visible && !mapped)
}

/// `wol shell hide transient`: the whole set at once (Esc, bound pass-through in hyprland.conf,
/// so it works while an app has the keyboard).
pub const GROUP: &str = "transient";

pub fn closes_all(c: &crate::protocol::Command) -> bool {
    c.target == GROUP && c.verb == crate::protocol::Verb::Hide
}

/// Panel apps in their own process (GApplication ids) that a shell panel opening closes.
pub const PANEL_APPS: &[&str] = &["org.wolos.search"];

/// A GApplication's D-Bus object path.
pub fn object_path(id: &str) -> String {
    format!("/{}", id.replace('.', "/"))
}

thread_local! {
    /// Members that are not panel windows (the Edit Widgets gallery): their closers.
    static EXTRA: std::cell::RefCell<Vec<Rc<dyn Fn()>>> = Default::default();
}

/// Adds a non-window member to the set (closed with the others).
pub fn also_close(f: Rc<dyn Fn()>) {
    EXTRA.with(|e| e.borrow_mut().push(f));
}

/// Closes the members outside the panel map: extras and [`PANEL_APPS`].
pub fn close_outside_members() {
    EXTRA.with(|e| e.borrow().clone()).iter().for_each(|f| f());
    close_apps();
}

/// [`PANEL_APPS`]: their `hide` action over D-Bus; nothing happens when the app is not running
/// (no activation file, `NO_AUTO_START`).
pub fn close_apps() {
    use gtk::gio;
    gio::bus_get(gio::BusType::Session, gio::Cancellable::NONE, |bus| {
        let Ok(bus) = bus else { return };
        for id in PANEL_APPS {
            use gtk::glib::prelude::ToVariant;
            let empty: Vec<gtk::glib::Variant> = vec![];
            let params = ("hide", empty, gtk::glib::VariantDict::new(None).end()).to_variant();
            bus.call(
                Some(id),
                &object_path(id),
                "org.gtk.Actions",
                "Activate",
                Some(&params),
                None,
                gio::DBusCallFlags::NO_AUTO_START,
                500,
                gio::Cancellable::NONE,
                |_| {},
            );
        }
    });
}

/// The panels a click outside, or another app in front, closes. The session panel stays while it
/// asks Log Out / Restart / Shut Down with its countdown: the click that opened it from the WolOS
/// menu, or any click after, used to cancel the question silently (QA-LIVE-32: it worked on the
/// third try). Esc and Cancel still end it.
pub fn panels_to_close(session_sheet_up: bool) -> Vec<&'static str> {
    PANELS
        .iter()
        .copied()
        .filter(|p| !(session_sheet_up && *p == "session"))
        .collect()
}

/// Hides every visible transient panel window, but the session panel while it asks.
pub fn close_panels(panels: &super::Panels) {
    hide(panels, &panels_to_close(super::panels::session_sheet_up()));
}

fn hide(panels: &super::Panels, names: &[&str]) {
    let open: Vec<gtk::Window> = names
        .iter()
        .filter_map(|p| panels.borrow().get(p).cloned())
        .collect();
    open.iter().for_each(dismiss);
}

/// Hides every visible transient panel and the other members of the set.
pub fn close_all(panels: &super::Panels) {
    close_panels(panels);
    EXTRA.with(|e| e.borrow().clone()).iter().for_each(|f| f());
}

/// Panels Esc (`hide transient`) also closes, outside the mutually exclusive [`PANELS`] (a click
/// outside or another app in front leaves them alone). Over a pointer-locked game an on-demand
/// layer gets no keyboard, so their own Esc never fires (audit3 R2A-11).
pub const ALSO_ESC: &[&str] = &["overview", "force-quit"];

/// `hide transient`: [`close_all`] and [`ALSO_ESC`].
pub fn close_on_escape(panels: &super::Panels) {
    hide(panels, PANELS);
    EXTRA.with(|e| e.borrow().clone()).iter().for_each(|f| f());
    let also: Vec<gtk::Window> = ALSO_ESC
        .iter()
        .filter_map(|p| panels.borrow().get(p).cloned())
        .collect();
    also.iter().for_each(dismiss);
}

/// Panel apps (Search): `close` on a click outside `win` or on a [`dismisses`] event.
pub fn close_outside(app: &gtk::Application, win: &gtk::ApplicationWindow, close: Rc<dyn Fn()>) {
    // Dev toplevels: a full-screen window would cover the desktop.
    if crate::kit::surface::layered() {
        let catcher = super::window(app, "catcher", crate::kit::surface::Spec::catcher());
        let click = gtk::GestureClick::new();
        click.set_button(0);
        let c = close.clone();
        click.connect_pressed(move |_, _, _, _| c());
        catcher.add_controller(click);
        // Top layer: always under the Overlay panel.
        catcher.present();
        let cw = catcher.clone();
        win.connect_hide(move |_| cw.close());
    }
    let Some(path) = hypr::socket(".socket2.sock") else {
        return;
    };
    use gtk::{gio, glib};
    glib::spawn_future_local(async move {
        let Ok(conn) = gio::SocketClient::new()
            .connect_future(&gio::UnixSocketAddress::new(&path))
            .await
        else {
            return;
        };
        let input = gio::DataInputStream::new(&conn.input_stream());
        while let Ok(Some(line)) = input
            .read_line_utf8_future(glib::Priority::DEFAULT_IDLE)
            .await
        {
            if hypr::parse_event(&line).as_ref().is_some_and(dismisses) {
                close();
            }
        }
    });
}

/// A press anywhere on `win` (the Dock) closes the transient panels first; the press still acts.
pub fn close_on_press(win: &impl IsA<gtk::Widget>, panels: &super::Panels) {
    let click = gtk::GestureClick::new();
    click.set_button(0);
    click.set_propagation_phase(gtk::PropagationPhase::Capture);
    let panels = panels.clone();
    click.connect_pressed(move |_, _, _, _| close_all(&panels));
    win.add_controller(click);
}

/// Opening any menu under `root` (menu bar menus) closes the transient panels.
pub fn close_on_menus(root: &impl IsA<gtk::Widget>, panels: &super::Panels) {
    let mut stack = vec![root.clone().upcast::<gtk::Widget>()];
    while let Some(w) = stack.pop() {
        if let Some(m) = w.downcast_ref::<gtk::MenuButton>() {
            // `active`, not the popover: the app menu's popover is replaced per app.
            let panels = panels.clone();
            m.connect_active_notify(move |m| {
                if m.is_active() {
                    close_all(&panels)
                }
            });
        }
        let mut c = w.first_child();
        while let Some(x) = c {
            c = x.next_sibling();
            stack.push(x);
        }
    }
}

/// Whether the one-display click-catcher goes under the open panel: only without a focus grab.
pub fn catcher_shown(open: bool, layered: bool, grab: bool) -> bool {
    open && layered && !grab
}

/// Windows a click may land on without closing the open panel: menu bars and the Dock.
fn kept(app: &gtk::Application) -> Vec<gtk::Window> {
    app.windows()
        .into_iter()
        .filter(|w| w.is_visible() && (w.has_css_class("menubar") || w.has_css_class("dock")))
        .collect()
}

/// Keeps the transient panels in `panels` exclusive and closes them on a click outside. Layer
/// surfaces get no reliable focus-out: the compositor's focus grab (`crate::focus_grab`) covers every
/// display; without it a click-catcher goes under the open panel on its display.
pub fn manage(app: &gtk::Application, panels: &super::Panels) {
    let catcher = super::window(app, "catcher", crate::kit::surface::Spec::catcher());
    crate::kit::surface::survive_output_loss(&catcher, crate::kit::surface::Spec::catcher());
    let click = gtk::GestureClick::new();
    click.set_button(0);
    {
        let panels = panels.clone();
        click.connect_pressed(move |_, _, _, _| close_all(&panels));
    }
    catcher.add_controller(click);
    let grab = gtk::gdk::Display::default().and_then(|d| {
        let panels = panels.clone();
        crate::focus_grab::FocusGrab::new(&d, Rc::new(move || close_all(&panels)))
    });
    let sync = {
        let (panels, catcher, app) = (panels.clone(), catcher.clone(), app.clone());
        Rc::new(move || {
            let open: Vec<gtk::Window> = PANELS
                .iter()
                .filter_map(|p| panels.borrow().get(p).cloned())
                .filter(|w| w.is_visible())
                .collect();
            let layered = crate::kit::surface::layered();
            catcher.set_visible(catcher_shown(!open.is_empty(), layered, grab.is_some()));
            if let Some(g) = &grab
                && g.set(&crate::focus_grab::whitelist(&open, &kept(&app)))
            {
                DISMISSER.with_borrow_mut(|d| d.expect_refocus(Instant::now()));
            }
        })
    };
    for name in PANELS {
        let Some(w) = panels.borrow().get(name).cloned() else {
            continue;
        };
        fade_on_map(&w, motion::Duration::Normal);
        // The grab names mapped surfaces only: it is made (or the panel added to it) once the
        // panel is on screen, not when it turns visible (a grab holding only the bars would be
        // cleared by the panel's map). The panel has the keyboard from its own map.
        let s = sync.clone();
        w.connect_map(move |_| s());
        let (panels, sync) = (panels.clone(), sync.clone());
        w.connect_visible_notify(move |w| {
            if w.is_visible() {
                DISMISSER.with_borrow_mut(|d| d.opened());
                close_outside_members();
                for o in others(name) {
                    if let Some(o) = panels.borrow().get(o) {
                        o.set_visible(false);
                    }
                }
                return;
            }
            if !any_open(&panels) {
                DISMISSER.with_borrow_mut(|d| d.closed());
            }
            // Hidden: the grab is released (or trimmed to the panel that took over, once that
            // one is on screen).
            let states: Vec<(bool, bool)> = PANELS
                .iter()
                .filter_map(|p| panels.borrow().get(p).cloned())
                .map(|o| (o.is_visible(), o.is_mapped()))
                .collect();
            if sync_on_hide(states) {
                sync();
            }
        });
    }
}

/// Whether one of the transient panels is on screen (the menu bar keeps its app name while a
/// panel takes the keyboard, `ipc::hypr_events`).
pub fn any_open(panels: &super::Panels) -> bool {
    let p = panels.borrow();
    PANELS
        .iter()
        .any(|n| p.get(n).is_some_and(|w| w.is_visible()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hypr::Event;

    /// `wol shell hide transient` (Esc, bound pass-through) closes the whole set.
    #[test]
    fn hide_transient_targets_the_set() {
        use crate::protocol::{Verb, parse};
        assert!(closes_all(&parse("hide transient").unwrap()));
        assert!(!closes_all(&parse("show transient").unwrap()));
        assert!(!closes_all(&parse("hide notifications").unwrap()));
        assert!(shared::shell::PANELS.contains(&GROUP));
        // Esc closes Overview and Force Quit too, which are not exclusive with the others.
        assert_eq!(ALSO_ESC, ["overview", "force-quit"]);
        assert!(ALSO_ESC.iter().all(|p| !is_transient(p)));
        assert_eq!(parse("hide transient").unwrap().verb, Verb::Hide);
    }

    /// Search (its own process) is closed over D-Bus: its GApplication's `hide` action.
    #[test]
    fn panel_apps_are_closed_by_their_action_path() {
        assert_eq!(PANEL_APPS, ["org.wolos.search"]);
        assert_eq!(object_path("org.wolos.search"), "/org/wolos/search");
    }

    /// The compositor's focus grab closes panels on a click on any display; the click-catcher (one
    /// display) only stands in where the grab is missing, and never in dev windows.
    #[test]
    fn the_catcher_only_stands_in_for_a_missing_focus_grab() {
        assert!(catcher_shown(true, true, false));
        assert!(
            !catcher_shown(true, true, true),
            "the grab covers every display"
        );
        assert!(!catcher_shown(false, true, false), "nothing open");
        assert!(!catcher_shown(true, false, false), "dev windows");
    }

    #[test]
    fn showing_one_panel_closes_the_others() {
        assert_eq!(
            others("control-center"),
            ["notifications", "clipboard", "session", "shortcuts"]
        );
        assert!(others("shortcuts").contains(&"control-center"));
        assert!(others("osd").is_empty());
        assert!(others("overview").is_empty());
    }

    #[test]
    fn another_app_or_workspace_closes_panels_the_panel_taking_keys_does_not() {
        let active = |c: &str| Event::ActiveWindow {
            class: c.into(),
            title: String::new(),
        };
        for (e, want) in [
            (active("org.wolos.Files"), true),
            (active(""), false),
            (Event::Workspace("2".into()), true),
            (
                Event::OpenWindow {
                    address: "1".into(),
                    class: "x".into(),
                },
                false,
            ),
            (Event::Other, false),
        ] {
            assert_eq!(dismisses(&e), want, "{e:?}");
        }
    }

    /// Live (lead, v0.12): the Control Center never stayed open. Opening it made the focus grab;
    /// a second sync recreated it, the compositor gave the keyboard back to Files, and
    /// `activewindow>>org.gnome.Nautilus` closed the panel. The window in front when the panel
    /// opened never dismisses it.
    #[test]
    fn the_window_behind_the_panel_getting_its_keys_back_does_not_dismiss() {
        let mut d = Dismisser::default();
        d.seed(Some("0x55D0A1B2C3D4"));
        d.opened();
        assert!(
            !d.feed("activewindow>>org.gnome.Nautilus,Home\n"),
            "decided by v2"
        );
        assert!(!d.feed("activewindowv2>>55d0a1b2c3d4\n"), "the same window");
        // The panel takes the keyboard: no window.
        assert!(!d.feed("activewindow>>,\n"));
        assert!(!d.feed("activewindowv2>>\n"));
        // Really another app in front.
        assert!(!d.feed("activewindow>>foot,~\n"));
        assert!(d.feed("activewindowv2>>55d0ffff0000\n"));
    }

    #[test]
    fn another_app_or_a_workspace_switch_still_dismisses() {
        let mut d = Dismisser::default();
        d.feed("activewindowv2>>1\n");
        d.opened();
        assert!(d.feed("workspace>>2\n"));
        assert!(!d.feed("activewindow>>foot,~\n"));
        assert!(d.feed("activewindowv2>>2\n"));
        // Without a seed or events, the old rule holds: a class dismisses.
        let mut fresh = Dismisser::default();
        fresh.opened();
        assert!(!fresh.feed("activewindow>>foot,~\n"));
        assert!(fresh.feed("activewindowv2>>7\n"));
        // An old compositor without activewindowv2: the next line settles it.
        let mut old = Dismisser::default();
        old.opened();
        assert!(!old.feed("activewindow>>foot,~\n"));
        assert!(old.feed("openwindow>>8,1,x,y\n"));
        assert!(!old.feed("openwindow>>9,1,x,y\n"), "nothing pending");
    }

    /// Audit2 A2-1: Control Center open, then the clock (Notification Center). The second panel
    /// opened while the first held the keyboard (no window active); the grab was recreated for
    /// it, the window behind got its keys back, and the second panel closed itself.
    #[test]
    fn a_panel_opened_from_another_panel_keeps_the_window_behind() {
        let mut d = Dismisser::default();
        d.seed(Some("0xA"));
        d.opened();
        assert!(!d.feed(
            "activewindow>>,
"
        ));
        assert!(!d.feed(
            "activewindowv2>>,
"
        ));
        // The second panel opens; the first hides (any panel still open: no `closed`).
        d.opened();
        assert!(!d.feed(
            "activewindow>>org.gnome.Nautilus,Home
"
        ));
        assert!(
            !d.feed(
                "activewindowv2>>a
"
            ),
            "the window behind both panels"
        );
        // Another app in front still closes the second panel.
        assert!(!d.feed(
            "activewindow>>foot,~
"
        ));
        assert!(d.feed(
            "activewindowv2>>b
"
        ));
    }

    /// Audit3 R2A-1: the shell destroyed its grab (a panel closed) and the compositor handed the
    /// keyboard to the window under the pointer, not the one the panel opened on. That pair is
    /// not "another app in front"; the next one is, and so is a pair after the window passed.
    #[test]
    fn a_refocus_after_the_shell_drops_its_grab_does_not_dismiss() {
        let t = Instant::now();
        let mut d = Dismisser::default();
        d.seed(Some("0xa"));
        d.opened();
        d.expect_refocus(t);
        assert!(!d.feed_at("activewindow>>foot,~\n", t));
        assert!(!d.feed_at("activewindowv2>>b\n", t), "the refocus");
        assert!(!d.feed_at("activewindow>>org.gnome.Nautilus,Home\n", t));
        assert!(
            d.feed_at("activewindowv2>>c\n", t),
            "a real switch after it"
        );
        d.expect_refocus(t);
        assert!(
            d.feed_at("workspace>>2\n", t),
            "a workspace switch still dismisses"
        );
        let late = t + REFOCUS_WINDOW;
        d.expect_refocus(t);
        assert!(!d.feed_at("activewindow>>foot,~\n", late));
        assert!(
            d.feed_at("activewindowv2>>b\n", late),
            "the refocus never came"
        );
    }

    const MS: u64 = 1_000_000;

    /// A panel fades in with `ease` over 250 ms and out with `in` over 150 ms, hidden once at
    /// nothing; a show during the fade-out turns it around and nothing hides.
    #[test]
    fn a_panel_fades_in_and_out_and_hides_once() {
        let clock = Clock::manual();
        let (seen, hides) = (Rc::new(Cell::new(1.0f32)), Rc::new(Cell::new(0)));
        let p = {
            let (s, h) = (seen.clone(), hides.clone());
            Presence::on_clock(&clock, move |v| s.set(v), move || h.set(h.get() + 1))
        };
        p.opened(motion::Duration::Normal);
        assert_eq!(seen.get(), 0.0, "starts from nothing");
        clock.step(0);
        clock.step(125 * MS);
        assert!(
            seen.get() > 0.5 && seen.get() < 1.0,
            "ease: past half at half time"
        );
        clock.step(250 * MS);
        assert_eq!(seen.get(), 1.0);
        p.close();
        clock.step(260 * MS);
        clock.step(335 * MS);
        assert!(seen.get() > 0.5, "in: slow start ({})", seen.get());
        assert_eq!(hides.get(), 0);
        clock.step(410 * MS);
        assert_eq!((seen.get(), hides.get()), (0.0, 1), "gone, hidden once");
        assert!(!p.is_closing());
        // Open again, close, and show it again half way out: it comes back and stays.
        p.opened(motion::Duration::Normal);
        clock.step(500 * MS);
        clock.step(750 * MS);
        p.close();
        clock.step(760 * MS);
        clock.step(830 * MS);
        let mid = seen.get();
        assert!(mid > 0.0 && mid < 1.0);
        p.keep();
        clock.step(840 * MS);
        assert!(seen.get() >= mid - 0.01, "turns around from where it is");
        clock.step(1200 * MS);
        assert_eq!((seen.get(), hides.get()), (1.0, 1), "kept: no second hide");
        assert!(clock.is_empty());
    }

    /// Reduce motion: no frames between, the hide is at once.
    #[test]
    fn under_reduce_motion_a_panel_snaps() {
        let clock = Clock::manual();
        clock.set_reduced(Some(true));
        let (seen, hides) = (Rc::new(Cell::new(1.0f32)), Rc::new(Cell::new(0)));
        let p = {
            let (s, h) = (seen.clone(), hides.clone());
            Presence::on_clock(&clock, move |v| s.set(v), move || h.set(h.get() + 1))
        };
        p.opened(motion::Duration::Normal);
        assert_eq!(seen.get(), 1.0);
        p.close();
        assert_eq!((seen.get(), hides.get()), (0.0, 1));
        assert!(clock.is_empty());
    }

    /// A panel switch: the hidden one does not resync while the new one waits for its map.
    #[test]
    fn a_panel_switch_resyncs_the_grab_on_the_new_panels_map() {
        assert!(!sync_on_hide([(false, false), (true, false)]), "switching");
        assert!(
            sync_on_hide([(false, false), (true, true)]),
            "the other is on screen"
        );
        assert!(sync_on_hide([(false, false); 5]), "the last one closed");
    }

    /// Closing the panel forgets the window; the next open records whoever is in front then.
    #[test]
    fn each_open_records_the_window_in_front_then() {
        let mut d = Dismisser::default();
        d.feed("activewindowv2>>1\n");
        d.opened();
        d.closed();
        d.feed("activewindow>>foot,~\n");
        d.feed("activewindowv2>>2\n");
        d.opened();
        assert!(!d.feed("activewindow>>foot,~\n"));
        assert!(!d.feed("activewindowv2>>2\n"), "window 2 opened the panel");
        assert!(!d.feed("activewindow>>a,b\n"));
        assert!(d.feed("activewindowv2>>1\n"), "window 1 is another app now");
        assert_eq!(address(" 0xAB12\n"), "ab12");
        assert_eq!(active_address("activewindowv2>>,"), Some(String::new()));
        assert_eq!(active_address("activewindow>>a,b"), None);
    }
}
