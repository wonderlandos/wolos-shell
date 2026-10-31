// SPDX-License-Identifier: GPL-2.0-or-later
//! Menu bar: logo menu, frontmost app name, app menus, status icons, clock, Search, Control Center.
use crate::kit::{self, widgets as w};
use crate::perf::Changed;
use crate::{appmenu, status};
use gtk::{glib, prelude::*};
use std::cell::RefCell;
use std::rc::Rc;

/// Last network, battery, volume icon, volume level and Focus icon pushed to widgets.
type Seen = (
    Changed<&'static str>,
    Changed<Option<&'static str>>,
    Changed<&'static str>,
    Changed<Option<u8>>,
    Changed<Option<&'static str>>,
);

pub struct Bar {
    pub root: gtk::Box,
    pub appname: gtk::Label,
    app_menu: gtk::MenuButton,
    window_menu: gtk::MenuButton,
    network: gtk::Image,
    battery: gtk::Image,
    volume: gtk::Image,
    focus: gtk::Image,
    percent: gtk::Label,
    show_percent: std::cell::Cell<bool>,
    /// Last values pushed to widgets: no relabel/redraw when unchanged.
    seen: RefCell<Seen>,
    name_seen: RefCell<Changed<String>>,
    /// The display this bar is on (connector name; "" until placed).
    pub display: RefCell<String>,
    /// Ends with the bar's window: its clock tick, reload hook and Focus watch stop.
    pub life: kit::lifetime::Lifetime,
}

impl Bar {
    /// Full strength on the focused display, dimmed on the others (macOS).
    pub fn set_active(&self, on: bool) {
        if on {
            self.root.remove_css_class("inactive");
        } else {
            self.root.add_css_class("inactive");
        }
    }

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
        self.app_menu.set_visible(!name.is_empty());
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
    let icon = crate::apps::icon_for(class, crate::apps::here(), &crate::desktop::load_all());
    super::panels::about(&app, name, &icon, class).present();
}

/// WolOS menu (the logo), macOS Apple menu order. Empty title = separator.
pub const LOGO_MENU: &[(&str, &[&str])] = &[
    ("About This Computer", &["wol-settings", "About"]),
    ("", &[]),
    ("System Settings…", &["wol-settings"]),
    ("App Store…", &["wol-store"]),
    ("", &[]),
    ("Lock Screen", &["wol-lock"]),
    ("Log Out…", &["wol", "shell", "toggle", "session"]),
    ("Shut Down…", &["wol", "shell", "toggle", "session"]),
];

fn run(argv: &'static [&'static str]) -> Box<dyn Fn()> {
    Box::new(move || kit::spawn(&argv.iter().map(|s| s.to_string()).collect::<Vec<_>>()))
}

