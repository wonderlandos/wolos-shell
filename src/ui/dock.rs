//! Dock: pinned apps (+ unpinned running ones), running dot from Hyprland clients, Trash.
use crate::apps::{self, PINS};
use crate::kit::{self, surface::Spec};
use gtk::prelude::*;
use std::collections::HashSet;
use std::rc::Rc;

pub struct Dock {
    pub window: gtk::ApplicationWindow,
    buttons: Vec<gtk::Button>,
}

fn app_button(icon: &str, name: &str, argv: Vec<String>) -> gtk::Button {
    let img = gtk::Image::from_icon_name(icon);
    img.set_pixel_size(48);
    let b = gtk::Button::builder()
        .child(&img)
        .tooltip_text(name)
        .build();
    b.connect_clicked(move |_| kit::spawn(&argv));
    b
}

pub fn build(app: &gtk::Application) -> Rc<Dock> {
    let window = super::window(app, "dock", Spec::dock());
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let buttons: Vec<gtk::Button> = PINS
        .iter()
        .map(|p| {
            app_button(
                p.icon,
                p.name,
                p.exec.iter().map(|s| s.to_string()).collect(),
            )
        })
        .collect();
    for b in &buttons {
        row.append(b);
    }
    row.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    row.append(&app_button(
        "wolos-trash",
        "Trash",
        vec!["wol-files".into(), "trash:".into()],
    ));
    window.set_child(Some(&row));
    let dock = Rc::new(Dock { window, buttons });
    dock.set_running(&crate::hypr::running_classes(&crate::hypr::clients()));
    dock
}

impl Dock {
    pub fn set_running(&self, classes: &HashSet<String>) {
        for (p, b) in PINS.iter().zip(&self.buttons) {
            let on = apps::is_running(p, classes);
            if b.has_css_class("running") != on {
                if on {
                    b.add_css_class("running")
                } else {
                    b.remove_css_class("running")
                }
            }
        }
    }
}
