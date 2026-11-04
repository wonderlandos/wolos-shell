// SPDX-License-Identifier: GPL-2.0-or-later
//! Focus (Do Not Disturb): `$XDG_CONFIG_HOME/wol/dnd`. Missing = off; empty = on until turned off;
//! `until=<unix secs>` = on until then. Banners stay quiet while on (critical ones still show);
//! the menu bar shows the moon.

pub const FILE: &str = "wol/dnd";
pub const HOUR: u64 = 3600;
/// "Until tomorrow" ends at this local hour.
pub const MORNING_HOUR: u8 = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Off,
    On,
    /// Unix seconds.
    Until(u64),
}

impl Focus {
    pub fn active(self) -> bool {
        self != Focus::Off
    }
}

/// Menu bar status icon: the moon while Focus is on.
pub fn menubar_icon(f: Focus) -> Option<&'static str> {
    f.active().then_some("weather-clear-night-symbolic")
}

/// File content at `now` (unix secs): an expired timer reads as off.
pub fn parse(content: Option<&str>, now: u64) -> Focus {
    let Some(t) = content.map(str::trim) else {
        return Focus::Off;
    };
    match t.strip_prefix("until=").map(|n| n.parse::<u64>()) {
        None => Focus::On,
        Some(Ok(u)) if u > now => Focus::Until(u),
        Some(_) => Focus::Off,
    }
}

/// File content for `f`; None = remove the file.
pub fn content(f: Focus) -> Option<String> {
    match f {
        Focus::Off => None,
        Focus::On => Some(String::new()),
        Focus::Until(u) => Some(format!("until={u}")),
    }
}

/// Seconds from local `h:m:s` to the next [`MORNING_HOUR`]:00 ("until tomorrow").
pub fn secs_to_morning(h: u8, m: u8, s: u8) -> u64 {
    let morning = u32::from(MORNING_HOUR) * 3600;
    u64::from(wol_core::clock::secs_until(h, m, s, &[morning]))
}

/// Control Center subtitle: "Off", "On", or "Until 15:05" (local `h:m` now, `now` unix secs).
pub fn label(f: Focus, now: u64, h: u8, m: u8) -> String {
    match f {
        Focus::Off => "Off".into(),
        Focus::On => "On".into(),
        Focus::Until(u) => {
            let mins = (h as u64 * 60 + m as u64 + u.saturating_sub(now).div_ceil(60)) % 1440;
            format!("Until {:02}:{:02}", mins / 60, mins % 60)
        }
    }
}

pub fn path() -> std::path::PathBuf {
    std::path::Path::new(&crate::kit::config_home()).join(FILE)
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Current Focus from the file.
pub fn read() -> Focus {
    parse(std::fs::read_to_string(path()).ok().as_deref(), unix_now())
}

/// Saves Focus through the settings journal as Settings (so Undo and `wol history` see it, and a
/// failure is the caller's to show) instead of writing the file behind its back (QA-SET-52).
pub fn write(f: Focus) -> std::io::Result<()> {
    write_as("settings", f)
}

/// [`write`] for another actor (`control-center`).
pub fn write_as(by: &str, f: Focus) -> std::io::Result<()> {
    write_in(
        std::path::Path::new(&crate::kit::config_home()),
        by,
        f,
        unix_now(),
    )
}

/// [`write`] under `config_home` at `now`.
pub fn write_in(
    config_home: &std::path::Path,
    by: &str,
    f: Focus,
    now: u64,
) -> std::io::Result<()> {
    wol_core::journal::Journal::for_config(config_home)
        .save("focus", by, &[(FILE, content(f).as_deref())], now)
        .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_round_trip_and_expiry() {
        for f in [Focus::Off, Focus::On, Focus::Until(2000)] {
            assert_eq!(parse(content(f).as_deref(), 1000), f);
        }
        assert_eq!(parse(Some("until=1000"), 1000), Focus::Off, "expired");
        assert_eq!(parse(Some("\n"), 0), Focus::On, "legacy empty file");
        assert_eq!(parse(Some("until=x"), 0), Focus::Off);
        assert!(!Focus::Off.active() && Focus::Until(1).active());
        assert_eq!(menubar_icon(Focus::Off), None);
        assert_eq!(
            menubar_icon(Focus::Until(5)),
            Some("weather-clear-night-symbolic")
        );
    }

    /// QA-SET-52: the Control Center wrote wol/dnd directly: no Undo entry. Every change is one.
    #[test]
    fn focus_writes_journaled() {
        let d = std::env::temp_dir().join(format!("wol-focus-journal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        write_in(&d, "control-center", Focus::On, 100).unwrap();
        assert_eq!(
            parse(std::fs::read_to_string(d.join(FILE)).ok().as_deref(), 100),
            Focus::On
        );
        write_in(&d, "control-center", Focus::Until(100 + HOUR), 101).unwrap();
        write_in(&d, "control-center", Focus::Off, 102).unwrap();
        assert!(!d.join(FILE).exists());
        let j = wol_core::journal::Journal::for_config(&d);
        let e = j.entries();
        assert_eq!(e.len(), 3, "one entry per change: {e:?}");
        assert!(
            e.iter()
                .all(|e| e.what == "focus" && e.by == "control-center")
        );
        j.undo(None, "settings", 103).unwrap();
        assert_eq!(
            parse(std::fs::read_to_string(d.join(FILE)).ok().as_deref(), 103),
            Focus::Until(100 + HOUR),
            "Undo brings the timer back"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn timers_and_labels() {
        assert_eq!(secs_to_morning(14, 5, 0), (16 * 60 + 55) * 60);
        assert_eq!(secs_to_morning(6, 59, 0), 60);
        assert_eq!(secs_to_morning(7, 0, 0), 86_400);
        assert_eq!(label(Focus::Until(1000 + HOUR), 1000, 14, 5), "Until 15:05");
        assert_eq!(label(Focus::Until(1000 + 50), 1000, 23, 59), "Until 00:00");
        assert_eq!(label(Focus::On, 0, 0, 0), "On");
        assert_eq!(label(Focus::Off, 0, 0, 0), "Off");
    }
}
