//! Control Center (macOS layout; port of IaraControlCenter/ControlCenter.tsx). Every action goes through `System`;
//! admin actions open the Authenticate sheet (IaraOS ADR 0026).
use crate::kit::{
    self,
    surface::{Keyboard, Spec},
    widgets as w,
};
use crate::system::{ActionError, System};
use gtk::{glib, prelude::*};
use std::cell::RefCell;
use std::rc::Rc;

pub const DND_FILE: &str = "wol/dnd";

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

fn dnd_path() -> std::path::PathBuf {
    std::path::Path::new(&kit::config_home()).join(DND_FILE)
}

type Action = Box<dyn Fn() -> Result<(), ActionError>>;

struct Ui {
    sys: Rc<dyn System>,
    message: gtk::Label,
    auth: w::AuthSheet,
    pending: RefCell<Option<Action>>,
}

impl Ui {
    /// Runs an action; `NeedsAuth` opens the Authenticate sheet and retries after `authorize`.
    fn act(self: &Rc<Self>, f: impl Fn() -> Result<(), ActionError> + 'static) {
        match f() {
            Ok(()) => self.message.set_visible(false),
            Err(ActionError::NeedsAuth) => {
                *self.pending.borrow_mut() = Some(Box::new(f));
                self.auth.revealer.set_reveal_child(true);
                self.auth.field.grab_focus();
            }
            Err(e) => {
                self.message.set_label(e.message());
                self.message.set_visible(true);
            }
        }
    }
    fn allow(self: &Rc<Self>) {
        let pw = self.auth.field.text();
        self.auth.field.set_text("");
        match self.sys.authorize(&pw) {
            Ok(()) => {
                self.auth.revealer.set_reveal_child(false);
                if let Some(f) = self.pending.borrow_mut().take() {
                    self.act(f);
                }
            }
            Err(e) => self.auth.message.set_label(e.message()),
        }
    }
}

pub fn content(sys: Rc<dyn System>) -> gtk::Widget {
    let panel = w::glass_panel(w::S2);
    panel.add_css_class("control-center");
    panel.set_width_request(w::CONTROL_CENTER_WIDTH);
    panel.set_valign(gtk::Align::Start);
    let ui = Rc::new(Ui {
        sys: sys.clone(),
        message: w::label("", "secondary"),
        auth: w::authenticate_sheet("Control Center"),
        pending: Default::default(),
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
    let wifi_on = net.wifi;
    let wifi = w::ControlTile::new(
        "network-wireless-symbolic",
        "Wi-Fi",
        &crate::system::wifi_subtitle(net, &nets),
        Some(wifi_on),
    );
    {
        let (u, s, t) = (ui.clone(), sys.clone(), wifi.circle.clone());
        wifi.circle.connect_clicked(move |_| {
            let on = !t.has_css_class("on");
            let s = s.clone();
            u.act(move || s.set_wifi(on));
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
        match bt {
            Some(true) => "On",
            Some(false) => "Off",
            None => "Unavailable",
        },
        bt,
    );
    {
        let (u, s) = (ui.clone(), sys.clone());
        let t = bluetooth.circle.clone();
        bluetooth.circle.connect_clicked(move |_| {
            let on = !t.has_css_class("on");
            let s = s.clone();
            u.act(move || s.set_bluetooth(on));
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

    if let Some(b) = sys.brightness() {
        let (u, s) = (ui.clone(), sys.clone());
        main.append(&w::control_slider(
            "Display",
            "display-brightness-symbolic",
            b as f64,
            move |v| {
                let s = s.clone();
                u.act(move || s.set_brightness(v as u8))
            },
        ));
    }
    let audio = sys.audio();
    let (u, s) = (ui.clone(), sys.clone());
    let sound = w::control_slider(
        "Sound",
        crate::status::volume(audio),
        audio.volume.unwrap_or(0) as f64,
        move |v| {
            let s = s.clone();
            u.act(move || s.set_volume(v as u8))
        },
    );
    main.append(&sound);

    if let Some(np) = sys.now_playing() {
        let r = w::hbox(w::S3);
        r.append(&w::icon("audio-x-generic-symbolic", w::ICON_M));
        let t = w::vbox(0);
        t.set_hexpand(true);
        t.append(&w::label(&np.title, "title"));
        t.append(&w::label(&np.artist, "secondary"));
        r.append(&t);
        let (u, s) = (ui.clone(), sys.clone());
        r.append(&w::icon_button(
            if np.playing {
                "media-playback-pause-symbolic"
            } else {
                "media-playback-start-symbolic"
            },
            "Play or pause",
            move || {
                let s = s.clone();
                u.act(move || s.media_toggle())
            },
        ));
        main.append(&w::module(&r));
    }

    let tiles = w::hbox(w::S2);
    tiles.set_homogeneous(true);
    if let Some(on) = sys.night_light() {
        let (u, s) = (ui.clone(), sys.clone());
        let t = w::square_tile("night-light-symbolic", "Night Shift", move |b| {
            let on = !b.has_css_class("on");
            let s = s.clone();
            u.act(move || s.set_night_light(on));
        });
        if on {
            t.add_css_class("on");
        }
        tiles.append(&t);
    }
    for (icon, title, verb) in [
        ("camera-photo-symbolic", "Screenshot", "screenshot"),
        ("media-record-symbolic", "Record", "record"),
    ] {
        tiles.append(&w::square_tile(icon, title, move |b| {
            if let Some(win) = b.root().and_downcast::<gtk::Window>() {
                win.close();
            }
            kit::spawn(&["wol".into(), "capture".into(), verb.into()]);
        }));
    }
    main.append(&tiles);
    ui.message.set_visible(false);
    main.append(&ui.message);
    panel.upcast()
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
            let (s, id, p) = (u.sys.clone(), id.borrow().clone(), pw.text().to_string());
            r.set_reveal_child(false);
            u.act(move || s.wifi_join(&id, Some(&p)));
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
                let (s, id) = (u.sys.clone(), n.ssid.clone());
                u.act(move || s.wifi_join(&id, None));
            }
        });
        list.append(&b);
    }
    page.append(&w::module(&list));
    page.append(&join);
    page
}
