// SPDX-License-Identifier: GPL-2.0-or-later
//! Network: one live graph per connection (received and sent), with its kind, whether it is
//! connected and what it moved since it came up.
use super::model::{self, Model};
use super::tile::Tile;
use crate::copy::monitor as copy;
use crate::kit::data::Tone;
use crate::kit::graph::GRAPH_L;
use crate::kit::widgets as w;
use gtk::prelude::*;
use std::cell::RefCell;

pub struct Page {
    pub root: gtk::Box,
    list: gtk::Box,
    empty: gtk::Box,
    tiles: RefCell<Vec<(String, Tile)>>,
}

impl Page {
    pub fn new() -> Page {
        let root = w::page(copy::NETWORK);
        let list = w::vbox(w::S6);
        root.append(&list);
        let empty = w::empty_state(
            "network-offline-symbolic",
            copy::NO_NETWORK.0,
            copy::NO_NETWORK.1,
            None,
        );
        empty.set_visible(false);
        root.append(&empty);
        Page {
            root,
            list,
            empty,
            tiles: RefCell::default(),
        }
    }

    pub fn update(&self, m: &Model) {
        let ifs = &m.last.net;
        let same = {
            let t = self.tiles.borrow();
            t.len() == ifs.len() && t.iter().zip(ifs).all(|((n, _), i)| *n == i.name)
        };
        if !same {
            while let Some(c) = self.list.first_child() {
                self.list.remove(&c);
            }
            let tiles = ifs
                .iter()
                .map(|i| {
                    let title = format!("{} · {}", copy::kind(i.kind), i.name);
                    let t = Tile::new(&title, &[Tone::Accent, Tone::Success], None, GRAPH_L);
                    self.list.append(&t.root);
                    (i.name.clone(), t)
                })
                .collect();
            *self.tiles.borrow_mut() = tiles;
        }
        for ((name, t), i) in self.tiles.borrow().iter().zip(ifs) {
            if let Some(p) = m.history.nets.get(name) {
                super::overview::pair(t, p, copy::RECEIVED, copy::SENT);
            }
            // Under the graph: connected or not, and the totals.
            t.note.set_visible(true);
            t.note.set_text(&format!(
                "{} · {}",
                if i.up {
                    copy::CONNECTED
                } else {
                    copy::NOT_CONNECTED
                },
                copy::totals(&model::bytes(i.rx_total), &model::bytes(i.tx_total))
            ));
        }
        self.empty.set_visible(ifs.is_empty());
    }
}

impl Default for Page {
    fn default() -> Self {
        Self::new()
    }
}
