//! Menu bar: logo menu, frontmost app name, app menus, status icons, clock, Spotlight, Control Center.
use super::panels::Osd;
use crate::kit::{self, widgets as w};
use crate::perf::Changed;
use crate::{appmenu, status};
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
    app_menu: gtk::MenuButton,
    window_menu: gtk::MenuButton,
    network: gtk::Image,
    battery: gtk::Image,
    volume: gtk::Image,
    percent: gtk::Label,
    show_percent: std::cell::Cell<bool>,
    /// Last values pushed to widgets: no relabel/redraw when unchanged.
    seen: RefCell<Seen>,
    name_seen: RefCell<Changed<String>>,
}

impl Bar {
    /// Frontmost app: bold name with its real menu; the Window menu only when a window is focused.
    pub fn set_app(&self, name: String, class: String) {
        if !self.name_seen.borrow_mut().set(format!("{name}\n{class}")) {
            return;
        }
        self.appname.set_label(&name);
        let rows = appmenu::app_menu(&name, &class);
        self.app_menu.set_popover(
            (!rows.is_empty())
                .then(|| w::menu_keys(actions(rows, &name, &class)))
                .as_ref(),
        );
        self.window_menu.set_visible(!class.is_empty());
    }
}

/// Menu rows -> clickable rows: About opens the about window, the rest run `wol-app <verb>`.
fn actions(rows: Vec<appmenu::Row>, name: &str, class: &str) -> Vec<w::MenuRow> {
    rows.into_iter()
        .map(|(label, key, act)| {
            let (name, class) = (name.to_string(), class.to_string());
            let f: Box<dyn Fn()> = match act {
                Some(appmenu::Act::About) => Box::new(move || about(&name, &class)),
                Some(a) => {
                    let mut argv = vec!["wol-app".to_string()];
                    argv.extend(
                        appmenu::verb(a)
                            .unwrap_or_default()
                            .split(' ')
                            .map(String::from),
                    );
                    Box::new(move || kit::spawn(&argv))
                }
                None => Box::new(|| {}),
            };
            (label, key, f)
        })
        .collect()
}

/// About <App>: icon and name from the dock pin or installed .desktop entry.
fn about(name: &str, class: &str) {
    let Some(app) = gtk::gio::Application::default().and_downcast::<gtk::Application>() else {
        return;
    };
    let icon = crate::apps::icon_for(class, crate::apps::PINS, &crate::desktop::load_all());
    super::panels::about(&app, name, &icon, class).present();
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
    // Only menus backed by real actions: the app menu and Window (no placeholder File/Edit/View).
    let appname = w::label(crate::apps::DESKTOP_NAME, "app-name");
    let app_menu = w::menubar_menu(&appname);
    root.append(&app_menu);
    let window_menu = w::menubar_menu(&w::label("Window", ""));
    window_menu.set_popover(Some(&w::menu_keys(actions(appmenu::window_menu(), "", ""))));
    window_menu.set_visible(false);
    root.append(&window_menu);
    root.append(&w::spacer());

    let (network, battery, volume) = (
        w::icon("network-wireless-symbolic", w::ICON),
        w::icon("battery-good-symbolic", w::ICON),
        w::icon("audio-volume-high-symbolic", w::ICON),
    );
    let percent = w::label("", "");
    let battery_box = w::hbox(w::S1);
    battery_box.append(&percent);
    battery_box.append(&battery);
    for (img, tip, argv) in [
        (
            battery_box.upcast_ref::<gtk::Widget>(),
            "Battery",
            &["wol-control-center"][..],
        ),
        (network.upcast_ref(), "Wi-Fi", &["wol-control-center"][..]),
        (volume.upcast_ref(), "Sound", &["wol-control-center"][..]),
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
    let show_date = Rc::new(std::cell::Cell::new(true));
    let tick = {
        let (clock, show_date) = (clock.clone(), show_date.clone());
        Rc::new(move || {
            let mut b = buf.borrow_mut();
            if show_date.get() {
                crate::clock::menubar(&mut b, kit::now());
            } else {
                crate::clock::menubar_time(&mut b, kit::now());
            }
            if clock.text() != b.as_str() {
                clock.set_label(&b);
            }
        })
    };
    {
        let tick = tick.clone();
        kit::every_minute(move || tick());
    }
    let bar = Rc::new(Bar {
        root,
        appname,
        app_menu,
        window_menu,
        network,
        battery,
        volume,
        percent,
        show_percent: Default::default(),
        seen: Default::default(),
        name_seen: Default::default(),
    });
    refresh(&bar, sys, None);
    {
        // Settings > Menu Bar: applied live on `reload`.
        let (b, battery) = (Rc::downgrade(&bar), sys.battery());
        super::ipc::on_reload(move |p| {
            show_date.set(p.show_date);
            tick();
            if let Some(b) = b.upgrade() {
                b.show_percent.set(p.battery_percent);
                b.percent.set_visible(p.battery_percent);
                if b.percent.text().is_empty() {
                    b.percent
                        .set_label(&battery.map_or(String::new(), |x| format!("{}%", x.percent)));
                }
            }
        });
    }
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
        // icon -> box -> menubar button
        if let Some(p) = bar.battery.parent().and_then(|p| p.parent()) {
            p.set_visible(b.is_some())
        }
        bar.battery.set_icon_name(b);
    }
    if bar.show_percent.get()
        && let Some(x) = sys.battery()
    {
        let t = format!("{}%", x.percent);
        if bar.percent.text() != t {
            bar.percent.set_label(&t);
        }
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
