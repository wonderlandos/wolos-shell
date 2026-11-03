// SPDX-License-Identifier: GPL-2.0-or-later
//! Which shell surfaces go on which display, pure side. Every display gets the wallpaper and a
//! menu bar (dimmed while another display has the focus); the main display (the
//! first one the compositor lists) also gets the Dock and the desktop widgets. `ui::displays` builds
//! and drops surfaces as displays come and go. Settings > Displays builds its `monitor =`
//! rules here too (refresh rate, scale, arrangement next to the main display).

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

// ---- Settings > Displays: resolution, refresh rate, scale, arrangement (ADR 0031 W5) ----

/// Where a display sits next to the main one. The compositor places monitors in logical pixels
/// (mode size divided by scale), top-left at "XxY".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// The compositor picks (`auto`: to the right, in the order it found them).
    Auto,
    Right,
    Left,
    Above,
    Below,
    /// Same picture as the main display.
    Mirror,
}

impl Place {
    pub const ALL: [Place; 6] = [
        Place::Auto,
        Place::Right,
        Place::Left,
        Place::Above,
        Place::Below,
        Place::Mirror,
    ];
    /// Label in the Arrangement menu.
    pub fn label(self) -> &'static str {
        match self {
            Place::Auto => "Automatic",
            Place::Right => "Right of Main Display",
            Place::Left => "Left of Main Display",
            Place::Above => "Above Main Display",
            Place::Below => "Below Main Display",
            Place::Mirror => "Mirror Main Display",
        }
    }
}

/// A display's mode and scale, enough to place it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}

impl Geometry {
    /// Size in logical pixels (what the compositor positions with). A scale outside 0.5..=3 counts
    /// as 1 (Settings never writes one; a hand-edited rule might).
    pub fn logical(self) -> (i64, i64) {
        let s = if (0.5..=3.0).contains(&self.scale) {
            self.scale
        } else {
            1.0
        };
        let l = |px: u32| (f64::from(px) / s).round() as i64;
        (l(self.width), l(self.height))
    }
}

/// Top-left of `this` for `place` next to `main` at 0x0; None for [`Place::Auto`] and
/// [`Place::Mirror`] (no position in the rule).
pub fn position(place: Place, main: Geometry, this: Geometry) -> Option<(i64, i64)> {
    let (mw, mh) = main.logical();
    let (tw, th) = this.logical();
    Some(match place {
        Place::Right => (mw, 0),
        Place::Left => (-tw, 0),
        Place::Above => (0, -th),
        Place::Below => (0, mh),
        Place::Auto | Place::Mirror => return None,
    })
}

/// Refresh rates the display offers at `w`×`h`, highest first, each once (the compositor lists
/// 59.95 and 60.00 as two modes; both are kept, they are different modes).
pub fn refresh_rates(modes: &[String], w: u32, h: u32) -> Vec<f64> {
    let mut v: Vec<f64> = modes
        .iter()
        .filter_map(|m| wol_core::controls::parse_mode(m))
        .filter(|(mw, mh, _)| *mw == w && *mh == h)
        .map(|(_, _, r)| r)
        .collect();
    v.sort_by(|a, b| b.total_cmp(a));
    v.dedup_by(|a, b| (*a - *b).abs() < 0.005);
    v
}

/// Refresh rate label: "60 Hz", "59.94 Hz", "143.98 Hz".
pub fn hz_label(r: f64) -> String {
    let s = format!("{r:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{s} Hz")
}

/// The compositor `monitor =` rule for one display: mode, refresh, position and scale, or a
/// mirror of `main`. None when a value does not check (`wol_core::controls::monitor_rule`) or
/// `main` is not a connector name.
pub fn rule(
    name: &str,
    mode: (u32, u32, f64),
    scale: f64,
    place: Place,
    main: Option<(&str, Geometry)>,
) -> Option<String> {
    let (w, h, hz) = mode;
    let base = wol_core::controls::monitor_rule(name, w, h, hz, scale)?;
    let this = Geometry {
        width: w,
        height: h,
        scale,
    };
    let Some((main_name, main_geo)) = main.filter(|(m, _)| *m != name) else {
        return Some(base);
    };
    if place == Place::Mirror {
        let ok = !main_name.is_empty()
            && main_name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        return ok.then(|| format!("{base},mirror,{main_name}"));
    }
    match position(place, main_geo, this) {
        Some((x, y)) => Some(base.replacen(",auto,", &format!(",{x}x{y},"), 1)),
        None => Some(base),
    }
}

