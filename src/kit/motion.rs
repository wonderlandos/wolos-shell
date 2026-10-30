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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setting_drives_gtk_animations_only_when_it_changes() {
        assert_eq!(animations_after(None, true), Some(false));
        assert_eq!(animations_after(None, false), None);
        assert_eq!(animations_after(Some(true), true), None);
        assert_eq!(animations_after(Some(true), false), Some(true));
        assert_eq!(animations_after(Some(false), true), Some(false));
    }
}
