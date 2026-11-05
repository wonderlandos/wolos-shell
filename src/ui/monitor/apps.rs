// SPDX-License-Identifier: GPL-2.0-or-later
//! Apps: one row per app scope with its memory (PSS), CPU, energy over the minute and what
//! Memory Saver is doing with it, and Quit (Force Quit for a paused app, which cannot answer).
use super::model::{self, AppRow, Model};
use crate::copy::monitor as copy;
use crate::kit::data::{self, Tone};
use crate::kit::form::ICON_ROW;
use crate::kit::widgets as w;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use wol_core::monitor::apps::Rest;

/// Quit: (the row, force).
pub type Quit = Rc<dyn Fn(&AppRow, bool)>;

struct Row {
    unit: String,
    name: gtk::Label,
    facts: gtk::Label,
    state: gtk::Label,
    quit: gtk::Button,
    /// What the Quit button acts on now (the row is updated in place every second).
    current: Rc<RefCell<Option<AppRow>>>,
}

pub struct Page {
    pub root: gtk::Box,
    list: gtk::Box,
    empty: gtk::Box,
    note: gtk::Label,
    rows: RefCell<Vec<Row>>,
    quit: Quit,
}

impl Page {
    pub fn new(quit: Quit) -> Page {
        let root = w::page(copy::APPS);
        let list = w::section("", &[]);
        root.append(&list);
        let note = w::footnote("");
        root.append(&note);
        let empty = w::empty_state(
            "view-app-grid-symbolic",
            copy::APPS_EMPTY.0,
            copy::APPS_EMPTY.1,
            None,
        );
        root.append(&empty);
        Page {
            root,
            list,
            empty,
            note,
            rows: RefCell::default(),
            quit,
        }
    }

    fn card(&self) -> Option<gtk::Box> {
        self.list.last_child().and_downcast()
    }

    fn row(&self, a: &AppRow) -> (gtk::Widget, Row) {
        let r = w::hbox(w::S3);
        r.add_css_class("list-row");
        r.append(&w::app_icon(&a.icon, ICON_ROW));
        let t = w::vbox(0);
        t.set_hexpand(true);
        t.set_valign(gtk::Align::Center);
        let name = w::label("", "");
        name.set_xalign(0.0);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        name.add_css_class("may-ellipsize");
        t.append(&name);
        let facts = w::row_subtitle("");
        facts.set_xalign(0.0);
        t.append(&facts);
        r.append(&t);
        let state = data::badge("", Tone::Accent);
        r.append(&state);
        let current: Rc<RefCell<Option<AppRow>>> = Rc::default();
        let quit = {
            let (q, c) = (self.quit.clone(), current.clone());
            w::button(copy::QUIT, w::Kind::Default, move || {
                if let Some(a) = c.borrow().as_ref() {
                    q(a, a.rest == Rest::Frozen);
                }
            })
        };
        quit.set_valign(gtk::Align::Center);
        r.append(&quit);
        (
            r.upcast(),
            Row {
                unit: a.unit.clone(),
                name,
                facts,
                state,
                quit,
                current,
            },
        )
    }

    pub fn update(&self, m: &Model) {
        let apps = m.app_rows();
        let Some(card) = self.card() else { return };
        let same = {
            let rows = self.rows.borrow();
            rows.len() == apps.len() && rows.iter().zip(&apps).all(|(r, a)| r.unit == a.unit)
        };
        if !same {
            while let Some(c) = card.first_child() {
                card.remove(&c);
            }
            let mut rows = Vec::with_capacity(apps.len());
            for a in &apps {
                let (wd, r) = self.row(a);
                card.append(&wd);
                rows.push(r);
            }
            *self.rows.borrow_mut() = rows;
        }
        for (r, a) in self.rows.borrow().iter().zip(&apps) {
            r.name.set_text(&a.name);
            r.facts.set_text(&copy::app_facts(
                &model::bytes(a.memory),
                &model::percent(a.cpu),
                a.energy,
            ));
            r.state.set_text(copy::rest(a.rest));
            r.state.set_visible(a.rest != Rest::Running);
            r.quit.set_label(if a.rest == Rest::Frozen {
                copy::FORCE_QUIT
            } else {
                copy::QUIT
            });
            *r.current.borrow_mut() = Some(a.clone());
        }
        self.list.set_visible(!apps.is_empty());
        self.empty.set_visible(apps.is_empty());
        self.note.set_visible(!apps.is_empty());
        let saver = if m.last.saver_on {
            copy::SAVER_ON
        } else {
            copy::SAVER_OFF
        };
        self.note
            .set_text(&format!("{} {saver}", copy::ENERGY_ABOUT));
    }
}
