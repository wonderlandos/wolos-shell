// SPDX-License-Identifier: GPL-2.0-or-later
//! Which shell surfaces go on which display, pure side. Every display gets the wallpaper and a
//! menu bar (dimmed while another display has the focus); the main display ([`main_of`]: the
//! built-in panel, else the first one the compositor lists) also gets the Dock and the desktop
//! widgets. `ui::displays` builds
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

/// Anything that names a display by its connector: a bare connector or a compositor monitor.
pub trait Connected {
    fn connector(&self) -> &str;
}

impl Connected for String {
    fn connector(&self) -> &str {
        self
    }
}

impl Connected for wol_core::controls::Monitor {
    fn connector(&self) -> &str {
        &self.name
    }
}

/// A laptop's own panel: `eDP`, `LVDS`, `DSI`.
pub fn is_builtin(connector: &str) -> bool {
    ["eDP", "LVDS", "DSI"]
        .iter()
        .any(|p| connector.starts_with(p))
}

/// The main display, the one place that decides it (QA-GFX-6): the built-in panel, else the first
/// listed. The Dock, the desktop, Settings > Displays and the Control Center's mirror list all ask
/// here, so they never name two different displays. None without a display.
pub fn main_of<T: Connected>(displays: &[T]) -> Option<&T> {
    displays
        .iter()
        .find(|d| is_builtin(d.connector()))
        .or(displays.first())
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
/// mirror of `main` (the main display itself gets the explicit position 0x0). None when a value does not check (`wol_core::controls::monitor_rule`) or
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
    // The main display is the origin every Arrangement is measured from. A rule for it that says
    // `auto` lets the compositor move it once another display has a position (QA-GFX-1: Below
    // put the main display at 1280x0, the two screens diagonal), so it always says 0x0.
    if main.is_some_and(|(m, _)| m == name) {
        return Some(base.replacen(",auto,", ",0x0,", 1));
    }
    let Some((main_name, main_geo)) = main else {
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

/// The rule that pins the main display to 0x0, when the saved rules (`prefs::monitor_rules`) do
/// not say so yet: it is missing (`fallback`, built from the display as it runs) or says `auto`.
/// None when it is already pinned (or hand-placed: that is left alone). Applied and saved before
/// any other display gets a position (QA-GFX-1).
pub fn anchor_rule(saved: &[String], main: &str, fallback: Option<&str>) -> Option<String> {
    // The last rule for a display is the one in force.
    let mine = saved
        .iter()
        .rev()
        .find(|r| r.split(',').next().map(str::trim) == Some(main));
    match mine {
        None => fallback.map(str::to_string),
        Some(r) => {
            let mut parts: Vec<&str> = r.split(',').map(str::trim).collect();
            if parts.get(2) == Some(&"auto") {
                parts[2] = "0x0";
                Some(parts.join(","))
            } else {
                None
            }
        }
    }
}

// ---- Scale ----

/// The scales the menu offers.
pub const SCALES: [(&str, f64); 5] = [
    ("100%", 1.0),
    ("125%", 1.25),
    ("150%", 1.5),
    ("175%", 1.75),
    ("200%", 2.0),
];

/// The Scale menu of a display running at `current`: the five scales, plus the one it runs at when
/// that is not among them (the compositor rounds 150% on a 1280-wide panel to 160%; a hand-edited
/// rule says 1.333), so changing only the refresh rate keeps the scale instead of writing 1.0
/// (QA-SET-18).
pub fn scale_choices(current: f64) -> Vec<(String, f64)> {
    let mut v: Vec<(String, f64)> = SCALES.iter().map(|s| (s.0.to_string(), s.1)).collect();
    if current.is_finite() && !v.iter().any(|s| (s.1 - current).abs() < 0.01) {
        let at = v.iter().position(|s| s.1 > current).unwrap_or(v.len());
        v.insert(
            at,
            (format!("{}% (current)", (current * 100.0).round()), current),
        );
    }
    v
}

/// The entry of [`scale_choices`] a display running at `current` is on.
pub fn scale_index(choices: &[(String, f64)], current: f64) -> usize {
    choices
        .iter()
        .position(|s| (s.1 - current).abs() < 0.01)
        .unwrap_or(0)
}

// ---- Keep or revert ----

/// Seconds a changed mode waits for "Keep" before the display goes back (QA-LIVE-13).
pub const REVERT_SECS: u32 = 15;

std::thread_local! {
    static TICK_MS: std::cell::Cell<u64> = const { std::cell::Cell::new(1000) };
}

/// The length of one countdown second in milliseconds (1000). Tests shorten it, so a countdown
/// of [`REVERT_SECS`] ticks runs in a blink; the count itself is untouched.
pub fn tick_millis() -> u64 {
    TICK_MS.with(std::cell::Cell::get)
}

/// Sets [`tick_millis`] for this thread (tests).
pub fn set_tick_millis(ms: u64) {
    TICK_MS.with(|t| t.set(ms.max(1)));
}

/// What puts one display back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Undo {
    pub name: String,
    /// The `monitor =` rule it ran with before the first change of this countdown.
    pub rule: String,
}

/// One second of [`Confirm`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tick {
    /// Seconds still left to keep the change.
    Left(u32),
    /// Nobody kept it: put the displays back.
    Revert,
}

