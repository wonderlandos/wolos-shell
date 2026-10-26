//! Menu bar: logo menu, frontmost app name, static menus, status pill (network, battery, volume), clock, Control Center.
use super::panels::Osd;
use crate::kit::{self, surface::Spec, widgets as w};
use crate::perf::Changed;
use crate::status;
use gtk::{glib, prelude::*};
use std::cell::RefCell;
use std::rc::Rc;

pub struct Bar {
    pub window: gtk::ApplicationWindow,
    pub appname: gtk::Label,
    network: gtk::Image,
    battery: gtk::Image,
    volume: gtk::Image,
    /// Last values pushed to widgets: no redraw when unchanged.
    seen: RefCell<(
        Changed<&'static str>,
        Changed<Option<&'static str>>,
        Changed<&'static str>,
        Changed<Option<u8>>,
    )>,
    name_seen: RefCell<Changed<String>>,
}

impl Bar {
    pub fn set_app(&self, name: String) {
        if self.name_seen.borrow_mut().set(name.clone()) {
            self.appname.set_label(&name);
        }
    }
}

fn menu(items: &[(&str, &'static [&'static str])]) -> gtk::Popover {
    let p = gtk::Popover::new();
    p.add_css_class("wol-menu");
    p.set_has_arrow(false);
    let b = w::vbox(0);
    for (label, argv) in items {
        if label.is_empty() {
            b.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
            continue;
        }
        let it = gtk::Button::with_label(label);
        if let Some(l) = it.child().and_downcast::<gtk::Label>() {
            l.set_xalign(0.0);
        }
        let (argv, p2) = (*argv, p.clone());
        it.connect_clicked(move |_| {
            p2.popdown();
            kit::spawn(&argv.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        });
        b.append(&it);
    }
    p.set_child(Some(&b));
    p
}

pub fn build(app: &gtk::Application, sys: Rc<dyn crate::system::System>) -> Rc<Bar> {
    let window = super::window(app, "menubar", Spec::menubar());
    let row = w::hbox(0);
    row.add_css_class("bar");

    let logo = gtk::MenuButton::new();
    logo.set_icon_name("wolos-glyph-symbolic");
    logo.add_css_class("logo");
    logo.set_popover(Some(&menu(&[
        ("About This Computer", &["wol-settings", "about"]),
        ("", &[]),
        ("System Settings…", &["wol-settings"]),
        ("", &[]),
        ("Lock Screen", &["wol-lock"]),
        ("Log Out…", &["wol", "shell", "toggle", "session"]),
        ("Shut Down…", &["wol", "shell", "toggle", "session"]),
    ])));
    row.append(&logo);
    let appname = gtk::Label::new(Some(crate::apps::DESKTOP_NAME));
    let appbtn = gtk::Button::builder().child(&appname).build();
    appbtn.add_css_class("appname");
    row.append(&appbtn);
    for m in ["File", "Edit", "View", "Go", "Window", "Help"] {
        row.append(&gtk::Button::with_label(m));
    }
    row.append(&w::spacer());

    let pill = w::hbox(2);
    pill.add_css_class("bar-pill");
    let (network, battery, volume) = (gtk::Image::new(), gtk::Image::new(), gtk::Image::new());
    let status_box = w::hbox(10);
    for i in [&network, &volume, &battery] {
        status_box.append(i);
    }
    let status_btn = gtk::Button::builder()
        .child(&status_box)
        .tooltip_text("Control Center")
        .build();
    status_btn.connect_clicked(|_| kit::spawn(&["wol-control-center".into()]));
    let search = gtk::Button::from_icon_name("system-search-symbolic");
    search.set_tooltip_text(Some("Spotlight"));
    search.connect_clicked(|_| kit::spawn(&["wol-spotlight".into()]));
    let cc = gtk::Button::from_icon_name("view-grid-symbolic");
    cc.set_tooltip_text(Some("Control Center"));
    cc.connect_clicked(|_| kit::spawn(&["wol-control-center".into()]));
    let clock = gtk::Label::new(None);
    clock.add_css_class("clock");
    let clock_btn = gtk::Button::builder()
        .child(&clock)
        .tooltip_text("Notification Center")
        .build();
    clock_btn.connect_clicked(|_| {
        kit::spawn(&[
            "wol".into(),
            "shell".into(),
            "toggle".into(),
            "notifications".into(),
        ])
    });
    for b in [&status_btn, &search, &cc, &clock_btn] {
        pill.append(b);
    }
    row.append(&pill);
    window.set_child(Some(&row));

    let buf = RefCell::new(String::with_capacity(32));
    kit::every_minute(move || {
        let mut b = buf.borrow_mut();
        crate::clock::menubar(&mut b, kit::now());
        clock.set_label(&b);
    });
    let bar = Rc::new(Bar {
        window,
        appname,
        network,
        battery,
        volume,
        seen: Default::default(),
        name_seen: Default::default(),
    });
    refresh(&bar, &*sys, None);
    bar
}

/// One status read; widgets touched only on change. Volume change -> OSD.
fn refresh(bar: &Bar, sys: &dyn crate::system::System, osd: Option<&Rc<Osd>>) {
    let audio = sys.audio();
    let mut s = bar.seen.borrow_mut();
    let n = status::network(sys.network());
    if s.0.set(n) {
        bar.network.set_icon_name(Some(n));
    }
    let b = status::battery(sys.battery());
    if s.1.set(b) {
        bar.battery.set_visible(b.is_some());
        bar.battery.set_icon_name(b);
    }
    let v = status::volume(audio);
    if s.2.set(v) {
        bar.volume.set_icon_name(Some(v));
    }
    let first = s.3.set(audio.volume) && osd.is_some();
    if let (true, Some(o), Some(level)) = (first, osd, audio.volume) {
        o.show(v, level);
    }
}

/// 5 s timer, coalesced to whole seconds by GLib (one wakeup per tick for all timers).
pub fn poll_status(bar: Rc<Bar>, sys: Rc<dyn crate::system::System>, osd: Rc<Osd>) {
    glib::timeout_add_seconds_local(5, move || {
        refresh(&bar, &*sys, Some(&osd));
        glib::ControlFlow::Continue
    });
}
