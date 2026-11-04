// SPDX-License-Identifier: GPL-2.0-or-later
//! Unit tests of the desktop widgets (ui/widgets.rs), split out of the parent file (`#[cfg(test)] mod tests;`).
use super::edit_after;
use super::{gallery_margins, mark_presence};
use crate::protocol::parse;

/// QA-100 H17-01: the empty desktop draws a pixel (alpha 1 of 255), so GTK has a frame to attach.
#[test]
fn the_empty_desktop_paints_a_pixel_for_the_buffer() {
    let mut s = gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, 4, 4).unwrap();
    let blank = {
        let cr = gtk::cairo::Context::new(&s).unwrap();
        drop(cr);
        s.flush();
        s.data().unwrap().iter().all(|b| *b == 0)
    };
    assert!(blank, "a fresh surface is transparent");
    mark_presence(&gtk::cairo::Context::new(&s).unwrap());
    s.flush();
    let d = s.data().unwrap();
    let alphas: Vec<u8> = d.chunks(4).map(|px| px[3]).collect();
    assert_eq!(alphas[0], 1, "the first pixel is at alpha 1/255");
    assert_eq!(
        alphas.iter().filter(|a| **a != 0).count(),
        1,
        "and only that pixel"
    );
}

#[test]
fn the_gallery_never_covers_a_card() {
    assert_eq!(gallery_margins([(24, 368)].into_iter(), 1920, 24), (392, 0));
    assert_eq!(
        gallery_margins([(1500, 1896)].into_iter(), 1920, 24),
        (0, 444)
    );
    assert_eq!(gallery_margins(std::iter::empty(), 1920, 24), (0, 0));
}

/// QA hunt F1: Esc ends Edit Widgets even while Settings has the keyboard. The gallery is a
/// `wol-panel-*` layer, so while it is mapped wol-hypr forwards every Esc as `hide transient`
/// (`wm_gates::SwitchKeys`, never consumed), and that command runs `transient::also_close`,
/// which ui/mod.rs points at `Board::set_edit(false)`. Its own Esc handler covers the case
/// where the gallery has the keyboard (an on-demand layer gets it when it maps).
#[test]
fn esc_reaches_the_gallery_without_its_keyboard() {
    use crate::kit::surface::Spec;
    assert!(Spec::widgets_gallery().namespace.starts_with("wol-panel-"));
    let esc = crate::protocol::parse("hide transient").unwrap();
    assert!(super::super::transient::closes_all(&esc));
}

// QA bug 5: `wol shell hide widgets` is Done (the gallery's button and Esc call `set_edit(false)`
// too); other targets are not the board's.

#[test]
fn hide_widgets_is_done() {
    let c = |l: &str| parse(l).expect(l);
    assert_eq!(edit_after(&c("hide widgets"), true), Some(false));
    assert_eq!(edit_after(&c("hide widgets"), false), Some(false));
    assert_eq!(edit_after(&c("show widgets"), false), Some(true));
    assert_eq!(edit_after(&c("toggle widgets"), true), Some(false));
    assert_eq!(edit_after(&c("hide transient"), true), None);
}
