// SPDX-License-Identifier: GPL-2.0-or-later
//! Motion for every shell surface: reduce motion, the named curves, the durations, the springs
//! and the stagger. Pure arithmetic; the frame clock and the [`super::anim::Animated`] handle live
//! in `kit::anim`.
//!
//! Reduce motion is one switch for every animation in every shell process. GTK's
//! `gtk-enable-animations` turns off CSS animations and transitions (revealers, stacks, the Dock
//! bounce); WolOS's own setting (Accessibility > Reduce motion, desktop prefs) drives it, and
//! hand-made animations ask [`reduced`] (`kit::anim` snaps to the target when it is true).
//!
//! Curves and durations are the theme contract that libWolkit apps use too (`Ease` in
//! `crates/wolkit/src/style/value.rs`, `DURATIONS_NS` in `crates/wolkit/src/anim/mod.rs`), so a
//! shell panel and a Wolkit app move alike. Docs: `docs/modules/hig.md` section 7.

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

/// A named easing curve: progress 0 to 1 over a time fraction 0 to 1. The first five
/// ([`Curve::CONTRACT`]) are libWolkit's `Ease` (`transition::bezier`), names and control points
/// verbatim. The last three name curves already in use outside the contract (kit.css
/// `--glass-spring`, the compositor's `spring` and `ease` beziers in `hyprland.conf`), so the
/// compositor and the shell can map their animations onto one table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Curve {
    /// `ease`, the theme contract's `--ease` (kit.css `--ease-out`): fast start, soft landing.
    /// Things that enter or move.
    Ease,
    /// `linear`: progress bars, colour cross-fades driven by a value, never for movement.
    Linear,
    /// `in` (kit.css `--ease-in`): things that leave.
    In,
    /// `out`: a gentler entrance for large surfaces (sheets, the wallpaper cross-fade).
    Out,
    /// `in-out`: a surface that moves from one place to another and stays on screen.
    InOut,
    /// `glass` (kit.css `--glass-spring`): a strong overshoot (peaks near 1.1) for a glass
    /// panel landing. Not monotonic.
    Glass,
    /// `overshoot` (the compositor's `spring` bezier, window moves): a light overshoot.
    /// Not monotonic.
    Overshoot,
    /// `soft` (the compositor's `ease` bezier, the classic CSS `ease`): fades, layers and
    /// workspace slides in the compositor.
    Soft,
}

impl Curve {
    /// The theme contract (libWolkit `Ease`): all monotonic, none overshoots.
    pub const CONTRACT: [Curve; 5] = [
        Curve::Ease,
        Curve::Linear,
        Curve::In,
        Curve::Out,
        Curve::InOut,
    ];

    /// Every named curve: the contract, then `glass`, `overshoot`, `soft`.
    pub const ALL: [Curve; 8] = [
        Curve::Ease,
        Curve::Linear,
        Curve::In,
        Curve::Out,
        Curve::InOut,
        Curve::Glass,
        Curve::Overshoot,
        Curve::Soft,
    ];

    /// The name (`ease`, `linear`, `in`, `out`, `in-out`, `glass`, `overshoot`, `soft`).
    pub fn name(self) -> &'static str {
        match self {
            Curve::Ease => "ease",
            Curve::Linear => "linear",
            Curve::In => "in",
            Curve::Out => "out",
            Curve::InOut => "in-out",
            Curve::Glass => "glass",
            Curve::Overshoot => "overshoot",
            Curve::Soft => "soft",
        }
    }

    pub fn from_name(name: &str) -> Option<Curve> {
        Curve::ALL.into_iter().find(|c| c.name() == name)
    }

    /// `cubic-bezier(x1, y1, x2, y2)` control points; None for `linear`.
    pub fn bezier(self) -> Option<[f32; 4]> {
        match self {
            Curve::Ease => Some([0.2, 0.8, 0.2, 1.0]),
            Curve::Linear => None,
            Curve::In => Some([0.4, 0.0, 1.0, 1.0]),
            Curve::Out => Some([0.0, 0.0, 0.2, 1.0]),
            Curve::InOut => Some([0.4, 0.0, 0.2, 1.0]),
            Curve::Glass => Some([0.34, 1.56, 0.64, 1.0]),
            Curve::Overshoot => Some([0.22, 1.2, 0.36, 1.0]),
            Curve::Soft => Some([0.25, 0.1, 0.25, 1.0]),
        }
    }

    /// The curve as a CSS timing function (`linear` or `cubic-bezier(...)`).
    pub fn css(self) -> String {
        match self.bezier() {
            None => "linear".into(),
            Some([a, b, c, d]) => format!("cubic-bezier({a}, {b}, {c}, {d})"),
        }
    }

    /// Progress at time fraction `t` (clamped to 0..1). Same solver as libWolkit's `ease`.
    pub fn at(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        let Some([x1, y1, x2, y2]) = self.bezier() else {
            return t;
        };
        let mut s = t;
        for _ in 0..8 {
            let err = bez(x1, x2, s) - t;
            if err.abs() < 1e-6 {
                return bez(y1, y2, s);
            }
            let d = bez_d(x1, x2, s);
            if d.abs() < 1e-6 {
                break;
            }
            s = (s - err / d).clamp(0.0, 1.0);
        }
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        s = t;
        for _ in 0..40 {
            let x = bez(x1, x2, s);
            if (x - t).abs() < 1e-6 {
                break;
            }
            if x < t {
                lo = s;
            } else {
                hi = s;
            }
            s = (lo + hi) / 2.0;
        }
        bez(y1, y2, s)
    }
}