/// A changed resolution, refresh rate or scale that is not kept yet. A wrong mode can leave nothing
/// to see, so the answer is a countdown nobody has to win: no answer reverts.
///
/// Nothing is written to displays.lua while it runs: a shell that dies, or a session that ends,
/// before Keep leaves the file as it was, so the next login does not bring the bad mode back.
/// Every change made meanwhile (a second mode, an arrangement) waits in [`Self::saves`] and is
/// written, in one journal entry, when the person keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Confirm {
    left: u32,
    undo: Vec<Undo>,
    saves: Vec<String>,
}

impl Confirm {
    /// A countdown of [`REVERT_SECS`] for `first`; `saves` are the rules to write on Keep.
    pub fn arm(first: Undo, saves: Vec<String>) -> Self {
        Confirm {
            left: REVERT_SECS,
            undo: vec![first],
            saves,
        }
    }
    /// Another mode change while waiting: the countdown starts over.
    pub fn change(&mut self, next: Undo, saves: Vec<String>) {
        self.left = REVERT_SECS;
        self.undo.push(next);
        self.saves.extend(saves);
    }
    /// Another display's undo while waiting (an arrangement): the countdown goes on, since
    /// nothing about the picture's readability changed.
    pub fn hold(&mut self, next: Undo, saves: Vec<String>) {
        self.undo.push(next);
        self.saves.extend(saves);
    }
    /// One second passed.
    pub fn tick(&mut self) -> Tick {
        self.left = self.left.saturating_sub(1);
        if self.left == 0 {
            Tick::Revert
        } else {
            Tick::Left(self.left)
        }
    }
    pub fn left(&self) -> u32 {
        self.left
    }
    /// What puts the displays back: each display once, with the rule it ran with before the
    /// first change (the last state somebody saw working).
    pub fn rules(&self) -> Vec<&Undo> {
        let mut seen: Vec<&Undo> = Vec::new();
        for u in &self.undo {
            if !seen.iter().any(|x| x.name == u.name) {
                seen.push(u);
            }
        }
        seen
    }
    /// The rules to write to displays.lua when the change is kept, oldest first (a later rule for
    /// the same display replaces an earlier one there).
    pub fn saves(&self) -> &[String] {
        &self.saves
    }
}

/// `rule` (the main display's) with the position the saved rules give that display, when they
/// give one that is not `auto`: somebody placed it by hand, and a mode change keeps that.
pub fn keep_position(rule: String, saved: &[String]) -> String {
    let mut parts: Vec<&str> = rule.split(',').collect();
    let name = parts.first().copied().unwrap_or("");
    let at = saved
        .iter()
        .filter(|r| r.split(',').next().map(str::trim) == Some(name))
        .find_map(|r| r.split(',').nth(2).map(str::trim))
        .filter(|p| !p.is_empty() && *p != "auto");
    match (at, parts.len() > 3) {
        (Some(p), true) => {
            parts[2] = p;
            parts.join(",")
        }
        _ => rule,
    }
}

