// SPDX-License-Identifier: GPL-2.0-or-later
//! Startup: the services that start with the person's session. Their own services switch here;
//! the ones WolOS ships show their state only. Apps that open at login live in Settings.
use super::model::{Model, Startup, StartupRow};
use crate::copy::monitor as copy;
use crate::kit::form;
use crate::kit::widgets as w;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

/// Switch a service: (unit name, on, done(ok)).
pub type Switch = Rc<dyn Fn(&str, bool, Box<dyn FnOnce(bool)>)>;

pub struct Page {
    pub root: gtk::Box,
    holder: gtk::Box,
    shown: RefCell<Option<Startup>>,
    switch: Switch,
}

impl Page {
    pub fn new(switch: Switch, login_items: impl Fn() + 'static) -> Page {
        let root = w::page(copy::STARTUP);
        root.append(&w::footnote(copy::STARTUP_ABOUT));
        let open = w::button(copy::LOGIN_ITEMS, w::Kind::Default, login_items);
        open.set_halign(gtk::Align::Start);
        root.append(&open);
        let holder = w::vbox(w::S3);
        holder.append(&w::spinner());
        root.append(&holder);
        Page {
            root,
            holder,
            shown: RefCell::default(),
            switch,
        }
    }

    fn row(&self, r: &StartupRow) -> gtk::Widget {
        if r.unit.mine {
            let (sw, name) = (self.switch.clone(), r.unit.name.clone());
            let t = w::pending_toggle(r.unit.enabled, move |on, done| sw(&name, on, done));
            form::row(&r.title, &r.unit.name, &t)
        } else {
            let v = w::label(
                if r.unit.enabled { copy::ON } else { copy::OFF },
                "secondary",
            );
            form::row(
                &r.title,
                &format!("{} · {}", r.unit.name, copy::SHIPPED),
                &v,
            )
        }
    }

    pub fn update(&self, m: &Model) {
        if *self.shown.borrow() == m.startup {
            return;
        }
        let Some(state) = &m.startup else { return };
        while let Some(c) = self.holder.first_child() {
            self.holder.remove(&c);
        }
        match state {
            Startup::NotRead => {
                let l = w::footnote(copy::STARTUP_NOT_READ);
                self.holder.append(&l);
            }
            Startup::Rows(rows) if rows.is_empty() => {
                self.holder.append(&w::empty_state(
                    "system-run-symbolic",
                    copy::STARTUP_EMPTY.0,
                    copy::STARTUP_EMPTY.1,
                    None,
                ));
            }
            Startup::Rows(rows) => {
                let widgets: Vec<gtk::Widget> = rows.iter().map(|r| self.row(r)).collect();
                self.holder.append(&w::section("", &widgets));
            }
        }
        *self.shown.borrow_mut() = m.startup.clone();
    }
}