/// One coordinate of a unit cubic Bezier with control values `a` and `b`, at parameter `t`.
fn bez(a: f32, b: f32, t: f32) -> f32 {
    let u = 1.0 - t;
    3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
}

fn bez_d(a: f32, b: f32, t: f32) -> f32 {
    let u = 1.0 - t;
    3.0 * u * u * a + 6.0 * u * t * (b - a) + 3.0 * t * t * (1.0 - b)
}

/// The three durations of the theme contract (`--motion-fast`, `-normal`, `-slow`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Duration {
    /// 150 ms: hover, press, small toggles, banners leaving.
    Fast,
    /// 250 ms: panels, popovers, banners entering, list rows.
    Normal,
    /// 400 ms: large surfaces (Overview, the wallpaper cross-fade, sheets).
    Slow,
}

/// [`Duration`] in milliseconds; libWolkit's `DURATIONS_NS` / 1e6.
pub const DURATIONS_MS: [u64; 3] = [150, 250, 400];

impl Duration {
    pub const ALL: [Duration; 3] = [Duration::Fast, Duration::Normal, Duration::Slow];

    pub fn ms(self) -> u64 {
        match self {
            Duration::Fast => DURATIONS_MS[0],
            Duration::Normal => DURATIONS_MS[1],
            Duration::Slow => DURATIONS_MS[2],
        }
    }

    /// The kit.css custom property that holds it.
    pub fn var(self) -> &'static str {
        match self {
            Duration::Fast => "--motion-fast",
            Duration::Normal => "--motion-normal",
            Duration::Slow => "--motion-slow",
        }
    }
}

/// Position and velocity of a spring, in units of the animated value (and per second).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpringState {
    pub pos: f32,
    pub vel: f32,
}

/// A damped spring material: stiffness and damping per second, mass. Springs are for drops and
/// drags (and the Dock magnify): things the hand moved and lets go.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub stiffness: f32,
    pub damping: f32,
    pub mass: f32,
}

impl Spring {
    /// libWolkit `spring: gentle`: no visible overshoot (damping ratio ~1). Settles (2%) in
    /// 450 ms ([`Spring::settle_ms`]).
    pub const GENTLE: Spring = Spring {
        stiffness: 170.0,
        damping: 26.0,
        mass: 1.0,
    };
    /// libWolkit `spring: snappy`: a short settle with a hint of overshoot. 230 ms.
    pub const SNAPPY: Spring = Spring {
        stiffness: 320.0,
        damping: 30.0,
        mass: 1.0,
    };
    /// libWolkit `spring: bouncy`: visible bounce; for playful drops only. 520 ms.
    pub const BOUNCY: Spring = Spring {
        stiffness: 260.0,
        damping: 14.0,
        mass: 1.0,
    };

    /// The presets by libWolkit name (`gentle`, `snappy`, `bouncy`).
    pub fn named(name: &str) -> Option<Spring> {
        match name {
            "gentle" => Some(Spring::GENTLE),
            "snappy" => Some(Spring::SNAPPY),
            "bouncy" => Some(Spring::BOUNCY),
            _ => None,
        }
    }

