// SPDX-License-Identifier: GPL-2.0-or-later
//! Overview: processor, memory, compressed memory, disk and network activity, graphics and
//! temperature, each a live graph of the last minute.
use super::model::{self, Model};
use super::tile::Tile;
use crate::copy::monitor as copy;
use crate::kit::data::Tone;
use crate::kit::graph::GRAPH_M;
use crate::kit::widgets as w;
use gtk::prelude::*;

pub struct Page {
    pub root: gtk::Box,
    grid: gtk::FlowBox,
    /// Until the first sample (HIG: never a blank pane).
    wait: gtk::Spinner,
    cpu: Tile,
    memory: Tile,
    swap: Tile,
    disk: Tile,
    net: Tile,
    gpu: Tile,
    temp: Tile,
}

const PAIR: [Tone; 2] = [Tone::Accent, Tone::Success];

impl Page {
    pub fn new() -> Page {
        let root = w::page(copy::OVERVIEW);
        let grid = w::wrap_row(w::S4, 2);
        grid.set_halign(gtk::Align::Fill);
        grid.set_valign(gtk::Align::Start);
        let tile = |title, tones: &[Tone], max| {
            let t = Tile::new(title, tones, max, GRAPH_M);
            grid.insert(&t.root, -1);
            t
        };
        let p = Page {
            cpu: tile(copy::CPU, &[Tone::Accent], Some(100.0)),
            memory: tile(copy::MEMORY, &[Tone::Accent], Some(100.0)),
            swap: tile(copy::SWAP, &[Tone::Accent], Some(100.0)),
            disk: tile(copy::DISK, &PAIR, None),
            net: tile(copy::NET, &PAIR, None),
            gpu: tile(copy::GPU, &[Tone::Accent], Some(100.0)),
            temp: tile(copy::TEMPERATURE, &[Tone::Warning], None),
            wait: w::spinner(),
            grid,
            root,
        };
        p.root.append(&p.grid);
        p.root.append(&p.wait);
        p
    }

    pub fn update(&self, m: &Model) {
        let first = m.history.cpu.is_empty();
        self.wait.set_visible(first);
        self.grid.set_visible(!first);
        if first {
            return;
        }
        let (s, h) = (&m.last, &m.history);
        let cores = s.cpu.len().saturating_sub(1);
        self.cpu.value.set_text(&model::percent(h.cpu.last()));
        self.cpu
            .set_legend(&[copy::cores(cores, s.load.map(|l| l.one))]);
        self.cpu.graph.set(&[h.cpu.values()]);

        let mem = &s.memory;
        self.memory
            .value
            .set_text(&model::bytes(u64::try_from(mem.used).unwrap_or(0)));
        self.memory.set_legend(&[copy::of_total(&model::bytes(
            u64::try_from(mem.total).unwrap_or(0),
        ))]);
        self.memory.graph.set(&[h.memory.values()]);

        self.swap
            .value
            .set_text(&model::bytes(u64::try_from(mem.swap_used).unwrap_or(0)));
        self.swap.set_legend(&[copy::of_total(&model::bytes(
            u64::try_from(mem.swap_total).unwrap_or(0),
        ))]);
        self.swap.graph.set(&[h.swap.values()]);

        pair(&self.disk, &h.disk, copy::READ, copy::WRITTEN);
        pair(&self.net, &h.net, copy::RECEIVED, copy::SENT);

        let busy = s.gpus.iter().find_map(|g| g.busy);
        self.gpu
            .value
            .set_text(&busy.map_or(String::new(), model::percent));
        let name = s.gpus.first().map_or("", |g| g.driver.as_str());
        self.gpu.set_legend(&[name.to_owned()]);
        self.gpu.show_note(match (s.gpus.is_empty(), busy) {
            (true, _) => Some(copy::NO_GPU),
            (false, None) => Some(copy::GPU_NO_LOAD),
            _ => None,
        });
        self.gpu.graph.set(&[h.gpu.values()]);

        let hot = wol_core::monitor::temps::hottest(&s.temps);
        self.temp
            .value
            .set_text(&hot.map_or(String::new(), |t| model::celsius(t.celsius)));
        self.temp
            .set_legend(&[hot.map_or(String::new(), |t| t.name.clone())]);
        self.temp.show_note(hot.is_none().then_some(copy::NO_TEMPS));
        self.temp.graph.set(&[h.temp.values()]);
    }
}

impl Default for Page {
    fn default() -> Self {
        Self::new()
    }
}

/// A two-series tile: total rate as the value, each series' rate in its tone.
pub fn pair(t: &Tile, p: &model::Pair, a: &str, b: &str) {
    t.value.set_text(&model::rate(p.0.last() + p.1.last()));
    t.set_legend(&[
        format!("{a} {}", model::rate(p.0.last())),
        format!("{b} {}", model::rate(p.1.last())),
    ]);
    t.graph.set(&[p.0.values(), p.1.values()]);
}
