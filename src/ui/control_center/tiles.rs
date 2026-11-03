// SPDX-License-Identifier: GPL-2.0-or-later
//! Control Center tile grid: the small square tiles (Battery, Screen Mirroring, Keyboard
//! Brightness, Night Shift, Screenshot, Record, the accessibility shortcuts) and the tiles other
//! parts of the shell add through [`TileSource`] (ADR 0031 `[[tile]]`: the plugin host fills it,
//! toggles and modules drive their state). Invariant: the grid lays out only the visible tiles,
//! left to right in a fixed order, so a hidden module leaves no hole and every tile keeps the
//! same size.
use crate::kit::{controls::ToggleTile, widgets as w};
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Each toggle shows once: Night Light and Silence already have their control; the others
    /// get a tile that runs the CLI's `wol toggle`, a word the CLI accepts.
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
}
