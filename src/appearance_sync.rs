// SPDX-License-Identifier: GPL-2.0-or-later
//! One appearance for the whole desktop at the Auto switch. In Auto the desktop is light from
//! 07:00 and dark from 19:00 (`wol_core::theme::accent`). Each shell process (and each native
//! Wolkit window) re-reads its own theme at that moment, but the compositor's title strip and
//! every GTK, Qt and foot window outside the shell are restyled only by `wol theme apply`, which
//! the session ran once at its start. So after dusk the App Store went dark under a strip that
//! stayed white (lead, 2026-11-04 22:17Z), and Nautilus stayed light all night.
//!
//! The rule here is pure: [`Sync`] says when the desktop must be told again. [`follow`] is the
//! only caller that acts on it, in the main `wol-shell` alone (a window process must not run
//! it, or each of them would restyle the desktop). The restyle is `wol theme apply`, the same
//! path the session start takes, so the strip, the toolkit files, gsettings, foot and the `theme`
//! event all move together and a compositor reload reads the file `apply` just wrote.
//!
//! Invariant (tested for every hour of two days, a suspend and a time-zone jump): the variant
//! the desktop was last told is the one Auto shows now, at most [`TICK_SECS`] seconds after the
//! switch. A theme chosen by the person (not Auto) never moves with the clock, so nothing runs.
use wol_core::theme::accent;

/// How often the clock is compared. A timer set for the exact switch second would be late after a
/// suspend (the main loop's seconds do not count while asleep) and wrong after a time-zone change;
/// comparing two booleans every half minute is neither.
pub const TICK_SECS: u32 = 30;

/// What the shell runs to restyle the desktop (the argv of the session start's `once`).
pub const APPLY: [&str; 3] = ["wol", "theme", "apply"];

/// The light/dark state the compositor and the other toolkits were last told, while Auto.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sync {
    /// `Some(dark)`: told (or started with) `dark`. None: the theme is the person's own, or the
    /// session has not looked yet.
    told: Option<bool>,
}

impl Sync {
    /// A session that has just applied the theme (`wol theme apply` at its start) at `hour`.
    pub fn started(saved: Option<&str>, hour: u8) -> Self {
        Sync {
            told: accent::is_auto(saved).then(|| accent::auto_is_dark(hour)),
        }
    }

    /// Whether the desktop must be restyled now: `saved` is the active theme id (None: the
    /// default, which is Auto), `hour` the local hour. True once per switch.
    pub fn due(&mut self, saved: Option<&str>, hour: u8) -> bool {
        if !accent::is_auto(saved) {
            // The person's own theme: Settings restyled the desktop when it was chosen.
            self.told = None;
            return false;
        }
        let dark = accent::auto_is_dark(hour);
        match self.told.replace(dark) {
            // Just switched to Auto: the change that did it (Settings, `wol theme set`) restyled.
            None => false,
            Some(before) => before != dark,
        }
    }
}

/// Starts the follower: every [`TICK_SECS`] it compares the clock with what the desktop was told
/// and runs `wol theme apply` off the GTK thread when Auto crossed a switch. Call once, from the
/// main shell, after the session's own `apply` ran (the first tick only reads).
pub fn follow() {
    use gtk::glib;
    let hour = || crate::kit::now().hour;
    let mut sync = Sync::started(crate::theme::saved().as_deref(), hour());
    glib::timeout_add_seconds_local(TICK_SECS, move || {
        if sync.due(crate::theme::saved().as_deref(), hour()) {
            run_apply();
        }
        glib::ControlFlow::Continue
    });
}

