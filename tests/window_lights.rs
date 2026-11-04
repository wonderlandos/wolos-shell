// SPDX-License-Identifier: GPL-2.0-or-later
//! The compositor draws the window buttons of Kit windows. The About panel is a layer surface, which
//! gets no strip, so it keeps a close light of its own, top right where the compositor puts close.

const CSS: &str = include_str!("../src/kit/kit.css");
const WIDGETS: &str = include_str!("../src/kit/widgets.rs");
const PANELS: &str = include_str!("../src/ui/panels.rs");
const PICKERS: &str = include_str!("../src/kit/pickers.rs");

#[test]
fn app_windows_draw_no_window_buttons() {
    for s in [
        "traffic-lights",
        "w.minimize()",
        "w.maximize()",
        "w.unmaximize()",
    ] {
        assert!(!WIDGETS.contains(s), "{s}");
    }
}

/// The strip above a Kit window shows the title and takes 28 px: the toolbar repeats neither.
#[test]
fn the_toolbar_leaves_the_title_to_the_strip() {
    assert!(!WIDGETS.contains("toolbar-title"));
    assert!(CSS.contains(".toolbar { min-height: 40px;"));
}

#[test]
fn the_about_panel_keeps_its_close_light() {
    assert!(PANELS.contains("traffic-lights"));
    // The glyph shows for the whole group at once; only the circle under the pointer takes the accent.
    assert!(CSS.contains(".traffic-lights:hover button.light image { color: var(--text); }"));
    assert!(CSS.contains("button.light:hover { background: var(--accent); }"));
    assert!(CSS.contains("button.light:active { background: shade(var(--accent), 0.8); }"));
    assert!(
        !CSS.contains("button.light:hover image { color: var(--text)"),
        "a per-button rule would show one glyph at a time"
    );
}

/// WolOS draws its own window buttons: one quiet circle for all three, no red, amber and green.
#[test]
fn the_buttons_are_not_traffic_lights() {
    for s in [
        "button.light.close",
        "button.light.minimize",
        "button.light.zoom",
        "var(--danger)",
        "var(--warning)",
        "var(--success)",
    ] {
        let line = CSS
            .lines()
            .find(|l| l.starts_with("button.light") && l.contains(s));
        assert!(line.is_none(), "{s}: {line:?}");
    }
}

/// Close sits at the right edge everywhere, as the compositor row draws it (close rightmost).
#[test]
fn the_about_close_light_sits_top_right() {
    assert!(PANELS.contains("lights.set_halign(gtk::Align::End);"));
    assert!(!PANELS.contains("close.set_halign(gtk::Align::Start)"));
    assert!(CSS.contains(".traffic-lights { margin-left: 16px; }"));
    assert!(!CSS.contains(".traffic-lights { margin-right"));
}

/// The theme previews draw the same controls: right-aligned, one quiet colour, no status colours.
#[test]
fn previews_draw_quiet_controls_on_the_right() {
    assert!(!PICKERS.contains("[t.danger, t.warning, t.success]"));
    assert!(PICKERS.contains("x + ww - r.max(bar * 0.5) - f64::from(i) * dot * 3.0"));
}
