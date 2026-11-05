// SPDX-License-Identifier: GPL-2.0-or-later
//! Choice controls of the WolOS Design System (ADR 0058 draft): checkbox, radio group, chips
//! (assist, filter, input, suggestion), the discrete slider and the range slider. Each one is a
//! spec in `design/components/<id>.toml` and is tested alone by `tests/kit_choice.rs`; the look is
//! `kit.css` ("Choice controls"), never the caller's.
use super::widgets as w;
use gtk::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

/// A checkbox with its label (the label is its accessible name). `changed` hears every flip a
/// person makes; a mixed box (some of a group on) is [`set_mixed`].
pub fn checkbox(label: &str, on: bool, changed: impl Fn(bool) + 'static) -> gtk::CheckButton {
    let c = gtk::CheckButton::with_label(label);
    c.add_css_class("checkbox");
    c.set_active(on);
    c.set_valign(gtk::Align::Center);
    c.connect_toggled(move |c| changed(c.is_active()));
    c
}

/// Shows the box as mixed (a group of which some are on); a click makes it on.
pub fn set_mixed(c: &gtk::CheckButton, mixed: bool) {
    c.set_inconsistent(mixed);
}

/// Exclusive options, one per line, the picked one with a dot. The group is a radio group for
/// assistive technology and is named by `title` (a row title or a section heading) when given.
pub fn radio_group(
    title: Option<&gtk::Label>,
    options: &[&str],
    selected: usize,
    picked: impl Fn(usize) + 'static,
) -> gtk::Box {
    let g = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(w::S1)
        .accessible_role(gtk::AccessibleRole::RadioGroup)
        .build();
    g.add_css_class("radio-group");
    if let Some(t) = title {
        g.update_relation(&[gtk::accessible::Relation::LabelledBy(&[t.upcast_ref()])]);
    }
    let picked = Rc::new(picked);
    let mut first: Option<gtk::CheckButton> = None;
    for (i, o) in options.iter().enumerate() {
        let r = gtk::CheckButton::with_label(o);
        r.add_css_class("radio");
        if let Some(f) = &first {
            r.set_group(Some(f));
        } else {
            first = Some(r.clone());
        }
        r.set_active(i == selected);
        let p = picked.clone();
        r.connect_toggled(move |r| {
            if r.is_active() {
                p(i)
            }
        });
        g.append(&r);
    }
    g
}

/// The four chips: compact elements that start an action, filter a view, stand for something a
/// person entered, or offer a suggestion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChipKind {
    /// Starts an action related to the content ("Add to Calendar"); may carry a leading glyph.
    Assist,
    /// Narrows the view; on shows a check in front of the label.
    Filter,
    /// Something the person entered (a recipient, a tag), with a remove button.
    Input,
    /// A ready-made answer or query the person can pick.
    Suggestion,
}

fn chip_class(kind: ChipKind) -> &'static str {
    match kind {
        ChipKind::Assist => "chip-assist",
        ChipKind::Filter => "chip-filter",
        ChipKind::Input => "chip-input",
        ChipKind::Suggestion => "chip-suggestion",
    }
}

fn chip_face(icon_name: &str, label: &str) -> (gtk::Box, gtk::Image) {
    let b = w::hbox(w::S1);
    let i = w::icon(
        if icon_name.is_empty() {
            "object-select-symbolic"
        } else {
            icon_name
        },
        w::ICON,
    );
    i.set_visible(!icon_name.is_empty());
    b.append(&i);
    b.append(&w::label(label, ""));
    (b, i)
}

/// An assist or suggestion chip: a button with a short label (and, for assist, an optional 16 px
/// glyph in front).
pub fn chip(
    kind: ChipKind,
    icon_name: &str,
    label: &str,
    clicked: impl Fn() + 'static,
) -> gtk::Button {
    let (face, _) = chip_face(
        if kind == ChipKind::Assist {
            icon_name
        } else {
            ""
        },
        label,
    );
    let b = gtk::Button::builder()
        .child(&face)
        .valign(gtk::Align::Center)
        .build();
    b.add_css_class("chip");
    b.add_css_class(chip_class(kind));
    b.connect_clicked(move |_| clicked());
    b
}

