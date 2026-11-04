// SPDX-License-Identifier: GPL-2.0-or-later
//! Live Graph (System Monitor, docs/modules/monitor.md): up to a few series over a fixed window
//! of samples, newest at the right edge, on a faint grid. Each series is its own drawing layer
//! carrying a tone class, and the grid takes `--line` (kit.css `.graph-grid`), so every colour
//! comes from the theme and follows light, dark and the accent live; nothing here names a colour.
//! The graph only draws what it is given ([`Graph::set`]): no reading, no timers.
use super::data::{Tone, tone_class};
use super::widgets::S1;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Height of an Overview graph and of a page's detail graph.
pub const GRAPH_M: i32 = 64;
pub const GRAPH_L: i32 = 120;
/// Narrowest a graph card gets: two side by side from a 1024 px window, one in a narrow one.
pub const GRAPH_TILE: i32 = super::widgets::CONTROL_CENTER_WIDTH;
/// Shortest the process list gets (it fills the window; this many rows show at least).
pub const LIST_MIN: i32 = 2 * GRAPH_L;
/// Width of a space meter at the end of a disk row.
pub const METER: i32 = super::widgets::SIDEBAR_WIDTH / 2;
/// Horizontal grid lines (quarters).
const GRID_ROWS: u32 = 4;
/// Auto-scaled graphs leave this much head room over the highest value.
const HEADROOM: f64 = 1.15;

pub struct Graph {
    pub root: gtk::Overlay,
    series: Vec<(gtk::DrawingArea, Rc<RefCell<Vec<f64>>>)>,
    /// Shared top of the scale: fixed (`max`), else the highest value of all series.
    top: Rc<Cell<f64>>,
    max: Option<f64>,
}

fn source(cr: &gtk::cairo::Context, c: &gtk::gdk::RGBA, alpha: f64) {
    cr.set_source_rgba(
        f64::from(c.red()),
        f64::from(c.green()),
        f64::from(c.blue()),
        f64::from(c.alpha()) * alpha,
    );
}

/// x of value `i` of `n` in a window of `cap` across `wd`: the newest sits on the right edge.
pub fn x_of(i: usize, n: usize, cap: usize, wd: f64) -> f64 {
    let step = wd / cap.saturating_sub(1).max(1) as f64;
    wd - (n.saturating_sub(1).saturating_sub(i)) as f64 * step
}

/// Top of the scale for `series`: `max` when fixed, else the highest value with head room
/// (never 0, so an idle graph draws a flat line on the baseline).
pub fn top_of(series: &[Vec<f64>], max: Option<f64>) -> f64 {
    max.unwrap_or_else(|| series.iter().flatten().copied().fold(0.0, f64::max) * HEADROOM)
        .max(f64::EPSILON)
}

impl Graph {
    /// One series per tone; `cap` samples wide; `max` fixes the scale (percent: 100).
    pub fn new(tones: &[Tone], cap: usize, max: Option<f64>, height: i32) -> Graph {
        let grid = gtk::DrawingArea::builder()
            .content_height(height)
            .hexpand(true)
            .build();
        grid.add_css_class("graph-grid");
        grid.set_draw_func(|a, cr, wd, ht| {
            let c = a.color();
            let (wd, ht) = (f64::from(wd), f64::from(ht));
            source(cr, &c, 1.0);
            for r in 0..=GRID_ROWS {
                let y = (ht - 1.0) * f64::from(r) / f64::from(GRID_ROWS);
                cr.rectangle(0.0, y.round(), wd, 1.0);
            }
            cr.fill().ok();
        });
        let root = gtk::Overlay::builder().child(&grid).hexpand(true).build();
        let top = Rc::new(Cell::new(max.unwrap_or(1.0)));
        let series = tones
            .iter()
            .map(|t| {
                let a = gtk::DrawingArea::builder().hexpand(true).build();
                a.add_css_class("graph-series");
                a.add_css_class(tone_class(*t));
                let values = Rc::new(RefCell::new(Vec::<f64>::new()));
                let (v, top) = (values.clone(), top.clone());
                a.set_draw_func(move |a, cr, wd, ht| {
                    let v = v.borrow();
                    if v.is_empty() {
                        return;
                    }
                    let c = a.color();
                    let (wd, ht) = (f64::from(wd), f64::from(ht));
                    let top = top.get();
                    let y = |x: f64| (ht - 1.0) - (x.clamp(0.0, top) / top) * (ht - f64::from(S1));
                    let pts: Vec<(f64, f64)> = v
                        .iter()
                        .enumerate()
                        .map(|(i, x)| (x_of(i, v.len(), cap, wd), y(*x)))
                        .collect();
                    let line = |cr: &gtk::cairo::Context| {
                        cr.move_to(pts[0].0, pts[0].1);
                        for p in &pts[1..] {
                            cr.line_to(p.0, p.1);
                        }
                    };
                    line(cr);
                    cr.line_to(wd, ht);
                    cr.line_to(pts[0].0, ht);
                    cr.close_path();
                    let g = gtk::cairo::LinearGradient::new(0.0, 0.0, 0.0, ht);
                    let (r, gr, b) = (
                        f64::from(c.red()),
                        f64::from(c.green()),
                        f64::from(c.blue()),
                    );
                    g.add_color_stop_rgba(0.0, r, gr, b, 0.24);
                    g.add_color_stop_rgba(1.0, r, gr, b, 0.0);
                    cr.set_source(&g).ok();
                    cr.fill().ok();
                    source(cr, &c, 1.0);
                    cr.set_line_width(1.5);
                    cr.set_line_join(gtk::cairo::LineJoin::Round);
                    cr.set_line_cap(gtk::cairo::LineCap::Round);
                    line(cr);
                    cr.stroke().ok();
                });
                root.add_overlay(&a);
                (a, values)
            })
            .collect();
        Graph {
            root,
            series,
            top,
            max,
        }
    }

    /// New values, one list per series (oldest first); redraws only what changed.
    pub fn set(&self, values: &[Vec<f64>]) {
        let top = top_of(values, self.max);
        let rescaled = (top - self.top.get()).abs() > f64::EPSILON;
        self.top.set(top);
        for ((a, v), new) in self.series.iter().zip(values) {
            if rescaled || *v.borrow() != *new {
                v.borrow_mut().clone_from(new);
                a.queue_draw();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_sample_sits_on_the_right_edge() {
        assert_eq!(x_of(59, 60, 60, 590.0), 590.0);
        assert_eq!(x_of(0, 60, 60, 590.0), 0.0);
        // Two samples of a 60-wide window: the right end, one step apart.
        assert_eq!(x_of(1, 2, 60, 590.0), 590.0);
        assert_eq!(x_of(0, 2, 60, 590.0), 580.0);
    }

    #[test]
    fn the_scale_is_fixed_or_follows_the_highest_value() {
        assert_eq!(top_of(&[vec![10.0]], Some(100.0)), 100.0);
        assert!((top_of(&[vec![1.0, 2.0], vec![4.0]], None) - 4.6).abs() < 1e-9);
        assert!(
            top_of(&[vec![0.0]], None) > 0.0,
            "idle: a flat line, no division by 0"
        );
    }
}
