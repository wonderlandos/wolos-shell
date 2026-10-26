//! Control Center (port of IaraControlCenter/ControlCenter.tsx): connectivity, focus, appearance, display,
//! sound, now playing, battery/power, capture. Every action goes through `System`; admin actions use the Authenticate sheet.
use crate::kit::{
    self,
    surface::{Keyboard, Spec},
    widgets as w,
};
use crate::system::{ActionError, PowerMode, Sysfs, System};
use gtk::{glib, prelude::*};
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
        build,
    )
}

fn dnd_path() -> std::path::PathBuf {
    std::path::Path::new(&kit::config_home()).join(DND_FILE)
}

struct Ui {
    sys: Rc<dyn System>,
    message: gtk::Label,
    sheet: gtk::Revealer,
    sheet_field: gtk::PasswordEntry,
    pending: std::cell::RefCell<Option<Box<dyn Fn() -> Result<(), ActionError>>>>,
}

impl Ui {
    /// Runs an action; NeedsAuth opens the Authenticate sheet and retries after `authorize`.
    fn act(self: &Rc<Self>, f: impl Fn() -> Result<(), ActionError> + 'static) {
        match f() {
            Ok(()) => self.message.set_label(""),
            Err(ActionError::NeedsAuth) => {
                *self.pending.borrow_mut() = Some(Box::new(f));
                self.sheet.set_reveal_child(true);
                self.sheet_field.grab_focus();
            }
            Err(e) => self.message.set_label(e.message()),
        }
    }
    fn allow(self: &Rc<Self>) {
        let pw = self.sheet_field.text();
        self.sheet_field.set_text("");
        match self.sys.authorize(&pw) {
            Ok(()) => {
                self.sheet.set_reveal_child(false);
                if let Some(f) = self.pending.borrow_mut().take() {
                    self.act(f);
                }
            }
            Err(e) => self.message.set_label(e.message()),
        }
    }
}

fn auth_sheet() -> (gtk::Revealer, gtk::PasswordEntry, gtk::Button, gtk::Button) {
    let b = w::vbox(8);
    b.add_css_class("tile");
    b.append(&w::label("Authenticate", "strong"));
    let l = w::label(
        "Control Center is trying to make changes. Enter your password to allow this.",
        "dim",
    );
    l.set_wrap(true);
    l.set_max_width_chars(36);
    b.append(&l);
    let field = gtk::PasswordEntry::builder()
        .placeholder_text("Password")
        .build();
    field.add_css_class("wol-field");
    b.append(&field);
    let row = w::hbox(6);
    row.append(&w::spacer());
    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("wol-button");
    cancel.add_css_class("secondary");
    let allow = gtk::Button::with_label("Allow");
    allow.add_css_class("wol-button");
    row.append(&cancel);
    row.append(&allow);
    b.append(&row);
    let r = gtk::Revealer::builder()
        .child(&b)
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .build();
    (r, field, cancel, allow)
}

