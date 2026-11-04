// SPDX-License-Identifier: GPL-2.0-or-later
//! Menu bar: logo menu, frontmost app name, app menus, status icons, clock, Search, Control Center,
//! and the enabled plugins' `[[menubar]]` items (ADR 0031). Items sit in the three slots of the
//! layout in `shell.json` (`shared::layout::Layout`), on the layout's edge.
use super::plugins;
use crate::kit::lifetime::Lifetime;
use crate::kit::{self, widgets as w};
use crate::perf::Changed;
use crate::{appmenu, status};
use gtk::{gio, glib, prelude::*};
use shared::layout::{BarItem, Layout};
use std::cell::RefCell;
use std::rc::Rc;

/// CSS class of the three slot boxes (left, centre, right).
pub const SLOT_CLASS: &str = "menubar-slot";

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
    /// What the AppMenus item holds: the frontmost Wolkit app's own menus, then Window.
    app_menus: gtk::Box,
    global: Rc<Global>,
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
    items: Items,
    /// Left, centre, right.
    slots: [gtk::Box; 3],
    spacers: [gtk::Box; 2],
    layout: RefCell<Layout>,
    /// This bar's plugin host (None: built-in items only, as in previews).
    plugins: Option<Rc<RefCell<plugins::Host>>>,
    /// Ends when the plugin items are made again (their refresh timers stop).
    plugin_life: RefCell<Lifetime>,
}

impl Bar {
    /// Full strength on the focused display, dimmed on the others.
    pub fn set_active(&self, on: bool) {
        if on {
            self.root.remove_css_class("inactive");
        } else {
            self.root.add_css_class("inactive");
        }
    }