/// The place a rule already says (from displays.conf), so the menu opens on it.
pub fn place_of(rule: &str, main: Geometry, this: Geometry) -> Place {
    let parts: Vec<&str> = rule.split(',').map(str::trim).collect();
    if parts.get(4) == Some(&"mirror") {
        return Place::Mirror;
    }
    let Some((x, y)) = parts
        .get(2)
        .and_then(|p| p.split_once('x'))
        .and_then(|(x, y)| Some((x.parse::<i64>().ok()?, y.parse::<i64>().ok()?)))
    else {
        return Place::Auto;
    };
    Place::ALL
        .into_iter()
        .find(|p| position(*p, main, this) == Some((x, y)))
        .unwrap_or(Place::Auto)
}

#[cfg(test)]
mod settings_tests {
    use super::*;

    const LAPTOP: Geometry = Geometry {
        width: 2560,
        height: 1600,
        scale: 2.0,
    };
    const TV: Geometry = Geometry {
        width: 1920,
        height: 1080,
        scale: 1.0,
    };

    #[test]
    fn arrangement_positions_in_logical_pixels() {
        assert_eq!(LAPTOP.logical(), (1280, 800));
        assert_eq!(position(Place::Right, LAPTOP, TV), Some((1280, 0)));
        assert_eq!(position(Place::Left, LAPTOP, TV), Some((-1920, 0)));
        assert_eq!(position(Place::Above, LAPTOP, TV), Some((0, -1080)));
        assert_eq!(position(Place::Below, LAPTOP, TV), Some((0, 800)));
        assert_eq!(position(Place::Auto, LAPTOP, TV), None);
        let odd = Geometry { scale: 0.0, ..TV };
        assert_eq!(odd.logical(), (1920, 1080), "no division by zero");
    }

    #[test]
    fn rules_carry_mode_refresh_position_and_scale() {
        let main = Some(("eDP-1", LAPTOP));
        assert_eq!(
            rule("HDMI-A-1", (1920, 1080, 144.0), 1.0, Place::Right, main).as_deref(),
            Some("HDMI-A-1,1920x1080@144.00,1280x0,1")
        );
        assert_eq!(
            rule("HDMI-A-1", (1920, 1080, 60.0), 1.0, Place::Left, main).as_deref(),
            Some("HDMI-A-1,1920x1080@60.00,-1920x0,1")
        );
        assert_eq!(
            rule("HDMI-A-1", (1920, 1080, 60.0), 1.0, Place::Mirror, main).as_deref(),
            Some("HDMI-A-1,1920x1080@60.00,auto,1,mirror,eDP-1")
        );
        // The main display itself, and one display alone, keep auto.
        assert_eq!(
            rule("eDP-1", (2560, 1600, 60.0), 2.0, Place::Right, main).as_deref(),
            Some("eDP-1,2560x1600@60.00,auto,2")
        );
        assert_eq!(rule("x;y", (1, 1, 60.0), 1.0, Place::Auto, None), None);
        assert_eq!(
            rule(
                "HDMI-A-1",
                (1, 1, 60.0),
                1.0,
                Place::Mirror,
                Some(("a,b", LAPTOP))
            ),
            None
        );
    }

    #[test]
    fn the_menu_opens_on_the_saved_place() {
        for p in [
            Place::Right,
            Place::Left,
            Place::Above,
            Place::Below,
            Place::Mirror,
        ] {
            let r = rule(
                "HDMI-A-1",
                (1920, 1080, 60.0),
                1.0,
                p,
                Some(("eDP-1", LAPTOP)),
            )
            .unwrap();
            assert_eq!(place_of(&r, LAPTOP, TV), p, "{r}");
        }
        assert_eq!(
            place_of("HDMI-A-1,1920x1080@60,auto,1", LAPTOP, TV),
            Place::Auto
        );
        assert_eq!(
            place_of("HDMI-A-1,1920x1080@60,7x7,1", LAPTOP, TV),
            Place::Auto
        );
        assert_eq!(place_of("garbage", LAPTOP, TV), Place::Auto);
    }

    #[test]
    fn refresh_rates_per_resolution() {
        let modes: Vec<String> = [
            "1920x1080@60.00Hz",
            "1920x1080@143.98Hz",
            "1920x1080@59.94Hz",
            "1920x1080@60.00Hz",
            "1280x720@60.00Hz",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(refresh_rates(&modes, 1920, 1080), [143.98, 60.0, 59.94]);
        assert_eq!(refresh_rates(&modes, 1280, 720), [60.0]);
        assert!(refresh_rates(&modes, 800, 600).is_empty());
        assert_eq!(hz_label(60.0), "60 Hz");
        assert_eq!(hz_label(59.94), "59.94 Hz");
        assert_eq!(hz_label(143.98), "143.98 Hz");
        assert_eq!(hz_label(74.5), "74.5 Hz");
    }
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
