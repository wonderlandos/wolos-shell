//! Timer widget, pure side: Focus (25 min work, 5 min break, a long break every fourth),
//! a countdown, or a stopwatch. Times are milliseconds from any monotonic origin; the desktop
//! passes the clock in, so nothing here reads one.

const MIN: u64 = 60_000;
/// Work rounds before a long break.
pub const ROUNDS: u32 = 4;

/// What the widget counts. Stored as the widget's `timer` option ([`Mode::id`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Focus,
    /// Minutes.
    Countdown(u32),
    Stopwatch,
}

/// Modes offered in the widget's menu.
pub const MODES: [Mode; 6] = [
    Mode::Focus,
    Mode::Countdown(5),
    Mode::Countdown(10),
    Mode::Countdown(15),
    Mode::Countdown(30),
    Mode::Stopwatch,
];

impl Mode {
    pub fn id(self) -> String {
        match self {
            Mode::Focus => "focus".into(),
            Mode::Countdown(m) => m.to_string(),
            Mode::Stopwatch => "stopwatch".into(),
        }
    }
    /// Unknown or missing ids are Focus.
    pub fn parse(s: Option<&str>) -> Mode {
        match s {
            Some("stopwatch") => Mode::Stopwatch,
            Some(n) => match n.parse::<u32>() {
                Ok(m) if (1..=24 * 60).contains(&m) => Mode::Countdown(m),
                _ => Mode::Focus,
            },
            None => Mode::Focus,
        }
    }
    pub fn label(self) -> String {
        match self {
            Mode::Focus => "Focus".into(),
            Mode::Countdown(m) => format!("{m} Minutes"),
            Mode::Stopwatch => "Stopwatch".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Work,
    Break,
    LongBreak,
}

/// A button on the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    /// Start, or pause when running.
    Toggle,
    /// Back to the start of this phase (Focus: of the first round).
    Reset,
    /// Focus: on to the next phase.
    Skip,
}

/// One widget's counter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Counter {
    pub mode: Mode,
    pub phase: Phase,
    /// Work rounds finished since the last long break.
    pub rounds: u32,
    /// Counted before the current run.
    banked: u64,
    /// Start of the current run; None = stopped.
    since: Option<u64>,
    /// A countdown reached zero.
    pub done: bool,
}

/// What the card shows.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub mode: Mode,
    /// "Focus", "Break", "Long Break", "10 Minutes", "Stopwatch", "Done".
    pub title: String,
    /// "24:59", "1:02:03".
    pub time: String,
    /// 0..1 of the phase gone (stopwatch: 0).
    pub progress: f64,
    pub running: bool,
    /// "Round 2 of 4" (Focus only).
    pub detail: Option<String>,
}