    /// The recording item as while a recording runs (goldens; the pid file drives it live).
    pub fn set_recording(&self, on: bool) {
        self.items.record.set_visible(on);
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
        self.global.follow(&class);
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

/// About <App>: icon and name from the dock pin or installed .desktop entry (parsed off the UI
/// thread: every .desktop file on each click stalled the bar).
fn about(name: &str, class: &str) {
    let Some(app) = gtk::gio::Application::default().and_downcast::<gtk::Application>() else {
        return;
    };
    let (name, class) = (name.to_string(), class.to_string());
    glib::spawn_future_local(async move {
        let installed = super::ipc::installed().await;
        let icon = crate::apps::icon_for(&class, crate::apps::here(), &installed);
        super::panels::about(&app, &name, &icon, &class).present();
    });
}

// ---- the global menu: a Wolkit app's own menus in the bar ----------------------------------

/// The menus of the frontmost Wolkit app (`appmenu::Source::Wolkit`): one title per menu, before
/// Window, filled from the app's `org.wolos.Menus` object and refreshed when it signals a change.
/// Choosing an item calls `Activate` on the app, which clicks the item in its window.
struct Global {
    /// Holds the menu titles and, last, the Window menu.
    host: gtk::Box,
    shown: RefCell<Vec<gtk::MenuButton>>,
    /// Bumped on every change of app; an answer for an older one is dropped.
    epoch: std::cell::Cell<u64>,
    /// Dropping it ends the subscription.
    watch: RefCell<Option<gio::SignalSubscription>>,
}

/// Longest wait for an app's answer; an app that stalls shows no menus.
const MENUS_TIMEOUT_MS: i32 = 500;

impl Global {
    fn new(host: &gtk::Box) -> Global {
        Global {
            host: host.clone(),
            shown: RefCell::new(Vec::new()),
            epoch: std::cell::Cell::new(0),
            watch: RefCell::new(None),
        }
    }

    /// The app in front is `class`: drop the last app's menus, and when the app publishes any
    /// ([`appmenu::source`]) show them and follow its changes.
    fn follow(self: &Rc<Self>, class: &str) {
        let epoch = self.epoch.get() + 1;
        self.epoch.set(epoch);
        self.watch.take();
        self.show(None, Vec::new());
        // Every app is a candidate; the call below fails at once for one that publishes none
        // (and never starts a service: `NO_AUTO_START`).
        let appmenu::Source::Wolkit { bus, path } = appmenu::source(class, |_| true) else {
            return;
        };
        let me = self.clone();
        glib::spawn_future_local(async move {
            let Ok(conn) = gio::bus_get_future(gio::BusType::Session).await else {
                return;
            };
            if me.epoch.get() != epoch {
                return;
            }
            // Subscribe before the first read, so a change in between is not lost. The callback
            // holds the bar's state weakly: the subscription lives in it.
            let again = {
                let (me, bus, path) = (Rc::downgrade(&me), bus.clone(), path.clone());
                move |s: gio::DBusSignalRef| {
                    if let Some(me) = me.upgrade() {
                        read_menus(&me, s.connection, &bus, &path, epoch);
                    }
                }
            };
            let sub = conn.subscribe_to_signal(
                Some(&bus),
                Some(appmenu::WOLKIT_MENUS),
                Some("Changed"),
                Some(&path),
                None,
                gio::DBusSignalFlags::NONE,
                again,
            );
            me.watch.replace(Some(sub));
            read_menus(&me, &conn, &bus, &path, epoch);
        });
    }

    /// Replace the titles with `menus` (none: the app's own menus are gone).
    fn show(
        &self,
        from: Option<(&gio::DBusConnection, &str, &str)>,
        menus: Vec<shared::view::Menu>,
    ) {
        for b in self.shown.take() {
            self.host.remove(&b);
        }
        let Some((conn, bus, path)) = from else {
            return;
        };
        let mut last: Option<gtk::MenuButton> = None;
        for m in &menus {
            let b = w::menubar_menu(&w::label(&m.title, ""));
            b.set_popover(Some(&popover(conn, bus, path, m)));
            hover_in(&b, &self.host);
            self.host.insert_child_after(&b, last.as_ref());
            last = Some(b.clone());
            self.shown.borrow_mut().push(b);
        }
    }
}

/// Read the app's menus again and show them, unless the app in front changed meanwhile.
fn read_menus(me: &Rc<Global>, conn: &gio::DBusConnection, bus: &str, path: &str, epoch: u64) {
    let (me, conn, bus, path) = (me.clone(), conn.clone(), bus.to_string(), path.to_string());
    glib::spawn_future_local(async move {
        let reply = conn
            .call_future(
                Some(&bus),
                &path,
                appmenu::WOLKIT_MENUS,
                "Get",
                None,
                glib::VariantTy::new("(s)").ok(),
                gio::DBusCallFlags::NO_AUTO_START,
                MENUS_TIMEOUT_MS,
            )
            .await;
        if me.epoch.get() != epoch {
            return;
        }
        let menus = reply
            .ok()
            .and_then(|v| v.get::<(String,)>())
            .map(|(json,)| appmenu::wolkit_menus(&json))
            .unwrap_or_default();
        me.show(Some((&conn, &bus, &path)), menus);
    });
}

/// One menu of the app: its items as buttons (disabled ones greyed, a checked one ticked); a
/// choice calls `Activate(menu, key)` on the app.
fn popover(
    conn: &gio::DBusConnection,
    bus: &str,
    path: &str,
    m: &shared::view::Menu,
) -> gtk::Popover {
    use shared::view::MenuEntry;
    let p = gtk::Popover::new();
    p.add_css_class("menu");
    p.set_has_arrow(false);
    let col = w::vbox(0);
    for e in &m.entries {
        match e {
            MenuEntry::Separator => col.append(&gtk::Separator::new(gtk::Orientation::Horizontal)),
            MenuEntry::Item {
                key,
                label,
                shortcut,
                enabled,
                checked,
            } => {
                let row = w::hbox(w::S6);
                let tick = if *checked { "\u{2713} " } else { "" };
                row.append(&w::label(&format!("{tick}{label}"), ""));
                if !shortcut.is_empty() {
                    row.append(&w::spacer());
                    row.append(&w::label(shortcut, "menu-key"));
                }
                let it = gtk::Button::builder().child(&row).build();
                it.add_css_class("menu-item");
                it.set_sensitive(*enabled);
                let (p2, conn, bus, path) =
                    (p.clone(), conn.clone(), bus.to_string(), path.to_string());
                let (id, key) = (m.id, key.clone());
                it.connect_clicked(move |_| {
                    p2.popdown();
                    activate(&conn, &bus, &path, id, &key);
                });
                col.append(&it);
            }
        }
    }
    p.set_child(Some(&col));
    p
}

/// `Activate(menu, key)` on the app; nothing comes back.
fn activate(conn: &gio::DBusConnection, bus: &str, path: &str, menu: u32, key: &str) {
    use glib::prelude::ToVariant;
    conn.call(
        Some(bus),
        path,
        appmenu::WOLKIT_MENUS,
        "Activate",
        Some(&(menu, key.to_string()).to_variant()),
        None,
        gio::DBusCallFlags::NO_AUTO_START,
        MENUS_TIMEOUT_MS,
        gio::Cancellable::NONE,
        |_| {},
    );
}

/// Once a menu in `host` is open, moving the pointer onto the title `b` opens that one.
fn hover_in(b: &gtk::MenuButton, host: &gtk::Box) {
    let motion = gtk::EventControllerMotion::new();
    let (b2, host) = (b.clone(), host.clone());
    motion.connect_enter(move |_, _, _| {
        let mut c = host.first_child();
        while let Some(x) = c {
            c = x.next_sibling();
            if let Some(m) = x.downcast_ref::<gtk::MenuButton>()
                && m != &b2
                && m.is_active()
            {
                m.set_active(false);
                b2.set_active(true);
                return;
            }
        }
    });
    b.add_controller(motion);
}

/// The Control Center page a status icon opens (audit2 A2-14: all four opened the main page).
/// Battery has no page of its own: the main page shows it.
pub fn status_page(tip: &str) -> &'static str {
    match tip {
        "Wi-Fi" => "wifi",
        "Sound" => "sound",
        "Focus" => "focus",
        _ => "main",
    }
}

/// Shell target for a status icon: `control-center:<page>` ([`super::ipc::cc_page`]).
fn status_target(tip: &str) -> String {
    format!("control-center:{}", status_page(tip))
}

/// WolOS menu (the logo), system menu order. Empty title = separator. Every power action has a
/// row (QA-FDAY-5: Sleep and Restart were only on the Super+Esc panel): Sleep acts at once, like
/// the session panel's; Restart…, Shut Down… and Log Out… open that panel on their question.
pub const LOGO_MENU: &[(&str, &[&str])] = &[
    ("About This Computer", &["wol-settings", "About"]),
    ("", &[]),
    ("System Settings…", &["wol-settings"]),
    ("App Store…", &["wol-store"]),
    ("", &[]),
    ("Sleep", &["wol", "power", "sleep"]),
    ("Restart…", &["wol", "shell", "show", "session-restart"]),
    ("Shut Down…", &["wol", "shell", "show", "session-shutdown"]),
    ("", &[]),
    (LOCK_ROW, &["wol-lock"]),
    ("Log Out…", &["wol", "shell", "show", "session-logout"]),
];

/// The Lock Screen row of [`LOGO_MENU`].
const LOCK_ROW: &str = "Lock Screen";

/// The WolOS menu on this kind of system. Live (`apps::LIVE_MARK`): no Lock Screen, the live
/// account has no password, so nothing could unlock it (QA-FDAY-7).
pub fn logo_menu(live: bool) -> Vec<(&'static str, &'static [&'static str])> {
    LOGO_MENU
        .iter()
        .copied()
        .filter(|(t, _)| !(live && *t == LOCK_ROW))
        .collect()
}

/// The WolOS menu's rows as the popover shows them, out of their popover (goldens), on the
/// popover's glass: kit.css draws `box.menu` like `popover.menu > contents` (the card had no
/// surface, so the dark goldens showed light text on the wallpaper).
pub fn logo_menu_view(live: bool) -> gtk::Widget {
    let p = w::menu(
        logo_menu(live)
            .into_iter()
            .map(|(t, argv)| (t, run(argv)))
            .collect(),
    );
    let rows = p.child().unwrap_or_else(|| w::vbox(0).upcast());
    p.set_child(None::<&gtk::Widget>);
    let card = w::vbox(0);
    card.add_css_class("menu");
    card.append(&rows);
    card.upcast()
}

fn run(argv: &'static [&'static str]) -> Box<dyn Fn()> {
    Box::new(move || kit::spawn(&argv.iter().map(|s| s.to_string()).collect::<Vec<_>>()))
}

/// The built-in items, made once and placed by the layout ([`Bar::arrange`]).
struct Items {
    logo: gtk::MenuButton,
    start: gtk::Button,
    focus: gtk::Button,
    battery: gtk::Button,
    network: gtk::Button,
    volume: gtk::Button,
    search: gtk::Button,
    control_center: gtk::Button,
    clock: gtk::Button,
    /// A red Stop while a screen recording runs ([`follow_recording`]); first in the status slot.
    record: gtk::Button,
    /// Apps' status items (SP-7, `ui::tray_items`).
    tray: Rc<super::tray_items::Strip>,
}

impl Bar {
    /// The widget of a built-in item (None: a plugin item, or one this bar does not draw yet:
    /// workspaces and tasks come with the taskbar style).
    fn builtin(&self, item: &BarItem) -> Option<gtk::Widget> {
        let i = &self.items;
        Some(match item {
            BarItem::Logo => i.logo.clone().upcast(),
            BarItem::AppName => self.app_menu.clone().upcast(),
            BarItem::AppMenus => self.app_menus.clone().upcast(),
            BarItem::Start => i.start.clone().upcast(),
            BarItem::Focus => i.focus.clone().upcast(),
            BarItem::Battery => i.battery.clone().upcast(),
            BarItem::Network => i.network.clone().upcast(),
            BarItem::Volume => i.volume.clone().upcast(),
            BarItem::Search => i.search.clone().upcast(),
            BarItem::ControlCenter => i.control_center.clone().upcast(),
            BarItem::Clock => i.clock.clone().upcast(),
            BarItem::Tray => i.tray.root.clone().upcast(),
            BarItem::Workspaces | BarItem::Tasks | BarItem::Plugin { .. } => {
                return None;
            }
        })
    }