fn build(_win: &gtk::ApplicationWindow) -> gtk::Widget {
    let sys: Rc<dyn System> = Rc::new(Sysfs::default());
    let panel = w::glass_panel(10);
    let (sheet, sheet_field, cancel, allow) = auth_sheet();
    let ui = Rc::new(Ui {
        sys: sys.clone(),
        message: w::label("", "dim"),
        sheet,
        sheet_field,
        pending: Default::default(),
    });
    {
        let u = ui.clone();
        cancel.connect_clicked(move |_| {
            u.pending.borrow_mut().take();
            u.sheet.set_reveal_child(false);
        });
    }
    {
        let u = ui.clone();
        allow.connect_clicked(move |_| u.allow());
    }
    {
        let u = ui.clone();
        ui.sheet_field.connect_activate(move |_| u.allow());
    }
    panel.append(&ui.sheet);

    let net = sys.network();
    let nets = sys.wifi_networks();
    let top = w::hbox(10);
    top.set_homogeneous(true);
    let conn = w::vbox(8);
    let u = ui.clone();
    let s2 = sys.clone();
    conn.append(&w::switch_row(
        &format!("Wi-Fi · {}", crate::system::wifi_subtitle(net, &nets)),
        net.wifi,
        move |on| {
            let s = s2.clone();
            u.act(move || s.set_wifi(on))
        },
    ));
    let (u, s2) = (ui.clone(), sys.clone());
    conn.append(&w::switch_row(
        &format!(
            "Bluetooth · {}",
            match sys.bluetooth() {
                Some(true) => "On",
                Some(false) => "Off",
                None => "Unavailable",
            }
        ),
        sys.bluetooth() == Some(true),
        move |on| {
            let s = s2.clone();
            u.act(move || s.set_bluetooth(on))
        },
    ));
    top.append(&w::tile("Connectivity", &conn));
    let right = w::vbox(10);
    right.append(&w::tile(
        "Focus",
        &w::switch_row("Do Not Disturb", dnd_path().exists(), |on| {
            let p = dnd_path();
            let _ = if on {
                p.parent().map(std::fs::create_dir_all);
                std::fs::write(&p, "")
            } else {
                std::fs::remove_file(&p)
            };
        }),
    ));
    let dark = gtk::Settings::default().is_some_and(|s| s.is_gtk_application_prefer_dark_theme());
    right.append(&w::tile(
        "Appearance",
        &w::switch_row("Dark Mode", dark, |on| {
            let _ = kit::style::set_active(if on { "wol-dark" } else { "wol-light" });
        }),
    ));
    if let Some(n) = sys.night_light() {
        let (u, s2) = (ui.clone(), sys.clone());
        right.append(&w::tile(
            "Display",
            &w::switch_row("Night Shift", n, move |on| {
                let s = s2.clone();
                u.act(move || s.set_night_light(on))
            }),
        ));
    }
    top.append(&right);
    panel.append(&top);

    // Wi-Fi networks + join.
    let list = w::vbox(2);
    if nets.is_empty() {
        list.append(&w::label("No networks found", "dim"));
    }
    let join = w::vbox(6);
    let join_rev = gtk::Revealer::builder().child(&join).build();
    let join_title = w::label("", "strong");
    let join_pw = gtk::PasswordEntry::builder()
        .placeholder_text("Password")
        .build();
    join_pw.add_css_class("wol-field");
    join.append(&join_title);
    join.append(&join_pw);
    let pending_ssid: Rc<std::cell::RefCell<String>> = Default::default();
    let jr = w::hbox(6);
    jr.append(&w::spacer());
    {
        let r = join_rev.clone();
        jr.append(&w::button("Cancel", "secondary", move || {
            r.set_reveal_child(false)
        }));
    }
    {
        let (u, s2, pw, ssid, r) = (
            ui.clone(),
            sys.clone(),
            join_pw.clone(),
            pending_ssid.clone(),
            join_rev.clone(),
        );
        jr.append(&w::button("Join", "", move || {
            let (s, id, p) = (s2.clone(), ssid.borrow().clone(), pw.text().to_string());
            r.set_reveal_child(false);
            u.act(move || s.wifi_join(&id, Some(&p)));
        }));
    }
    join.append(&jr);
    for n in nets.iter().take(8) {
        let row = w::list_row(
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
        let (n2, u, s2, t, rev, ssid) = (
            n.clone(),
            ui.clone(),
            sys.clone(),
            join_title.clone(),
            join_rev.clone(),
            pending_ssid.clone(),
        );
        row.connect_clicked(move |_| {
            if n2.secure && !n2.connected {
                *ssid.borrow_mut() = n2.ssid.clone();
                t.set_label(&format!("Join “{}”", n2.ssid));
                rev.set_reveal_child(true);
            } else {
                let (s, id) = (s2.clone(), n2.ssid.clone());
                u.act(move || s.wifi_join(&id, None));
            }
        });
        list.append(&row);
    }
    let exp_box = w::vbox(6);
    exp_box.append(&list);
    exp_box.append(&join_rev);
    let exp = gtk::Expander::builder()
        .label("Wi-Fi Networks")
        .child(&exp_box)
        .build();
    panel.append(&exp);

    if let Some(b) = sys.brightness() {
        let (u, s2) = (ui.clone(), sys.clone());
        panel.append(&w::tile(
            "Display",
            &w::slider(Some("display-brightness-symbolic"), b as f64, move |v| {
                let s = s2.clone();
                u.act(move || s.set_brightness(v as u8))
            }),
        ));
    }
    let audio = sys.audio();
    let sound = w::vbox(6);
    let row = w::hbox(6);
    {
        let (u, s2) = (ui.clone(), sys.clone());
        row.append(&w::icon_button(
            crate::status::volume(audio),
            "Mute",
            move || {
                let s = s2.clone();
                u.act(move || s.set_muted(!audio.muted))
            },
        ));
    }
    {
        let (u, s2) = (ui.clone(), sys.clone());
        let sl = w::slider(None, audio.volume.unwrap_or(0) as f64, move |v| {
            let s = s2.clone();
            u.act(move || s.set_volume(v as u8))
        });
        sl.set_hexpand(true);
        row.append(&sl);
    }
    sound.append(&row);
    let outs = sys.outputs();
    if outs.len() > 1 {
        let r = w::hbox(6);
        r.append(&w::label("Output", "dim"));
        r.append(&w::spacer());
        let names: Vec<&str> = outs.iter().map(String::as_str).collect();
        r.append(&gtk::DropDown::from_strings(&names));
        sound.append(&r);
    }
    panel.append(&w::tile("Sound", &sound));
    if let Some(np) = sys.now_playing() {
        let r = w::hbox(8);
        let t = w::vbox(0);
        t.append(&w::label(&np.title, "strong"));
        t.append(&w::label(&np.artist, "dim"));
        t.set_hexpand(true);
        r.append(&t);
        let (u, s2) = (ui.clone(), sys.clone());
        r.append(&w::icon_button(
            if np.playing {
                "media-playback-pause-symbolic"
            } else {
                "media-playback-start-symbolic"
            },
            "Play/Pause",
            move || {
                let s = s2.clone();
                u.act(move || s.media_toggle())
            },
        ));
        panel.append(&w::tile("Now Playing", &r));
    }
    if let Some(label) = crate::system::battery_label(sys.battery()) {
        let b = w::vbox(8);
        let r = w::hbox(8);
        r.append(&gtk::Image::from_icon_name(
            crate::status::battery(sys.battery()).unwrap_or("battery-good-symbolic"),
        ));
        r.append(&w::label(&label, ""));
        b.append(&r);
        if let Some(m) = sys.power_mode() {
            let modes = [
                PowerMode::LowPower,
                PowerMode::Automatic,
                PowerMode::HighPerformance,
            ];
            let (u, s2) = (ui.clone(), sys.clone());
            b.append(&w::segmented(
                &["Low Power", "Automatic", "High"],
                modes.iter().position(|x| *x == m).unwrap_or(1),
                move |i| {
                    let s = s2.clone();
                    u.act(move || s.set_power_mode(modes[i]))
                },
            ));
        }
        panel.append(&w::tile("Battery", &b));
    }
    let cap = w::hbox(8);
    cap.set_homogeneous(true);
    for (icon, label, verb) in [
        ("camera-photo-symbolic", "Screenshot", "screenshot"),
        ("media-record-symbolic", "Record Screen", "record"),
    ] {
        let b = w::list_row(icon, label, "");
        b.add_css_class("tile");
        b.connect_clicked(move |b| {
            if let Some(w) = b.root().and_downcast::<gtk::Window>() {
                w.close();
            }
            kit::spawn(&["wol".into(), "capture".into(), verb.into()]);
        });
        cap.append(&b);
    }
    panel.append(&cap);
    panel.append(&ui.message);
    panel.upcast()
}
