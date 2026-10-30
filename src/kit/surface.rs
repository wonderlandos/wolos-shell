//! Layer-shell placement. Without the `layer-shell` feature windows are plain toplevels (dev on any desktop).
use gtk::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Background,
    Bottom,
    Top,
    Overlay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyboard {
    None,
    OnDemand,
    Exclusive,
}

/// How a surface treats other surfaces' exclusive zones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    /// Reserves its own size (menu bar, dock).
    Reserve,
    /// Placed inside what others reserve.
    Respect,
    /// Spans the whole output (wallpaper, Mission Control).
    Ignore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spec {
    /// Layer namespace; the compositor's blur rules match `wol-menubar`, `wol-dock`, `wol-panel-*`.
    pub namespace: &'static str,
    pub layer: Layer,
    /// top, bottom, left, right
    pub anchor: [bool; 4],
    pub exclusive: bool,
    pub keyboard: Keyboard,
}

impl Spec {
    pub fn zone(&self) -> Zone {
        match (self.exclusive, self.anchor, self.layer) {
            (true, _, _) => Zone::Reserve,
            // Desktop widgets fill the area between the menu bar and the Dock.
            // and so does the click-catcher (menu bar and Dock clicks reach them).
            (false, [true, true, true, true], Layer::Bottom | Layer::Top) => Zone::Respect,
            (false, [true, true, true, true], _) => Zone::Ignore,
            _ => Zone::Respect,
        }
    }
    /// Screenshot toolbar: bottom centre, takes the keyboard (Esc, Return).
    pub const fn capture_bar() -> Self {
        Self::panel(
            "wol-panel-capture",
            [false, true, false, false],
            Keyboard::Exclusive,
        )
    }
    /// Thumbnail after a capture: bottom right, never takes the keyboard.
    pub const fn capture_thumb() -> Self {
        Self::panel(
            "wol-panel-capture-thumb",
            [false, true, false, true],
            Keyboard::None,
        )
    }
    pub const fn menubar() -> Self {
        Spec {
            namespace: "wol-menubar",
            layer: Layer::Top,
            anchor: [true, false, true, true],
            exclusive: true,
            keyboard: Keyboard::None,
        }
    }
    pub const fn dock() -> Self {
        Spec {
            namespace: "wol-dock",
            layer: Layer::Top,
            anchor: [false, true, false, false],
            exclusive: true,
            keyboard: Keyboard::None,
        }
    }
    pub const fn wallpaper() -> Self {
        Spec {
            namespace: "wol-wallpaper",
            layer: Layer::Background,
            anchor: [true; 4],
            exclusive: false,
            keyboard: Keyboard::None,
        }
    }
    /// Desktop widgets: above the wallpaper, below windows, inside the menu bar and Dock zones.
    /// Edit Widgets raises it above windows (`set_layer`) and lets it take the keyboard.
    pub const fn desktop() -> Self {
        Spec {
            namespace: "wol-desktop",
            layer: Layer::Bottom,
            anchor: [true; 4],
            exclusive: false,
            keyboard: Keyboard::None,
        }
    }
    /// Click-catcher under open transient panels: covers the screen between the menu bar and
    /// the Dock above windows, never takes the keyboard; a click on it closes the panels.
    pub const fn catcher() -> Self {
        Spec {
            namespace: "wol-catcher",
            layer: Layer::Top,
            anchor: [true; 4],
            exclusive: false,
            keyboard: Keyboard::None,
        }
    }
    pub const fn panel(namespace: &'static str, anchor: [bool; 4], keyboard: Keyboard) -> Self {
        Spec {
            namespace,
            layer: Layer::Overlay,
            anchor,
            exclusive: false,
            keyboard,
        }
    }
}

/// What a shell surface does when the compositor closes it because its display went away.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputLost {
    /// Shown again on a display that remains (menu bar, Dock, wallpaper, desktop).
    Reopen,
    /// Hidden, not destroyed: the next open shows it again (panels, the click-catcher).
    Hide,
}

impl Spec {
    pub fn on_output_lost(&self) -> OutputLost {
        match (self.layer, self.exclusive) {
            (Layer::Background | Layer::Bottom, _) | (_, true) => OutputLost::Reopen,
            _ => OutputLost::Hide,
        }
    }
}