    /// Places the items: the layout's three slots with the plugin items of this bar's host
    /// (`plugins::Host::bar`). Plugin items are made again; the old ones' timers end.
    pub fn arrange(&self) {
        let layout = self.layout.borrow().clone();
        let tray_placed = [&layout.left, &layout.centre, &layout.right]
            .iter()
            .any(|s| s.contains(&BarItem::Tray));
        let slots = match &self.plugins {
            Some(h) => h.borrow().bar(&layout),
            None => [layout.left, layout.centre, layout.right],
        };
        // A child of the bar's lifetime: the old one unlinks as it ends (audit5 S8-11).
        self.plugin_life.replace(self.life.child()).end();
        let life = self.plugin_life.borrow().clone();
        for slot in &self.slots {
            while let Some(c) = slot.first_child() {
                slot.remove(&c);
            }
        }
        for (n, (slot, items)) in self.slots.iter().zip(slots).enumerate() {
            // A layout without the tray item: the status slot starts with it (hidden when empty).
            if n == 2 {
                slot.append(&self.items.record);
            }
            if n == 2 && !tray_placed {
                slot.append(&self.items.tray.root);
            }
            for item in &items {
                if let Some(w) = self.builtin(item) {
                    slot.append(&w);
                } else if let (BarItem::Plugin { plugin, id }, Some(h)) = (item, &self.plugins) {
                    slot.append(&plugins::bar_item(h, plugin, id, &life));
                }
            }
        }
        self.slots[1].set_visible(self.slots[1].first_child().is_some());
    }