/// "MM:SS", or "H:MM:SS" from an hour.
pub fn hms(ms: u64) -> String {
    let s = ms / 1000;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

impl Counter {
    pub fn new(mode: Mode) -> Counter {
        Counter {
            mode,
            phase: Phase::Work,
            rounds: 0,
            banked: 0,
            since: None,
            done: false,
        }
    }

    /// Length of the current phase; None = counts up.
    pub fn length(&self) -> Option<u64> {
        match self.mode {
            Mode::Stopwatch => None,
            Mode::Countdown(m) => Some(m as u64 * MIN),
            Mode::Focus => Some(match self.phase {
                Phase::Work => 25 * MIN,
                Phase::Break => 5 * MIN,
                Phase::LongBreak => 15 * MIN,
            }),
        }
    }

    pub fn running(&self) -> bool {
        self.since.is_some()
    }

    pub fn elapsed(&self, now: u64) -> u64 {
        self.banked + self.since.map_or(0, |s| now.saturating_sub(s))
    }

    pub fn press(&mut self, p: Press, now: u64) {
        match p {
            Press::Toggle => match self.since.take() {
                Some(s) => self.banked += now.saturating_sub(s),
                None => {
                    if self.done {
                        self.restart();
                    }
                    self.since = Some(now);
                }
            },
            Press::Reset => {
                if self.mode == Mode::Focus && self.elapsed(now) == 0 {
                    self.phase = Phase::Work;
                    self.rounds = 0;
                }
                self.restart();
            }
            Press::Skip => {
                if self.mode == Mode::Focus {
                    self.advance();
                }
            }
        }
    }

    fn restart(&mut self) {
        self.banked = 0;
        self.since = None;
        self.done = false;
    }

    fn advance(&mut self) {
        self.phase = match self.phase {
            Phase::Work => {
                self.rounds += 1;
                if self.rounds >= ROUNDS {
                    Phase::LongBreak
                } else {
                    Phase::Break
                }
            }
            Phase::LongBreak => {
                self.rounds = 0;
                Phase::Work
            }
            Phase::Break => Phase::Work,
        };
        self.restart();
    }

    /// Settles a phase that ran out. Returns the phase that finished, for a banner.
    pub fn tick(&mut self, now: u64) -> Option<Phase> {
        let len = self.length()?;
        if !self.running() || self.elapsed(now) < len {
            return None;
        }
        let ended = self.phase;
        match self.mode {
            // The next phase waits for the user: a break never starts on its own.
            Mode::Focus => self.advance(),
            _ => {
                self.since = None;
                self.banked = len;
                self.done = true;
            }
        }
        Some(ended)
    }

    pub fn view(&self, now: u64) -> View {
        let e = self.elapsed(now);
        let (time, progress) = match self.length() {
            None => (hms(e), 0.0),
            // Round up: a fresh 25 min timer reads 25:00, its last second 00:01.
            Some(len) => (
                hms(len.saturating_sub(e).div_ceil(1000) * 1000),
                (e as f64 / len as f64).min(1.0),
            ),
        };
        let title = match (self.mode, self.phase) {
            _ if self.done => "Done".into(),
            (Mode::Focus, Phase::Work) => "Focus".into(),
            (Mode::Focus, Phase::Break) => "Break".into(),
            (Mode::Focus, Phase::LongBreak) => "Long Break".into(),
            (m, _) => m.label(),
        };
        View {
            mode: self.mode,
            title,
            time,
            progress,
            running: self.running(),
            detail: (self.mode == Mode::Focus).then(|| {
                let n = match self.phase {
                    Phase::Work => self.rounds + 1,
                    _ => self.rounds,
                };
                format!("Round {} of {ROUNDS}", n.clamp(1, ROUNDS))
            }),
        }
    }
}

/// The notification when `ended` ran out in `mode`: (summary, body).
pub fn finished(mode: Mode, ended: Phase) -> (String, String) {
    let (a, b) = match (mode, ended) {
        (Mode::Focus, Phase::Work) => (
            "Time for a break",
            "Step away for a few minutes. Start the break when you are ready.",
        ),
        (Mode::Focus, _) => (
            "Break is over",
            "Start the next focus session when you are ready.",
        ),
        _ => ("Timer done", "Your countdown has finished."),
    };
    (a.into(), b.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    const S: u64 = 1000;

    #[test]
    fn modes_round_trip_and_default_to_focus() {
        for m in MODES {
            assert_eq!(Mode::parse(Some(&m.id())), m);
        }
        assert_eq!(Mode::parse(None), Mode::Focus);
        assert_eq!(Mode::parse(Some("0")), Mode::Focus);
        assert_eq!(Mode::parse(Some("bogus")), Mode::Focus);
        assert_eq!(Mode::Countdown(10).label(), "10 Minutes");
    }

    #[test]
    fn finished_notes() {
        assert_eq!(finished(Mode::Focus, Phase::Work).0, "Time for a break");
        assert_eq!(finished(Mode::Focus, Phase::LongBreak).0, "Break is over");
        assert_eq!(finished(Mode::Countdown(5), Phase::Work).0, "Timer done");
        for m in MODES {
            for p in [Phase::Work, Phase::Break] {
                let (a, b) = finished(m, p);
                assert!(!a.ends_with('.'));
                assert_eq!(crate::copy::lint(&b), Vec::<String>::new(), "{b}");
            }
        }
    }

    #[test]
    fn time_text() {
        assert_eq!(hms(0), "00:00");
        assert_eq!(hms(61 * S), "01:01");
        assert_eq!(hms(3723 * S), "1:02:03");
    }

    #[test]
    fn countdown_pauses_resumes_and_finishes() {
        let mut c = Counter::new(Mode::Countdown(5));
        assert_eq!(c.view(0).time, "05:00");
        c.press(Press::Toggle, 1_000);
        assert_eq!(c.view(1_500).time, "05:00", "rounds up");
        assert_eq!(c.view(61_000).time, "04:00");
        c.press(Press::Toggle, 61_000); // pause
        assert_eq!(c.view(500_000).time, "04:00", "paused time does not count");
        c.press(Press::Toggle, 500_000);
        assert_eq!(c.tick(500_000 + 239 * S), None);
        assert_eq!(c.tick(500_000 + 240 * S), Some(Phase::Work));
        let v = c.view(900_000);
        assert_eq!(
            (v.title.as_str(), v.time.as_str(), v.running),
            ("Done", "00:00", false)
        );
        assert_eq!(v.progress, 1.0);
        c.press(Press::Toggle, 900_000);
        assert_eq!(
            c.view(900_000).time,
            "05:00",
            "start after done begins again"
        );
        assert!(c.running());
    }

    #[test]
    fn focus_cycles_work_and_breaks_and_waits_between() {
        let mut c = Counter::new(Mode::Focus);
        let v = c.view(0);
        assert_eq!((v.title.as_str(), v.time.as_str()), ("Focus", "25:00"));
        assert_eq!(v.detail.as_deref(), Some("Round 1 of 4"));
        let mut now = 0;
        let mut seen = vec![];
        for _ in 0..8 {
            c.press(Press::Toggle, now);
            now += c.length().unwrap();
            assert!(c.tick(now).is_some());
            assert!(!c.running(), "the next phase waits for Start");
            seen.push(c.view(now).title);
        }
        assert_eq!(
            seen,
            [
                "Break",
                "Focus",
                "Break",
                "Focus",
                "Break",
                "Focus",
                "Long Break",
                "Focus"
            ]
        );
        assert_eq!(c.view(now).detail.as_deref(), Some("Round 1 of 4"));
    }

    #[test]
    fn reset_and_skip() {
        let mut c = Counter::new(Mode::Focus);
        c.press(Press::Skip, 0);
        assert_eq!(c.phase, Phase::Break);
        c.press(Press::Toggle, 0);
        c.press(Press::Reset, 10 * S);
        assert_eq!(
            (c.phase, c.elapsed(20 * S)),
            (Phase::Break, 0),
            "reset keeps the phase"
        );
        c.press(Press::Reset, 20 * S);
        assert_eq!(
            (c.phase, c.rounds),
            (Phase::Work, 0),
            "a second reset starts over"
        );
        let mut sw = Counter::new(Mode::Stopwatch);
        sw.press(Press::Toggle, 0);
        assert_eq!(sw.tick(10 * 60 * 60 * S), None, "a stopwatch never ends");
        let v = sw.view(3723 * S);
        assert_eq!(
            (v.time.as_str(), v.progress, v.detail),
            ("1:02:03", 0.0, None)
        );
        sw.press(Press::Skip, 0);
        assert_eq!(sw.phase, Phase::Work);
    }
}
