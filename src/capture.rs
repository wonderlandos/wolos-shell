// SPDX-License-Identifier: GPL-2.0-or-later
//! Screenshot toolbar (Super+Shift+5) model: what to capture, where it goes, the wol-screenshot
//! argv, and Markup (ADR 0031 W5): pen strokes drawn over a capture before it is saved or copied.
//! Invariant: Markup is bounded ([`MAX_STROKES`] strokes of [`MAX_POINTS`] points) and keeps its
//! points in picture pixels, so the saved file has the picture's own size.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Screen,
    Window,
    Selection,
    RecordScreen,
    RecordSelection,
}

impl Mode {
    pub const ALL: [Mode; 5] = [
        Mode::Screen,
        Mode::Window,
        Mode::Selection,
        Mode::RecordScreen,
        Mode::RecordSelection,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Mode::Screen => "Capture Entire Screen",
            Mode::Window => "Capture Selected Window",
            Mode::Selection => "Capture Selected Portion",
            Mode::RecordScreen => "Record Entire Screen",
            Mode::RecordSelection => "Record Selected Portion",
        }
    }
    pub fn icon(self) -> &'static str {
        match self {
            Mode::Screen => "wolos-capture-screen-symbolic",
            Mode::Window => "wolos-capture-window-symbolic",
            Mode::Selection => "wolos-capture-selection-symbolic",
            Mode::RecordScreen => "wolos-record-screen-symbolic",
            Mode::RecordSelection => "wolos-record-selection-symbolic",
        }
    }
    pub fn records(self) -> bool {
        matches!(self, Mode::RecordScreen | Mode::RecordSelection)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dest {
    Desktop,
    Pictures,
    /// Only the clipboard: no file, no thumbnail (`wol-screenshot --copy`).
    Clipboard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub dest: Dest,
    /// Seconds before capture: 0, 5 or 10.
    pub delay: u32,
}

/// wol-screenshot's recorder pid file in `$XDG_RUNTIME_DIR` ("<pid> <file>"): there while a screen
/// recording runs, whoever started it (the toolbar, the Record tile, a hotkey).
pub const RECORD_PID: &str = "wol-record.pid";

/// The pid file of this session's recording.
pub fn record_pid_file(runtime: &str) -> std::path::PathBuf {
    std::path::Path::new(runtime).join(RECORD_PID)
}

/// Ends the running recording (saves the file, says where).
pub const STOP_ARGV: [&str; 2] = ["wol-screenshot", "stop"];

/// What `wol shell show|toggle|hide screen-record` (the Record tile) does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordAsk {
    /// A recording runs: the tile stops it (live-1 H4: it offered a second Record instead).
    Stop,
    /// The toolbar in Record mode shows (true) or hides.
    Toolbar(bool),
}

pub fn record_ask(recording: bool, verb: crate::protocol::Verb, visible: bool) -> RecordAsk {
    use crate::protocol::Verb;
    if recording && verb != Verb::Hide {
        return RecordAsk::Stop;
    }
    RecordAsk::Toolbar(crate::protocol::apply(verb, visible))
}

/// The banner that keeps a toolbar capture in Notification Center and names where it went
/// (QA-LIVE1-45: the thumbnail alone said nothing of the folder, and no notice stayed). Same words
/// and image hint as wol-screenshot's own banner for the hotkeys.
pub fn saved_notice_argv(saved: &std::path::Path) -> Vec<String> {
    let folder = saved
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    ["notify-send", "-a", "Screenshot", "-i", "wolos-photos"]
        .iter()
        .map(|s| s.to_string())
        .chain([
            "-h".into(),
            format!("string:image-path:{}", saved.display()),
            format!("Screenshot saved to {folder}"),
        ])
        .collect()
}

/// Wait between hiding the toolbar and capturing, so the compositor has drawn a frame without it.
pub const SETTLE: std::time::Duration = std::time::Duration::from_millis(150);

pub const DELAYS: [u32; 3] = [0, 5, 10];

impl Default for Options {
    fn default() -> Self {
        Options {
            dest: Dest::Desktop,
            delay: 0,
        }
    }
}

