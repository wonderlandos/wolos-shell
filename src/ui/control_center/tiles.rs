// SPDX-License-Identifier: GPL-2.0-or-later
//! Control Center tile grid: the small square tiles (Battery, Screen Mirroring, Keyboard
//! Brightness, Night Shift, Screenshot, Record, the accessibility shortcuts) and the tiles other
//! parts of the shell add through [`TileSource`] (ADR 0031 `[[tile]]`: the plugin host fills it,
//! toggles and modules drive their state). Invariant: the grid lays out only the visible tiles,
//! left to right in a fixed order, so a hidden module leaves no hole and every tile keeps the
//! same size.
use crate::kit::{controls::ToggleTile, widgets as w};
use crate::ui::settings::Touch;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::time::Instant;
use wol_core::toggles::Toggle;

/// One tile another part of the shell adds (a plugin `[[tile]]`, a WolOS toggle, a module).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tile {
    /// Unique among the source's tiles (`<plugin id>.<tile id>` for plugins).
    pub id: String,
    /// Symbolic icon name.
    pub icon: String,
    /// Caption under the icon, up to two lines.
    pub label: String,
    /// Lit (the toggle behind it is on).
    pub on: bool,
}

/// Where extra tiles come from. Read on every Control Center refresh while it is visible, so
/// [`TileSource::tiles`] must answer from memory (no file or socket reads on the main thread).
pub trait TileSource {
    /// The tiles to show now, in order.
    fn tiles(&self) -> Vec<Tile>;
    /// The tile `id` was clicked; `on` is the state the person asked for.
    fn toggle(&self, id: &str, on: bool);
}

/// Desktop toggles with a control of their own already: Night Light is the Night Shift tile,
/// Silence the Focus card (the same Do Not Disturb file).
const ELSEWHERE: [Toggle; 2] = [Toggle::NightLight, Toggle::Silence];

/// The desktop toggles (`wol_core::toggles`, W2-E) that get a tile of their own: Stay Awake.
pub(super) fn toggle_tiles() -> Vec<Toggle> {
    Toggle::ALL
        .into_iter()
        .filter(|t| !ELSEWHERE.contains(t))
        .collect()
}

/// `wol toggle <id>`: flips `t` (the CLI runs its commands and moves its state file).
pub(super) fn toggle_argv(t: Toggle) -> Vec<String> {
    ["wol", "toggle", t.id()].map(String::from).to_vec()
}

/// One `wol toggle` at a time per tile: a second click while the first runs would read the old
/// state and flip it back (QA-SET-53: a double click on Stay Awake ended where it started).
/// True when this click may start one, and it is marked running.
pub(super) fn begin_flip(running: &std::cell::Cell<bool>) -> bool {
    !running.replace(true)
}

/// Is `t` on now: one file check (no read), cheap enough for every refresh.
pub(super) fn toggle_on(t: Toggle) -> bool {
    let config = std::path::PathBuf::from(crate::kit::config_home());
    wol_core::toggles::is_on(t, &config, &wol_core::journal::state_home(), |p| p.exists())
}

/// Columns for a panel `width` px wide: four, three on a narrow panel.
pub(super) fn columns(width: i32) -> i32 {
    if width >= NARROW_BELOW { 4 } else { 3 }
}

/// Panel widths under this lay the grid out in three columns.
pub const NARROW_BELOW: i32 = 300;

/// Cell (column, row) of the `i`-th visible tile.
pub(super) fn cell(i: usize, columns: i32) -> (i32, i32) {
    let c = usize::try_from(columns.max(1)).unwrap_or(1);
    let at = |n: usize| i32::try_from(n).unwrap_or(i32::MAX);
    (at(i % c), at(i / c))
}

/// The grid; re-laid only when the visible set or the column count changed.
pub(super) struct Grid {
    pub root: gtk::Grid,
    laid: RefCell<Vec<gtk::Widget>>,
    columns: Cell<i32>,
}

