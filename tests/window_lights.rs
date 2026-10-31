// SPDX-License-Identifier: GPL-2.0-or-later
//! Kit window lights: hovering any of them shows the glyphs on all three, and all three act.

const CSS: &str = include_str!("../src/kit/kit.css");
const WIDGETS: &str = include_str!("../src/kit/widgets.rs");

#[test]
fn hovering_the_group_shows_every_glyph() {
    assert!(CSS.contains("button.light image { -gtk-icon-size: 8px; color: transparent; }"));
    assert!(CSS.contains(".traffic-lights:hover button.light image { color: rgba(0,0,0,0.55); }"));
    assert!(
        !CSS.contains("button.light:hover image"),
        "a per-button rule would show one glyph at a time"
    );
}

#[test]
fn every_light_has_a_glyph_and_an_action() {
    for icon in [
        "window-close-symbolic",
        "window-minimize-symbolic",
        "window-maximize-symbolic",
    ] {
        assert!(WIDGETS.contains(icon), "{icon}");
    }
    assert!(WIDGETS.contains("w.close()") && WIDGETS.contains("w.minimize()"));
    assert!(WIDGETS.contains("w.maximize()") && WIDGETS.contains("w.unmaximize()"));
}
