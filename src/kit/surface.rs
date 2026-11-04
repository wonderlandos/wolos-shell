// SPDX-License-Identifier: GPL-2.0-or-later
//! Layer-shell placement. Without the `layer-shell` feature windows are plain toplevels (dev on any desktop).
use gtk::prelude::*;
pub use shared::layout::{Edge, Orientation};

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
    /// Spans the whole output (wallpaper, Overview).
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
    /// The menu bar on today's edge (top): [`Spec::bar`] with [`Edge::Top`].
    pub const fn menubar() -> Self {
        Self::bar(Edge::Top)
    }
    /// The bar on `edge` (ADR 0031 `Layout::bar`): spans that edge and reserves its height (or
    /// width on a side). The namespace stays `wol-menubar` on every edge (blur rules).
    pub const fn bar(edge: Edge) -> Self {
        Spec {
            namespace: "wol-menubar",
            layer: Layer::Top,
            anchor: span(edge),
            exclusive: true,
            keyboard: Keyboard::None,
        }
    }
    /// The Dock on `edge` (ADR 0031 `Layout::dock`): centred on that edge, reserving its shelf.
    pub const fn dock(edge: Edge) -> Self {
        Spec {
            namespace: "wol-dock",
            layer: Layer::Top,
            anchor: only(edge),
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
    /// It stays there in Edit Widgets; the gallery has its own surface ([`Spec::widgets_gallery`]).
    pub const fn desktop() -> Self {
        Spec {
            namespace: "wol-desktop",
            layer: Layer::Bottom,
            anchor: [true; 4],
            exclusive: false,
            keyboard: Keyboard::None,
        }
    }
    /// Edit Widgets' gallery: its own surface above app windows (Top, not the desktop re-layered
    /// at runtime, which stayed under Settings: QA bug 5), along the bottom edge inside the Dock
    /// zone, left/right layer margins keep it clear of the cards. A `wol-panel-*` namespace:
    /// the compositor's glass rule blurs it and forwards Esc while it is mapped.
    pub const fn widgets_gallery() -> Self {
        Spec {
            layer: Layer::Top,
            ..Self::panel("wol-panel-widgets", span(Edge::Bottom), Keyboard::OnDemand)
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
    /// Notification banners: top right, never take the keyboard. On the Top layer, not Overlay,
    /// so a zoomed window's menu bar band stays theirs. The compositor draws a Top layer mapped after a
    /// window went full screen above it, so the shell holds banners while the active workspace
    /// is in full screen and shows them after (`ui::notify::held`, audit3 R2A-9).
    pub const fn banners() -> Self {
        Spec {
            layer: Layer::Top,
            ..Self::panel(
                "wol-panel-banners",
                [true, false, false, true],
                Keyboard::None,
            )
        }
    }
}

/// Anchors (top, bottom, left, right) of a surface pinned to `edge` alone (the Dock).
pub const fn only(edge: Edge) -> [bool; 4] {
    match edge {
        Edge::Top => [true, false, false, false],
        Edge::Bottom => [false, true, false, false],
        Edge::Left => [false, false, true, false],
        Edge::Right => [false, false, false, true],
    }
}

/// Anchors of a surface that runs the whole length of `edge` (the bar): the edge and both
/// edges across it.
pub const fn span(edge: Edge) -> [bool; 4] {
    match edge {
        Edge::Top => [true, false, true, true],
        Edge::Bottom => [false, true, true, true],
        Edge::Left => [true, true, true, false],
        Edge::Right => [true, true, false, true],
    }
}

/// How an autohidden surface on `edge` slides into view: out of its edge, toward the screen.
pub fn reveal_from(edge: Edge) -> gtk::RevealerTransitionType {
    use gtk::RevealerTransitionType as T;
    match edge {
        Edge::Top => T::SlideDown,
        Edge::Bottom => T::SlideUp,
        Edge::Left => T::SlideRight,
        Edge::Right => T::SlideLeft,
    }
}

/// Box orientation of the items of a bar or Dock on `edge`.
pub fn orientation(edge: Edge) -> gtk::Orientation {
    match edge.orientation() {
        Orientation::Horizontal => gtk::Orientation::Horizontal,
        Orientation::Vertical => gtk::Orientation::Vertical,
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

/// Moves a placed surface to the anchors of `s` (the layout's bar or Dock edge changed).
#[cfg(feature = "layer-shell")]
pub fn reanchor(w: &impl IsA<gtk::Window>, s: Spec) {
    use gtk4_layer_shell::{Edge, LayerShell};
    if !gtk4_layer_shell::is_supported() {
        return;
    }
    for (e, on) in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right]
        .into_iter()
        .zip(s.anchor)
    {
        w.set_anchor(e, on);
    }
}

#[cfg(not(feature = "layer-shell"))]
pub fn reanchor(_w: &impl IsA<gtk::Window>, _s: Spec) {}

/// Puts `w` on `display` (None: the compositor picks). A mapped surface moves at once.
#[cfg(feature = "layer-shell")]
pub fn set_monitor(w: &impl IsA<gtk::Window>, display: Option<&gtk::gdk::Monitor>) {
    use gtk4_layer_shell::LayerShell;
    if gtk4_layer_shell::is_supported() {
        w.set_monitor(display);
    }
}

#[cfg(not(feature = "layer-shell"))]
pub fn set_monitor(_w: &impl IsA<gtk::Window>, _display: Option<&gtk::gdk::Monitor>) {}

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
        [true, false, true, true] | [false, true, true, true] => w.set_default_size(1280, 28),
        [true, true, true, false] | [true, true, false, true] => w.set_default_size(28, 800),
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

/// Reserve exactly `px` of screen space (0 releases it). The Dock uses this instead of the
/// automatic zone: its layer window is taller than the shelf (hot zone, lift room above the
/// tiles), and reserving all of it left a band between zoomed windows and the shelf.
#[cfg(feature = "layer-shell")]
pub fn set_exclusive_px(w: &impl IsA<gtk::Window>, px: i32) {
    use gtk4_layer_shell::LayerShell;
    if gtk4_layer_shell::is_supported() {
        w.set_exclusive_zone(px.max(0));
    }
}

#[cfg(not(feature = "layer-shell"))]
pub fn set_exclusive_px(_w: &impl IsA<gtk::Window>, _px: i32) {}

/// The one Esc handler (audit3 R2A-22): a key controller on `w` that runs `f` on Esc and stops
/// the key there; every other key goes on. Text fields that use Esc to cancel an edit keep their
/// own handler (`kit::form`).
pub fn on_escape(w: &impl IsA<gtk::Widget>, f: impl Fn() + 'static) {
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(move |_, k, _, _| {
        if k == gtk::gdk::Key::Escape {
            f();
            gtk::glib::Propagation::Stop
        } else {
            gtk::glib::Propagation::Proceed
        }
    });
    w.add_controller(keys);
}

/// Which screen size to use: the display under the surface, else the first display; a display
/// that reports 0×0 (headless at start, an output being unplugged) does not count. None when
/// neither is known; the caller picks its own fallback (audit2 A2-2, A2-11; audit3 R2A-22).
pub fn pick_screen(
    on_surface: Option<(i32, i32)>,
    first: Option<(i32, i32)>,
) -> Option<(i32, i32)> {
    let real = |s: &(i32, i32)| s.0 > 0 && s.1 > 0;
    on_surface.filter(real).or(first.filter(real))
}

/// The one screen-geometry lookup (audit3 R2A-22): the logical size (width, height) of the
/// display `w` is on, or of the first display before `w` has a surface. None without a display.
pub fn screen_of(w: &impl IsA<gtk::Widget>) -> Option<(i32, i32)> {
    let size = |m: gtk::gdk::Monitor| (m.geometry().width(), m.geometry().height());
    let d = w.as_ref().display();
    let on_surface = monitor_of(w.as_ref()).map(size);
    let first = d
        .monitors()
        .item(0)
        .and_downcast::<gtk::gdk::Monitor>()
        .map(size);
    pick_screen(on_surface, first)
}

/// Whether a reservation on display `theirs` counts for a panel on display `ours`: the same
/// display, or either one not known yet (then it counts, the safe side: the panel ends higher).
pub fn same_display<T: PartialEq>(theirs: Option<T>, ours: Option<T>) -> bool {
    match (theirs, ours) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    }
}

/// The display a widget's surface is on, None before it has one.
fn monitor_of(w: &gtk::Widget) -> Option<gtk::gdk::Monitor> {
    let d = w.display();
    w.native()
        .and_then(|n| n.surface())
        .and_then(|s| d.monitor_at_surface(&s))
}

/// The connector name (`eDP-1`) of the output `w` is on; None before it has a surface or when the
/// output has none. The one place that asks the display which monitor a surface is on.
pub fn connector_of(w: &impl IsA<gtk::Widget>) -> Option<String> {
    Some(monitor_of(w.as_ref())?.connector()?.to_string())
}

/// [`reserved_bottom`] for the display `on` is on only: a Dock on another display does not
/// shorten a panel here (audit3 R2A-4).
#[cfg(feature = "layer-shell")]
pub fn reserved_bottom_on(on: &impl IsA<gtk::Widget>) -> i32 {
    use gtk4_layer_shell::{Edge, LayerShell};
    if !gtk4_layer_shell::is_supported() {
        return 0;
    }
    let ours = monitor_of(on.as_ref());
    let tops = gtk::Window::toplevels();
    (0..tops.n_items())
        .filter_map(|i| tops.item(i).and_downcast::<gtk::Window>())
        .filter(|w| {
            w.is_layer_window()
                && w.is_mapped()
                && w.is_anchor(Edge::Bottom)
                && !w.is_anchor(Edge::Top)
        })
        .filter(|w| {
            let theirs = LayerShell::monitor(w).or_else(|| monitor_of(w.upcast_ref()));
            same_display(theirs, ours.clone())
        })
        .map(|w| w.exclusive_zone().max(0))
        .sum()
}

#[cfg(not(feature = "layer-shell"))]
pub fn reserved_bottom_on(_on: &impl IsA<gtk::Widget>) -> i32 {
    0
}

/// Pixels this process's layer surfaces reserve at the bottom of the screen (the Dock's shelf,
/// [`set_exclusive_px`]): mapped layers anchored to the bottom edge and not the top, summed.
/// 0 without layer-shell or when nothing reserves (Dock autohide).
#[cfg(feature = "layer-shell")]
pub fn reserved_bottom() -> i32 {
    use gtk4_layer_shell::{Edge, LayerShell};
    if !gtk4_layer_shell::is_supported() {
        return 0;
    }
    let tops = gtk::Window::toplevels();
    (0..tops.n_items())
        .filter_map(|i| tops.item(i).and_downcast::<gtk::Window>())
        .filter(|w| {
            w.is_layer_window()
                && w.is_mapped()
                && w.is_anchor(Edge::Bottom)
                && !w.is_anchor(Edge::Top)
        })
        .map(|w| w.exclusive_zone().max(0))
        .sum()
}

#[cfg(not(feature = "layer-shell"))]
pub fn reserved_bottom() -> i32 {
    0
}

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

/// Keyboard mode of the menu bar and the Dock while invoked (⌃F2, ⌃F3). The compositor 0.56 gives a
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
                // A container (the Dock's shelf, rebuilt with its layout) hands the focus to
                // its first focusable child.
                if !first.grab_focus() {
                    first.child_focus(gtk::DirectionType::TabForward);
                }
            } else {
                GtkWindowExt::set_focus(&win, None::<&gtk::Widget>);
            }
        })
    };
    let a = access.clone();
    on_escape(&win, move || a(false));
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

    /// R2A-22 (was overview `screen_size`, A2-2): the display under the surface wins, a 0×0 one
    /// falls through to the first display, and nothing known is None.
    /// R2A-4: the Dock on display 1 does not shorten a panel on display 2; unknown counts.
    #[test]
    fn a_reservation_counts_on_its_own_display_only() {
        assert!(same_display(Some(1), Some(1)));
        assert!(!same_display(Some(1), Some(2)));
        assert!(same_display(None, Some(2)));
        assert!(same_display(Some(1), None::<i32>));
    }

    #[test]
    fn the_screen_is_the_real_display_under_the_surface() {
        assert_eq!(
            pick_screen(Some((1280, 800)), Some((1920, 1080))),
            Some((1280, 800))
        );
        assert_eq!(
            pick_screen(Some((0, 0)), Some((1920, 1080))),
            Some((1920, 1080))
        );
        assert_eq!(pick_screen(None, Some((0, 0))), None);
        assert_eq!(pick_screen(None, None), None);
    }

    /// J13: the menu bar and the Dock never take the keyboard by themselves (typing stays in the
    /// app), and ⌃F2/⌃F3 must take it at once: Exclusive, the only mode the compositor focuses on change.
    #[test]
    fn menu_bar_and_dock_take_the_keyboard_only_when_invoked() {
        assert_eq!(Spec::menubar().keyboard, Keyboard::None);
        assert_eq!(Spec::dock(Edge::Bottom).keyboard, Keyboard::None);
        assert_eq!(INVOKED, Keyboard::Exclusive);
    }

    /// ⌘C/V/X with a terminal in front taps Ctrl+Shift+C/V/X (appmenu::send); a shell text
    /// field that holds the keyboard (Search over a terminal) must still copy and paste.
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

    /// ADR 0031: the bar and the Dock take their anchors from the layout's edges; today's
    /// desktop is unchanged, the namespaces stay, and the Dock slides in from its own edge.
    #[test]
    fn bar_and_dock_follow_the_layout_edges() {
        assert_eq!(Spec::menubar(), Spec::bar(Edge::Top));
        assert_eq!(Spec::bar(Edge::Top).anchor, [true, false, true, true]);
        assert_eq!(Spec::dock(Edge::Bottom).anchor, [false, true, false, false]);
        for e in Edge::ALL {
            let (bar, dock) = (Spec::bar(e), Spec::dock(e));
            let i = Edge::ALL.iter().position(|x| *x == e).unwrap();
            assert!(bar.anchor[i] && dock.anchor[i], "{e:?}");
            assert_eq!(bar.anchor.iter().filter(|a| **a).count(), 3, "spans {e:?}");
            assert_eq!(
                dock.anchor.iter().filter(|a| **a).count(),
                1,
                "centred on {e:?}"
            );
            assert!(!bar.anchor[Edge::ALL.iter().position(|x| *x == e.opposite()).unwrap()]);
            assert_eq!((bar.namespace, dock.namespace), ("wol-menubar", "wol-dock"));
            assert_eq!((bar.zone(), dock.zone()), (Zone::Reserve, Zone::Reserve));
        }
        assert_eq!(
            reveal_from(Edge::Bottom),
            gtk::RevealerTransitionType::SlideUp
        );
        assert_eq!(
            reveal_from(Edge::Left),
            gtk::RevealerTransitionType::SlideRight
        );
        assert_eq!(orientation(Edge::Right), gtk::Orientation::Vertical);
        assert_eq!(orientation(Edge::Top), gtk::Orientation::Horizontal);
    }

    #[test]
    fn shell_surfaces_match_compositor_glass_namespaces() {
        assert_eq!(Spec::menubar().namespace, "wol-menubar");
        assert_eq!(Spec::dock(Edge::Bottom).namespace, "wol-dock");
        assert!(Spec::menubar().exclusive && Spec::dock(Edge::Bottom).exclusive);
        assert_eq!(Spec::wallpaper().layer, Layer::Background);
        // The compositor blurs `^(wol-menubar|wol-dock|wol-panel-.*)$` (desktop hyprland.conf).
        assert!(Spec::widgets_gallery().namespace.starts_with("wol-panel-"));
    }

    /// QA bug 5: the gallery is its own Top-layer surface (above Settings) that takes the
    /// keyboard on demand; the desktop under it stays on the Bottom layer.
    #[test]
    fn widgets_gallery_is_its_own_top_surface() {
        let g = Spec::widgets_gallery();
        assert_eq!(g.namespace, "wol-panel-widgets");
        assert_eq!(g.layer, Layer::Top);
        assert_eq!(g.keyboard, Keyboard::OnDemand);
        // [top, bottom, left, right]: the bottom edge, both sides (layer margins clear the cards).
        assert_eq!(g.anchor, [false, true, true, true]);
        assert!(!g.exclusive);
        assert_eq!(g.zone(), Zone::Respect, "above the Dock, not under it");
        assert_eq!(g.on_output_lost(), OutputLost::Hide);
        let d = Spec::desktop();
        assert_eq!((d.layer, d.keyboard), (Layer::Bottom, Keyboard::None));
    }

    /// Live on the compositor: the screenshot bar sat at the left edge, the thumbnail bottom-left.
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
            Spec::dock(Edge::Bottom),
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

    /// Audit3 R2A-9: the layer alone did not keep banners off a fullscreen game (a Top layer
    /// mapped after the game went full screen draws above it). They are held while the active
    /// workspace is in full screen, critical ones excepted; a zoomed window holds nothing.
    #[test]
    fn banners_are_held_while_a_window_is_in_full_screen() {
        use crate::hypr::Fullscreen;
        let note = |critical| crate::ui::notify::Note {
            id: 1,
            app: String::new(),
            icon: String::new(),
            summary: String::new(),
            body: String::new(),
            critical,
            timeout_ms: -1,
            image: String::new(),
            actions: vec![],
        };
        assert!(crate::ui::notify::held(Fullscreen::Full, &note(false)));
        assert!(!crate::ui::notify::held(Fullscreen::Full, &note(true)));
        assert!(!crate::ui::notify::held(
            Fullscreen::Maximized,
            &note(false)
        ));
        let b = Spec::banners();
        assert_eq!(b.layer, Layer::Top);
        assert_eq!(b.namespace, "wol-panel-banners");
        assert_eq!(b.anchor, [true, false, false, true]);
        assert_eq!(b.keyboard, Keyboard::None);
        assert!(!b.exclusive);
        assert_eq!(b.zone(), Zone::Respect);
        assert_eq!(b.on_output_lost(), OutputLost::Hide);
    }

    /// Live on the compositor: the wallpaper stopped at the menu bar and dock zones (black strips).
    #[test]
    fn full_screen_surfaces_ignore_reserved_zones() {
        for (spec, want) in [
            (Spec::menubar(), Zone::Reserve),
            (Spec::dock(Edge::Bottom), Zone::Reserve),
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