    /// A new layout (Settings, `shell.json`): items placed again, in the bar edge's direction.
    pub fn set_layout(&self, layout: Layout) {
        let o = kit::surface::orientation(layout.bar);
        self.root.set_orientation(o);
        for s in self.slots.iter().chain([&self.items.tray.root]) {
            s.set_orientation(o);
        }
        for s in &self.spacers {
            s.set_hexpand(o == gtk::Orientation::Horizontal);
            s.set_vexpand(o == gtk::Orientation::Vertical);
        }
        self.layout.replace(layout);
        self.arrange();
    }
}

pub fn content(sys: &dyn crate::system::System) -> Rc<Bar> {
    content_with(sys, Layout::default(), None)
}

/// A bar laid out by `layout`, with the plugin items of `plugins` (None: built-in items only).
pub fn content_with(
    sys: &dyn crate::system::System,
    layout: Layout,
    plugins: Option<Rc<RefCell<plugins::Host>>>,
) -> Rc<Bar> {
    let root = w::hbox(0);
    root.add_css_class("menubar-strip");
    let logo = gtk::MenuButton::new();
    logo.set_child(Some(&w::icon("wolos-glyph-symbolic", w::ICON)));
    logo.set_tooltip_text(Some("WolOS menu"));
    logo.set_valign(gtk::Align::Center);
    logo.set_popover(Some(&w::menu(
        logo_menu(crate::apps::is_live())
            .into_iter()
            .map(|(t, argv)| (t, run(argv)))
            .collect(),
    )));
    // Only menus backed by real actions: the app menu and Window (no placeholder File/Edit/View).
    let appname = w::label(crate::apps::DESKTOP_NAME, "app-name");
    let app_menu = w::menubar_menu(&appname);
    app_menu.set_visible(false);
    let window_menu = w::menubar_menu(&w::label("Window", ""));
    window_menu.set_popover(Some(&w::menu_keys(actions(appmenu::window_menu(), "", ""))));
    window_menu.set_visible(false);
    let app_menus = w::hbox(0);
    app_menus.append(&window_menu);
    let global = Rc::new(Global::new(&app_menus));
    hover_in(&window_menu, &app_menus);
    let slots = [w::hbox(0), w::hbox(0), w::hbox(0)];
    let spacers = [w::spacer(), w::spacer()];
    root.append(&slots[0]);
    root.append(&spacers[0]);
    root.append(&slots[1]);
    root.append(&spacers[1]);
    root.append(&slots[2]);
    for s in &slots {
        s.add_css_class(SLOT_CLASS);
    }

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
    let status = [
        (focus.upcast_ref::<gtk::Widget>(), "Focus"),
        (battery_box.upcast_ref::<gtk::Widget>(), "Battery"),
        (network.upcast_ref(), "Wi-Fi"),
        (volume.upcast_ref(), "Sound"),
    ]
    .map(|(img, tip)| {
        let b = w::menubar_item(img);
        b.set_tooltip_text(Some(tip));
        // In process (the bar is part of wol-shell): the Control Center on the icon's page.
        let target = status_target(tip);
        b.connect_clicked(move |_| {
            super::ipc::local(crate::protocol::Command {
                verb: crate::protocol::Verb::Toggle,
                target: target.clone(),
            })
        });
        b
    });
    let [search, control_center, start] = [
        ("system-search-symbolic", "Search", &["wol-search"][..]),
        (
            "view-grid-symbolic",
            "Control Center",
            &["wol", "shell", "toggle", "control-center"][..],
        ),
        ("wolos-glyph-symbolic", "Start", &["wol-search"][..]),
    ]
    .map(|(icon, tip, argv)| {
        let b = w::menubar_item(&w::icon(icon, w::ICON));
        b.set_tooltip_text(Some(tip));
        let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        b.connect_clicked(move |_| kit::spawn(&argv));
        b
    });
    let record = recording_item();
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
    let [focus_b, battery_b, network_b, volume_b] = status;
    let bar = Rc::new(Bar {
        root,
        appname,
        app_menu,
        window_menu,
        app_menus,
        global,
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
        items: Items {
            logo,
            start,
            focus: focus_b,
            battery: battery_b,
            network: network_b,
            volume: volume_b,
            search,
            control_center,
            clock: cb,
            record,
            tray: super::tray_items::strip(),
        },
        slots,
        spacers,
        layout: RefCell::new(Layout::default()),
        plugins,
        plugin_life: RefCell::new(Lifetime::new()),
    });
    bar.set_layout(layout);
    BARS.with_borrow_mut(|b| {
        b.retain(|x| x.strong_count() > 0);
        b.push(Rc::downgrade(&bar));
    });
    refresh(&bar, sys);
    follow_focus(&bar);
    follow_recording(&bar);
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
                read_layout(&b);
            }
        });
    }
    bar
}

