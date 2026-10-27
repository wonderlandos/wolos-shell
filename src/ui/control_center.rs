//! Control Center (macOS layout; port of IaraControlCenter/ControlCenter.tsx). Every action goes through `System`;
//! admin actions open the Authenticate sheet (IaraOS ADR 0026).
use crate::kit::{
    self,
    surface::{Keyboard, Spec},
    widgets as w,
};
use crate::system::{Action, ActionError, System};
use gtk::{glib, prelude::*};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub const DND_FILE: &str = "wol/dnd";

/// Every argv the Control Center spawns (tests/cc_spawns.rs checks each resolves).
pub const SPAWNS: &[&[&str]] = &[SCREENSHOT, RECORD];
/// Screenshot tile: the capture toolbar, like Super+Shift+5.
const SCREENSHOT: &[&str] = &["wol", "shell", "show", "screenshot"];
/// Record tile: the capture toolbar in Record Entire Screen mode.
const RECORD: &[&str] = &["wol", "shell", "show", crate::ui::capture::RECORD];

pub fn run() -> glib::ExitCode {
    super::run_panel_app(
        "org.wolos.controlcenter",
        "controlcenter",
        Spec::panel(
            "wol-panel-controlcenter",
            [true, false, false, true],
            Keyboard::OnDemand,
        ),
        |_, sys| content(sys),
    )
}

