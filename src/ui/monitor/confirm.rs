// SPDX-License-Identifier: GPL-2.0-or-later
//! The one question System Monitor asks (HIG §3 destructive actions): a sheet from the toolbar
//! with Cancel and the destructive button on the right, for Quit and End Process.
use crate::kit::widgets::{self as w, AppWindow};
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

type Action = Box<dyn Fn()>;

pub struct Confirm {
    pub revealer: gtk::Revealer,
    title: gtk::Label,
    body: gtk::Label,
    ok: gtk::Button,
    cancel: gtk::Button,
    action: Rc<RefCell<Option<Action>>>,
}

impl Confirm {
    pub fn new(aw: &AppWindow) -> Rc<Confirm> {
        let b = w::vbox(w::S3);
        let title = w::label("", "title");
        title.set_wrap(true);
        title.set_xalign(0.0);
        b.append(&title);
        let body = w::label("", "secondary");
        body.set_wrap(true);
        body.set_xalign(0.0);
        b.append(&body);
        let row = w::hbox(w::S2);
        row.append(&w::spacer());
        b.append(&row);
        let revealer = w::toolbar_sheet(aw, &b);
        let action: Rc<RefCell<Option<Action>>> = Rc::default();
        let cancel = {
            let (r, a) = (revealer.clone(), action.clone());
            w::button(crate::copy::monitor::CANCEL, w::Kind::Default, move || {
                a.borrow_mut().take();
                r.set_reveal_child(false);
            })
        };
        let ok = {
            let (r, a) = (revealer.clone(), action.clone());
            w::button("", w::Kind::Destructive, move || {
                r.set_reveal_child(false);
                // Taken out first: the action may ask again.
                let run = a.borrow_mut().take();
                if let Some(f) = run {
                    f();
                }
            })
        };
        row.append(&cancel);
        row.append(&ok);
        {
            let c = cancel.clone();
            crate::kit::surface::on_escape(&b, move || c.emit_clicked());
        }
        Rc::new(Confirm {
            revealer,
            title,
            body,
            ok,
            cancel,
            action,
        })
    }

    /// Shows the question; `then` runs on the destructive button only.
    pub fn ask(&self, title: &str, body: &str, button: &str, then: impl Fn() + 'static) {
        self.title.set_text(title);
        self.body.set_text(body);
        self.ok.set_label(button);
        self.ok.set_visible(true);
        self.cancel.set_label(crate::copy::monitor::CANCEL);
        *self.action.borrow_mut() = Some(Box::new(then));
        self.revealer.set_reveal_child(true);
        self.ok.grab_focus();
    }

    /// What happened after the question, in the same sheet; one button closes it.
    pub fn note(&self, body: &str) {
        self.title.set_text(crate::copy::monitor::AFTER_TITLE);
        self.body.set_text(body);
        self.ok.set_visible(false);
        self.cancel.set_label(crate::copy::monitor::CLOSE);
        self.action.borrow_mut().take();
        self.revealer.set_reveal_child(true);
        self.cancel.grab_focus();
    }

    pub fn is_open(&self) -> bool {
        self.revealer.reveals_child()
    }
}