/// Reads the layout in `shell.json` off the GTK thread; a different one is applied to `bar`
/// (and its window moves to the new edge).
fn read_layout(bar: &Rc<Bar>) {
    let b = Rc::downgrade(bar);
    glib::spawn_future_local(async move {
        let path = std::path::Path::new(&kit::config_home()).join(wol_core::prefs::SHELL_FILE);
        let text = gio::spawn_blocking(move || std::fs::read_to_string(path).ok())
            .await
            .ok()
            .flatten();
        let l = wol_core::prefs::layout(text.as_deref());
        if let Some(b) = b.upgrade()
            && *b.layout.borrow() != l
        {
            if let Some(w) = b.root.root().and_downcast::<gtk::Window>() {
                kit::surface::reanchor(&w, kit::surface::Spec::bar(l.bar));
            }
            b.set_layout(l);
        }
    });
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
    let host = Rc::new(RefCell::new(plugins::Host::new(
        plugins::Paths::user(),
        plugins::BAR_KINDS,
    )));
    let bar = content_with(sys, Layout::default(), Some(host.clone()));
    bar.life.tie(&win);
    super::tray_items::start();
    {
        // Plugins load off the GTK thread and again whenever their folder changes; the
        // primary bar registers their hotkeys.
        let b = Rc::downgrade(&bar);
        let last = Rc::new(RefCell::new(Vec::new()));
        let paths = host.borrow().paths().clone();
        if primary {
            // A config reload drops every `keyword` bind (audit5 S9-4): put the plugin hotkeys in
            // again. The hook lives for the session; a bar that is gone does nothing.
            let (b, paths, last) = (Rc::downgrade(&bar), paths.clone(), last.clone());
            crate::hypr::on_config_reloaded(move || {
                if b.upgrade().is_some() {
                    plugins::replay_hotkeys(&paths, &last);
                }
            });
        }
        plugins::watch(
            &host,
            &bar.life,
            Rc::new(move || {
                if let Some(b) = b.upgrade() {
                    b.arrange();
                }
                if primary {
                    plugins::register_hotkeys(&paths, &last);
                }
            }),
        );
    }
    read_layout(&bar);
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

/// The menu to open when the pointer enters title `hovered` while menu `open` is open.
pub fn hover_switch(open: Option<usize>, hovered: usize) -> Option<usize> {
    open.filter(|o| *o != hovered).map(|_| hovered)
}

/// The bar's items in order: the children of its slot boxes.
fn items(root: &gtk::Box) -> Vec<gtk::Widget> {
    let mut out = Vec::new();
    let mut c = root.first_child();
    while let Some(w) = c {
        c = w.next_sibling();
        if w.has_css_class(SLOT_CLASS) {
            let mut i = w.first_child();
            while let Some(x) = i {
                i = x.next_sibling();
                out.push(x);
            }
        } else {
            out.push(w);
        }
    }
    out
}

/// Once a menu-bar menu is open, moving the pointer onto another title opens that one.
fn hover_menus(root: &gtk::Box) {
    let menus: Vec<gtk::MenuButton> = items(root)
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::MenuButton>().ok())
        .collect();
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
    let all = items(root);
    let Some(first) = all.first() else {
        return Rc::new(|_| {});
    };
    let access = kit::surface::keyboard_access(win, first, |_| {});
    for w in all {
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
    }
    access
}