/// `wol theme apply` on its own thread; a failure leaves the strip as it was (apply says why on
/// its standard error, which the session log keeps).
fn run_apply() {
    let mut cmd = std::process::Command::new(APPLY[0]);
    cmd.args(&APPLY[1..])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit());
    let _ = std::thread::Builder::new()
        .name("wol-appearance".into())
        .spawn(move || cmd.status());
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTO: Option<&str> = Some("wol-auto-violet");
    const LIGHT: Option<&str> = Some("wol-light");

    /// The desktop as two things that can disagree: what the shell's own windows show (they follow
    /// the clock) and what the strip and the other toolkits were told.
    struct Desktop {
        sync: Sync,
        strip_dark: bool,
        applies: u32,
    }

    impl Desktop {
        fn login_at(hour: u8) -> Self {
            Desktop {
                sync: Sync::started(AUTO, hour),
                strip_dark: accent::auto_is_dark(hour),
                applies: 0,
            }
        }

        /// One tick; the apply finishes before the next (it takes a second or two).
        fn tick(&mut self, saved: Option<&str>, hour: u8) {
            if self.sync.due(saved, hour) {
                self.strip_dark = accent::auto_is_dark(hour);
                self.applies += 1;
            }
        }
    }

    /// Ticks (half minutes) in 48 hours from a login at `start` after which the strip showed the
    /// other variant than the body. `follow` false is the code before this module: nothing re-ran
    /// `wol theme apply` after the session start.
    fn lagging_ticks(start: u8, follow: bool) -> (u32, u32) {
        let mut d = Desktop::login_at(start);
        let mut lagging = 0;
        for step in 0..(48 * 3600 / TICK_SECS) {
            let secs = u64::from(start) * 3600 + u64::from(step) * u64::from(TICK_SECS);
            let hour = ((secs / 3600) % 24) as u8;
            if follow {
                d.tick(AUTO, hour);
            }
            if d.strip_dark != accent::auto_is_dark(hour) {
                lagging += 1;
            }
        }
        (lagging, d.applies)
    }

    /// The lead's screenshot: logged in by day, the strip stays white after 19:00 while the
    /// shell's windows turn dark. Sweep 48 hours with a tick every half minute: after each tick
    /// the strip has the variant the body shows (between ticks it can lag by at most `TICK_SECS`).
    #[test]
    fn after_every_tick_the_strip_has_the_variant_the_body_shows() {
        for start in [0u8, 6, 7, 12, 18, 19, 23] {
            let (lagging, applies) = lagging_ticks(start, true);
            assert_eq!(lagging, 0, "login at {start}: the strip lagged the body");
            // Two days cross four switches; each one is applied once.
            assert!(
                (3..=4).contains(&applies),
                "login at {start}: {applies} applies"
            );
        }
    }

    /// The same sweep without the follower fails: hours of a white strip over a dark body (or a
    /// dark one over a light body), which is what the App Store showed.
    #[test]
    fn without_the_follower_the_strip_lags_for_hours() {
        for start in [0u8, 6, 7, 12, 18, 19, 23] {
            let (lagging, applies) = lagging_ticks(start, false);
            assert!(
                lagging >= 2 * 3600 / TICK_SECS,
                "login at {start}: {lagging} ticks"
            );
            assert_eq!(applies, 0);
        }
    }

    /// A laptop closed at 18:00 and opened at 22:00: the main loop's seconds did not run, the
    /// first tick after the wake sees the new hour.
    #[test]
    fn a_suspend_across_the_switch_is_caught_by_the_next_tick() {
        let mut d = Desktop::login_at(17);
        d.tick(AUTO, 18);
        assert_eq!(d.applies, 0);
        d.tick(AUTO, 22);
        assert_eq!(d.applies, 1);
        assert!(d.strip_dark);
    }

    /// Flying east: the local hour jumps from 15 to 22 at once.
    #[test]
    fn a_time_zone_jump_restyles_once() {
        let mut d = Desktop::login_at(15);
        d.tick(AUTO, 22);
        d.tick(AUTO, 22);
        assert_eq!(d.applies, 1);
        // And back west.
        d.tick(AUTO, 9);
        assert_eq!(d.applies, 2);
        assert!(!d.strip_dark);
    }

    #[test]
    fn a_theme_the_person_chose_never_follows_the_clock() {
        let mut d = Desktop::login_at(10);
        for hour in 0..48u8 {
            d.tick(LIGHT, hour % 24);
        }
        assert_eq!(d.applies, 0);
    }

    /// Light chosen by hand at noon, Auto chosen at night: Settings restyled when it was chosen,
    /// so the follower does not run a second apply for the same change.
    #[test]
    fn choosing_auto_is_not_applied_twice() {
        let mut d = Desktop::login_at(10);
        d.tick(LIGHT, 21);
        d.tick(AUTO, 21);
        d.tick(AUTO, 21);
        assert_eq!(d.applies, 0);
        d.tick(AUTO, 8);
        assert_eq!(d.applies, 1);
    }

    #[test]
    fn no_theme_file_is_auto() {
        let mut s = Sync::started(None, 10);
        assert!(!s.due(None, 12));
        assert!(s.due(None, 20));
        assert!(!s.due(None, 21));
    }

    #[test]
    fn the_session_start_variant_counts_as_told() {
        // Login at 19:00:05: the session's own apply already made the strip dark.
        let mut s = Sync::started(AUTO, 19);
        assert!(!s.due(AUTO, 19));
        assert!(!s.due(AUTO, 20));
    }

    #[test]
    fn it_runs_the_session_starts_command() {
        assert_eq!(APPLY.join(" "), "wol theme apply");
        let lua = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../desktop/overlay/usr/share/wolos/hypr/hyprland.lua"
        ));
        if let Ok(lua) = lua {
            assert!(lua.contains("once([[wol theme apply]])"));
        }
    }
}
