//! Menu bar: logo menu, frontmost app name, app menus, status icons, clock, Spotlight, Control Center.
use super::panels::Osd;
use crate::kit::{self, widgets as w};
use crate::perf::Changed;
use crate::status;
use gtk::{glib, prelude::*};
use std::cell::RefCell;
use std::rc::Rc;

/// Last network, battery, volume icon and volume level pushed to widgets.
type Seen = (
    Changed<&'static str>,
    Changed<Option<&'static str>>,
    Changed<&'static str>,
    Changed<Option<u8>>,
);

pub struct Bar {
    pub root: gtk::Box,
    pub appname: gtk::Label,
    network: gtk::Image,
    battery: gtk::Image,
    volume: gtk::Image,
    /// Last values pushed to widgets: no relabel/redraw when unchanged.
    seen: RefCell<Seen>,
    name_seen: RefCell<Changed<String>>,
}

impl Bar {
    pub fn set_app(&self, name: String) {
        if self.name_seen.borrow_mut().set(name.clone()) {
            self.appname.set_label(&name);
        }
    }
}

fn run(argv: &'static [&'static str]) -> Box<dyn Fn()> {
    Box::new(move || kit::spawn(&argv.iter().map(|s| s.to_string()).collect::<Vec<_>>()))
}

pub fn content(sys: &dyn crate::system::System) -> Rc<Bar> {
    let root = w::hbox(0);
    root.add_css_class("menubar-strip");
    let logo = gtk::MenuButton::new();
    logo.set_child(Some(&w::icon("wolos-glyph-symbolic", w::ICON)));
    logo.set_valign(gtk::Align::Center);
    logo.set_popover(Some(&w::menu(vec![
        ("About This Computer", run(&["wol-settings", "About"])),
        ("", Box::new(|| {})),
        ("System Settings…", run(&["wol-settings"])),
        ("", Box::new(|| {})),
        ("Lock Screen", run(&["wol-lock"])),
        ("Log Out…", run(&["wol", "shell", "toggle", "session"])),
        ("Shut Down…", run(&["wol", "shell", "toggle", "session"])),
    ])));
    root.append(&logo);
    let appname = w::label(crate::apps::DESKTOP_NAME, "app-name");
    root.append(&w::menubar_item(&appname));
    for m in ["File", "Edit", "View", "Go", "Window", "Help"] {
        root.append(&w::menubar_item(&w::label(m, "")));
    }
    root.append(&w::spacer());

    let (network, battery, volume) = (
        w::icon("network-wireless-symbolic", w::ICON),
        w::icon("battery-good-symbolic", w::ICON),
        w::icon("audio-volume-high-symbolic", w::ICON),
    );
    for (img, tip, argv) in [
        (&battery, "Battery", &["wol-control-center"][..]),
        (&network, "Wi-Fi", &["wol-control-center"][..]),
        (&volume, "Sound", &["wol-control-center"][..]),
    ] {
        let b = w::menubar_item(img);
        b.set_tooltip_text(Some(tip));
        let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        b.connect_clicked(move |_| kit::spawn(&argv));
        root.append(&b);
    }
    for (icon, tip, argv) in [
        ("system-search-symbolic", "Spotlight", "wol-spotlight"),
        ("view-grid-symbolic", "Control Center", "wol-control-center"),
    ] {
        let b = w::menubar_item(&w::icon(icon, w::ICON));
        b.set_tooltip_text(Some(tip));
        b.connect_clicked(move |_| kit::spawn(&[argv.to_string()]));
        root.append(&b);
    }
    let clock = w::label("", "");
    let cb = w::menubar_item(&clock);
    cb.set_tooltip_text(Some("Notification Center"));
    cb.connect_clicked(|_| {
        kit::spawn(&[
            "wol".into(),
            "shell".into(),
            "toggle".into(),
            "notifications".into(),
        ])
    });
    root.append(&cb);
    let buf = RefCell::new(String::with_capacity(32));
    kit::every_minute(move || {
        let mut b = buf.borrow_mut();
        crate::clock::menubar(&mut b, kit::now());
        if clock.text() != b.as_str() {
            clock.set_label(&b);
        }
    });
    let bar = Rc::new(Bar {
        root,
        appname,
        network,
        battery,
        volume,
        seen: Default::default(),
        name_seen: Default::default(),
    });
    refresh(&bar, sys, None);
    bar
}

pub fn build(
    app: &gtk::Application,
    sys: &dyn crate::system::System,
) -> (gtk::ApplicationWindow, Rc<Bar>) {
    let win = super::window(app, "menubar", kit::surface::Spec::menubar());
    let bar = content(sys);
    win.set_child(Some(&bar.root));
    (win, bar)
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
        if let Some(p) = bar.battery.parent() {
            p.set_visible(b.is_some())
        }
        bar.battery.set_icon_name(b);
    }
    let v = status::volume(audio);
    if s.2.set(v) {
        bar.volume.set_icon_name(Some(v));
    }
    let changed = s.3.set(audio.volume);
    if let (true, Some(o), Some(level)) = (changed, osd, audio.volume) {
        o.show(v, level);
    }
}

/// 5 s timer on whole seconds (GLib coalesces wakeups).
pub fn poll_status(bar: Rc<Bar>, sys: Rc<dyn crate::system::System>, osd: Rc<Osd>) {
    glib::timeout_add_seconds_local(5, move || {
        refresh(&bar, &*sys, Some(&osd));
        glib::ControlFlow::Continue
    });
}