// ---- The displays the page shows ----

/// Whether the page has to be built again: a display came or went, or one runs another mode or
/// scale than the page shows (QA-GFX-5).
pub fn monitors_changed(
    shown: &[wol_core::controls::Monitor],
    now: &[wol_core::controls::Monitor],
) -> bool {
    shown.len() != now.len()
        || shown.iter().zip(now).any(|(a, b)| {
            a.name != b.name
                || (a.width, a.height) != (b.width, b.height)
                || (a.refresh - b.refresh).abs() > 0.01
                || (a.scale - b.scale).abs() > 0.01
                || a.modes != b.modes
        })
}

/// The snapshot of the displays (it is read every 10 to 60 s) without the ones the desktop no
/// longer shows (`live`: the connectors it lists now). All of them stay when the desktop lists
/// none of them (previews, a session that cannot be asked): no list is then better than the
/// snapshot (QA-GFX-5).
pub fn still_connected(
    snapshot: Vec<wol_core::controls::Monitor>,
    live: &[String],
) -> Vec<wol_core::controls::Monitor> {
    if !snapshot.iter().any(|m| live.contains(&m.name)) {
        return snapshot;
    }
    snapshot
        .into_iter()
        .filter(|m| live.contains(&m.name))
        .collect()
}

/// The size a `monitor =` rule (`NAME,WxH@R,POS,SCALE`) gives its display; None for a rule
/// without a mode (`preferred`, a mirror of another output's size).
pub fn rule_geometry(rule: &str) -> Option<Geometry> {
    let parts: Vec<&str> = rule.split(',').map(str::trim).collect();
    let (width, height, _) = wol_core::controls::parse_mode(parts.get(1)?)?;
    let scale = parts
        .get(3)
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(1.0);
    Some(Geometry {
        width,
        height,
        scale,
    })
}

/// The main display `main` went from `old` to `new` (mode or scale): the rules that move every
/// display placed next to it (Right, Below: the place `saved` says, measured from `old`) to that
/// place for `new`, each as (rule in force, moved rule). A position measured from the old size
/// would leave a gap or an overlap (QA-GFX-19 follow-up). Hand-placed, `auto` and mirrored
/// displays are left alone; the last rule for a display is the one in force.
pub fn follow_main(
    saved: &[String],
    main: &str,
    old: Geometry,
    new: Geometry,
) -> Vec<(String, String)> {
    let mut seen: Vec<&str> = Vec::new();
    let mut out = Vec::new();
    for r in saved.iter().rev() {
        let name = r.split(',').next().map(str::trim).unwrap_or_default();
        if name.is_empty() || name == main || seen.contains(&name) {
            continue;
        }
        seen.push(name);
        let Some(this) = rule_geometry(r) else {
            continue;
        };
        let place = place_of(r, old, this);
        let (Some(from), Some((x, y))) = (position(place, old, this), position(place, new, this))
        else {
            continue;
        };
        if from == (x, y) {
            continue;
        }
        let mut parts: Vec<String> = r.split(',').map(|p| p.trim().to_string()).collect();
        parts[2] = format!("{x}x{y}");
        out.push((r.clone(), parts.join(",")));
    }
    out.reverse();
    out
}