/// `wol-screenshot` argv. `window` is the focused window's "x,y wxh" (Window mode only).
pub fn argv(mode: Mode, o: Options, window: Option<&str>, home: &str) -> Vec<String> {
    let mut v = vec!["wol-screenshot".to_string()];
    match (mode, window) {
        (Mode::Screen, _) | (Mode::Window, None) => v.push("full".into()),
        (Mode::Window, Some(g)) => {
            v.push("window".into());
            v.extend(g.split(' ').map(String::from));
        }
        (Mode::Selection, _) => v.push("region".into()),
        (Mode::RecordScreen, _) => v.push("record".into()),
        (Mode::RecordSelection, _) => v.push("record-region".into()),
    }
    let dir = match (o.dest, mode.records()) {
        (Dest::Clipboard, false) => None,
        (Dest::Desktop, _) | (Dest::Clipboard, true) => Some(format!("{home}/Desktop")),
        (Dest::Pictures, false) => Some(format!("{home}/Pictures/Screenshots")),
        (Dest::Pictures, true) => Some(format!("{home}/Movies")),
    };
    match dir {
        Some(d) => v.extend(["--dir".into(), d]),
        None => v.push("--copy".into()),
    }
    if o.delay > 0 {
        v.extend(["--delay".into(), o.delay.to_string()]);
    }
    v
}

/// The capture button's title.
pub fn action(mode: Mode) -> &'static str {
    if mode.records() { "Record" } else { "Capture" }
}

// ---- Markup ----

/// Most strokes on one picture (undo walks back through them).
pub const MAX_STROKES: usize = 200;
/// Most points in one stroke (a long scribble keeps its first part).
pub const MAX_POINTS: usize = 4000;
/// Pen width in picture pixels at 1× (scaled with the picture so a 4K shot is not hairlines).
pub const PEN_WIDTH: f64 = 4.0;

/// Pen colours (the system palette's red, yellow, blue).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    Red,
    Yellow,
    Blue,
}

impl Ink {
    pub const ALL: [Ink; 3] = [Ink::Red, Ink::Yellow, Ink::Blue];
    pub fn label(self) -> &'static str {
        match self {
            Ink::Red => "Red",
            Ink::Yellow => "Yellow",
            Ink::Blue => "Blue",
        }
    }
    /// sRGB 0..1.
    pub fn rgb(self) -> (f64, f64, f64) {
        match self {
            Ink::Red => (1.0, 0.231, 0.188),
            Ink::Yellow => (1.0, 0.8, 0.0),
            Ink::Blue => (0.0, 0.478, 1.0),
        }
    }
}

/// One pen stroke, in picture pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub ink: Ink,
    pub width: f64,
    pub points: Vec<(f64, f64)>,
}

/// The strokes drawn over one picture of `size` pixels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Markup {
    pub size: (f64, f64),
    pub strokes: Vec<Stroke>,
}

impl Markup {
    pub fn new(width: f64, height: f64) -> Self {
        Markup {
            size: (width.max(1.0), height.max(1.0)),
            strokes: Vec::new(),
        }
    }

    /// Pen width for this picture: [`PEN_WIDTH`] per 1000 px of its longer side, at least 2.
    pub fn pen(&self) -> f64 {
        (PEN_WIDTH * self.size.0.max(self.size.1) / 1000.0).max(2.0)
    }

    fn clamp(&self, p: (f64, f64)) -> (f64, f64) {
        let ok = |v: f64, max: f64| {
            if v.is_finite() {
                v.clamp(0.0, max)
            } else {
                0.0
            }
        };
        (ok(p.0, self.size.0), ok(p.1, self.size.1))
    }

    /// Starts a stroke at `at` (picture pixels). False when [`MAX_STROKES`] is reached.
    pub fn begin(&mut self, ink: Ink, at: (f64, f64)) -> bool {
        if self.strokes.len() >= MAX_STROKES {
            return false;
        }
        let at = self.clamp(at);
        let width = self.pen();
        self.strokes.push(Stroke {
            ink,
            width,
            points: vec![at],
        });
        true
    }