/// Keeps `w` alive when its display is unplugged: gtk4-layer-shell reports the layer surface's
/// `closed` as a window close, which would destroy it. A [`OutputLost::Reopen`] surface maps
/// again once a display is there (the compositor picks it).
pub fn survive_output_loss(w: &impl IsA<gtk::Window>, s: Spec) {
    if !layered() {
        return;
    }
    w.as_ref().connect_close_request(move |w| {
        w.set_visible(false);
        if s.on_output_lost() == OutputLost::Reopen {
            reopen_when_output(w);
        }
        gtk::glib::Propagation::Stop
    });
}

fn reopen_when_output(w: &gtk::Window) {
    let w = w.clone();
    // The display list may still hold the display that went away: give it a moment.
    gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
        let monitors = WidgetExt::display(&w).monitors();
        if monitors.n_items() > 0 {
            w.present();
            return;
        }
        let id = std::rc::Rc::new(std::cell::Cell::new(None));
        let id2 = id.clone();
        let h = monitors.connect_items_changed(move |m, _, _, added| {
            if added > 0 {
                w.present();
                if let Some(h) = id2.take() {
                    m.disconnect(h);
                }
            }
        });
        id.set(Some(h));
    });
}

#[cfg(feature = "layer-shell")]
pub fn place(w: &impl IsA<gtk::Window>, s: Spec) {
    use gtk4_layer_shell::{Edge, KeyboardMode, Layer as L, LayerShell};
    if !gtk4_layer_shell::is_supported() {
        return dev(w, s);
    }
    w.init_layer_shell();
    w.set_namespace(Some(s.namespace));
    w.set_layer(match s.layer {
        Layer::Background => L::Background,
        Layer::Bottom => L::Bottom,
        Layer::Top => L::Top,
        Layer::Overlay => L::Overlay,
    });
    for (e, on) in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right]
        .into_iter()
        .zip(s.anchor)
    {
        w.set_anchor(e, on);
    }
    match s.zone() {
        Zone::Reserve => w.auto_exclusive_zone_enable(),
        Zone::Ignore => w.set_exclusive_zone(-1),
        Zone::Respect => {}
    }
    w.set_keyboard_mode(match s.keyboard {
        Keyboard::None => KeyboardMode::None,
        Keyboard::OnDemand => KeyboardMode::OnDemand,
        Keyboard::Exclusive => KeyboardMode::Exclusive,
    });
}

/// True when surfaces really are layers (not dev toplevels).
pub fn layered() -> bool {
    #[cfg(feature = "layer-shell")]
    return gtk4_layer_shell::is_supported();
    #[cfg(not(feature = "layer-shell"))]
    false
}

#[cfg(not(feature = "layer-shell"))]
pub fn place(w: &impl IsA<gtk::Window>, s: Spec) {
    dev(w, s)
}

/// Plain window stand-in: sized like the layer surface would be.
fn dev(w: &impl IsA<gtk::Window>, s: Spec) {
    let w = w.as_ref();
    w.set_title(Some(s.namespace));
    w.set_decorated(false);
    match s.anchor {
        [true, false, true, true] => w.set_default_size(1280, 28),
        [true, true, true, true] => w.set_default_size(1280, 800),
        _ => {}
    }
}

/// Reserve (or release) screen space at runtime: Dock autohide.
#[cfg(feature = "layer-shell")]
pub fn set_exclusive(w: &impl IsA<gtk::Window>, on: bool) {
    use gtk4_layer_shell::LayerShell;
    if !gtk4_layer_shell::is_supported() {
        return;
    }
    if on {
        w.auto_exclusive_zone_enable();
    } else {
        w.set_exclusive_zone(0);
    }
}

#[cfg(not(feature = "layer-shell"))]
pub fn set_exclusive(_w: &impl IsA<gtk::Window>, _on: bool) {}

/// Move a surface between layers and keyboard modes at runtime (Edit Widgets).
#[cfg(feature = "layer-shell")]
pub fn set_layer(w: &impl IsA<gtk::Window>, layer: Layer, keyboard: Keyboard) {
    use gtk4_layer_shell::{KeyboardMode, Layer as L, LayerShell};
    if !gtk4_layer_shell::is_supported() {
        return;
    }
    w.set_layer(match layer {
        Layer::Background => L::Background,
        Layer::Bottom => L::Bottom,
        Layer::Top => L::Top,
        Layer::Overlay => L::Overlay,
    });
    w.set_keyboard_mode(match keyboard {
        Keyboard::None => KeyboardMode::None,
        Keyboard::OnDemand => KeyboardMode::OnDemand,
        Keyboard::Exclusive => KeyboardMode::Exclusive,
    });
}