/// Resident panel in wol-shell (`wol shell toggle control-center`): opening is one frame, no
/// process start. Content is built on show and dropped on hide, so its snapshot watcher (1 s
/// refresh) only lives while the panel is visible.
pub fn panel(app: &gtk::Application, sys: Rc<dyn System>) -> gtk::Window {
    let win = super::window(
        app,
        "controlcenter",
        Spec::panel(
            "wol-panel-controlcenter",
            [true, false, false, true],
            Keyboard::OnDemand,
        ),
    );
    win.set_hide_on_close(true);
    // Built once (live perf: rebuilding per open leaked ~0.5 MB each); the watcher (1 s refresh)
    // only lives while visible.
    let (content, sync) = build(sys.clone());
    win.set_child(Some(&content));
    let running: Rc<RefCell<Option<glib::JoinHandle<()>>>> = Default::default();
    {
        let (running, sync) = (running.clone(), sync.clone());
        win.connect_show(move |_| {
            sync();
            *running.borrow_mut() = follow(&sys, sync.clone());
        });
    }
    win.connect_hide(move |_| {
        if let Some(h) = running.borrow_mut().take() {
            h.abort();
        }
    });
    let keys = gtk::EventControllerKey::new();
    let w2 = win.clone();
    keys.connect_key_pressed(move |_, k, _, _| {
        if k == gtk::gdk::Key::Escape {
            w2.set_visible(false);
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    win.add_controller(keys);
    win.upcast()
}

fn dnd_path() -> std::path::PathBuf {
    std::path::Path::new(&kit::config_home()).join(DND_FILE)
}

struct Ui {
    sys: Rc<dyn System>,
    message: gtk::Label,
    auth: w::AuthSheet,
    /// Admin action waiting for the password.
    pending: RefCell<Option<Action>>,
    /// Slider actions: one in flight per kind, newest value wins (no fork storm while dragging).
    busy: RefCell<std::collections::HashMap<&'static str, Option<Action>>>,
    /// True while a snapshot pushes values into widgets (their handlers must not act).
    syncing: Cell<bool>,
}

impl Ui {
    /// Runs `a` off the UI thread; `NeedsAuth`/`Denied` open the Authenticate sheet and retry after Allow.
    fn act(self: &Rc<Self>, a: Action) {
        let rx = self.sys.spawn(a.clone());
        let u = self.clone();
        glib::spawn_future_local(async move {
            let Ok(r) = rx.recv().await else { return };
            u.done(a, r);
        });
    }
    fn done(self: &Rc<Self>, a: Action, r: Result<(), ActionError>) {
        match r {
            Ok(()) => self.message.set_visible(false),
            Err(e @ (ActionError::NeedsAuth | ActionError::Denied)) => {
                self.auth.message.set_label(if e == ActionError::Denied {
                    e.message()
                } else {
                    ""
                });
                *self.pending.borrow_mut() = Some(a);
                self.auth.revealer.set_reveal_child(true);
                self.auth.field.grab_focus();
            }
            Err(e) => {
                self.message.set_label(e.message());
                self.message.set_visible(true);
            }
        }
    }
    /// Coalesced variant of [`Ui::act`] for sliders.
    fn act_latest(self: &Rc<Self>, kind: &'static str, a: Action) {
        if self.syncing.get() {
            return;
        }
        if let Some(slot) = self.busy.borrow_mut().get_mut(kind) {
            *slot = Some(a);
            return;
        }
        self.busy.borrow_mut().insert(kind, None);
        let rx = self.sys.spawn(a.clone());
        let u = self.clone();
        glib::spawn_future_local(async move {
            let r = rx.recv().await.unwrap_or(Ok(()));
            let next = u.busy.borrow_mut().remove(kind).flatten();
            if r.is_err() {
                u.done(a, r);
            }
            if let Some(n) = next {
                u.act_latest(kind, n);
            }
        });
    }
    fn allow(self: &Rc<Self>) {
        let pw = self.auth.field.text();
        self.auth.field.set_text("");
        match self.sys.authorize(&pw) {
            Ok(()) => {
                self.auth.revealer.set_reveal_child(false);
                if let Some(a) = self.pending.borrow_mut().take() {
                    self.act(a);
                }
            }
            Err(e) => self.auth.message.set_label(e.message()),
        }
    }
}

fn scale_in(w: &gtk::Widget) -> Option<gtk::Scale> {
    if let Some(s) = w.downcast_ref::<gtk::Scale>() {
        return Some(s.clone());
    }
    let mut c = w.first_child();
    while let Some(ch) = c {
        if let Some(s) = scale_in(&ch) {
            return Some(s);
        }
        c = ch.next_sibling();
    }
    None
}

fn bluetooth_label(bt: Option<bool>) -> &'static str {
    match bt {
        Some(true) => "On",
        Some(false) => "Off",
        None => "Unavailable",
    }
}

/// Widgets plus `sync` (push the current snapshot into them).
fn build(sys: Rc<dyn System>) -> (gtk::Widget, Rc<dyn Fn()>) {
    let panel = w::glass_panel(w::S2);
    panel.add_css_class("control-center");
    panel.set_width_request(w::CONTROL_CENTER_WIDTH);
    panel.set_valign(gtk::Align::Start);
    let ui = Rc::new(Ui {
        sys: sys.clone(),
        message: w::label("", "secondary"),
        auth: w::authenticate_sheet("Control Center"),
        pending: Default::default(),
        busy: Default::default(),
        syncing: Cell::new(false),
    });
    {
        let u = ui.clone();
        ui.auth.cancel.connect_clicked(move |_| {
            u.pending.borrow_mut().take();
            u.auth.revealer.set_reveal_child(false);
        });
    }
    {
        let u = ui.clone();
        ui.auth.allow.connect_clicked(move |_| u.allow());
    }
    {
        let u = ui.clone();
        ui.auth.field.connect_activate(move |_| u.allow());
    }
    panel.append(&ui.auth.revealer);

    let pages = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::SlideLeftRight)
        .vhomogeneous(false)
        .interpolate_size(true)
        .build();
    let main = w::vbox(w::S2);
    pages.add_named(&main, Some("main"));
    let wifi_page = wifi_page(&ui, &pages);
    pages.add_named(&wifi_page, Some("wifi"));
    panel.append(&pages);

    // Connectivity + Focus/Appearance.
    let net = sys.network();
    let nets = sys.wifi_networks();
    let top = w::hbox(w::S2);
    top.set_homogeneous(true);
    let conn = w::vbox(w::S2);
    let wifi_on = sys.wifi_enabled().unwrap_or(net.wifi);
    let wifi = w::ControlTile::new(
        "network-wireless-symbolic",
        "Wi-Fi",
        &crate::system::wifi_subtitle(net, &nets),
        Some(wifi_on),
    );
    {
        let (u, t) = (ui.clone(), wifi.circle.clone());
        wifi.circle.connect_clicked(move |_| {
            u.act(Action::Wifi(!t.has_css_class("on")));
        });
    }
    {
        let p = pages.clone();
        wifi.detail
            .connect_clicked(move |_| p.set_visible_child_name("wifi"));
    }
    conn.append(&wifi.root);
    let bt = sys.bluetooth();
    let bluetooth = w::ControlTile::new(
        "bluetooth-active-symbolic",
        "Bluetooth",
        bluetooth_label(bt),
        bt,
    );
    {
        let u = ui.clone();
        let t = bluetooth.circle.clone();
        bluetooth.circle.connect_clicked(move |_| {
            u.act(Action::Bluetooth(!t.has_css_class("on")));
        });
    }
    conn.append(&bluetooth.root);
    top.append(&w::module(&conn));
    let right = w::vbox(w::S2);
    let dnd = w::ControlTile::new(
        "notifications-disabled-symbolic",
        "Focus",
        "Do Not Disturb",
        Some(dnd_path().exists()),
    );
    {
        let c = dnd.circle.clone();
        dnd.circle.connect_clicked(move |_| {
            let p = dnd_path();
            let on = !c.has_css_class("on");
            let _ = if on {
                p.parent().map(std::fs::create_dir_all);
                std::fs::write(&p, "")
            } else {
                std::fs::remove_file(&p)
            };
            if on {
                c.add_css_class("on")
            } else {
                c.remove_css_class("on")
            }
        });
    }
    right.append(&w::module(&dnd.root));
    let dark = gtk::Settings::default().is_some_and(|s| s.is_gtk_application_prefer_dark_theme());
    let appearance = w::ControlTile::new(
        "weather-clear-night-symbolic",
        "Dark Mode",
        if dark { "On" } else { "Off" },
        Some(dark),
    );
    {
        let c = appearance.circle.clone();
        appearance.circle.connect_clicked(move |_| {
            let on = !c.has_css_class("on");
            let _ = kit::style::set_active(if on { "wol-dark" } else { "wol-light" });
            if on {
                c.add_css_class("on")
            } else {
                c.remove_css_class("on")
            }
        });
    }
    right.append(&w::module(&appearance.root));
    top.append(&right);
    main.append(&top);

    let display = {
        let u = ui.clone();
        w::control_slider(
            "Display",
            "display-brightness-symbolic",
            sys.brightness().unwrap_or(0) as f64,
            move |v| u.act_latest("brightness", Action::Brightness(v as u8)),
        )
    };
    display.set_visible(sys.brightness().is_some());
    main.append(&display);
    let audio = sys.audio();
    let sound = {
        let u = ui.clone();
        w::control_slider(
            "Sound",
            crate::status::volume(audio),
            audio.volume.unwrap_or(0) as f64,
            move |v| u.act_latest("volume", Action::Volume(v as u8)),
        )
    };
    main.append(&sound);

    if let Some(np) = sys.now_playing() {
        let r = w::hbox(w::S3);
        r.append(&w::icon("audio-x-generic-symbolic", w::ICON_M));
        let t = w::vbox(0);
        t.set_hexpand(true);
        t.append(&w::label(&np.title, "title"));
        t.append(&w::label(&np.artist, "secondary"));
        r.append(&t);
        let u = ui.clone();
        r.append(&w::icon_button(
            if np.playing {
                "media-playback-pause-symbolic"
            } else {
                "media-playback-start-symbolic"
            },
            "Play or pause",
            move || u.act(Action::MediaToggle),
        ));
        main.append(&w::module(&r));
    }

    let tiles = w::hbox(w::S2);
    tiles.set_homogeneous(true);
    if let Some(on) = sys.night_light() {
        let u = ui.clone();
        let t = w::square_tile("night-light-symbolic", "Night Shift", move |b| {
            let on = !b.has_css_class("on");
            if on {
                b.add_css_class("on")
            } else {
                b.remove_css_class("on")
            }
            u.act(Action::NightLight(on));
        });
        if on {
            t.add_css_class("on");
        }
        tiles.append(&t);
    }
    for (icon, title, argv) in [
        ("camera-photo-symbolic", "Screenshot", SCREENSHOT),
        ("media-record-symbolic", "Record", RECORD),
    ] {
        tiles.append(&w::square_tile(icon, title, move |b| {
            if let Some(win) = b.root().and_downcast::<gtk::Window>() {
                win.close();
            }
            kit::spawn(&argv.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        }));
    }
    main.append(&tiles);
    ui.message.set_visible(false);
    main.append(&ui.message);

    // Live: the backend snapshot refreshes off-thread; widgets follow it via `sync`.
    let sync: Rc<dyn Fn()> = {
        let (u, display, sound) = (ui.clone(), display.clone(), sound.clone());
        Rc::new(move || {
            let s = &u.sys;
            u.syncing.set(true);
            let nets = s.wifi_networks();
            wifi.set_subtitle(&crate::system::wifi_subtitle(s.network(), &nets));
            wifi.set_on(s.wifi_enabled().unwrap_or(false));
            bluetooth.set_subtitle(bluetooth_label(s.bluetooth()));
            bluetooth.set_on(s.bluetooth().unwrap_or(false));
            display.set_visible(s.brightness().is_some());
            if let (Some(b), Some(sc)) = (s.brightness(), scale_in(display.upcast_ref()))
                && !u.busy.borrow().contains_key("brightness")
            {
                sc.set_value(b as f64);
            }
            if let (Some(v), Some(sc)) = (s.audio().volume, scale_in(sound.upcast_ref()))
                && !u.busy.borrow().contains_key("volume")
            {
                sc.set_value(v as f64);
            }
            u.syncing.set(false);
        })
    };
    (panel.upcast(), sync)
}

/// Follows the snapshot until the returned handle is aborted (dropping the watcher).
fn follow(sys: &Rc<dyn System>, sync: Rc<dyn Fn()>) -> Option<glib::JoinHandle<()>> {
    let rx = sys.watch()?;
    Some(glib::spawn_future_local(async move {
        while rx.recv().await.is_ok() {
            sync();
        }
    }))
}

/// Control Center app content: follows the snapshot for the life of the process.
pub fn content(sys: Rc<dyn System>) -> gtk::Widget {
    let (w, sync) = build(sys.clone());
    std::mem::forget(follow(&sys, sync));
    w
}

/// Wi-Fi detail page: back, networks, join with password.
fn wifi_page(ui: &Rc<Ui>, pages: &gtk::Stack) -> gtk::Box {
    let page = w::vbox(w::S2);
    let head = w::hbox(w::S2);
    head.add_css_class("back-row");
    {
        let p = pages.clone();
        head.append(&w::icon_button("go-previous-symbolic", "Back", move || {
            p.set_visible_child_name("main")
        }));
    }
    head.append(&w::label("Wi-Fi", "title"));
    page.append(&head);
    let nets = ui.sys.wifi_networks();
    let list = w::vbox(0);
    if nets.is_empty() {
        list.append(&w::label("No networks found", "secondary"));
    }
    let join_title = w::label("", "title");
    let join_pw = w::password_field("Password");
    let ssid: Rc<RefCell<String>> = Default::default();
    let join_box = w::vbox(w::S2);
    join_box.append(&join_title);
    join_box.append(&join_pw);
    let row = w::hbox(w::S2);
    row.append(&w::spacer());
    let join = w::sheet(&join_box);
    {
        let r = join.clone();
        row.append(&w::button("Cancel", w::Kind::Default, move || {
            r.set_reveal_child(false)
        }));
    }
    {
        let (u, pw, id, r) = (ui.clone(), join_pw.clone(), ssid.clone(), join.clone());
        row.append(&w::button("Join", w::Kind::Primary, move || {
            let (id, p) = (id.borrow().clone(), pw.text().to_string());
            pw.set_text("");
            r.set_reveal_child(false);
            u.act(Action::WifiJoin(id, Some(p)));
        }));
    }
    join_box.append(&row);
    for n in nets.iter().take(8) {
        let b = w::list_button(
            crate::system::signal_icon(n.signal),
            &n.ssid,
            if n.connected {
                "Connected"
            } else if n.secure {
                "Secured"
            } else {
                "Open"
            },
        );
        let (n, u, t, r, id) = (
            n.clone(),
            ui.clone(),
            join_title.clone(),
            join.clone(),
            ssid.clone(),
        );
        b.connect_clicked(move |_| {
            if n.secure && !n.connected {
                *id.borrow_mut() = n.ssid.clone();
                t.set_label(&format!("Join “{}”", n.ssid));
                r.set_reveal_child(true);
            } else {
                u.act(Action::WifiJoin(n.ssid.clone(), None));
            }
        });
        list.append(&b);
    }
    page.append(&w::module(&list));
    page.append(&join);
    page
}
