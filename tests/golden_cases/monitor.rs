// SPDX-License-Identifier: GPL-2.0-or-later
//! System Monitor (docs/modules/monitor.md): every page over the demo minute
//! (`ui::monitor::demo`), at 1280x800 and in the narrow window (sidebar collapsed), and the
//! empty, loading and error states and the Quit question.
use super::Case;
use gtk::prelude::*;
use shell::copy::monitor as copy;
use shell::kit::widgets as w;
use shell::ui::monitor::{self, Actions, demo, model};
use std::cell::RefCell;
use std::rc::Rc;

const PAGES: [(&str, &str); 6] = [
    ("overview", copy::OVERVIEW),
    ("apps", copy::APPS),
    ("processes", copy::PROCESSES),
    ("disks", copy::DISKS),
    ("network", copy::NETWORK),
    ("startup", copy::STARTUP),
];

/// The window content on `page` over `m`, after `then` ran on it.
fn shown(m: model::Model, page: &'static str, then: impl Fn(&Rc<monitor::Monitor>)) -> gtk::Widget {
    let mon = monitor::content(Rc::new(RefCell::new(m)), Actions::none());
    mon.open(page);
    mon.update();
    then(&mon);
    mon.aw.root.clone().upcast()
}

pub fn cases() -> Vec<Case> {
    let mut v = Vec::new();
    for (tag, size) in [("1280", (1280, 800)), ("narrow", w::APP_NARROW)] {
        for (name, page) in PAGES {
            v.push(Case {
                name: format!("monitor-{name}-{tag}"),
                size,
                class: "preview-app",
                wallpaper: false,
                build: Box::new(move || shown(demo::model(), page, |_| {})),
            });
        }
    }
    let state = |name: &str, page: &'static str, m: fn() -> model::Model| Case {
        name: format!("monitor-{name}"),
        size: w::APP_SIZE,
        class: "preview-app",
        wallpaper: false,
        build: Box::new(move || shown(m(), page, |_| {})),
    };
    // Nothing sampled yet / nothing open: the empty states, never a blank page.
    v.push(state("apps-empty", copy::APPS, model::Model::default));
    v.push(state(
        "overview-first-second",
        copy::OVERVIEW,
        model::Model::default,
    ));
    v.push(state(
        "startup-loading",
        copy::STARTUP,
        model::Model::default,
    ));
    v.push(state("startup-not-read", copy::STARTUP, || model::Model {
        startup: Some(model::Startup::NotRead),
        ..Default::default()
    }));
    v.push(state("processes-no-match", copy::PROCESSES, || {
        model::Model {
            filter: "zzz".into(),
            ..demo::model()
        }
    }));
    v.push(Case {
        name: "monitor-quit-paused".into(),
        size: w::APP_SIZE,
        class: "preview-app",
        wallpaper: false,
        build: Box::new(|| {
            shown(demo::model(), copy::APPS, |m| {
                m.confirm.ask(
                    &copy::quit_title("Discord"),
                    copy::FORCE_QUIT_BODY,
                    copy::FORCE_QUIT,
                    || {},
                );
            })
        }),
    });
    v
}