    /// Continues the last stroke to `at`. False when there is none or it is full.
    pub fn extend(&mut self, at: (f64, f64)) -> bool {
        let at = self.clamp(at);
        match self.strokes.last_mut() {
            Some(s) if s.points.len() < MAX_POINTS => {
                s.points.push(at);
                true
            }
            _ => false,
        }
    }

    /// Takes the last stroke away.
    pub fn undo(&mut self) -> bool {
        self.strokes.pop().is_some()
    }

    pub fn is_empty(&self) -> bool {
        self.strokes.is_empty()
    }

    /// How the picture fits an area of `area` pixels, centred and never enlarged:
    /// (scale, x offset, y offset). Widget point -> picture point is `(p - offset) / scale`.
    pub fn fit(&self, area: (f64, f64)) -> (f64, f64, f64) {
        let s = (area.0 / self.size.0).min(area.1 / self.size.1).min(1.0);
        let s = if s.is_finite() && s > 0.0 { s } else { 1.0 };
        (
            s,
            (area.0 - self.size.0 * s) / 2.0,
            (area.1 - self.size.1 * s) / 2.0,
        )
    }

    /// A point in an area of `area` pixels -> picture pixels.
    pub fn to_picture(&self, area: (f64, f64), p: (f64, f64)) -> (f64, f64) {
        let (s, x, y) = self.fit(area);
        ((p.0 - x) / s, (p.1 - y) / s)
    }
}

