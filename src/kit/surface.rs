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
    if s.exclusive {
        w.auto_exclusive_zone_enable();
    }
    w.set_keyboard_mode(match s.keyboard {
        Keyboard::None => KeyboardMode::None,
        Keyboard::OnDemand => KeyboardMode::OnDemand,
        Keyboard::Exclusive => KeyboardMode::Exclusive,
    });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_surfaces_match_compositor_glass_namespaces() {
        assert_eq!(Spec::menubar().namespace, "wol-menubar");
        assert_eq!(Spec::dock().namespace, "wol-dock");
        assert!(Spec::menubar().exclusive && Spec::dock().exclusive);
        assert_eq!(Spec::wallpaper().layer, Layer::Background);
    }
}
