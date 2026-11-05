// SPDX-License-Identifier: GPL-2.0-or-later
//! Date and time pickers of the WolOS Design System (ADR 0058 draft). The month arithmetic is
//! `crate::widgets::calendar` (weeks from Monday, the calendar widget's); the month grid look is
//! [`super::data::month_grid`]'s, with days a person can pick. Specs:
//! `design/components/{date-picker,time-picker}.toml`; tests: `tests/kit_calendar.rs`.
use super::widgets as w;
use crate::widgets::calendar::{MONTHS, WEEKDAYS, weeks};
use gtk::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

/// A month of days to pick one from: the month and year with previous and next buttons, the
/// weekday letters, then the days; today has a ring, the picked day the accent disc. Arrows move
/// between days (the grid's focus), Space or Enter picks.
pub struct DatePicker {
    pub root: gtk::Box,
    pub title: gtk::Label,
    pub days: Rc<std::cell::RefCell<Vec<gtk::ToggleButton>>>,
    /// The days (a grid for assistive technology).
    pub grid: gtk::Grid,
    shown: Cell<(i32, u32)>,
    picked: Cell<Option<(i32, u32, u8)>>,
    today: (i32, u32, u8),
    on_pick: Rc<dyn Fn(i32, u32, u8)>,
    /// A range picker: two clicks pick the first and the last day, the days between are tinted.
    ranged: bool,
    span: Cell<Option<(Date, Date)>>,
    /// The first click of a range that waits for its last.
    anchor: Cell<Option<Date>>,
    on_range: Rc<dyn Fn(Date, Date)>,
    cells: Rc<std::cell::RefCell<Vec<(Date, gtk::ToggleButton)>>>,
}

/// A day as (year, month 1..=12, day).
pub type Date = (i32, u32, u8);

impl DatePicker {
    /// Shows `month` (1..=12) of `year`; `today` is marked; `picked` hears the day a person picks.
    pub fn new(
        (year, month): (i32, u32),
        selected: Option<u8>,
        today: (i32, u32, u8),
        (prev, next): (&str, &str),
        picked: impl Fn(i32, u32, u8) + 'static,
    ) -> Rc<DatePicker> {
        Self::build(
            (year, month),
            selected.map(|d| (year, month, d)),
            None,
            today,
            (prev, next),
            Rc::new(picked),
            Rc::new(|_, _| {}),
        )
    }

    /// A range of days: the first click picks where it starts, the second where it ends (in
    /// either order); `ranged` hears the finished range. A third click starts a new one.
    pub fn range(
        (year, month): (i32, u32),
        span: Option<(Date, Date)>,
        today: Date,
        (prev, next): (&str, &str),
        ranged: impl Fn(Date, Date) + 'static,
    ) -> Rc<DatePicker> {
        Self::build(
            (year, month),
            None,
            Some(span),
            today,
            (prev, next),
            Rc::new(|_, _, _| {}),
            Rc::new(ranged),
        )
    }

    fn build(
        (year, month): (i32, u32),
        selected: Option<Date>,
        span: Option<Option<(Date, Date)>>,
        today: Date,
        (prev, next): (&str, &str),
        picked: Rc<dyn Fn(i32, u32, u8)>,
        on_range: Rc<dyn Fn(Date, Date)>,
    ) -> Rc<DatePicker> {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(w::S2)
            .accessible_role(gtk::AccessibleRole::Group)
            .build();
        root.add_css_class("date-picker");
        let head = w::hbox(w::S2);
        head.add_css_class("date-picker-head");
        let title = w::label("", "title");
        title.set_hexpand(true);
        head.append(&title);
        root.update_relation(&[gtk::accessible::Relation::LabelledBy(&[title.upcast_ref()])]);
        let back = w::icon_button("go-previous-symbolic", prev, || {});
        let fwd = w::icon_button("go-next-symbolic", next, || {});
        head.append(&back);
        head.append(&fwd);
        root.append(&head);
        let grid = gtk::Grid::builder()
            .column_homogeneous(true)
            .row_spacing(w::S1)
            .column_spacing(w::S1)
            .accessible_role(gtk::AccessibleRole::Grid)
            .build();
        grid.add_css_class("date-grid");
        grid.update_relation(&[gtk::accessible::Relation::LabelledBy(&[title.upcast_ref()])]);
        root.append(&grid);
        let p = Rc::new(DatePicker {
            root,
            title,
            days: Rc::default(),
            grid,
            shown: Cell::new((year, month)),
            picked: Cell::new(selected),
            today,
            on_pick: picked,
            ranged: span.is_some(),
            span: Cell::new(span.flatten()),
            anchor: Cell::new(None),
            on_range,
            cells: Rc::default(),
        });
        p.fill();
        for (b, by) in [(&back, -1i32), (&fwd, 1)] {
            let me = Rc::downgrade(&p);
            b.connect_clicked(move |_| {
                if let Some(p) = me.upgrade() {
                    p.step(by)
                }
            });
        }
        p
    }

    /// Moves the shown month by `by` (one back or forward).
    pub fn step(self: &Rc<Self>, by: i32) {
        let (y, m) = self.shown.get();
        let i = y * 12 + i32::try_from(m).unwrap_or(1) - 1 + by;
        self.shown.set((
            i.div_euclid(12),
            u32::try_from(i.rem_euclid(12) + 1).unwrap_or(1),
        ));
        self.fill();
    }

    /// The picked date, if any.
    pub fn picked(&self) -> Option<(i32, u32, u8)> {
        self.picked.get()
    }