#[cfg(not(feature = "layer-shell"))]
pub fn set_layer(_w: &impl IsA<gtk::Window>, _layer: Layer, _keyboard: Keyboard) {}

/// Keyboard mode of the menu bar and the Dock while invoked (⌃F2, ⌃F3). Hyprland 0.56 gives a
/// mapped layer the keyboard on a None -> Exclusive change only (LayerSurface.cpp), not OnDemand.
pub const INVOKED: Keyboard = Keyboard::Exclusive;

#[cfg(feature = "layer-shell")]
pub fn set_keyboard(w: &impl IsA<gtk::Window>, keyboard: Keyboard) {
    use gtk4_layer_shell::{KeyboardMode, LayerShell};
    if !gtk4_layer_shell::is_supported() {
        return;
    }
    w.set_keyboard_mode(match keyboard {
        Keyboard::None => KeyboardMode::None,
        Keyboard::OnDemand => KeyboardMode::OnDemand,
        Keyboard::Exclusive => KeyboardMode::Exclusive,
    });
}

#[cfg(not(feature = "layer-shell"))]
pub fn set_keyboard(_w: &impl IsA<gtk::Window>, _keyboard: Keyboard) {}

/// Keyboard access to a surface that never takes the keyboard by itself (menu bar, Dock):
/// `access(true)` takes it ([`INVOKED`]) and focuses `first` with a visible focus ring;
/// `access(false)` or Esc gives it back. `changed` hears every change.
pub fn keyboard_access(
    win: &impl IsA<gtk::Window>,
    first: &impl IsA<gtk::Widget>,
    changed: impl Fn(bool) + 'static,
) -> std::rc::Rc<dyn Fn(bool)> {
    let (win, first): (gtk::Window, gtk::Widget) = (win.clone().upcast(), first.clone().upcast());
    let on = std::rc::Rc::new(std::cell::Cell::new(false));
    let access: std::rc::Rc<dyn Fn(bool)> = {
        let win = win.clone();
        std::rc::Rc::new(move |take: bool| {
            if on.replace(take) == take {
                return;
            }
            changed(take);
            set_keyboard(&win, if take { INVOKED } else { Keyboard::None });
            win.set_focus_visible(take);
            if take {
                first.grab_focus();
            } else {
                GtkWindowExt::set_focus(&win, None::<&gtk::Widget>);
            }
        })
    };
    let keys = gtk::EventControllerKey::new();
    let a = access.clone();
    keys.connect_key_pressed(move |_, k, _, _| {
        if k == gtk::gdk::Key::Escape {
            a(false);
            gtk::glib::Propagation::Stop
        } else {
            gtk::glib::Propagation::Proceed
        }
    });
    win.add_controller(keys);
    access
}

/// The clipboard action Ctrl+Shift+`key` stands for: what ⌘C/V/X sends while a terminal is in
/// front (`appmenu::send`), so a shell field over a terminal still copies and pastes.
pub fn terminal_clipboard_action(
    key: gtk::gdk::Key,
    mods: gtk::gdk::ModifierType,
) -> Option<&'static str> {
    use gtk::gdk::{Key, ModifierType as M};
    let cs = M::CONTROL_MASK | M::SHIFT_MASK;
    if mods & (cs | M::ALT_MASK | M::SUPER_MASK | M::META_MASK) != cs {
        return None;
    }
    match key.to_lower() {
        Key::c => Some("clipboard.copy"),
        Key::v => Some("clipboard.paste"),
        Key::x => Some("clipboard.cut"),
        _ => None,
    }
}