/// The place a rule already says (from displays.lua, `prefs::monitor_rules`), so the menu opens on it.
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
        // The main display itself sits at 0x0; with no main named, the compositor chooses.
        assert_eq!(
            rule("eDP-1", (2560, 1600, 60.0), 2.0, Place::Right, main).as_deref(),
            Some("eDP-1,2560x1600@60.00,0x0,2")
        );
        assert_eq!(
            rule("eDP-1", (2560, 1600, 60.0), 2.0, Place::Auto, None).as_deref(),
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

    /// QA-GFX-19 follow-up: the main display goes from 2560x1600@2 to 1920x1080@1; the TV saved
    /// Right of it (1280x0) moves to 1920x0, the one Below (0x800) to 0x1080; one placed by hand
    /// and one on `auto` stay as they are, and an older rule for the TV does not count.
    #[test]
    fn displays_next_to_the_main_one_follow_its_new_size() {
        let saved = [
            "HDMI-A-1,1920x1080@60.00,9x9,1".to_string(),
            "HDMI-A-1,1920x1080@60.00,1280x0,1".to_string(),
            "DP-1,1920x1080@60.00,0x800,1".to_string(),
            "DP-2,1920x1080@60.00,4000x77,1".to_string(),
            "DP-3,1920x1080@60.00,auto,1".to_string(),
            "eDP-1,2560x1600@60.00,0x0,2".to_string(),
        ];
        assert_eq!(
            follow_main(&saved, "eDP-1", LAPTOP, TV),
            [
                (
                    saved[1].clone(),
                    "HDMI-A-1,1920x1080@60.00,1920x0,1".to_string()
                ),
                (
                    saved[2].clone(),
                    "DP-1,1920x1080@60.00,0x1080,1".to_string()
                )
            ]
        );
        assert!(follow_main(&saved, "eDP-1", LAPTOP, LAPTOP).is_empty());
    }

    fn mon(name: &str) -> wol_core::controls::Monitor {
        wol_core::controls::Monitor {
            name: name.into(),
            width: 1280,
            height: 800,
            refresh: 75.0,
            scale: 1.0,
            modes: vec!["1280x800@75.00Hz".into()],
            ..Default::default()
        }
    }

    /// QA-GFX-1: a rule that places another display (Below) was saved next to a main display
    /// rule that said `auto`, and the compositor moved the main one to 1280x0.
    #[test]
    fn main_gets_explicit_0x0_when_other_positioned() {
        let main = Some(("Virtual-1", TV));
        let below = rule("Virtual-2", (1280, 800, 74.99), 1.0, Place::Below, main).unwrap();
        assert_eq!(below, "Virtual-2,1280x800@74.99,0x1080,1");
        let own = rule("Virtual-1", (1920, 1080, 60.0), 1.0, Place::Auto, main).unwrap();
        assert_eq!(own, "Virtual-1,1920x1080@60.00,0x0,1");
        // Saved with `auto` or not at all: the page pins it first.
        let saved = ["Virtual-1,1920x1080@60.00,auto,1".to_string()];
        assert_eq!(
            anchor_rule(&saved, "Virtual-1", None).as_deref(),
            Some("Virtual-1,1920x1080@60.00,0x0,1")
        );
        assert_eq!(
            anchor_rule(&[], "Virtual-1", Some(&own)).as_deref(),
            Some(own.as_str())
        );
        assert_eq!(anchor_rule(&[], "Virtual-1", None), None);
        // Already pinned, or placed by hand: left alone.
        assert_eq!(anchor_rule(&[own.clone()], "Virtual-1", Some(&own)), None);
        let by_hand = ["Virtual-1,1920x1080@60.00,100x0,1".to_string()];
        assert_eq!(anchor_rule(&by_hand, "Virtual-1", Some(&own)), None);
        // The last rule wins (a held mode change comes after the file's).
        let both = [
            "Virtual-1,1920x1080@60.00,auto,1".to_string(),
            "Virtual-1,1280x800@60.00,0x0,1".to_string(),
        ];
        assert_eq!(anchor_rule(&both, "Virtual-1", Some(&own)), None);
        // Another display's rule is not the main display's.
        assert_eq!(
            anchor_rule(&[below.clone()], "Virtual-1", Some(&own)).as_deref(),
            Some(own.as_str())
        );
    }

    /// QA-SET-18: a display at 1.333 listed no 133%, showed 100% and saved 1.0 on a rate change.
    #[test]
    fn choice_keeps_1_333_scale() {
        let plain = scale_choices(1.5);
        assert_eq!(plain.len(), SCALES.len(), "a listed scale adds nothing");
        assert_eq!(plain[scale_index(&plain, 1.5)].1, 1.5);
        let odd = scale_choices(1.333);
        assert_eq!(odd.len(), SCALES.len() + 1);
        let at = scale_index(&odd, 1.333);
        assert_eq!(odd[at].1, 1.333, "the menu opens on the scale it runs at");
        assert_eq!(odd[at].0, "133% (current)");
        assert!(odd.windows(2).all(|w| w[0].1 < w[1].1), "in order");
        // The rule built from that choice carries 1.333, not 1.
        let r = rule("eDP-1", (1920, 1080, 60.0), odd[at].1, Place::Auto, None).unwrap();
        assert_eq!(r, "eDP-1,1920x1080@60.00,auto,1.333");
        // The compositor's 160 % on a 1280 panel sits between 150 and 175.
        let c = scale_choices(1.6);
        assert_eq!(c[3].1, 1.6);
        assert!(scale_choices(f64::NAN).len() == SCALES.len());
    }

    /// QA-GFX-6: the Dock, Settings and the Control Center name one main display: the built-in
    /// panel, even when the compositor lists an external one first.
    #[test]
    fn main_of_prefers_builtin() {
        let list = [mon("HDMI-A-1"), mon("eDP-1")];
        assert_eq!(main_of(&list).map(|m| m.name.as_str()), Some("eDP-1"));
        let lvds = [mon("DP-2"), mon("LVDS-1"), mon("eDP-2")];
        assert_eq!(main_of(&lvds).map(|m| m.name.as_str()), Some("LVDS-1"));
        // No panel (a desktop, a VM): the first listed.
        let ext = [mon("Virtual-2"), mon("Virtual-1")];
        assert_eq!(main_of(&ext).map(|m| m.name.as_str()), Some("Virtual-2"));
        assert!(main_of::<wol_core::controls::Monitor>(&[]).is_none());
        // Connector lists (the Dock) agree with monitor lists.
        assert_eq!(
            main_of(&["HDMI-A-1".to_string(), "eDP-1".to_string()]).map(String::as_str),
            Some("eDP-1")
        );
        let c = change(&["HDMI-A-1".into()], &["HDMI-A-1".into(), "eDP-1".into()]);
        assert_eq!(c.main.as_deref(), Some("eDP-1"));
    }

    /// QA-GFX-5: the page is built again when a display goes, and a stale snapshot does not bring
    /// a display back that the desktop already dropped.
    #[test]
    fn rebuild_on_monitors_changed() {
        let two = vec![mon("Virtual-1"), mon("Virtual-2")];
        let one = vec![mon("Virtual-1")];
        assert!(monitors_changed(&two, &one), "unplugged");
        assert!(monitors_changed(&one, &two), "plugged in");
        assert!(!monitors_changed(&two, &two.clone()));
        let mut moded = two.clone();
        moded[1].width = 1920;
        assert!(monitors_changed(&two, &moded), "another mode");
        let mut scaled = two.clone();
        scaled[0].scale = 2.0;
        assert!(monitors_changed(&two, &scaled), "another scale");
        let live = ["Virtual-1".to_string()];
        let kept = still_connected(two.clone(), &live);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].name, "Virtual-1");
        // The desktop lists none of them (a preview): the snapshot stands.
        assert_eq!(
            still_connected(two.clone(), &["Other-1".to_string()]).len(),
            2
        );
        assert_eq!(still_connected(two.clone(), &[]).len(), 2);
    }

    fn undo(name: &str, rule: &str) -> Undo {
        Undo {
            name: name.into(),
            rule: rule.into(),
        }
    }

    /// QA-LIVE-13: a changed mode waits 15 s for Keep; the countdown ends in a revert.
    #[test]
    fn apply_arms_revert_timer_15s() {
        assert_eq!(REVERT_SECS, 15);
        let mut c = Confirm::arm(undo("eDP-1", "eDP-1,2560x1600@60.00,0x0,2"), vec![]);
        assert_eq!(c.left(), 15);
        for want in (1..15).rev() {
            assert_eq!(c.tick(), Tick::Left(want));
        }
        assert_eq!(c.tick(), Tick::Revert, "the 15th second without Keep");
        // Another mode change while waiting starts the count again; an arrangement does not.
        let mut c = Confirm::arm(undo("eDP-1", "a"), vec![]);
        for _ in 0..10 {
            c.tick();
        }
        c.hold(undo("DP-1", "p"), vec![]);
        assert_eq!(c.left(), 5, "an arrangement leaves the clock alone");
        c.change(undo("eDP-1", "b"), vec![]);
        assert_eq!(c.left(), 15);
        // A late tick never wraps.
        let mut d = Confirm::arm(undo("x", "a"), vec![]);
        for _ in 0..40 {
            d.tick();
        }
        assert_eq!(d.left(), 0);
    }

    /// QA-LIVE-13: the revert puts back the rule the display ran with before the FIRST change,
    /// once per display, including a display only rearranged meanwhile.
    #[test]
    fn revert_restores_previous_mode() {
        let mut c = Confirm::arm(undo("eDP-1", "eDP-1,2560x1600@60.00,0x0,2"), vec![]);
        c.change(undo("eDP-1", "eDP-1,1920x1200@60.00,0x0,2"), vec![]);
        c.hold(
            undo("HDMI-A-1", "HDMI-A-1,1920x1080@60.00,2560x0,1"),
            vec![],
        );
        let rules: Vec<(&str, &str)> = c
            .rules()
            .iter()
            .map(|u| (u.name.as_str(), u.rule.as_str()))
            .collect();
        assert_eq!(
            rules,
            [
                ("eDP-1", "eDP-1,2560x1600@60.00,0x0,2"),
                ("HDMI-A-1", "HDMI-A-1,1920x1080@60.00,2560x0,1")
            ]
        );
    }

    /// Nothing is saved while the countdown runs: a shell that dies mid-countdown leaves
    /// displays.lua alone. Every change made meanwhile (a mode, then an arrangement) is kept
    /// for Keep, oldest first, so none of them is lost or left out of the journal entry.
    #[test]
    fn nothing_is_saved_until_keep() {
        let mut c = Confirm::arm(
            undo("eDP-1", "old"),
            vec!["main-pin".into(), "eDP-1,new".into()],
        );
        c.hold(undo("DP-1", "p"), vec!["DP-1,below".into()]);
        c.change(undo("eDP-1", "mid"), vec!["eDP-1,newer".into()]);
        assert_eq!(
            c.saves(),
            ["main-pin", "eDP-1,new", "DP-1,below", "eDP-1,newer"]
        );
        let src = include_str!("ui/settings/displays.rs");
        let send = &src[src.find("fn send(").unwrap()..];
        let send = &send[..send.find("\n}\n").unwrap()];
        assert!(
            send.contains("if hold.is_some() {\n                        return Ok(());"),
            "a held change writes no file"
        );
        assert!(
            send.contains("page.hold("),
            "a held change goes to the page"
        );
        assert!(
            !src.contains("sys.undo("),
            "no journal entry exists before Keep, so nothing is undone"
        );
    }

    /// A main display somebody placed by hand keeps that position through a mode change.
    #[test]
    fn hand_placed_main_keeps_its_position() {
        let rule = "eDP-1,1920x1200@60.00,0x0,2".to_string();
        let by_hand = ["eDP-1,2560x1600@60.00,100x50,2".to_string()];
        assert_eq!(
            keep_position(rule.clone(), &by_hand),
            "eDP-1,1920x1200@60.00,100x50,2"
        );
        let auto = ["eDP-1,2560x1600@60.00,auto,2".to_string()];
        assert_eq!(keep_position(rule.clone(), &auto), rule);
        assert_eq!(keep_position(rule.clone(), &[]), rule);
        let other = ["DP-1,1920x1080@60.00,100x50,1".to_string()];
        assert_eq!(keep_position(rule.clone(), &other), rule);
    }

    /// The change lands in the journal under "displays" (Settings > Restore lists it), once, when
    /// kept; the generic Undo toast (file only) is not how it is taken back.
    #[test]
    fn the_journal_topic_is_displays() {
        let src = include_str!("ui/settings/displays.rs");
        assert!(src.contains("\"displays\","));
        assert!(!src.contains("save_journaled("));
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