/// One status read; widgets touched only on change. Volume change -> OSD.
fn refresh(bar: &Bar, sys: &dyn crate::system::System) {
    let audio = sys.audio();
    let mut s = bar.seen.borrow_mut();
    let n = status::menubar_glyph(sys.network(), sys.wifi_enabled(), sys.airplane());
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

/// Tooltip of the recording item.
pub const STOP_RECORDING: &str = "Stop Screen Recording";

/// The menu bar's recording item: a red record mark that stops the recording (live-1 H4: a
/// recording ran with nothing on screen saying so and nothing to stop it).
fn recording_item() -> gtk::Button {
    let dot = w::icon("media-record-symbolic", w::ICON);
    dot.add_css_class("tone-danger");
    let b = w::menubar_item(&dot);
    b.set_tooltip_text(Some(STOP_RECORDING));
    b.set_visible(false);
    b.connect_clicked(|b| {
        b.set_visible(false);
        kit::spawn(&crate::capture::STOP_ARGV.map(String::from));
    });
    b
}

/// The recording item shows while wol-screenshot's pid file is there, whoever started the
/// recording (toolbar, Record tile, hotkey), and goes when it ends.
fn follow_recording(bar: &Rc<Bar>) {
    use gtk::gio;
    let file = crate::capture::record_pid_file(&kit::runtime_dir());
    bar.items.record.set_visible(file.exists());
    let Ok(m) = gio::File::for_path(&file)
        .monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
    else {
        return;
    };
    let b = Rc::downgrade(bar);
    m.connect_changed(move |_, _, _, _| {
        if let Some(bar) = b.upgrade() {
            bar.items.record.set_visible(file.exists());
        }
    });
    bar.life.on_end(move || {
        m.cancel();
    });
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

/// The status-item strip of every live menu bar (`ui::tray_items`).
pub fn tray_strips() -> Vec<Rc<super::tray_items::Strip>> {
    bars().iter().map(|b| b.items.tray.clone()).collect()
}

/// The app in front changed: every bar shows it.
pub fn set_app_all(name: String, class: String) {
    LAST_APP.set(Some((name.clone(), class.clone())));
    for b in bars() {
        b.set_app(name.clone(), class.clone());
    }
}

/// The app the bar names once a window closed: none left means the desktop (empty class and
/// title), else the compositor's own `activewindow` events decide (QA-LN-6).
pub fn app_when_no_client(clients: &[crate::hypr::Client]) -> Option<(String, String)> {
    clients
        .iter()
        .all(|c| c.class.is_empty())
        .then(|| (String::new(), String::new()))
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
        crate::alerts::poll(sys.battery(), sys.connectivity());
        glib::ControlFlow::Continue
    });
}