impl Grid {
    pub fn new() -> Self {
        let root = gtk::Grid::builder()
            .row_spacing(w::S2)
            .column_spacing(w::S2)
            .column_homogeneous(true)
            .row_homogeneous(true)
            .build();
        Grid {
            root,
            laid: Default::default(),
            columns: Cell::new(4),
        }
    }

    /// Lays out `tiles` (the visible ones, in order) in `columns` columns; hides an empty grid.
    pub fn set(&self, tiles: Vec<gtk::Widget>, columns: i32) {
        self.root.set_visible(!tiles.is_empty());
        if *self.laid.borrow() == tiles && self.columns.get() == columns {
            return;
        }
        while let Some(c) = self.root.first_child() {
            self.root.remove(&c);
        }
        for (i, t) in tiles.iter().enumerate() {
            let (x, y) = cell(i, columns);
            t.set_visible(true);
            self.root.attach(t, x, y, 1, 1);
        }
        *self.laid.borrow_mut() = tiles;
        self.columns.set(columns);
    }
}

/// Extra tiles built from a [`TileSource`]; rebuilt only when what it lists changed.
#[derive(Default)]
pub(super) struct Extra {
    shown: RefCell<Vec<Tile>>,
    buttons: RefCell<Vec<gtk::Widget>>,
}

impl Extra {
    /// The buttons for `tiles`; a click calls `toggle(id, !on)`.
    pub fn buttons(
        &self,
        tiles: Vec<Tile>,
        toggle: impl Fn(&str, bool) + Clone + 'static,
    ) -> Vec<gtk::Widget> {
        if *self.shown.borrow() != tiles {
            let b: Vec<gtk::Widget> = tiles
                .iter()
                .map(|t| {
                    let tile = ToggleTile::new(&t.icon, &t.label);
                    tile.set_on(t.on);
                    let (id, on, f) = (t.id.clone(), t.on, toggle.clone());
                    tile.button.connect_clicked(move |_| f(&id, !on));
                    tile.button.upcast()
                })
                .collect();
            *self.buttons.borrow_mut() = b;
            *self.shown.borrow_mut() = tiles;
        }
        self.buttons.borrow().clone()
    }
}