    /// Damping ratio: below 1 it overshoots, at 1 and above it does not.
    pub fn damping_ratio(self) -> f32 {
        self.damping / (2.0 * (self.stiffness * self.mass).sqrt())
    }

    /// Advance `s` toward `target` by `dt` seconds: sub-stepped at 240 Hz with semi-implicit
    /// Euler (stable for every preset), `dt` clamped to 0..0.25 s so a stalled frame never
    /// explodes. The same integrator as libWolkit's `spring_step`, without its rest snap (the
    /// caller decides what "at rest" means for its unit, see [`Spring::at_rest`]).
    pub fn step(self, s: SpringState, target: f32, dt: f32) -> SpringState {
        let mut s = s;
        let h_max = 1.0 / 240.0;
        let mut left = dt.clamp(0.0, 0.25);
        while left > 0.0 {
            let h = left.min(h_max);
            let force = -self.stiffness * (s.pos - target) - self.damping * s.vel;
            s.vel += force / self.mass * h;
            s.pos += s.vel * h;
            left -= h;
        }
        s
    }

    /// At rest for a move of `span` units: within 0.1% of the span and slower than 1% of it per
    /// second (spans below 1 count as 1, the libWolkit thresholds for a 0-to-1 value).
    pub fn at_rest(s: SpringState, target: f32, span: f32) -> bool {
        let k = span.abs().max(1.0);
        (s.pos - target).abs() < 0.001 * k && s.vel.abs() < 0.01 * k
    }

    /// Milliseconds after which a 0-to-1 move from rest stays within 2% of the target (the
    /// documented settle time; the motion tests hold the presets to the doc).
    pub fn settle_ms(self) -> u64 {
        let (mut s, dt) = (SpringState { pos: 0.0, vel: 0.0 }, 0.001);
        let mut last_out = 0u64;
        for ms in 1..=5_000u64 {
            s = self.step(s, 1.0, dt);
            if (s.pos - 1.0).abs() > 0.02 {
                last_out = ms;
            }
        }
        last_out
    }
}

/// Items of a list entering one after another: item `i` starts `delay_ms(i)` after the first.
/// The whole spread is capped at [`Stagger::MAX_SPREAD_MS`] (a long list shrinks the gap, the
/// order stays), so the last row never waits long. Under reduce motion `kit::anim` snaps and
/// the delays do not matter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stagger {
    n: usize,
    gap_ms: u64,
}

impl Stagger {
    /// Longest wait for the last item.
    pub const MAX_SPREAD_MS: u64 = 300;
    /// The default gap between banners, Center rows and Control Center tiles.
    pub const GAP_MS: u64 = 30;

    pub fn new(n: usize, gap_ms: u64) -> Stagger {
        let steps = n.saturating_sub(1) as u64;
        let gap_ms = Self::MAX_SPREAD_MS
            .checked_div(steps)
            .map_or(gap_ms, |cap| gap_ms.min(cap));
        Stagger { n, gap_ms }
    }

    pub fn len(self) -> usize {
        self.n
    }

    pub fn is_empty(self) -> bool {
        self.n == 0
    }

    /// The gap after the cap.
    pub fn gap_ms(self) -> u64 {
        self.gap_ms
    }

    /// Delay of item `i` entering (first item first). Indexes past the end wait like the last.
    pub fn delay_ms(self, i: usize) -> u64 {
        i.min(self.n.saturating_sub(1)) as u64 * self.gap_ms
    }

    /// Delay of item `i` leaving (last item first).
    pub fn delay_rev_ms(self, i: usize) -> u64 {
        let last = self.n.saturating_sub(1);
        (last - i.min(last)) as u64 * self.gap_ms
    }

    /// Time from the first start to the last item done, for one item lasting `item_ms`.
    pub fn total_ms(self, item_ms: u64) -> u64 {
        if self.n == 0 {
            0
        } else {
            self.delay_ms(self.n - 1) + item_ms
        }
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

        /// The same constants as a [`super::Spring`] for `kit::anim` (mass 1).
        pub fn spring(self) -> super::Spring {
            super::Spring {
                stiffness: self.stiffness as f32,
                damping: self.damping as f32,
                mass: 1.0,
            }
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
