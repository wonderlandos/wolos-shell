// SPDX-License-Identifier: GPL-2.0-or-later
//! The Search bar's meaning providers (docs/modules/search.md): one index over what the bar
//! finds by name (apps, system actions, Settings pages) plus every Settings row and the manual's
//! sections, ranked by cosine fused with a word score; "deixar o texto maior" finds Larger text,
//! "tirar uma captura da tela" finds the Screenshots page of the manual. The bar keeps its word
//! and prefix hits first ([`crate::search::search`]); what this finds follows them, and when
//! nothing matches at all it offers the nearest title as "Did you mean". On the device, no network.
//! Invariant: no table installed (a host without `/usr/share/wolos/manual`) means no change at all.

use std::sync::{Arc, Mutex};

use wolsem::{Index, ItemIn, Opts, Procrustes, Table};

use crate::search::Item;

/// Kind shown on a manual section.
pub const MANUAL: &str = "Manual";
/// Kind shown on the row that offers the nearest title when nothing matched.
pub const DID_YOU_MEAN: &str = "Did you mean";
/// Where a manual section is read: the site (the image has no local copy of the pages yet).
pub const MANUAL_BASE: &str = "https://wolos.org";
/// Manual rows in one list at most: a page has many sections and the list is eight rows.
pub const MANUAL_ROWS: usize = 2;
/// Meaning rows after word hits at most, and the cosine each needs then.
const AFTER_WORDS: usize = 2;
const COS_AFTER_WORDS: f32 = 0.5;
/// A manual section needs this cosine (the manual is dense: every page talks of settings and apps).
const COS_MANUAL: f32 = 0.28;
/// A title this near in meaning is offered as "Did you mean".
const DID_YOU_MEAN_COS: f32 = 0.3;

/// What a meaning hit opens.
pub struct Semantic {
    index: Index,
    items: Vec<Item>,
}

/// The shipped table, the index over the bar's own items and the manual's index (the shipped one).
pub struct Providers<'a> {
    pub sem: Semantic,
    pub installed: &'a wolsem::Installed,
}

static GLOBAL: Mutex<Option<Arc<Providers<'static>>>> = Mutex::new(None);

/// The providers, if they were built (see [`build_for`]).
pub fn get() -> Option<Arc<Providers<'static>>> {
    GLOBAL.lock().ok().and_then(|g| g.clone())
}

/// Builds the providers for `items` (the bar's index) and keeps them for [`get`]; slow (tens of
/// milliseconds): call it off the interface thread. Nothing is kept where no table is installed.
pub fn build_for(items: &[Item]) {
    let Some(installed) = wolsem::installed() else {
        return;
    };
    let p = Arc::new(Providers {
        sem: Semantic::build(&installed.table, items),
        installed,
    });
    if let Ok(mut g) = GLOBAL.lock() {
        *g = Some(p);
    }
}

/// The item of a manual section: opens its page at the section in the browser.
pub fn manual_item(id: &str, title: &str) -> Item {
    let browser = crate::apps::here().iter().find(|p| p.name == "Browser");
    let exec = browser.map_or("helium", |p| p.exec[0]);
    Item::new(
        title,
        MANUAL,
        "help-contents-symbolic",
        vec![exec.into(), format!("{MANUAL_BASE}{id}")],
    )
}

impl Semantic {
    pub fn build(table: &Table, base: &[Item]) -> Semantic {
        let mut items: Vec<Item> = Vec::new();
        let mut parts: Vec<(String, String, String)> = Vec::new();
        for it in base {
            parts.push((
                format!("{}/{}", it.kind, it.title),
                it.title.clone(),
                it.words.trim().to_string(),
            ));
            items.push(it.clone());
        }
        for (page, row, text) in crate::ui::settings::meaning_rows() {
            let title = format!("{row} \u{b7} {page}");
            parts.push((
                format!("{}/{page}/{row}", crate::ui::searchbar::SETTINGS),
                row.to_string(),
                text,
            ));
            items.push(Item::new(
                &title,
                crate::ui::searchbar::SETTINGS,
                "wolos-settings",
                vec!["wol-settings".into(), page.into()],
            ));
        }
        let ins: Vec<ItemIn> = parts
            .iter()
            .map(|(id, t, x)| ItemIn {
                id,
                title: t,
                text: x,
            })
            .collect();
        Semantic {
            index: Index::build(table, &ins, wolsem::DIMS),
            items,
        }
    }

