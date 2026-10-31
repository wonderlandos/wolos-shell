// SPDX-License-Identifier: GPL-2.0-or-later
//! The compositor draws the window buttons of Kit windows. The About panel is a layer surface, which
//! gets no strip, so it keeps a close light of its own.

const CSS: &str = include_str!("../src/kit/kit.css");
const WIDGETS: &str = include_str!("../src/kit/widgets.rs");
const PANELS: &str = include_str!("../src/ui/panels.rs");

#[test]
fn app_windows_draw_no_window_buttons() {
    for s in ["traffic-lights", "w.minimize()", "w.maximize()", "w.unmaximize()"] {
        assert!(!WIDGETS.contains(s), "{s}");
    }
}

/// The strip above a Kit window shows the title and takes 28 px: the toolbar repeats neither.
#[test]
fn the_toolbar_leaves_the_title_to_the_strip() {
    assert!(!WIDGETS.contains("toolbar-title"));
    assert!(CSS.contains(".toolbar { min-height: 36px;"));
}

#[test]
fn the_about_panel_keeps_its_close_light() {
    assert!(PANELS.contains("traffic-lights"));
    assert!(CSS.contains(".traffic-lights:hover button.light image { color: rgba(0,0,0,0.55); }"));
    assert!(
        !CSS.contains("button.light:hover image"),
        "a per-button rule would show one glyph at a time"
    );
}