#[cfg(test)]
mod hover_tests {
    use super::hover_switch;

    /// With a menu open, the pointer over another title opens that one instead.
    #[test]
    fn hovering_a_title_switches_only_while_a_menu_is_open() {
        assert_eq!(hover_switch(Some(0), 2), Some(2));
        assert_eq!(hover_switch(Some(2), 2), None);
        assert_eq!(hover_switch(None, 1), None);
    }
}

#[cfg(test)]
mod logo_tests {
    use super::{LOGO_MENU, status_page, status_target};

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

    /// Audit2 A2-14: Wi-Fi, Sound and Focus open their own Control Center page.
    #[test]
    fn status_icons_open_their_control_center_page() {
        use crate::ui::control_center::PAGES;
        for (tip, page) in [
            ("Wi-Fi", "wifi"),
            ("Sound", "sound"),
            ("Focus", "focus"),
            ("Battery", "main"),
        ] {
            assert_eq!(status_page(tip), page, "{tip}");
            assert!(PAGES.contains(&page), "{page}");
            assert_eq!(
                crate::ui::ipc::cc_page(&status_target(tip)),
                Some(page),
                "{tip}"
            );
        }
    }

    /// QA-FDAY-7: on the live stick Lock Screen was a dead end (the live account's password is
    /// locked, every unlock says Incorrect password). The live menu has no Lock Screen; an
    /// installed system keeps it.
    #[test]
    fn live_hides_lock() {
        use super::logo_menu;
        let titles = |live| logo_menu(live).iter().map(|r| r.0).collect::<Vec<_>>();
        assert!(titles(false).contains(&"Lock Screen"));
        assert!(!titles(true).contains(&"Lock Screen"), "{:?}", titles(true));
        assert!(
            logo_menu(true).iter().all(|(_, a)| *a != ["wol-lock"]),
            "nothing in the live menu starts wol-lock"
        );
        // Everything else is the same menu.
        assert_eq!(titles(true).len(), titles(false).len() - 1);
        for live in [false, true] {
            let m = logo_menu(live);
            assert!(!m.last().unwrap().0.is_empty(), "no separator last");
            assert!(
                m.windows(2)
                    .all(|w| !(w[0].0.is_empty() && w[1].0.is_empty()))
            );
        }
    }