/// Tiles the person just flipped (QA hunt 1 G2): the 1 s poll pushed the snapshot taken before
/// the change landed, so a tile flipped back, then forward once nmcli or bluetoothctl answered.
/// While a change is out, and [`Touch::hold`] after it succeeds (the service's echo), every view
/// of that state (tile, detail page switch) shows what was asked; a failure lets go at once, so
/// the next refresh shows the host's state again.
#[derive(Default)]
pub(crate) struct Holds(RefCell<std::collections::HashMap<&'static str, Hold>>);

struct Hold {
    want: bool,
    /// Changes sent and not answered yet.
    flying: u32,
    /// When the last one succeeded (the echo window starts).
    settled: Option<Instant>,
}

impl Holds {
    /// The person asked for `want` on `key`; a change is on its way.
    pub(crate) fn touch(&self, key: &'static str, want: bool) {
        let mut h = self.0.borrow_mut();
        let e = h.entry(key).or_insert(Hold {
            want,
            flying: 0,
            settled: None,
        });
        e.want = want;
        e.flying += 1;
    }
    /// A change on `key` answered: on success the tile waits for the echo, else it lets go.
    pub(crate) fn answered(&self, key: &'static str, ok: bool, now: Instant) {
        let mut h = self.0.borrow_mut();
        let Some(e) = h.get_mut(key) else { return };
        e.flying = e.flying.saturating_sub(1);
        e.settled = ok.then_some(now);
        if !ok && e.flying == 0 {
            h.remove(key);
        }
    }
    /// What a view of `key` shows: the asked state while held, else `actual`.
    pub(crate) fn shown(&self, key: &'static str, actual: bool) -> bool {
        self.shown_at(key, actual, Instant::now())
    }
    pub(crate) fn shown_at(&self, key: &'static str, actual: bool, now: Instant) -> bool {
        let mut h = self.0.borrow_mut();
        let Some(e) = h.get(key) else { return actual };
        let held = e.flying > 0
            || e.settled
                .is_some_and(|t| now.saturating_duration_since(t) < Touch::hold(0));
        if held {
            e.want
        } else {
            h.remove(key);
            actual
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each toggle shows once: Night Light and Silence already have their control; the others
    /// get a tile that runs the CLI's `wol toggle`, a word the CLI accepts.
    /// QA-SET-53: the second click of a double click starts nothing while the first flip runs;
    /// once it ended the next click flips again.
    #[test]
    fn a_double_click_flips_once() {
        let running = std::cell::Cell::new(false);
        assert!(begin_flip(&running));
        assert!(!begin_flip(&running), "second click while the first runs");
        running.set(false);
        assert!(begin_flip(&running));
    }

    #[test]
    fn every_toggle_has_one_control() {
        let t = toggle_tiles();
        assert_eq!(t, [Toggle::StayAwake]);
        assert_eq!(toggle_argv(Toggle::StayAwake), super::super::STAY_AWAKE);
        for t in Toggle::ALL {
            assert_ne!(t.label(), "");
            let argv = toggle_argv(t);
            assert_eq!(argv[..2], ["wol", "toggle"]);
            assert!(
                wol_core::toggles::words().contains(&argv[2].as_str()),
                "{argv:?}"
            );
        }
    }

    #[test]
    fn a_narrow_panel_has_three_columns() {
        assert_eq!(columns(crate::kit::widgets::CONTROL_CENTER_WIDTH), 4);
        assert_eq!(columns(NARROW_BELOW), 4);
        assert_eq!(columns(NARROW_BELOW - 1), 3);
    }

    /// Visible tiles fill rows left to right: no hole where a hidden module was.
    #[test]
    fn tiles_fill_rows_left_to_right() {
        let four: Vec<_> = (0..6).map(|i| cell(i, 4)).collect();
        assert_eq!(four, [(0, 0), (1, 0), (2, 0), (3, 0), (0, 1), (1, 1)]);
        assert_eq!(cell(4, 3), (1, 1));
        assert_eq!(cell(2, 0), (0, 2), "no columns lays one per row");
    }

    /// QA hunt 1 G2: a tapped tile keeps what was asked through every poll while its change is
    /// out and for the echo after it succeeds; a failure shows the host's state on the next one.
    #[test]
    fn tile_hold_survives_poll() {
        let h = Holds::default();
        let t0 = Instant::now();
        assert!(!h.shown_at("wifi", false, t0), "untouched: the snapshot");
        h.touch("wifi", true);
        for s in 1..=5 {
            let poll = t0 + std::time::Duration::from_secs(s);
            assert!(h.shown_at("wifi", false, poll), "poll {s} rewrote the tile");
        }
        assert!(
            !h.shown_at("bluetooth", false, t0),
            "other tiles follow the poll"
        );
        let answer = t0 + std::time::Duration::from_secs(5);
        h.answered("wifi", true, answer);
        assert!(
            h.shown_at("wifi", false, answer + Touch::hold(0) / 2),
            "echo"
        );
        assert!(
            !h.shown_at("wifi", false, answer + Touch::hold(0)),
            "let go"
        );
        assert!(h.shown_at("wifi", true, answer + Touch::hold(0)));
        // Two taps on, then off: the last ask shows until both answered; a failure lets go.
        h.touch("night", true);
        h.touch("night", false);
        h.answered("night", false, t0);
        assert!(
            !h.shown_at("night", true, t0),
            "one still out: the last ask"
        );
        h.answered("night", false, t0);
        assert!(h.shown_at("night", true, t0), "failed: the host's state");
    }
}
