//! Transient panels (Control Center, Notification Center, clipboard, session menu): one at a
//! time, and gone on a click outside, Esc, another app in front or a workspace switch.
use crate::hypr;
use gtk::prelude::*;
use std::rc::Rc;

/// Shell panels that close like macOS menus. Not the OSD, Mission Control, the screenshot bar or
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

/// A Hyprland event that closes transient panels: an app window came to the front, or the
/// workspace changed. `activewindow>>,` (a layer surface took the keyboard, e.g. the panel itself)
/// does not.
pub fn dismisses(e: &hypr::Event) -> bool {
    match e {
        hypr::Event::ActiveWindow { class, .. } => !class.is_empty(),
        hypr::Event::Workspace(_) => true,
        _ => false,
    }
}

/// `wol shell hide transient`: the whole set at once (Esc, bound pass-through in hyprland.conf,
/// so it works while an app has the keyboard).
pub const GROUP: &str = "transient";

pub fn closes_all(c: &crate::protocol::Command) -> bool {
    c.target == GROUP && c.verb == crate::protocol::Verb::Hide
}

/// Panel apps in their own process (GApplication ids) that a shell panel opening closes.
pub const PANEL_APPS: &[&str] = &["org.wolos.spotlight"];

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

/// Hides every visible transient panel window.
pub fn close_panels(panels: &super::Panels) {
    for p in PANELS {
        if let Some(w) = panels.borrow().get(p) {
            w.set_visible(false);
        }
    }
}

/// Hides every visible transient panel and the other members of the set.
pub fn close_all(panels: &super::Panels) {
    close_panels(panels);
    EXTRA.with(|e| e.borrow().clone()).iter().for_each(|f| f());
}

/// Panel apps (Spotlight): `close` on a click outside `win` or on a [`dismisses`] event.
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

/// Keeps the transient panels in `panels` exclusive and puts a click-catcher under them while one
/// is open. Layer surfaces get no reliable focus-out, so a click outside lands on the catcher.
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
    let sync = {
        let (panels, catcher) = (panels.clone(), catcher.clone());
        Rc::new(move || {
            let open = PANELS
                .iter()
                .any(|p| panels.borrow().get(p).is_some_and(|w| w.is_visible()));
            catcher.set_visible(open && crate::kit::surface::layered());
        })
    };
    for name in PANELS {
        let Some(w) = panels.borrow().get(name).cloned() else {
            continue;
        };
        let (panels, sync) = (panels.clone(), sync.clone());
        w.connect_visible_notify(move |w| {
            if w.is_visible() {
                close_outside_members();
                for o in others(name) {
                    if let Some(o) = panels.borrow().get(o) {
                        o.set_visible(false);
                    }
                }
            }
            sync();
        });
    }
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
        assert_eq!(parse("hide transient").unwrap().verb, Verb::Hide);
    }

    /// Spotlight (its own process) is closed over D-Bus: its GApplication's `hide` action.
    #[test]
    fn panel_apps_are_closed_by_their_action_path() {
        assert_eq!(PANEL_APPS, ["org.wolos.spotlight"]);
        assert_eq!(object_path("org.wolos.spotlight"), "/org/wolos/spotlight");
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
}