    fn fill(self: &Rc<Self>) {
        while let Some(c) = self.grid.first_child() {
            self.grid.remove(&c);
        }
        let (y, m) = self.shown.get();
        self.title.set_label(&format!("{} {y}", month_name(m)));
        for (i, d) in WEEKDAYS.iter().enumerate() {
            let l = w::centered(&d[..1], "caption");
            l.add_css_class("tone-dim");
            l.update_property(&[gtk::accessible::Property::Label(d)]);
            self.grid.attach(&l, cell(i), 0, 1, 1);
        }
        let mut days = Vec::new();
        let mut cells = Vec::new();
        let mut first: Option<gtk::ToggleButton> = None;
        for (r, week) in weeks(y, m).iter().enumerate() {
            for (c, day) in week.iter().enumerate() {
                let Some(day) = *day else { continue };
                let b = gtk::ToggleButton::with_label(&day.to_string());
                b.add_css_class("day");
                b.set_halign(gtk::Align::Center);
                if self.ranged {
                    // Not a radio group: the two ends and the days between are all lit.
                } else if let Some(f) = &first {
                    b.set_group(Some(f));
                } else {
                    first = Some(b.clone());
                }
                if self.today == (y, m, day) {
                    b.add_css_class("today");
                }
                if self.picked.get() == Some((y, m, day)) {
                    b.set_active(true);
                }
                b.update_property(&[gtk::accessible::Property::Label(&format!(
                    "{} {day} {} {y}",
                    WEEKDAYS[c],
                    month_name(m)
                ))]);
                let me = Rc::downgrade(self);
                if self.ranged {
                    b.connect_clicked(move |_| {
                        if let Some(p) = me.upgrade() {
                            p.click_range((y, m, day))
                        }
                    });
                } else {
                    b.connect_toggled(move |b| {
                        let Some(p) = me.upgrade() else { return };
                        if b.is_active() {
                            p.picked.set(Some((y, m, day)));
                            (p.on_pick)(y, m, day);
                        }
                    });
                }
                self.grid.attach(&b, cell(c), cell(r) + 1, 1, 1);
                cells.push(((y, m, day), b.clone()));
                days.push(b);
            }
        }
        *self.days.borrow_mut() = days;
        *self.cells.borrow_mut() = cells;
        if self.ranged {
            self.paint();
        }
    }

    /// The range, if one is picked: first day, last day.
    pub fn span(&self) -> Option<(Date, Date)> {
        self.span.get()
    }

    fn click_range(self: &Rc<Self>, day: Date) {
        match self.anchor.take() {
            None => {
                self.anchor.set(Some(day));
                self.span.set(Some((day, day)));
            }
            Some(first) => {
                let (a, b) = if first <= day {
                    (first, day)
                } else {
                    (day, first)
                };
                self.span.set(Some((a, b)));
                (self.on_range)(a, b);
            }
        }
        self.paint();
    }

    /// Lights the two ends and tints the days between them.
    fn paint(&self) {
        let span = self.span.get();
        for (date, b) in self.cells.borrow().iter() {
            let end = span.is_some_and(|(a, z)| *date == a || *date == z);
            let between = span.is_some_and(|(a, z)| a < *date && *date < z);
            b.set_active(end);
            if between {
                b.add_css_class("in-range");
            } else {
                b.remove_css_class("in-range");
            }
        }
    }
}

/// An hour and a minute, each a spin button of two digits (24-hour clock, the minutes wrap), with
/// the colon between them. `changed` hears every change as (hour, minute).
pub fn time_picker(
    (hour, minute): (u8, u8),
    (hour_name, minute_name): (&str, &str),
    changed: impl Fn(u8, u8) + 'static,
) -> gtk::Box {
    let b = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(w::S1)
        .accessible_role(gtk::AccessibleRole::Group)
        .build();
    b.add_css_class("time-picker");
    b.set_halign(gtk::Align::Start);
    let spin = |v: u8, max: f64, name: &str| {
        let s = gtk::SpinButton::with_range(0.0, max, 1.0);
        s.set_value(f64::from(v));
        s.set_wrap(true);
        s.set_digits(0);
        s.set_numeric(true);
        s.set_width_chars(2);
        s.set_valign(gtk::Align::Center);
        s.add_css_class("time-part");
        s.update_property(&[gtk::accessible::Property::Label(name)]);
        // Two digits always: 09, not 9.
        s.connect_output(|s| {
            s.set_text(&format!("{:02}", s.value_as_int()));
            gtk::glib::Propagation::Stop
        });
        s
    };
    let h = spin(hour.min(23), 23.0, hour_name);
    let m = spin(minute.min(59), 59.0, minute_name);
    b.append(&h);
    b.append(&w::label(":", "time-colon"));
    b.append(&m);
    let changed = Rc::new(changed);
    for s in [&h, &m] {
        let (h2, m2, c) = (h.downgrade(), m.downgrade(), changed.clone());
        s.connect_value_changed(move |_| {
            if let (Some(h), Some(m)) = (h2.upgrade(), m2.upgrade()) {
                c(part(&h), part(&m))
            }
        });
    }
    b
}

/// The month's name, January for 1; empty outside 1..=12.
fn month_name(m: u32) -> &'static str {
    usize::try_from(m)
        .ok()
        .and_then(|m| m.checked_sub(1))
        .and_then(|i| MONTHS.get(i))
        .copied()
        .unwrap_or("")
}

/// A grid row or column (a week has seven, a month six).
fn cell(i: usize) -> i32 {
    i32::try_from(i).unwrap_or(0)
}

/// A spin button's whole value as an hour or a minute (both fit a byte).
fn part(s: &gtk::SpinButton) -> u8 {
    u8::try_from(s.value_as_int()).unwrap_or(0)
}
