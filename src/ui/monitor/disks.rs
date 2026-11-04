// SPDX-License-Identifier: GPL-2.0-or-later
//! Disks: space per mounted disk, and read and write activity per physical disk.
use super::model::{self, Model};
use super::tile::Tile;
use crate::copy::monitor as copy;
use crate::kit::data::{self, Tone};
use crate::kit::graph::{GRAPH_L, METER};
use crate::kit::widgets as w;
use gtk::prelude::*;
use std::cell::RefCell;

pub struct Page {
    pub root: gtk::Box,
    space: gtk::Box,
    activity: gtk::Box,
    empty: gtk::Box,
    /// One tile per disk, by name; rebuilt when disks come or go.
    tiles: RefCell<Vec<(String, Tile)>>,
    mounts: RefCell<Vec<String>>,
}

impl Page {
    pub fn new() -> Page {
        let root = w::page(copy::DISKS);
        let space = w::vbox(0);
        root.append(&space);
        let activity = w::vbox(w::S6);
        root.append(&activity);
        let empty = w::empty_state(
            "drive-harddisk-symbolic",
            copy::NO_DISKS.0,
            copy::NO_DISKS.1,
            None,
        );
        empty.set_visible(false);
        root.append(&empty);
        Page {
            root,
            space,
            activity,
            empty,
            tiles: RefCell::default(),
            mounts: RefCell::default(),
        }
    }

    pub fn update(&self, m: &Model) {
        if let Some(mounts) = &m.last.mounts {
            let key: Vec<String> = mounts
                .iter()
                .map(|d| format!("{} {} {}", d.mount, d.used, d.total))
                .collect();
            if *self.mounts.borrow() != key {
                while let Some(c) = self.space.first_child() {
                    self.space.remove(&c);
                }
                let rows: Vec<gtk::Widget> = mounts
                    .iter()
                    .map(|d| {
                        let (used, total) = (
                            u64::try_from(d.used).unwrap_or(0),
                            u64::try_from(d.total).unwrap_or(0),
                        );
                        let share = if total > 0 {
                            used as f64 / total as f64
                        } else {
                            0.0
                        };
                        let meter = data::meter(
                            share,
                            if share > 0.9 {
                                Tone::Warning
                            } else {
                                Tone::Accent
                            },
                        );
                        meter.set_hexpand(false);
                        meter.set_width_request(METER);
                        w::list_row(
                            "drive-harddisk-symbolic",
                            &copy::mount_name(&d.mount),
                            &copy::used_of(&model::bytes(used), &model::bytes(total)),
                            Some(meter.upcast_ref()),
                        )
                        .upcast()
                    })
                    .collect();
                if !rows.is_empty() {
                    self.space.append(&w::section(copy::DISK_SPACE, &rows));
                }
                *self.mounts.borrow_mut() = key;
            }
        }
        let names: Vec<String> = m.history.disks.keys().cloned().collect();
        let same = {
            let t = self.tiles.borrow();
            t.len() == names.len() && t.iter().zip(&names).all(|((n, _), k)| n == k)
        };
        if !same {
            while let Some(c) = self.activity.first_child() {
                self.activity.remove(&c);
            }
            let tiles: Vec<(String, Tile)> = names
                .iter()
                .map(|n| {
                    let t = Tile::new(n, &[Tone::Accent, Tone::Success], None, GRAPH_L);
                    self.activity.append(&t.root);
                    (n.clone(), t)
                })
                .collect();
            *self.tiles.borrow_mut() = tiles;
        }
        for (n, t) in self.tiles.borrow().iter() {
            if let Some(p) = m.history.disks.get(n) {
                super::overview::pair(t, p, copy::READ, copy::WRITTEN);
            }
        }
        let none = names.is_empty() && m.last.mounts.as_ref().is_none_or(Vec::is_empty);
        self.empty.set_visible(none);
    }
}

impl Default for Page {
    fn default() -> Self {
        Self::new()
    }
}