/// File name for a marked-up copy next to the original (`Shot.png` -> `Shot (Markup).png`).
pub fn markup_path(original: &std::path::Path) -> std::path::PathBuf {
    let stem = original
        .file_stem()
        .map_or_else(|| "Screenshot".into(), |s| s.to_string_lossy().into_owned());
    original.with_file_name(format!("{stem} (Markup).png"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// QA-LIVE1-45: the notice a toolbar capture leaves names its folder and carries the picture.
    #[test]
    fn saved_notice_names_folder() {
        let p = std::path::Path::new("/home/alice/Desktop/Screenshot 2026-11-04 at 09.00.00.png");
        let a = saved_notice_argv(p);
        assert_eq!(a[0], "notify-send");
        assert_eq!(a.last().unwrap(), "Screenshot saved to Desktop");
        assert!(
            a.contains(&format!("string:image-path:{}", p.display())),
            "{a:?}"
        );
        let pics = std::path::Path::new("/home/alice/Pictures/Screenshots/x.png");
        assert_eq!(
            saved_notice_argv(pics).last().unwrap(),
            "Screenshot saved to Screenshots"
        );
    }

    /// Live-1 H4: a running recording had no way to stop from the UI. The shell follows the
    /// script's own pid file and stops through the script's own `stop`.
    #[test]
    fn record_tile_toggles_stop() {
        let script = include_str!("../../../desktop/overlay/usr/bin/wol-screenshot");
        assert!(
            script.contains(&format!("pidf=${{run:+\"$run/{RECORD_PID}\"}}")),
            "the pid file the shell watches is the one the script writes"
        );
        assert!(script.contains("|stop)"), "the script has `stop`");
        assert_eq!(STOP_ARGV, ["wol-screenshot", "stop"]);
        assert_eq!(
            record_pid_file("/run/user/1000"),
            std::path::Path::new("/run/user/1000/wol-record.pid")
        );
        use crate::protocol::Verb;
        assert_eq!(record_ask(true, Verb::Show, false), RecordAsk::Stop);
        assert_eq!(record_ask(true, Verb::Toggle, false), RecordAsk::Stop);
        assert_eq!(
            record_ask(true, Verb::Hide, true),
            RecordAsk::Toolbar(false)
        );
        assert_eq!(
            record_ask(false, Verb::Show, false),
            RecordAsk::Toolbar(true)
        );
        assert_eq!(
            record_ask(false, Verb::Toggle, true),
            RecordAsk::Toolbar(false)
        );
    }

    /// Screenshots land on the Desktop, as on a Mac and as ⇧⌘3 does (wol-screenshot).
    #[test]
    fn screenshots_go_to_the_desktop_by_default() {
        assert_eq!(Options::default().dest, Dest::Desktop);
    }

    #[test]
    fn argv_per_mode() {
        let pics = Options {
            dest: Dest::Pictures,
            delay: 0,
        };
        let desk = Options {
            dest: Dest::Desktop,
            delay: 5,
        };
        for (mode, o, win, want) in [
            (
                Mode::Screen,
                pics,
                None,
                "wol-screenshot full --dir /h/Pictures/Screenshots",
            ),
            (
                Mode::Selection,
                desk,
                None,
                "wol-screenshot region --dir /h/Desktop --delay 5",
            ),
            (
                Mode::Window,
                pics,
                Some("10,20 300x200"),
                "wol-screenshot window 10,20 300x200 --dir /h/Pictures/Screenshots",
            ),
            (
                Mode::Window,
                pics,
                None,
                "wol-screenshot full --dir /h/Pictures/Screenshots",
            ),
            (
                Mode::RecordScreen,
                pics,
                None,
                "wol-screenshot record --dir /h/Movies",
            ),
            (
                Mode::RecordSelection,
                desk,
                None,
                "wol-screenshot record-region --dir /h/Desktop --delay 5",
            ),
        ] {
            assert_eq!(argv(mode, o, win, "/h").join(" "), want, "{mode:?}");
        }
    }

    /// Live on the compositor: the shot contained the toolbar (captured before the unmap was drawn).
    #[test]
    fn capture_waits_for_a_frame_without_the_toolbar() {
        assert!(SETTLE >= std::time::Duration::from_millis(2 * 1000 / 30));
    }

    #[test]
    fn clipboard_only_copies() {
        let clip = Options {
            dest: Dest::Clipboard,
            delay: 0,
        };
        assert_eq!(
            argv(Mode::Selection, clip, None, "/h").join(" "),
            "wol-screenshot region --copy"
        );
        // A recording cannot live on the clipboard: it goes to the Desktop.
        assert_eq!(
            argv(Mode::RecordScreen, clip, None, "/h").join(" "),
            "wol-screenshot record --dir /h/Desktop"
        );
    }

    #[test]
    fn markup_strokes_are_bounded_and_in_picture_pixels() {
        let mut m = Markup::new(2000.0, 1000.0);
        assert_eq!(m.pen(), 8.0);
        assert!(!m.extend((1.0, 1.0)), "no stroke yet");
        assert!(m.begin(Ink::Red, (-5.0, 5000.0)));
        assert_eq!(
            m.strokes[0].points[0],
            (0.0, 1000.0),
            "clamped to the picture"
        );
        assert!(m.extend((f64::NAN, 10.0)));
        assert_eq!(m.strokes[0].points[1], (0.0, 10.0));
        for i in 0..MAX_POINTS + 10 {
            m.extend((i as f64, 1.0));
        }
        assert_eq!(m.strokes[0].points.len(), MAX_POINTS);
        assert!(m.undo() && !m.undo() && m.is_empty());
        for _ in 0..MAX_STROKES {
            assert!(m.begin(Ink::Blue, (1.0, 1.0)));
        }
        assert!(!m.begin(Ink::Blue, (1.0, 1.0)));
        assert_eq!(Markup::new(10.0, 10.0).pen(), 2.0);
    }

    #[test]
    fn markup_fits_the_window_and_maps_points_back() {
        let m = Markup::new(2000.0, 1000.0);
        let (s, x, y) = m.fit((1000.0, 1000.0));
        assert_eq!((s, x, y), (0.5, 0.0, 250.0));
        assert_eq!(
            m.to_picture((1000.0, 1000.0), (500.0, 500.0)),
            (1000.0, 500.0)
        );
        // Never enlarged: a small picture sits centred at 1×.
        let small = Markup::new(100.0, 100.0);
        assert_eq!(small.fit((300.0, 200.0)), (1.0, 100.0, 50.0));
        assert_eq!(Markup::new(0.0, 0.0).fit((0.0, 0.0)).0, 1.0);
        assert_eq!(
            markup_path(std::path::Path::new("/d/Screenshot 1.png")),
            std::path::PathBuf::from("/d/Screenshot 1 (Markup).png")
        );
    }

    #[test]
    fn action_titles() {
        assert_eq!(action(Mode::Screen), "Capture");
        assert_eq!(action(Mode::RecordSelection), "Record");
    }
}