    /// QA-LN-6: the last app ended by a signal and the bar kept "Terminal  Window" for good: with
    /// no client left the bar names the desktop; with one left the compositor's events decide.
    #[test]
    fn last_client_gone_clears_app_name() {
        use super::app_when_no_client;
        let c = |class: &str| crate::hypr::Client::new("0x1", class, "");
        assert_eq!(
            app_when_no_client(&[]),
            Some((String::new(), String::new()))
        );
        assert_eq!(
            app_when_no_client(&[c("")]),
            Some((String::new(), String::new()))
        );
        assert_eq!(app_when_no_client(&[c("foot")]), None);
    }

    /// QA-FDAY-5: the WolOS menu had no Sleep and no Restart (only Super+Esc reached them). Both
    /// are rows now, before Shut Down…: Sleep runs the session panel's own command, Restart…
    /// asks first on the session panel.
    #[test]
    fn wolos_menu_has_sleep_and_restart() {
        let titles: Vec<&str> = LOGO_MENU.iter().map(|r| r.0).collect();
        let at = |t: &str| {
            titles
                .iter()
                .position(|x| *x == t)
                .unwrap_or_else(|| panic!("{t}"))
        };
        assert!(at("Sleep") < at("Restart…") && at("Restart…") < at("Shut Down…"));
        let argv = |title: &str| LOGO_MENU.iter().find(|r| r.0 == title).unwrap().1;
        assert_eq!(
            Some(argv("Sleep")),
            crate::clipboard::session_command("sleep"),
            "Sleep runs what the session panel's Sleep runs"
        );
        let [_, _, _, target] = argv("Restart…") else {
            panic!("wol shell show <target>")
        };
        assert_eq!(crate::ui::panels::session_action(target), Some("restart"));
    }

    /// Live: Log Out… and Shut Down… both toggled the plain session panel. Each opens it on its
    /// own question, through a shell target the CLI and wol-shell both know.
    #[test]
    fn log_out_and_shut_down_ask_their_own_question() {
        let argv = |title: &str| LOGO_MENU.iter().find(|r| r.0 == title).unwrap().1;
        let (out, down, re) = (argv("Log Out…"), argv("Shut Down…"), argv("Restart…"));
        assert_ne!(out, down);
        for a in [out, down, re] {
            let [wol, shell, verb, target] = a else {
                panic!("{a:?}: wol shell <verb> <target>")
            };
            assert_eq!((*wol, *shell, *verb), ("wol", "shell", "show"));
            assert!(shared::shell::PANELS.contains(target), "{target}");
            assert!(
                crate::ui::panels::session_action(target).is_some(),
                "{target}"
            );
        }
    }
}
