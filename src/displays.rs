// SPDX-License-Identifier: GPL-2.0-or-later
//! Which shell surfaces go on which display, pure side. Every display gets the wallpaper and a
//! menu bar (dimmed while another display has the focus, like macOS); the main display (the
//! first one Hyprland lists) also gets the Dock and the desktop widgets. `ui::displays` builds
//! and drops surfaces as displays come and go.

/// A display's connector name ("eDP-1", "HDMI-A-1").
pub type Connector = String;

/// What changed between two display lists.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Change {
    pub added: Vec<Connector>,
    pub removed: Vec<Connector>,
    /// The main display is now this one (None: unchanged, or no display left).
    pub main: Option<Connector>,
}

/// The main display: the first listed.
pub fn main_of(displays: &[Connector]) -> Option<&Connector> {
    displays.first()
}

pub fn change(old: &[Connector], new: &[Connector]) -> Change {
    let main = main_of(new).filter(|m| main_of(old) != Some(*m)).cloned();
    Change {
        added: new.iter().filter(|c| !old.contains(c)).cloned().collect(),
        removed: old.iter().filter(|c| !new.contains(c)).cloned().collect(),
        main,
    }
}

/// A menu bar is shown at full strength on the focused display only. Unknown focus (startup,
/// one display): every bar is active.
pub fn bar_active(bar: &str, focused: Option<&str>) -> bool {
    focused.is_none_or(|f| f == bar)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<Connector> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn plugging_and_unplugging_displays() {
        let c = change(&[], &v(&["eDP-1"]));
        assert_eq!((c.added, c.main), (v(&["eDP-1"]), Some("eDP-1".into())));
        let c = change(&v(&["eDP-1"]), &v(&["eDP-1", "HDMI-A-1"]));
        assert_eq!(
            (c.added, c.removed, c.main),
            (v(&["HDMI-A-1"]), vec![], None)
        );
        // Unplugging the main display moves the Dock and the desktop to the next one.
        let c = change(&v(&["eDP-1", "HDMI-A-1"]), &v(&["HDMI-A-1"]));
        assert_eq!(
            (c.removed, c.main),
            (v(&["eDP-1"]), Some("HDMI-A-1".into()))
        );
        let c = change(&v(&["eDP-1"]), &[]);
        assert_eq!((c.removed, c.main), (v(&["eDP-1"]), None));
        assert_eq!(change(&v(&["a", "b"]), &v(&["a", "b"])), Change::default());
    }

    #[test]
    fn only_the_focused_display_has_a_full_strength_menu_bar() {
        assert!(bar_active("eDP-1", Some("eDP-1")));
        assert!(!bar_active("HDMI-A-1", Some("eDP-1")));
        assert!(bar_active("HDMI-A-1", None));
    }
}