    /// Items by meaning for `query`, best first, with their cosines.
    pub fn find(
        &self,
        table: &Table,
        query: &str,
        map: Option<&Procrustes>,
        k: usize,
    ) -> Vec<(Item, f32)> {
        let o = Opts {
            k,
            ..Opts::default()
        };
        self.index
            .search(Some(table), query, map, &o)
            .into_iter()
            .filter_map(|h| self.items.get(h.index).map(|i| (i.clone(), h.cos)))
            .collect()
    }
}

impl Providers<'_> {
    /// Rows after the word hits: Settings rows and apps by meaning, then manual sections
    /// ([`MANUAL_ROWS`] at most), fused by reciprocal rank; `have_words` hits already answer the
    /// query, so then only the near ones are added.
    pub fn extra(&self, query: &str, have_words: bool, want: usize) -> Vec<Item> {
        let t = &self.installed.table;
        let local = self.sem.find(t, query, None, 8);
        let ix = &self.installed.manual_index;
        let c = &self.installed.manual;
        let manual: Vec<(Item, f32)> = ix
            .search(
                Some(t),
                query,
                None,
                &Opts {
                    k: MANUAL_ROWS * 3,
                    ..Opts::default()
                },
            )
            .into_iter()
            .map(|h| (manual_item(&c.ids[h.index], &c.titles[h.index]), h.cos))
            .collect();
        let a: Vec<usize> = (0..local.len()).collect();
        let b: Vec<usize> = (local.len()..local.len() + manual.len()).collect();
        let all: Vec<&(Item, f32)> = local.iter().chain(manual.iter()).collect();
        let (mut out, mut from_manual) = (Vec::new(), 0);
        for (i, _) in wolsem::rrf(&[&a, &b], 60.0) {
            let (item, cos) = all[i];
            if have_words && *cos < COS_AFTER_WORDS {
                continue;
            }
            if item.kind == MANUAL {
                if *cos < COS_MANUAL || from_manual >= MANUAL_ROWS {
                    continue;
                }
                from_manual += 1;
            }
            if out
                .iter()
                .any(|o: &Item| o.title == item.title && o.kind == item.kind)
            {
                continue;
            }
            out.push(item.clone());
            if out.len() >= want.min(if have_words { AFTER_WORDS } else { want }) {
                break;
            }
        }
        out
    }

    /// For a query that found nothing: the nearest title, as a "Did you mean" row that opens it.
    /// A misspelt word is first put right by the words of the titles ("pasword" -> "password").
    pub fn did_you_mean(&self, items: &[Item], query: &str) -> Option<Item> {
        let t = &self.installed.table;
        let query = wolsem::capped(query);
        let fixed = self.sem.index.did_you_mean(query);
        if let Some(q) = &fixed
            && let Some(hit) = crate::search::search(items, q, 1).first()
        {
            return Some(offer(hit));
        }
        let near = self.sem.find(t, fixed.as_deref().unwrap_or(query), None, 1);
        let (item, cos) = near.into_iter().next()?;
        (cos >= DID_YOU_MEAN_COS).then(|| offer(&item))
    }
}

/// `item` as a "Did you mean" row: same title, icon and action, a kind that says why it is there.
fn offer(item: &Item) -> Item {
    Item {
        kind: DID_YOU_MEAN,
        ..item.clone()
    }
}