/// A filter chip: on and off like a toggle button; on shows a check in front of the label.
pub fn filter_chip(label: &str, on: bool, changed: impl Fn(bool) + 'static) -> gtk::ToggleButton {
    let (face, check) = chip_face("object-select-symbolic", label);
    check.set_visible(on);
    let b = gtk::ToggleButton::builder()
        .child(&face)
        .active(on)
        .valign(gtk::Align::Center)
        .build();
    b.add_css_class("chip");
    b.add_css_class(chip_class(ChipKind::Filter));
    b.connect_toggled(move |b| {
        check.set_visible(b.is_active());
        changed(b.is_active())
    });
    b
}

/// An input chip: the entered thing and a remove button (`remove` is its tooltip and name,
/// "Remove Ana"). `removed` runs after the chip has left its parent.
pub fn input_chip(label: &str, remove: &str, removed: impl Fn() + 'static) -> gtk::Box {
    let b = w::hbox(w::S1);
    b.add_css_class("chip");
    b.add_css_class(chip_class(ChipKind::Input));
    b.set_valign(gtk::Align::Center);
    b.append(&w::label(label, ""));
    let me = b.downgrade();
    let x = w::icon_button("window-close-symbolic", remove, move || {
        if let Some(c) = me.upgrade()
            && let Some(p) = c.parent().and_downcast::<gtk::Box>()
        {
            p.remove(&c);
        }
        removed();
    });
    x.add_css_class("chip-remove");
    b.append(&x);
    b
}

/// A line of chips, 8 px apart.
pub fn chip_set(chips: &[gtk::Widget]) -> gtk::Box {
    let b = w::hbox(w::S2);
    b.add_css_class("chip-set");
    b.set_halign(gtk::Align::Start);
    for c in chips {
        b.append(c);
    }
    b
}

/// A slider that stops on whole steps (`step` apart from `min` to `max`), with a tick at each
/// stop: for a value that only makes sense in steps (a level 1 to 5, a size).
pub fn discrete_slider(
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    changed: impl Fn(f64) + 'static,
) -> gtk::Scale {
    let s = gtk::Scale::with_range(gtk::Orientation::Horizontal, min, max, step);
    s.add_css_class("slider");
    // The knob stays inside its trough (kit.css), in a list row or not.
    s.add_css_class("row-slider");
    s.add_css_class("discrete");
    s.set_draw_value(false);
    s.set_hexpand(true);
    s.set_round_digits(0);
    let mut v = min;
    while v <= max + f64::EPSILON {
        s.add_mark(v, gtk::PositionType::Bottom, None);
        v += step;
    }
    s.set_value(value);
    let last = Rc::new(Cell::new(f64::NAN));
    s.connect_value_changed(move |s| {
        // Snap to the nearest stop; a drag between stops tells nobody.
        let snapped = min + ((s.value() - min) / step).round() * step;
        if (s.value() - snapped).abs() > f64::EPSILON {
            s.set_value(snapped);
            return;
        }
        if last.replace(snapped) != snapped {
            changed(snapped);
        }
    });
    s
}

/// A range: two knobs on one track, the part between them in the accent. Each knob is a slider of
/// its own for the keyboard and for assistive technology (named by `low_name` and `high_name`,
/// "Lowest price"), and neither can pass the other.
pub struct RangeSlider {
    pub root: gtk::Overlay,
    pub low: gtk::Scale,
    pub high: gtk::Scale,
}