pub fn content(sys: &dyn crate::system::System) -> Rc<Bar> {
    let root = w::hbox(0);
    root.add_css_class("menubar-strip");
    let logo = gtk::MenuButton::new();
    logo.set_child(Some(&w::icon("wolos-glyph-symbolic", w::ICON)));
    logo.set_tooltip_text(Some("WolOS menu"));
    logo.set_valign(gtk::Align::Center);
    logo.set_popover(Some(&w::menu(
        LOGO_MENU.iter().map(|&(t, argv)| (t, run(argv))).collect(),
    )));
    root.append(&logo);
    // Only menus backed by real actions: the app menu and Window (no placeholder File/Edit/View).
    let appname = w::label(crate::apps::DESKTOP_NAME, "app-name");
    let app_menu = w::menubar_menu(&appname);
    app_menu.set_visible(false);
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
    let focus = w::icon("weather-clear-night-symbolic", w::ICON);
    let percent = w::label("", "");
    let battery_box = w::hbox(w::S1);
    battery_box.append(&percent);
    battery_box.append(&battery);
    for (img, tip, argv) in [
        (
            focus.upcast_ref::<gtk::Widget>(),
            "Focus",
            &["wol", "shell", "toggle", "control-center"][..],
        ),
        (
            battery_box.upcast_ref::<gtk::Widget>(),
            "Battery",
            &["wol", "shell", "toggle", "control-center"][..],
        ),
        (
            network.upcast_ref(),
            "Wi-Fi",
            &["wol", "shell", "toggle", "control-center"][..],
        ),
        (
            volume.upcast_ref(),
            "Sound",
            &["wol", "shell", "toggle", "control-center"][..],
        ),
    ] {
        let b = w::menubar_item(img);
        b.set_tooltip_text(Some(tip));
        let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        b.connect_clicked(move |_| kit::spawn(&argv));
        root.append(&b);
    }
    for (icon, tip, argv) in [
        ("system-search-symbolic", "Search", &["wol-search"][..]),
        (
            "view-grid-symbolic",
            "Control Center",
            &["wol", "shell", "toggle", "control-center"][..],
        ),
    ] {
        let b = w::menubar_item(&w::icon(icon, w::ICON));
        b.set_tooltip_text(Some(tip));
        let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        b.connect_clicked(move |_| kit::spawn(&argv));
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
    let life = kit::lifetime::Lifetime::new();
    {
        let tick = tick.clone();
        kit::lifetime::every_minute_while(&life, move || tick());
    }
    let bar = Rc::new(Bar {
        root,
        appname,
        app_menu,
        window_menu,
        network,
        battery,
        volume,
        focus,
        percent,
        show_percent: Default::default(),
        seen: Default::default(),
        name_seen: Default::default(),
        display: Default::default(),
        life: life.clone(),
    });
    BARS.with_borrow_mut(|b| {
        b.retain(|x| x.strong_count() > 0);
        b.push(Rc::downgrade(&bar));
    });
    refresh(&bar, sys);
    follow_focus(&bar);
    {
        // Settings > Menu Bar: applied live on `reload`.
        let (b, battery) = (Rc::downgrade(&bar), sys.battery());
        super::ipc::on_reload_while(&life, move |p| {
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

/// A menu bar window on `display` (None: the compositor picks). The `primary` bar takes the
/// keyboard on ⌃F2 and survives its display going away (it moves to the next main display);
/// the others belong to their display and go with it.
pub fn build(
    app: &gtk::Application,
    sys: &dyn crate::system::System,
    display: Option<&gtk::gdk::Monitor>,
    primary: bool,
) -> (gtk::ApplicationWindow, Rc<Bar>) {
    let win = super::window(app, "menubar", kit::surface::Spec::menubar());
    if primary {
        crate::kit::surface::survive_output_loss(&win, kit::surface::Spec::menubar());
    }
    kit::surface::set_monitor(&win, display);
    let bar = content(sys);
    bar.life.tie(&win);
    if let Some(m) = display {
        bar.display
            .replace(m.connector().map(|c| c.to_string()).unwrap_or_default());
    }
    if let Some((name, class)) = LAST_APP.with_borrow(|a| a.clone()) {
        bar.set_app(name, class);
    }
    let focused = FOCUSED.with_borrow(|f| f.clone());
    bar.set_active(crate::displays::bar_active(
        &bar.display.borrow(),
        focused.as_deref(),
    ));
    win.set_child(Some(&bar.root));
    hover_menus(&bar.root);
    (win, bar)
}

/// The menu to open when the pointer enters title `hovered` while menu `open` is open (macOS).
pub fn hover_switch(open: Option<usize>, hovered: usize) -> Option<usize> {
    open.filter(|o| *o != hovered).map(|_| hovered)
}

/// Once a menu-bar menu is open, moving the pointer onto another title opens that one.
fn hover_menus(root: &gtk::Box) {
    let mut menus = vec![];
    let mut c = root.first_child();
    while let Some(w) = c {
        c = w.next_sibling();
        if let Ok(m) = w.downcast::<gtk::MenuButton>() {
            menus.push(m);
        }
    }
    let menus = Rc::new(menus);
    for (i, m) in menus.iter().enumerate() {
        let motion = gtk::EventControllerMotion::new();
        let all = menus.clone();
        motion.connect_enter(move |_, _, _| {
            let open = all.iter().position(|m| m.is_active());
            if let Some(n) = hover_switch(open, i) {
                if let Some(o) = open {
                    all[o].set_active(false);
                }
                all[n].set_active(true);
            }
        });
        m.add_controller(motion);
    }
}

/// ⌃F2 (`wol shell show menubar`): the keyboard moves into the bar, the WolOS menu focused;
/// arrows move, Return opens. A menu closing, an item chosen, or Esc gives the keyboard back.
pub fn keyboard(win: &gtk::ApplicationWindow, root: &gtk::Box) -> Rc<dyn Fn(bool)> {
    let Some(first) = root.first_child() else {
        return Rc::new(|_| {});
    };
    let access = kit::surface::keyboard_access(win, &first, |_| {});
    let mut c = root.first_child();
    while let Some(w) = c {
        let a = access.clone();
        if let Some(m) = w.downcast_ref::<gtk::MenuButton>() {
            m.connect_active_notify(move |m| {
                if !m.is_active() {
                    a(false);
                }
            });
        } else if let Some(b) = w.downcast_ref::<gtk::Button>() {
            b.connect_clicked(move |_| a(false));
        }
        c = w.next_sibling();
    }
    access
}

/// One status read; widgets touched only on change. Volume change -> OSD.
fn refresh(bar: &Bar, sys: &dyn crate::system::System) {
    let audio = sys.audio();
    let mut s = bar.seen.borrow_mut();
    let n = status::menubar_network(sys.network(), sys.wifi_enabled());
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
    // The OSD is for volume keys only (`wol shell show osd` from wol-volume), not for polls.
    s.3.set(audio.volume);
    set_focus(bar, &mut s.4);
}

/// Moon while Focus is on (a timer runs out on the next poll).
fn set_focus(bar: &Bar, seen: &mut Changed<Option<&'static str>>) {
    let f = crate::focus::menubar_icon(crate::focus::read());
    if seen.set(f)
        && let Some(b) = bar.focus.parent()
    {
        b.set_visible(f.is_some());
    }
}

/// The moon follows the Focus file at once (Control Center writes it), not on the next poll.
fn follow_focus(bar: &Rc<Bar>) {
    use gtk::gio;
    let Ok(m) = gio::File::for_path(crate::focus::path())
        .monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
    else {
        return;
    };
    let b = Rc::downgrade(bar);
    m.connect_changed(move |_, _, _, _| {
        if let Some(bar) = b.upgrade() {
            set_focus(&bar, &mut bar.seen.borrow_mut().4);
        }
    });
    // Watched as long as the bar's window is there.
    bar.life.on_end(move || {
        m.cancel();
    });
}

thread_local! {
    /// Every menu bar on screen (one per display).
    static BARS: RefCell<Vec<std::rc::Weak<Bar>>> = const { RefCell::new(Vec::new()) };
    /// The app in front and the focused display, for bars made later (a display plugged in).
    static LAST_APP: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
    static FOCUSED: RefCell<Option<String>> = const { RefCell::new(None) };
    /// The session's system (set by [`poll_status`]) for [`refresh_now`].
    static SYS: RefCell<Option<Rc<dyn crate::system::System>>> = const { RefCell::new(None) };
}

/// Every live menu bar.
pub fn bars() -> Vec<Rc<Bar>> {
    BARS.with_borrow(|b| b.iter().filter_map(|x| x.upgrade()).collect())
}

/// The app in front changed: every bar shows it.
pub fn set_app_all(name: String, class: String) {
    LAST_APP.set(Some((name.clone(), class.clone())));
    for b in bars() {
        b.set_app(name.clone(), class.clone());
    }
}

/// The focus moved to display `connector`: its bar at full strength, the others dimmed.
pub fn set_focused(connector: &str) {
    FOCUSED.set(Some(connector.to_string()));
    for b in bars() {
        b.set_active(crate::displays::bar_active(
            &b.display.borrow(),
            Some(connector),
        ));
    }
}

/// Re-reads the status icons now: Control Center calls this whenever its snapshot or one of its
/// actions settles, so Wi-Fi, volume and battery in the bar never lag the 5 s poll.
pub fn refresh_now() {
    SYS.with_borrow(|sys| {
        if let Some(sys) = sys {
            for bar in bars() {
                refresh(&bar, &**sys);
            }
        }
    });
}

/// 5 s timer on whole seconds (GLib coalesces wakeups).
pub fn poll_status(sys: Rc<dyn crate::system::System>) {
    SYS.set(Some(sys.clone()));
    glib::timeout_add_seconds_local(5, move || {
        for bar in bars() {
            refresh(&bar, &*sys);
        }
        crate::alerts::poll(sys.battery());
        glib::ControlFlow::Continue
    });
}

#[cfg(test)]
mod hover_tests {
    use super::hover_switch;

    /// macOS: with a menu open, the pointer over another title opens that one instead.
    #[test]
    fn hovering_a_title_switches_only_while_a_menu_is_open() {
        assert_eq!(hover_switch(Some(0), 2), Some(2));
        assert_eq!(hover_switch(Some(2), 2), None);
        assert_eq!(hover_switch(None, 1), None);
    }
}

#[cfg(test)]
mod logo_tests {
    use super::LOGO_MENU;

    #[test]
    fn logo_menu_opens_the_app_store_after_system_settings() {
        let titles: Vec<&str> = LOGO_MENU.iter().map(|r| r.0).collect();
        let at = titles.iter().position(|t| *t == "App Store…").unwrap();
        assert_eq!(titles[at - 1], "System Settings…");
        assert_eq!(LOGO_MENU[at].1, ["wol-store"]);
        assert_eq!(titles[0], "About This Computer");
        // Separators run nothing; every other row runs a program.
        assert!(LOGO_MENU.iter().all(|(t, a)| t.is_empty() == a.is_empty()));
    }
}
