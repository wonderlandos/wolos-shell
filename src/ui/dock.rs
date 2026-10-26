//! Dock: pinned apps, running dots from Hyprland clients, separator, Trash.
use crate::apps::{self, PINS};
use crate::kit::{self, surface::Spec, widgets as w};
use gtk::prelude::*;
use std::collections::HashSet;
use std::rc::Rc;

pub struct Dock {
    pub root: gtk::Box,
    tiles: Vec<w::DockTile>,
}

pub fn content(running: &HashSet<String>) -> Rc<Dock> {
    let root = w::hbox(w::S2);
    root.add_css_class("dock-shelf");
    root.add_css_class("no-row-check"); // tiles lift on hover by design
    root.set_halign(gtk::Align::Center);
    root.set_valign(gtk::Align::End);
    let tiles: Vec<w::DockTile> = PINS
        .iter()
        .map(|p| {
            let argv: Vec<String> = p.exec.iter().map(|s| s.to_string()).collect();
            w::DockTile::new(p.icon, p.name, move || kit::spawn(&argv))
        })
        .collect();
    for t in &tiles {
        root.append(&t.button);
    }
    root.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    root.append(
        &w::DockTile::new("wolos-trash", "Trash", || {
            kit::spawn(&["wol-files".into(), "trash:".into()])
        })
        .button,
    );
    let d = Rc::new(Dock { root, tiles });
    d.set_running(running);
    d
}

pub fn build(app: &gtk::Application) -> (gtk::ApplicationWindow, Rc<Dock>) {
    let win = super::window(app, "dock", Spec::dock());
    let d = content(&crate::hypr::running_classes(&crate::hypr::clients()));
    win.set_child(Some(&d.root));
    (win, d)
}

impl Dock {
    pub fn set_running(&self, classes: &HashSet<String>) {
        for (p, t) in PINS.iter().zip(&self.tiles) {
            t.set_running(apps::is_running(p, classes));
        }
    }
}
