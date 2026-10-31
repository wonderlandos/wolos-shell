// SPDX-License-Identifier: GPL-2.0-or-later
//! Reduce motion: one switch for every animation in every shell process. GTK's
//! `gtk-enable-animations` turns off CSS animations and transitions (revealers, stacks, the Dock
//! bounce); WolOS's own setting (Accessibility > Reduce motion, desktop prefs) drives it, and
//! hand-made animations ask [`reduced`].

/// True when animations are off (the WolOS setting or GTK's).
pub fn reduced() -> bool {
    gtk::Settings::default().is_some_and(|s| !s.is_gtk_enable_animations())
}

/// What `gtk-enable-animations` becomes when the WolOS setting reads `now` (last read `prev`):
/// Some(on) to set it, None to leave it (unchanged, or never turned on: a system-wide GTK setting
/// stays the user's).
pub fn animations_after(prev: Option<bool>, now: bool) -> Option<bool> {
    match prev {
        Some(p) if p == now => None,
        None if !now => None,
        _ => Some(!now),
    }
}

/// Window open/close animation tokens: a damped spring per direction and the plain duration.
pub mod window {
    /// Spring constants (mass 1): opening a window.
    pub const OPEN: Spring = Spring {
        stiffness: 385.5,
        damping: 31.4,
    };
    /// Spring constants (mass 1): closing a window.
    pub const CLOSE: Spring = Spring {
        stiffness: 322.3,
        damping: 30.5,
    };
    /// `duration.window`: seconds for a window to open, close, move or resize.
    pub const DURATION_SECS: f64 = 0.2;

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Spring {
        pub stiffness: f64,
        pub damping: f64,
    }

    impl Spring {
        /// Damping ratio for mass 1: below 1 the window overshoots a little, at 1 it does not.
        pub fn damping_ratio(self) -> f64 {
            self.damping / (2.0 * self.stiffness.sqrt())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_tokens_are_the_agreed_constants() {
        assert_eq!(
            (window::OPEN.stiffness, window::OPEN.damping),
            (385.5, 31.4)
        );
        assert_eq!(
            (window::CLOSE.stiffness, window::CLOSE.damping),
            (322.3, 30.5)
        );
        assert_eq!(window::DURATION_SECS, 0.2);
        // Both are lightly underdamped: a settle, not a wobble.
        for s in [window::OPEN, window::CLOSE] {
            assert!((0.7..1.0).contains(&s.damping_ratio()), "{s:?}");
        }
    }

    #[test]
    fn the_setting_drives_gtk_animations_only_when_it_changes() {
        assert_eq!(animations_after(None, true), Some(false));
        assert_eq!(animations_after(None, false), None);
        assert_eq!(animations_after(Some(true), true), None);
        assert_eq!(animations_after(Some(true), false), Some(true));
        assert_eq!(animations_after(Some(false), true), Some(false));
    }
}
