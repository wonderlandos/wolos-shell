//! World Clock widget, pure side: the cities it offers and each city's wall time against this
//! computer's. Offsets come from the system time zone database (`ui::widgets`); nothing here
//! reads a clock.

/// Cities the widget offers: (name, IANA zone). A placed widget stores the zones it shows.
pub const CITIES: [(&str, &str); 16] = [
    ("Honolulu", "Pacific/Honolulu"),
    ("San Francisco", "America/Los_Angeles"),
    ("Chicago", "America/Chicago"),
    ("New York", "America/New_York"),
    ("São Paulo", "America/Sao_Paulo"),
    ("London", "Europe/London"),
    ("Paris", "Europe/Paris"),
    ("Berlin", "Europe/Berlin"),
    ("Cairo", "Africa/Cairo"),
    ("Moscow", "Europe/Moscow"),
    ("Dubai", "Asia/Dubai"),
    ("Mumbai", "Asia/Kolkata"),
    ("Singapore", "Asia/Singapore"),
    ("Shanghai", "Asia/Shanghai"),
    ("Tokyo", "Asia/Tokyo"),
    ("Sydney", "Australia/Sydney"),
];

/// Shown until the user picks cities.
pub const DEFAULT: [&str; 4] = [
    "America/New_York",
    "Europe/London",
    "Asia/Tokyo",
    "Australia/Sydney",
];

/// The city name for a zone ("Asia/Tokyo" -> "Tokyo"; unknown zones use their last part).
pub fn city(zone: &str) -> String {
    CITIES
        .iter()
        .find(|c| c.1 == zone)
        .map(|c| c.0.to_string())
        .unwrap_or_else(|| zone.rsplit('/').next().unwrap_or(zone).replace('_', " "))
}

/// The zones a widget shows: its choice, else [`DEFAULT`].
pub fn zones(chosen: Option<&[String]>) -> Vec<String> {
    match chosen {
        Some(z) if !z.is_empty() => z.to_vec(),
        _ => DEFAULT.iter().map(|s| s.to_string()).collect(),
    }
}

/// `zone` added when absent, removed when present (the last city cannot be removed).
pub fn toggle(chosen: Option<&[String]>, zone: &str) -> Vec<String> {
    let mut z = zones(chosen);
    match z.iter().position(|x| x == zone) {
        Some(i) if z.len() > 1 => {
            z.remove(i);
        }
        Some(_) => {}
        None => z.push(zone.into()),
    }
    z
}

/// How many cities fit a card.
pub fn fits(size: shared::widgets::Size) -> usize {
    use shared::widgets::Size;
    match size {
        Size::Small => 2,
        Size::Medium => 4,
        Size::Large => 6,
    }
}

/// One city's time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CityTime {
    pub name: String,
    pub hour: u32,
    pub minute: u32,
    /// Calendar day against here: -1 yesterday, 0 today, 1 tomorrow.
    pub day: i32,
    /// Minutes ahead of here (negative: behind).
    pub ahead: i32,
}

impl CityTime {
    /// `utc`: seconds since the epoch; offsets in seconds east of UTC.
    pub fn at(name: &str, utc: i64, here: i32, there: i32) -> CityTime {
        let wall = |off: i32| utc + off as i64;
        let t = wall(there);
        let mins = t.rem_euclid(86_400) / 60;
        CityTime {
            name: name.into(),
            hour: (mins / 60) as u32,
            minute: (mins % 60) as u32,
            day: (t.div_euclid(86_400) - wall(here).div_euclid(86_400)) as i32,
            ahead: (there - here) / 60,
        }
    }

    pub fn hhmm(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }

    /// "Yesterday", "Today", "Tomorrow".
    pub fn day_word(&self) -> &'static str {
        match self.day {
            d if d < 0 => "Yesterday",
            0 => "Today",
            _ => "Tomorrow",
        }
    }

    /// "+5 h", "−3 h 30 min", "same time".
    pub fn offset(&self) -> String {
        if self.ahead == 0 {
            return "same time".into();
        }
        let sign = if self.ahead > 0 { "+" } else { "−" };
        let (h, m) = (self.ahead.abs() / 60, self.ahead.abs() % 60);
        match (h, m) {
            (0, m) => format!("{sign}{m} min"),
            (h, 0) => format!("{sign}{h} h"),
            (h, m) => format!("{sign}{h} h {m} min"),
        }
    }

    /// "Today, +5 h", "Tomorrow, +9 h 30 min", "Yesterday, −3 h", "Today, same time".
    pub fn relative(&self) -> String {
        format!("{}, {}", self.day_word(), self.offset())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::widgets::Size;

    // 2026-10-30 22:15 UTC.
    const UTC: i64 = 1_793_398_500;

    #[test]
    fn wall_time_and_day_against_here() {
        let here = -3 * 3600; // São Paulo, 19:15
        let tokyo = CityTime::at("Tokyo", UTC, here, 9 * 3600);
        assert_eq!(
            (tokyo.hhmm(), tokyo.day, tokyo.ahead),
            ("07:15".into(), 1, 720)
        );
        assert_eq!(tokyo.relative(), "Tomorrow, +12 h");
        let hnl = CityTime::at("Honolulu", UTC, here, -10 * 3600);
        assert_eq!((hnl.hhmm(), hnl.day), ("12:15".into(), 0));
        assert_eq!(hnl.relative(), "Today, −7 h");
        let mumbai = CityTime::at("Mumbai", UTC, here, 19_800);
        assert_eq!(mumbai.relative(), "Tomorrow, +8 h 30 min");
        let same = CityTime::at("Here", UTC, here, here);
        assert_eq!(same.relative(), "Today, same time");
        // Tokyo morning, Honolulu the evening before.
        let early = CityTime::at("Honolulu", UTC, 9 * 3600, -10 * 3600);
        assert_eq!(early.relative(), "Yesterday, −19 h");
        assert_eq!(CityTime::at("x", UTC, 0, 1800).relative(), "Today, +30 min");
        assert_eq!(
            (tokyo.day_word(), tokyo.offset()),
            ("Tomorrow", "+12 h".into())
        );
    }

    #[test]
    fn cities_default_and_toggle() {
        assert_eq!(city("Asia/Tokyo"), "Tokyo");
        assert_eq!(city("America/Port_of_Spain"), "Port of Spain");
        assert_eq!(zones(None).len(), 4);
        assert_eq!(zones(Some(&[])), zones(None));
        let one = vec!["Asia/Tokyo".to_string()];
        assert_eq!(toggle(Some(&one), "Asia/Tokyo"), one, "keeps the last city");
        assert_eq!(
            toggle(Some(&one), "Europe/Paris"),
            ["Asia/Tokyo", "Europe/Paris"]
        );
        assert!(!toggle(None, "Europe/London").contains(&"Europe/London".to_string()));
        assert_eq!((fits(Size::Small), fits(Size::Large)), (2, 6));
        assert!(CITIES.iter().all(|c| !c.1.is_empty()));
        assert!(DEFAULT.iter().all(|z| CITIES.iter().any(|c| c.1 == *z)));
    }
}
