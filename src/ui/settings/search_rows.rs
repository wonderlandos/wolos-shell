// SPDX-License-Identifier: GPL-2.0-or-later
//! Settings search by meaning (docs/modules/search.md): every pane, subpage and row is an item of
//! a [`wolsem::Index`] whose vectors come from its English title, its pane's words and the
//! sentence the pane shows under it, computed once; a question in any language (the table is
//! multilingual) meets those vectors, so "deixar o texto maior" finds Larger text although no row
//! is translated. [`super::search::hits`] puts what this finds after the word-prefix hits.
//! Invariant: an item's id is its English `Page/Row`, so a translated title cannot move it.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use wolsem::{Index, ItemIn, Opts, Procrustes, Table};

use super::search::{Hit, ROWS, data_rows, parent};
use crate::copy::settings as copy;

/// The sentence a pane shows under a row (read from the copy module, so the two cannot drift):
/// found by meaning, "stops a click when sound starts" finds Keep device awake.
pub(super) const DESCRIBED: [(&str, &str, &str); 14] = [
    ("Sound", "Allow louder", copy::SOUND_LOUDER),
    ("Sound", "Automatic input level", copy::SOUND_AUTO_LEVEL),
    ("Sound", "Reduce background noise", copy::SOUND_NOISE),
    (
        "Sound",
        "Use the headset microphone automatically",
        copy::SOUND_AUTOSWITCH,
    ),
    ("Sound", "Keep device awake", copy::SOUND_AWAKE),
    ("Sound", "Ignore hardware volume", copy::SOUND_SOFT_VOLUME),
    ("Sound", "Reset audio", copy::SOUND_RESET_ABOUT),
    ("Sound", "Pro Audio", copy::SOUND_PRO_NOTE),
    (
        "Software Update",
        "Download on metered connections",
        copy::AUTO_METERED_HINT,
    ),
    (
        "Privacy & Security",
        "Send error reports",
        copy::REPORTS_HINT,
    ),
    (
        "Privacy & Security",
        "Screen Recording",
        copy::PRIVACY_SCREEN_NOTE,
    ),
    (
        "Appearance",
        "Theme from your wallpaper",
        copy::THEME_FROM_WALLPAPER,
    ),
    ("Network", "Firewall", copy::FIREWALL_ON),
    ("Sharing", "Internet Sharing", copy::HOTSPOT_OFF),
];

/// What a row is found by besides its title: its page, the page's words ("cursor" for Pointer
/// size) and the sentence the pane shows under it.
fn row_text(page: &str, row: &str) -> String {
    let ctx = super::SIDEBAR
        .iter()
        .flat_map(|g| g.iter())
        .find(|p| p.title == parent(page))
        .map_or("", |p| p.words);
    let about = DESCRIBED
        .iter()
        .find(|d| d.0 == page && d.1 == row)
        .map_or("", |d| d.2);
    format!("{page}. {ctx}. {about}")
}

/// Every row Settings search knows with the text it is found by: `(page, row, text)`. The Search
/// bar's meaning index reads this too.
pub fn rows() -> Vec<(&'static str, &'static str, String)> {
    let all: Vec<(&'static str, Vec<&'static str>)> = ROWS
        .iter()
        .map(|(p, r)| (*p, r.to_vec()))
        .chain(data_rows())
        .collect();
    let mut v = Vec::new();
    for (page, titles) in all {
        for r in titles.into_iter().filter(|r| *r != page) {
            v.push((page, r, row_text(page, r)));
        }
    }
    v
}

/// The items and what each opens.
pub(super) struct Sem {
    index: Index,
    hits: Vec<Hit>,
}

/// A table with the index built on it and the map for the person's language, if any.
pub(super) struct Ctx<'a> {
    pub sem: &'a Sem,
    pub table: &'a Table,
    pub map: Option<&'a Procrustes>,
}

impl Sem {
    pub(super) fn new(table: &Table) -> Sem {
        let mut parts: Vec<(String, String, String, Hit)> = Vec::new();
        let words: Vec<(&'static str, &'static str)> = super::SIDEBAR
            .iter()
            .flat_map(|g| g.iter())
            .map(|p| (p.title, p.words))
            .collect();
        for (page, w) in words.iter().copied() {
            parts.push((
                page.to_string(),
                page.to_string(),
                w.to_string(),
                Hit { page, row: None },
            ));
        }
        for (page, row, text) in rows() {
            if !words.iter().any(|w| w.0 == page) && !parts.iter().any(|p| p.0 == page) {
                // A subpage: found by its title and the pane it is reached from.
                parts.push((
                    page.to_string(),
                    page.to_string(),
                    parent(page).to_string(),
                    Hit { page, row: None },
                ));
            }
            parts.push((
                format!("{page}/{row}"),
                row.to_string(),
                text,
                Hit {
                    page,
                    row: Some(row),
                },
            ));
        }
        let items: Vec<ItemIn> = parts
            .iter()
            .map(|(id, title, text, _)| ItemIn { id, title, text })
            .collect();
        let index = Index::build(table, &items, wolsem::DIMS);
        Sem {
            index,
            hits: parts.into_iter().map(|p| p.3).collect(),
        }
    }

    /// What `query` means, best first, with the cosine of each (0: found by words only).
    pub(super) fn find(&self, cx: &Ctx, query: &str) -> Vec<(Hit, f32)> {
        let o = Opts {
            k: 8,
            ..Opts::strict()
        };
        self.index
            .search(Some(cx.table), query, cx.map, &o)
            .into_iter()
            .filter_map(|h| self.hits.get(h.index).map(|x| (*x, h.cos)))
            .collect()
    }
}

static STARTED: AtomicBool = AtomicBool::new(false);
static GLOBAL: OnceLock<Option<(&'static Table, Sem)>> = OnceLock::new();

fn build_global() -> Option<(&'static Table, Sem)> {
    let t = &wolsem::installed()?.table;
    Some((t, Sem::new(t)))
}

/// Loads the table and builds the index off the interface thread, so the first keystroke does
/// not wait for it; until it is ready search is by words only.
pub(super) fn warm() {
    if GLOBAL.get().is_some() || STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("settings-search-warm".into())
        .spawn(|| {
            let _ = GLOBAL.get_or_init(build_global);
        });
}

/// The shared index, if it is ready.
pub(super) fn global() -> Option<(&'static Sem, &'static Table)> {
    match GLOBAL.get() {
        Some(Some((t, s))) => Some((s, t)),
        Some(None) => None,
        None => {
            warm();
            None
        }
    }
}