/// Every shell surface: Ctrl+Shift+C/V/X act on the focused text field.
pub fn terminal_clipboard_keys(win: &impl IsA<gtk::Window>) {
    let w: gtk::Window = win.clone().upcast();
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let win = w.clone();
    keys.connect_key_pressed(move |_, k, _, m| {
        if let Some(a) = terminal_clipboard_action(k, m)
            && let Some(f) = GtkWindowExt::focus(&win)
            && f.activate_action(a, None).is_ok()
        {
            return gtk::glib::Propagation::Stop;
        }
        gtk::glib::Propagation::Proceed
    });
    w.add_controller(keys);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// J13: the menu bar and the Dock never take the keyboard by themselves (typing stays in the
    /// app), and ⌃F2/⌃F3 must take it at once: Exclusive, the only mode Hyprland focuses on change.
    #[test]
    fn menu_bar_and_dock_take_the_keyboard_only_when_invoked() {
        assert_eq!(Spec::menubar().keyboard, Keyboard::None);
        assert_eq!(Spec::dock().keyboard, Keyboard::None);
        assert_eq!(INVOKED, Keyboard::Exclusive);
    }

    /// ⌘C/V/X with a terminal in front taps Ctrl+Shift+C/V/X (appmenu::send); a shell text
    /// field that holds the keyboard (Spotlight over a terminal) must still copy and paste.
    #[test]
    fn shell_text_fields_take_the_terminal_clipboard_keys() {
        use gtk::gdk::{Key, ModifierType as M};
        let cs = M::CONTROL_MASK | M::SHIFT_MASK;
        for (key, mods, want) in [
            (Key::C, cs, Some("clipboard.copy")),
            (Key::V, cs | M::LOCK_MASK, Some("clipboard.paste")),
            (Key::x, cs, Some("clipboard.cut")),
            (Key::v, M::CONTROL_MASK, None),
            (Key::V, cs | M::ALT_MASK, None),
            (Key::Z, cs, None),
        ] {
            assert_eq!(
                terminal_clipboard_action(key, mods),
                want,
                "{key:?} {mods:?}"
            );
        }
    }

    #[test]
    fn shell_surfaces_match_compositor_glass_namespaces() {
        assert_eq!(Spec::menubar().namespace, "wol-menubar");
        assert_eq!(Spec::dock().namespace, "wol-dock");
        assert!(Spec::menubar().exclusive && Spec::dock().exclusive);
        assert_eq!(Spec::wallpaper().layer, Layer::Background);
    }

    /// Live Hyprland: the screenshot bar sat at the left edge, the thumbnail bottom-left.
    #[test]
    fn screenshot_surfaces_sit_bottom_centre_and_bottom_right() {
        // [top, bottom, left, right]
        assert_eq!(Spec::capture_bar().anchor, [false, true, false, false]);
        assert_eq!(Spec::capture_thumb().anchor, [false, true, false, true]);
    }

    /// Unplugging the display a layer surface sits on makes the compositor close it, and
    /// gtk4-layer-shell turns that into a window close: GTK destroyed the menu bar, Dock,
    /// wallpaper and desktop for good. The always-there surfaces come back on the remaining
    /// display; panels only hide, so the next open works.
    #[test]
    fn surfaces_survive_their_display_going_away() {
        for s in [
            Spec::menubar(),
            Spec::dock(),
            Spec::wallpaper(),
            Spec::desktop(),
        ] {
            assert_eq!(s.on_output_lost(), OutputLost::Reopen, "{}", s.namespace);
        }
        for s in [
            Spec::catcher(),
            Spec::capture_thumb(),
            Spec::panel(
                "wol-panel-cc",
                [true, false, false, true],
                Keyboard::OnDemand,
            ),
        ] {
            assert_eq!(s.on_output_lost(), OutputLost::Hide, "{}", s.namespace);
        }
    }

    /// Live Hyprland: the wallpaper stopped at the menu bar and dock zones (black strips).
    #[test]
    fn full_screen_surfaces_ignore_reserved_zones() {
        for (spec, want) in [
            (Spec::menubar(), Zone::Reserve),
            (Spec::dock(), Zone::Reserve),
            (Spec::wallpaper(), Zone::Ignore),
            (Spec::desktop(), Zone::Respect),
            (Spec::catcher(), Zone::Respect),
            (
                Spec::panel("wol-panel-overview", [true; 4], Keyboard::Exclusive),
                Zone::Ignore,
            ),
            (
                Spec::panel("wol-panel-switcher", [false; 4], Keyboard::Exclusive),
                Zone::Respect,
            ),
        ] {
            assert_eq!(spec.zone(), want, "{}", spec.namespace);
        }
    }
}