impl RangeSlider {
    pub fn new(
        (min, max): (f64, f64),
        (lo, hi): (f64, f64),
        (low_name, high_name): (&str, &str),
        changed: impl Fn(f64, f64) + 'static,
    ) -> RangeSlider {
        let scale = |v: f64, name: &str, class: &str| {
            let s =
                gtk::Scale::with_range(gtk::Orientation::Horizontal, min, max, (max - min) / 100.0);
            s.add_css_class("slider");
            s.add_css_class("row-slider");
            s.add_css_class("range-knob");
            s.add_css_class(class);
            s.set_draw_value(false);
            s.set_hexpand(true);
            s.set_value(v);
            s.update_property(&[gtk::accessible::Property::Label(name)]);
            s
        };
        let low = scale(lo.min(hi), low_name, "range-low");
        let high = scale(hi.max(lo), high_name, "range-high");
        let track = gtk::DrawingArea::new();
        track.add_css_class("range-track");
        track.set_hexpand(true);
        track.set_content_height(w::S4);
        let root = gtk::Overlay::builder().child(&track).build();
        root.add_css_class("range-slider");
        for k in [&low, &high] {
            k.set_valign(gtk::Align::Center);
            root.add_overlay(k);
            root.set_measure_overlay(k, true);
        }
        // The track between the knobs, in the accent; the trough is drawn by the scales' CSS.
        {
            let (l, h) = (low.downgrade(), high.downgrade());
            track.set_draw_func(move |a, cr, wd, ht| {
                let (Some(l), Some(h)) = (l.upgrade(), h.upgrade()) else {
                    return;
                };
                let knob = 16.0;
                let span = (wd as f64 - knob).max(1.0);
                let at = |v: f64| knob / 2.0 + (v - min) / (max - min) * span;
                // The whole track is the accent at a quarter, the range the accent itself (the
                // scales draw only their knobs: their troughs are clear, kit.css).
                let c = a.color();
                let (r, g, b) = (c.red().into(), c.green().into(), c.blue().into());
                let y = ht as f64 / 2.0 - 2.0;
                cr.set_source_rgba(r, g, b, 0.24);
                cr.rectangle(knob / 2.0, y, span, 4.0);
                let _ = cr.fill();
                cr.set_source_rgba(r, g, b, c.alpha().into());
                cr.rectangle(
                    at(l.value()),
                    y,
                    (at(h.value()) - at(l.value())).max(0.0),
                    4.0,
                );
                let _ = cr.fill();
            });
        }
        let changed = Rc::new(changed);
        let busy = Rc::new(Cell::new(false));
        let keep = |me: &gtk::Scale, other: &gtk::Scale, below: bool| {
            let (o, t, c, busy) = (
                other.downgrade(),
                track.downgrade(),
                changed.clone(),
                busy.clone(),
            );
            let me2 = me.downgrade();
            me.connect_value_changed(move |s| {
                let (Some(o), Some(me)) = (o.upgrade(), me2.upgrade()) else {
                    return;
                };
                if busy.get() {
                    return;
                }
                // Neither knob passes the other.
                let v = if below {
                    s.value().min(o.value())
                } else {
                    s.value().max(o.value())
                };
                if (v - s.value()).abs() > f64::EPSILON {
                    busy.set(true);
                    me.set_value(v);
                    busy.set(false);
                }
                if let Some(t) = t.upgrade() {
                    t.queue_draw();
                }
                let (lo, hi) = if below {
                    (v, o.value())
                } else {
                    (o.value(), v)
                };
                c(lo, hi);
            });
        };
        keep(&low, &high, true);
        keep(&high, &low, false);
        // The pointer drives the nearer knob: only that scale takes it (both cover the track).
        let motion = gtk::EventControllerMotion::new();
        motion.set_propagation_phase(gtk::PropagationPhase::Capture);
        {
            let (l, h) = (low.downgrade(), high.downgrade());
            let pick = move |x: f64| {
                let (Some(l), Some(h)) = (l.upgrade(), h.upgrade()) else {
                    return;
                };
                let wd = f64::from(l.width().max(1));
                let at = |v: f64| (v - min) / (max - min) * wd;
                let near_low = (x - at(l.value())).abs() <= (x - at(h.value())).abs()
                    && !(l.value() >= h.value() && x > at(h.value()));
                h.set_can_target(!near_low);
                l.set_can_target(true);
            };
            let p2 = pick.clone();
            motion.connect_enter(move |_, x, _| pick(x));
            motion.connect_motion(move |_, x, _| p2(x));
        }
        root.add_controller(motion);
        RangeSlider { root, low, high }
    }

    /// The two values, low first.
    pub fn values(&self) -> (f64, f64) {
        (self.low.value(), self.high.value())
    }
}
