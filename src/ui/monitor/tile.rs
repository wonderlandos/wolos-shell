// SPDX-License-Identifier: GPL-2.0-or-later
//! A graph card: a section with the current value, a legend in the series' tones and the live
//! [`Graph`] under them. Overview, Disks and Network are made of these.
use crate::kit::data::Tone;
use crate::kit::graph::{GRAPH_TILE, Graph};
use crate::kit::widgets as w;
use gtk::prelude::*;

pub struct Tile {
    pub root: gtk::Box,
    pub value: gtk::Label,
    /// One label per series, in its tone (the legend), or one plain line.
    pub legend: Vec<gtk::Label>,
    /// Shown instead of the graph when there is nothing to draw (no sensor, no load reading).
    pub note: gtk::Label,
    pub graph: Graph,
}

impl Tile {
    /// `tones`: one per series (and legend entry); `max`: fixed scale (percent), else automatic.
    pub fn new(title: &str, tones: &[Tone], max: Option<f64>, height: i32) -> Tile {
        let body = w::vbox(w::S2);
        body.add_css_class("graph-tile");
        let head = w::hbox(w::S3);
        let value = w::label("", "heading");
        head.append(&value);
        head.append(&w::spacer());
        let legend: Vec<gtk::Label> = tones
            .iter()
            .map(|t| {
                let l = w::label("", "secondary");
                if tones.len() > 1 {
                    l.add_css_class(crate::kit::data::tone_class(*t));
                }
                head.append(&l);
                l
            })
            .collect();
        body.append(&head);
        let graph = Graph::new(tones, super::model::HISTORY, max, height);
        body.append(&graph.root);
        let note = w::label("", "secondary");
        note.set_wrap(true);
        note.set_xalign(0.0);
        note.set_visible(false);
        body.append(&note);
        let root = w::section(title, &[body.upcast()]);
        root.set_width_request(GRAPH_TILE);
        root.set_hexpand(true);
        Tile {
            root,
            value,
            legend,
            note,
            graph,
        }
    }

    /// The graph, or `note` in its place.
    pub fn show_note(&self, note: Option<&str>) {
        self.graph.root.set_visible(note.is_none());
        self.note.set_visible(note.is_some());
        self.note.set_text(note.unwrap_or(""));
    }

    pub fn set_legend(&self, texts: &[String]) {
        for (l, t) in self.legend.iter().zip(texts) {
            l.set_text(t);
        }
    }
}
