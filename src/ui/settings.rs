//! System Settings (port of apps/IaraSettings) on the HIG AppWindow: sidebar of pages, grouped Section cards.
//! Every control applies: theme contract (Appearance), shell prefs + `reload` (Menu Bar & Dock), Hyprland
//! fragments + `hyprctl reload` (Keyboard, Displays), hypridle config (Lock & Idle), `System` actions
//! (Sound, Network, Accounts, Software Update). Slow work runs off the UI thread; root work asks through
//! the Authenticate sheet (IaraOS ADR 0026).
use crate::kit::{self, widgets as w};
use crate::system::{Action, ActionError, System};
use gtk::{glib, prelude::*};
use shared::theme::{Color, Variant};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use wol_core::prefs::{self, ShellPrefs};
use wol_core::theme::accent;

pub const PAGES: [&str; 10] = [
    "Appearance",
    "Menu Bar & Dock",
    "Keyboard",
    "Lock & Idle",
    "Displays",
    "Sound",
    "Network",
    "Accounts",
    "Software Update",
    "About",
];

pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("org.wolos.settings")
        .build();
    app.connect_activate(|app| {
        kit::style::install("");
        let win = gtk::ApplicationWindow::builder()
            .application(app)
            .title("System Settings")
            .default_width(w::APP_SIZE.0)
            .default_height(w::APP_SIZE.1)
            .decorated(false)
            .build();
        let aw = content(crate::system::default_system(), &Host::read());
        if let Some(p) = std::env::args().nth(1) {
            aw.select(&p);
        }
        win.set_child(Some(&aw.root));
        kit::snapshot_on_map(&win, "settings");
        win.present();
    });
    app.run_with_args::<&str>(&[])
}

/// Host facts shown by Accounts/About (injected so golden renders do not depend on the build machine).
#[derive(Clone, Debug, Default)]
pub struct Host {
    pub hostname: String,
    pub users: Vec<String>,
    pub os: String,
    pub processor: String,
    pub memory: String,
}

impl Host {
    pub fn read() -> Self {
        let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
        let shells = std::fs::read_to_string("/etc/shells").unwrap_or_default();
        let info = wol_core::system::info::SystemInfo::read(&wol_core::system::PosixHost);
        Host {
            hostname: std::fs::read_to_string("/etc/hostname")
                .unwrap_or_default()
                .trim()
                .to_string(),
            users: crate::login::users(&passwd, &shells),
            os: info.kernel,
            processor: info.processor,
            memory: info.memory,
        }
    }
}

/// `$XDG_CONFIG_HOME/<rel>`.
fn config(rel: &str) -> PathBuf {
    PathBuf::from(kit::config_home()).join(rel)
}

fn read(p: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(p).ok()
}

/// Atomic write (watchers never see half a file).
fn write(p: &std::path::Path, text: &str) -> std::io::Result<()> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, p)
}

/// Runs argv lists in order on a worker thread (2 s timeout each); never blocks the UI.
fn background(cmds: Vec<Vec<String>>) {
    std::thread::spawn(move || {
        for c in cmds {
            let argv: Vec<&str> = c.iter().map(String::as_str).collect();
            let _ = wol_core::system::run_with_timeout(&argv, wol_core::system::RUN_TIMEOUT);
        }
    });
}

fn argv(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// Completion callback for one action.
pub(crate) type Done = Rc<dyn Fn(Result<(), ActionError>)>;

/// Shared page state: the System, the Authenticate sheet and an inline status line per action.
pub(crate) struct Ctx {
    pub(crate) sys: Rc<dyn System>,
    auth: w::AuthSheet,
    pending: RefCell<Option<(Action, Done)>>,
}

impl Ctx {
    /// Wires the Authenticate sheet; place it on `aw` after any other sheets.
    pub(crate) fn new(sys: Rc<dyn System>, who: &str) -> Rc<Self> {
        let ctx = Rc::new(Ctx {
            sys,
            auth: w::authenticate_sheet(who),
            pending: Default::default(),
        });
        let c = ctx.clone();
        ctx.auth.allow.connect_clicked(move |_| c.allow());
        let c = ctx.clone();
        ctx.auth.field.connect_activate(move |_| c.allow());
        let c = ctx.clone();
        ctx.auth.cancel.connect_clicked(move |_| c.cancel());
        ctx
    }
    pub(crate) fn place(&self, aw: &w::AppWindow) {
        w::toolbar_place(aw, &self.auth.revealer);
    }
    /// `a` off the UI thread; root actions ask for the password, then retry; `done` gets the final result.
    pub(crate) fn act(self: &Rc<Self>, a: Action, done: Rc<dyn Fn(Result<(), ActionError>)>) {
        let rx = self.sys.spawn(a.clone());
        let c = self.clone();
        glib::spawn_future_local(async move {
            let Ok(r) = rx.recv().await else { return };
            match r {
                Err(e @ (ActionError::NeedsAuth | ActionError::Denied)) => {
                    c.auth.message.set_label(if e == ActionError::Denied {
                        e.message()
                    } else {
                        ""
                    });
                    *c.pending.borrow_mut() = Some((a, done));
                    c.auth.revealer.set_reveal_child(true);
                    c.auth.field.grab_focus();
                }
                r => done(r),
            }
        });
    }
    fn allow(self: &Rc<Self>) {
        let pw = self.auth.field.text();
        self.auth.field.set_text("");
        if let Err(e) = self.sys.authorize(&pw) {
            return self.auth.message.set_label(e.message());
        }
        self.auth.revealer.set_reveal_child(false);
        if let Some((a, done)) = self.pending.borrow_mut().take() {
            self.act(a, done);
        }
    }
    fn cancel(&self) {
        if let Some((_, done)) = self.pending.borrow_mut().take() {
            done(Err(ActionError::Denied));
        }
        self.auth.revealer.set_reveal_child(false);
    }
}

/// Inline one-sentence status under a section (HIG §3 errors).
fn status_label() -> gtk::Label {
    let l = w::label("", "secondary");
    l.set_wrap(true);
    l.set_visible(false);
    l
}

fn say(l: &gtk::Label, text: &str) {
    l.set_label(text);
    l.set_visible(!text.is_empty());
}

pub fn content(sys: Rc<dyn System>, host: &Host) -> w::AppWindow {
    let aw = w::AppWindow::new("System Settings", true, false);
    let ctx = Ctx::new(sys.clone(), "System Settings");
    let sheets = Sheets {
        join: w::toolbar_sheet(&aw, &gtk::Box::new(gtk::Orientation::Vertical, 0)),
        user: w::toolbar_sheet(&aw, &gtk::Box::new(gtk::Orientation::Vertical, 0)),
    };
    // Added last: the Authenticate sheet sits above the others.
    ctx.place(&aw);

    aw.source_section("Personal");
    aw.source_item(
        "preferences-desktop-appearance-symbolic",
        "Appearance",
        &appearance(),
    );
    aw.source_item("user-desktop-symbolic", "Menu Bar & Dock", &menubar_dock());
    aw.source_item("input-keyboard-symbolic", "Keyboard", &keyboard());
    aw.source_item("system-lock-screen-symbolic", "Lock & Idle", &lock_idle());
    aw.source_section("Hardware");
    aw.source_item("video-display-symbolic", "Displays", &displays(&ctx));
    aw.source_item("audio-speakers-symbolic", "Sound", &sound(&ctx));
    aw.source_item(
        "network-wireless-symbolic",
        "Network",
        &network(&ctx, &sheets.join),
    );
    aw.source_section("System");
    aw.source_item(
        "system-users-symbolic",
        "Accounts",
        &accounts(&ctx, host, &sheets.user),
    );
    aw.source_item(
        "software-update-available-symbolic",
        "Software Update",
        &software_update(&ctx),
    );
    aw.source_item("help-about-symbolic", "About", &about(host, &*sys));
    aw
}

struct Sheets {
    join: gtk::Revealer,
    user: gtk::Revealer,
}

/// Replace a toolbar sheet's body.
fn sheet_body(r: &gtk::Revealer, body: &impl IsA<gtk::Widget>) {
    if let Some(card) = r.child().and_downcast::<gtk::Box>() {
        while let Some(c) = card.first_child() {
            card.remove(&c);
        }
        card.append(body);
    }
}

fn row(title: &str, control: &impl IsA<gtk::Widget>) -> gtk::Widget {
    w::list_row("", title, "", Some(control.upcast_ref())).upcast()
}

fn row_sub(title: &str, subtitle: &str, control: &impl IsA<gtk::Widget>) -> gtk::Widget {
    w::list_row("", title, subtitle, Some(control.upcast_ref())).upcast()
}

fn info(title: &str, value: &str) -> gtk::Widget {
    let v = w::label(value, "secondary");
    v.set_wrap(true);
    v.set_width_chars(w::VALUE_CHARS);
    v.set_xalign(1.0);
    v.set_justify(gtk::Justification::Right);
    row(title, &v)
}

fn dropdown(options: &[&str], selected: usize, changed: impl Fn(usize) + 'static) -> gtk::DropDown {
    let d = w::dropdown(options, selected as u32);
    d.connect_selected_notify(move |d| changed(d.selected() as usize));
    d
}

// ---- Appearance ----

fn appearance() -> gtk::Box {
    let p = w::page("Appearance");
    let (mode, acc) = accent::split(kit::style::active_id().as_deref());
    let state = Rc::new(RefCell::new((mode, acc.to_string())));
    let light = crate::theme::resolve(Some("wol-light"), false);
    let dark = crate::theme::resolve(Some("wol-dark"), true);
    let tiles = w::wrap_row(w::S3, 3);
    let choices: [(Option<Variant>, &str, Vec<w::Palette>); 3] = [
        (Some(Variant::Light), "Light", vec![w::Palette::of(&light)]),
        (Some(Variant::Dark), "Dark", vec![w::Palette::of(&dark)]),
        (
            None,
            "Auto",
            vec![w::Palette::of(&light), w::Palette::of(&dark)],
        ),
    ];
    let mut first: Option<gtk::ToggleButton> = None;
    let apply = |s: &(Option<Variant>, String)| {
        let _ = match accent::theme_id(s.0, &s.1) {
            Some(id) => kit::style::set_active(&id),
            None => std::fs::remove_file(crate::theme::active_path()),
        };
    };
    for (v, title, slices) in choices {
        let t = w::theme_preview(slices, title, v == mode);
        if let Some(f) = &first {
            t.set_group(Some(f));
        } else {
            first = Some(t.clone());
        }
        let st = state.clone();
        t.connect_toggled(move |t| {
            if t.is_active() {
                st.borrow_mut().0 = v;
                apply(&st.borrow());
            }
        });
        tiles.append(&t);
    }
    p.append(&w::section("", &[tiles.clone().upcast()]));
    if let Some(card) = tiles.parent() {
        card.add_css_class("preview-card");
    }

    let swatches = w::wrap_row(w::S2, 8);
    let variant = if mode == Some(Variant::Dark) {
        Variant::Dark
    } else {
        Variant::Light
    };
    let mut first: Option<gtk::ToggleButton> = None;
    for (id, name, ..) in accent::ACCENTS {
        let hex = accent::color(variant, id).unwrap_or_default();
        let c = Color::parse(hex).unwrap_or(Color::WHITE);
        let dot = Color::parse(accent::text_on(hex)).unwrap_or(Color::WHITE);
        let s = w::accent_swatch(c, dot, name, id == acc);
        if let Some(f) = &first {
            s.set_group(Some(f));
        } else {
            first = Some(s.clone());
        }
        let st = state.clone();
        s.connect_toggled(move |s| {
            if s.is_active() {
                st.borrow_mut().1 = id.to_string();
                apply(&st.borrow());
            }
        });
        swatches.append(&s);
    }
    p.append(&w::section("", &[row("Accent color", &swatches)]));
    p
}

// ---- Menu Bar & Dock ----

/// Write shell.json and tell the running shell (`reload` on wol-shell.sock).
fn save_shell(p: ShellPrefs) {
    let _ = write(&config(prefs::SHELL_FILE), &p.to_json());
    let sock = crate::protocol::socket_path(&kit::runtime_dir());
    std::thread::spawn(move || {
        use std::io::Write;
        if let Ok(mut s) = std::os::unix::net::UnixStream::connect(sock) {
            let _ = s.write_all(b"reload\n");
        }
    });
}

fn menubar_dock() -> gtk::Box {
    let p = w::page("Menu Bar & Dock");
    let cur = Rc::new(std::cell::Cell::new(ShellPrefs::parse(
        read(&config(prefs::SHELL_FILE)).as_deref(),
    )));
    let t = |get: fn(&ShellPrefs) -> bool, set: fn(&mut ShellPrefs, bool)| {
        let c = cur.clone();
        w::toggle(get(&cur.get()), move |on| {
            let mut v = c.get();
            set(&mut v, on);
            c.set(v);
            save_shell(v);
        })
    };
    p.append(&w::section(
        "Menu Bar",
        &[
            row(
                "Show battery percentage",
                &t(|p| p.battery_percent, |p, v| p.battery_percent = v),
            ),
            row("Show date", &t(|p| p.show_date, |p, v| p.show_date = v)),
        ],
    ));
    p.append(&w::section(
        "Dock",
        &[
            row(
                "Automatically hide and show the Dock",
                &t(|p| p.dock_autohide, |p, v| p.dock_autohide = v),
            ),
            row(
                "Magnification",
                &t(|p| p.dock_magnify, |p, v| p.dock_magnify = v),
            ),
        ],
    ));
    p
}

// ---- Keyboard ----

/// Keyboard/display fragments live in `hypr/wolos/`; the user hyprland.conf sources them.
fn save_hypr(name: &str, text: &str) {
    let dir = config(prefs::HYPR_DIR);
    let _ = write(&dir.join(name), text);
    let user = config(prefs::HYPR_USER);
    let glob = dir.join("*.conf");
    if let Some(t) = prefs::ensure_source(read(&user).as_deref(), &glob.to_string_lossy()) {
        let _ = write(&user, &t);
    }
    background(vec![argv(&["hyprctl", "reload"])]);
}

fn keyboard() -> gtk::Box {
    let p = w::page("Keyboard");
    let file = config(prefs::HYPR_DIR).join("keyboard.conf");
    let (layout, repeat) = prefs::parse_keyboard(read(&file).as_deref());
    let cur = Rc::new(std::cell::Cell::new((layout, repeat)));
    let save = {
        let cur = cur.clone();
        move || {
            let (l, r) = cur.get();
            if let Some(t) = prefs::keyboard_conf(prefs::LAYOUTS[l].0, r) {
                save_hypr("keyboard.conf", &t);
            }
        }
    };
    let save = Rc::new(save);
    let labels: Vec<&str> = prefs::LAYOUTS.iter().map(|l| l.1).collect();
    let repeats: Vec<&str> = prefs::REPEATS.iter().map(|r| r.0).collect();
    let (c1, s1, c2, s2) = (cur.clone(), save.clone(), cur.clone(), save.clone());
    p.append(&w::section(
        "",
        &[
            row(
                "Input source",
                &dropdown(&labels, layout, move |i| {
                    c1.set((i, c1.get().1));
                    s1();
                }),
            ),
            row(
                "Key repeat rate",
                &dropdown(&repeats, repeat, move |i| {
                    c2.set((c2.get().0, i));
                    s2();
                }),
            ),
        ],
    ));
    let field = w::text_field("Type here to test");
    field.set_hexpand(true);
    p.append(&w::section("", &[row("Try it", &field)]));
    p
}

// ---- Lock & Idle ----

fn lock_idle() -> gtk::Box {
    let p = w::page("Lock & Idle");
    let file = config(prefs::IDLE_FILE);
    let (secs, sleep) = prefs::parse_hypridle(read(&file).as_deref());
    let cur = Rc::new(std::cell::Cell::new((secs, sleep)));
    let save = Rc::new({
        let cur = cur.clone();
        move || {
            let (s, z) = cur.get();
            let _ = write(&file, &prefs::hypridle_conf(s, z));
            // hypridle reads its config at start: restart it on the new file.
            background(vec![argv(&["pkill", "-x", "hypridle"])]);
            glib::timeout_add_local_once(std::time::Duration::from_millis(300), || {
                kit::spawn(&argv(&["hypridle"]))
            });
        }
    });
    let labels: Vec<&str> = prefs::IDLE_CHOICES.iter().map(|c| c.0).collect();
    let sel = prefs::IDLE_CHOICES
        .iter()
        .position(|c| c.1 == secs)
        .unwrap_or(2);
    let (c1, s1, c2, s2) = (cur.clone(), save.clone(), cur.clone(), save.clone());
    p.append(&w::section(
        "",
        &[
            row(
                "Lock screen when inactive",
                &dropdown(&labels, sel, move |i| {
                    c1.set((prefs::IDLE_CHOICES[i].1, c1.get().1));
                    s1();
                }),
            ),
            row(
                "Lock when the computer sleeps",
                &w::toggle(sleep, move |on| {
                    c2.set((c2.get().0, on));
                    s2();
                }),
            ),
        ],
    ));
    p.append(&w::footnote(
        "The display turns off one minute after the screen locks.",
    ));
    p
}

// ---- Displays ----

fn resolutions(m: &crate::system::Monitor) -> Vec<(u32, u32, f64)> {
    let mut v: Vec<(u32, u32, f64)> = Vec::new();
    for mode in &m.modes {
        if let Some((w, h, r)) = wol_core::controls::parse_mode(mode)
            && !v.iter().any(|x| x.0 == w && x.1 == h)
        {
            v.push((w, h, r));
        }
    }
    if v.is_empty() {
        v.push((m.width, m.height, m.refresh));
    }
    v
}

const SCALES: [(&str, f64); 5] = [
    ("100%", 1.0),
    ("125%", 1.25),
    ("150%", 1.5),
    ("175%", 1.75),
    ("200%", 2.0),
];

fn displays(ctx: &Rc<Ctx>) -> gtk::Box {
    let p = w::page("Displays");
    let mons = ctx.sys.monitors();
    if mons.is_empty() {
        p.append(&w::empty_state(
            "video-display-symbolic",
            "No displays found",
            "Displays appear here while Hyprland is running.",
            None,
        ));
        return p;
    }
    let status = status_label();
    for m in mons {
        let res = resolutions(&m);
        let cur = Rc::new(std::cell::Cell::new((
            res.iter()
                .position(|r| r.0 == m.width && r.1 == m.height)
                .unwrap_or(0),
            SCALES
                .iter()
                .position(|s| (s.1 - m.scale).abs() < 0.01)
                .unwrap_or(0),
        )));
        let apply = Rc::new({
            let (cur, ctx, name, res, status) = (
                cur.clone(),
                ctx.clone(),
                m.name.clone(),
                res.clone(),
                status.clone(),
            );
            move || {
                let (r, s) = cur.get();
                let (w, h, hz) = res[r];
                let Some(rule) = wol_core::controls::monitor_rule(&name, w, h, hz, SCALES[s].1)
                else {
                    return;
                };
                let dfile = config(prefs::HYPR_DIR).join("displays.conf");
                let text = prefs::displays_conf(read(&dfile).as_deref(), &rule);
                let _ = write(&dfile, &text);
                let st = status.clone();
                ctx.act(
                    Action::Monitor(rule),
                    Rc::new(move |r| {
                        say(&st, r.err().map_or("", |e| e.message()));
                    }),
                );
            }
        });
        let labels: Vec<String> = res.iter().map(|r| format!("{} × {}", r.0, r.1)).collect();
        let lref: Vec<&str> = labels.iter().map(String::as_str).collect();
        let scales: Vec<&str> = SCALES.iter().map(|s| s.0).collect();
        let (c1, a1, c2, a2) = (cur.clone(), apply.clone(), cur.clone(), apply.clone());
        let title = if m.description.is_empty() {
            m.name.clone()
        } else {
            m.description.clone()
        };
        let night_ctx = ctx.clone();
        let night_status = status.clone();
        p.append(&w::section(
            &title,
            &[
                row(
                    "Resolution",
                    &dropdown(&lref, cur.get().0, move |i| {
                        c1.set((i, c1.get().1));
                        a1();
                    }),
                ),
                row(
                    "Scale",
                    &dropdown(&scales, cur.get().1, move |i| {
                        c2.set((c2.get().0, i));
                        a2();
                    }),
                ),
                row_sub(
                    "Night Shift",
                    "Warmer colours after dark",
                    &w::toggle(ctx.sys.night_light().unwrap_or(false), move |on| {
                        let st = night_status.clone();
                        night_ctx.act(
                            Action::NightLight(on),
                            Rc::new(move |r| say(&st, r.err().map_or("", |e| e.message()))),
                        );
                    }),
                ),
            ],
        ));
    }
    p.append(&status);
    p
}

// ---- Sound ----

fn fill_outputs(list: &gtk::Box, ctx: &Rc<Ctx>, st: &gtk::Label, outs: Vec<crate::system::Output>) {
    while let Some(c) = list.first_child() {
        list.remove(&c);
    }
    if outs.is_empty() {
        list.append(&row("No output devices", &w::label("", "secondary")));
    }
    for o in &outs {
        let b = w::choice_row("audio-speakers-symbolic", &o.name, "", o.default);
        let (lc, ctx, st, id, all) = (
            list.clone(),
            ctx.clone(),
            st.clone(),
            o.id.clone(),
            outs.clone(),
        );
        b.connect_clicked(move |_| {
            let next: Vec<_> = all
                .iter()
                .cloned()
                .map(|mut x| {
                    x.default = x.id == id;
                    x
                })
                .collect();
            let (lc, c2, st) = (lc.clone(), ctx.clone(), st.clone());
            ctx.act(
                Action::Output(id.clone()),
                Rc::new(move |r| match r {
                    Ok(()) => {
                        say(&st, "");
                        fill_outputs(&lc, &c2, &st, next.clone());
                    }
                    Err(e) => say(&st, e.message()),
                }),
            );
        });
        list.append(&b);
    }
}

/// Volume while dragging: one action in flight, newest value queued.
#[derive(Default)]
struct Coalesce {
    busy: std::cell::Cell<bool>,
    latest: std::cell::Cell<Option<u8>>,
}

fn send_volume(ctx: &Rc<Ctx>, q: &Rc<Coalesce>, v: u8) {
    if q.busy.replace(true) {
        return q.latest.set(Some(v));
    }
    let (c2, q2) = (ctx.clone(), q.clone());
    ctx.act(
        Action::Volume(v),
        Rc::new(move |_| {
            q2.busy.set(false);
            if let Some(n) = q2.latest.take() {
                send_volume(&c2, &q2, n);
            }
        }),
    );
}

fn sound(ctx: &Rc<Ctx>) -> gtk::Box {
    let p = w::page("Sound");
    let status = status_label();
    let list = w::vbox(0);
    fill_outputs(&list, ctx, &status, ctx.sys.output_devices());
    p.append(&w::section("Output", &[list.upcast()]));
    let audio = ctx.sys.audio();
    let q = Rc::new(Coalesce::default());
    let slider = {
        let ctx = ctx.clone();
        w::row_slider(audio.volume.unwrap_or(0) as f64, move |v| {
            send_volume(&ctx, &q, v as u8)
        })
    };
    slider.set_sensitive(audio.volume.is_some());
    let mute = {
        let (ctx, st) = (ctx.clone(), status.clone());
        w::toggle(audio.muted, move |on| {
            let st = st.clone();
            ctx.act(
                Action::Muted(on),
                Rc::new(move |r| say(&st, r.err().map_or("", |e| e.message()))),
            );
        })
    };
    let vol = w::hbox(w::S3);
    vol.append(&w::icon("audio-volume-low-symbolic", w::ICON));
    vol.append(&slider);
    vol.append(&w::icon("audio-volume-high-symbolic", w::ICON));
    vol.set_hexpand(true);
    p.append(&w::section(
        "",
        &[row("Output volume", &vol), row("Mute", &mute)],
    ));
    p.append(&status);
    p
}

// ---- Network ----

fn network(ctx: &Rc<Ctx>, join_sheet: &gtk::Revealer) -> gtk::Box {
    let p = w::page("Network");
    let status = status_label();
    let wifi_on = ctx.sys.wifi_enabled();
    let toggle = {
        let (ctx, st) = (ctx.clone(), status.clone());
        w::toggle(wifi_on.unwrap_or(false), move |on| {
            let st = st.clone();
            ctx.act(
                Action::Wifi(on),
                Rc::new(move |r| say(&st, r.err().map_or("", |e| e.message()))),
            );
        })
    };
    toggle.set_sensitive(wifi_on.is_some());
    let connected = ctx
        .sys
        .wifi_networks()
        .into_iter()
        .find(|n| n.connected)
        .map_or_else(
            || crate::system::wifi_subtitle(ctx.sys.network(), &[]),
            |n| n.ssid,
        );
    p.append(&w::section(
        "",
        &[w::list_row(
            "network-wireless-symbolic",
            "Wi-Fi",
            &connected,
            Some(toggle.upcast_ref()),
        )
        .upcast()],
    ));

    // Join sheet: password for a secured network.
    let title = w::label("", "title");
    title.set_wrap(true);
    let pw = w::password_field("Password");
    let body = w::vbox(w::S3);
    body.append(&title);
    body.append(&pw);
    let buttons = w::hbox(w::S2);
    buttons.append(&w::spacer());
    let ssid: Rc<RefCell<String>> = Default::default();
    {
        let r = join_sheet.clone();
        buttons.append(&w::button("Cancel", w::Kind::Default, move || {
            r.set_reveal_child(false)
        }));
    }
    let join: Rc<dyn Fn(String, Option<String>)> = {
        let (ctx, st) = (ctx.clone(), status.clone());
        Rc::new(move |id: String, pass: Option<String>| {
            say(&st, &format!("Joining “{id}”…"));
            let st = st.clone();
            let id2 = id.clone();
            ctx.act(
                Action::WifiJoin(id, pass),
                Rc::new(move |r| match r {
                    Ok(()) => say(&st, &format!("Connected to “{id2}”.")),
                    Err(ActionError::Denied) => {
                        say(&st, "Could not join. Check the password and try again.")
                    }
                    Err(e) => say(&st, e.message()),
                }),
            );
        })
    };
    {
        let (r, pwc, id, join) = (join_sheet.clone(), pw.clone(), ssid.clone(), join.clone());
        let go = Rc::new(move || {
            let pass = pwc.text().to_string();
            pwc.set_text("");
            r.set_reveal_child(false);
            join(id.borrow().clone(), Some(pass));
        });
        let g = go.clone();
        let pw2 = pw.clone();
        pw2.connect_activate(move |_| g());
        buttons.append(&w::button("Join", w::Kind::Primary, move || go()));
    }
    body.append(&buttons);
    sheet_body(join_sheet, &body);

    let list = w::vbox(0);
    let fill = {
        let (list, sheet, title, pw, ssid, join) = (
            list.clone(),
            join_sheet.clone(),
            title.clone(),
            pw.clone(),
            ssid.clone(),
            join.clone(),
        );
        move |nets: Vec<crate::system::WifiNetwork>| {
            while let Some(c) = list.first_child() {
                list.remove(&c);
            }
            if nets.is_empty() {
                list.append(&row("No networks found", &w::label("", "secondary")));
            }
            for n in nets.iter().take(10) {
                let sub = if n.connected {
                    "Connected"
                } else if n.secure {
                    "Secured"
                } else {
                    "Open"
                };
                let b = w::choice_row(
                    crate::system::signal_icon(n.signal),
                    &n.ssid,
                    sub,
                    n.connected,
                );
                let (n, sheet, title, pw, ssid, join) = (
                    n.clone(),
                    sheet.clone(),
                    title.clone(),
                    pw.clone(),
                    ssid.clone(),
                    join.clone(),
                );
                b.connect_clicked(move |_| {
                    if n.connected {
                        return;
                    }
                    if n.secure {
                        *ssid.borrow_mut() = n.ssid.clone();
                        title.set_label(&format!("Enter the password for “{}”", n.ssid));
                        sheet.set_reveal_child(true);
                        pw.grab_focus();
                    } else {
                        join(n.ssid.clone(), None);
                    }
                });
                list.append(&b);
            }
        }
    };
    fill(ctx.sys.wifi_networks());
    p.append(&w::section("Networks", &[list.upcast()]));
    p.append(&status);
    if let Some(rx) = ctx.sys.watch() {
        let sys = ctx.sys.clone();
        let last = RefCell::new(sys.wifi_networks());
        glib::spawn_future_local(async move {
            while rx.recv().await.is_ok() {
                let n = sys.wifi_networks();
                if *last.borrow() != n {
                    fill(n.clone());
                    *last.borrow_mut() = n;
                }
            }
        });
    }
    p
}

// ---- Accounts ----

/// "Alice Liddell" -> "alice".
pub fn account_name(full: &str) -> String {
    let first = full.split_whitespace().next().unwrap_or("");
    let s: String = first
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .take(32)
        .collect();
    s.trim_start_matches(|c: char| c.is_ascii_digit())
        .to_string()
}

fn accounts(ctx: &Rc<Ctx>, host: &Host, sheet: &gtk::Revealer) -> gtk::Box {
    let p = w::page("Accounts");
    let status = status_label();
    let list = w::vbox(0);
    let add_row = |list: &gtk::Box, u: &str, sub: &str| {
        let r = w::list_row("", u, sub, None);
        r.prepend(&w::avatar(u, w::ICON_M));
        list.append(&r);
    };
    if host.users.is_empty() {
        list.append(&row("No other accounts", &w::label("", "secondary")));
    }
    for u in &host.users {
        add_row(&list, u, "");
    }
    p.append(&w::section("Users", &[list.clone().upcast()]));

    let full = w::text_field("Full name");
    let short = w::text_field("Account name");
    let pass = w::password_field("Password");
    let verify = w::password_field("Verify");
    let admin = w::toggle(false, |_| {});
    {
        let s = short.clone();
        full.connect_changed(move |f| s.set_text(&account_name(&f.text())));
    }
    let body = w::vbox(w::S3);
    body.append(&w::label("New account", "title"));
    body.append(&full);
    body.append(&short);
    body.append(&pass);
    body.append(&verify);
    body.append(&w::list_row(
        "",
        "Allow this user to administer this computer",
        "",
        Some(admin.upcast_ref()),
    ));
    let buttons = w::hbox(w::S2);
    buttons.append(&w::spacer());
    {
        let r = sheet.clone();
        buttons.append(&w::button("Cancel", w::Kind::Default, move || {
            r.set_reveal_child(false)
        }));
    }
    {
        let (pass, verify) = (pass.clone(), verify.clone());
        let (ctx, r, full, short, admin, st, list) = (
            ctx.clone(),
            sheet.clone(),
            full.clone(),
            short.clone(),
            admin.clone(),
            status.clone(),
            list.clone(),
        );
        buttons.append(&w::button("Create User", w::Kind::Primary, move || {
            let (name, full_name) = (short.text().to_string(), full.text().to_string());
            if !wol_core::settings::users::is_valid_name(&name) {
                return say(&st, "Account names use lowercase letters, digits, - and _.");
            }
            let password = pass.text().to_string();
            if let Some(e) = password_problem(&password, &verify.text()) {
                return say(&st, e);
            }
            pass.set_text("");
            verify.set_text("");
            r.set_reveal_child(false);
            say(&st, &format!("Creating “{name}”…"));
            let (st, list, n2, f2) = (st.clone(), list.clone(), name.clone(), full_name.clone());
            ctx.act(
                Action::UserAdd {
                    name,
                    full_name,
                    admin: admin.is_active(),
                    password,
                },
                Rc::new(move |res| match res {
                    Ok(()) => {
                        say(&st, "");
                        let r = w::list_row("", &n2, &f2, None);
                        r.prepend(&w::avatar(&n2, w::ICON_M));
                        list.append(&r);
                    }
                    Err(e) => say(&st, e.message()),
                }),
            );
        }));
    }
    body.append(&buttons);
    sheet_body(sheet, &body);
    let bar = w::hbox(w::S2);
    bar.append(&w::spacer());
    {
        let (r, full) = (sheet.clone(), full.clone());
        bar.append(&w::button("Add User…", w::Kind::Default, move || {
            r.set_reveal_child(true);
            full.grab_focus();
        }));
    }
    p.append(&bar);
    p.append(&status);
    p
}

/// Inline error for the Add User password fields, None when acceptable.
pub fn password_problem(password: &str, verify: &str) -> Option<&'static str> {
    if !wol_core::settings::users::is_valid_password(password) {
        Some("Use a password of at least 8 characters.")
    } else if password != verify {
        Some("The passwords do not match.")
    } else {
        None
    }
}

// ---- Software Update ----

#[derive(Clone, Debug, PartialEq)]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    Available(Vec<String>),
    Installing,
    Installed,
    Failed(String),
}

/// `wol update check --json` stdout -> state.
pub fn parse_check(out: Option<&str>) -> UpdateState {
    match out.and_then(|o| serde_json::from_str::<Vec<String>>(o.trim()).ok()) {
        Some(v) if v.is_empty() => UpdateState::UpToDate,
        Some(v) => UpdateState::Available(v),
        None => UpdateState::Failed(
            "Could not check for updates. Make sure pkg is installed and you are online.".into(),
        ),
    }
}

fn software_update(ctx: &Rc<Ctx>) -> gtk::Box {
    let p = w::page("Software Update");
    let icon = w::icon("software-update-available-symbolic", w::ICON_M);
    icon.add_css_class("accent-icon");
    let title = w::label("", "title");
    title.set_wrap(true);
    let detail = w::label("", "secondary");
    detail.set_wrap(true);
    let text = w::vbox(0);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    text.append(&title);
    text.append(&detail);
    let spin = gtk::Spinner::new();
    let head = w::hbox(w::S3);
    head.add_css_class("list-row");
    head.add_css_class("hero-row");
    head.append(&icon);
    head.append(&text);
    head.append(&spin);
    let bar = w::progress();
    let progress_row = w::hbox(0);
    progress_row.add_css_class("list-row");
    progress_row.append(&bar);
    let names = w::label("", "secondary");
    names.set_wrap(true);
    let names_row = w::hbox(0);
    names_row.add_css_class("list-row");
    names_row.append(&names);
    p.append(&w::section(
        "",
        &[
            head.upcast(),
            progress_row.clone().upcast(),
            names_row.clone().upcast(),
        ],
    ));
    let buttons = w::hbox(w::S2);
    buttons.append(&w::spacer());
    let check = w::button("Check Now", w::Kind::Default, || {});
    let install = w::button("Update Now", w::Kind::Primary, || {});
    buttons.append(&check);
    buttons.append(&install);
    p.append(&buttons);

    let show: Rc<dyn Fn(&UpdateState)> = {
        let (title, detail, spin, progress_row, names_row, names, check, install, bar) = (
            title.clone(),
            detail.clone(),
            spin.clone(),
            progress_row.clone(),
            names_row.clone(),
            names.clone(),
            check.clone(),
            install.clone(),
            bar.clone(),
        );
        let pulse: Rc<RefCell<Option<glib::SourceId>>> = Default::default();
        Rc::new(move |s: &UpdateState| {
            let busy = matches!(s, UpdateState::Checking | UpdateState::Installing);
            spin.set_spinning(busy);
            spin.set_visible(busy);
            progress_row.set_visible(*s == UpdateState::Installing);
            if let Some(id) = pulse.borrow_mut().take() {
                id.remove();
            }
            if *s == UpdateState::Installing {
                let b = bar.clone();
                *pulse.borrow_mut() = Some(glib::timeout_add_local(
                    std::time::Duration::from_millis(120),
                    move || {
                        b.pulse();
                        glib::ControlFlow::Continue
                    },
                ));
            }
            check.set_sensitive(!busy);
            install.set_visible(matches!(s, UpdateState::Available(_)));
            names_row.set_visible(matches!(s, UpdateState::Available(_)));
            let (t, d) = match s {
                UpdateState::Idle => (
                    "Software Update".to_string(),
                    "Check for new WolOS packages.".to_string(),
                ),
                UpdateState::Checking => ("Checking for updates…".into(), String::new()),
                UpdateState::UpToDate => (
                    "WolOS is up to date".into(),
                    "All packages are current.".into(),
                ),
                UpdateState::Available(v) => {
                    names.set_label(&wol_core::update::summary(v, 6));
                    (
                        format!(
                            "{} update{} available",
                            v.len(),
                            if v.len() == 1 { "" } else { "s" }
                        ),
                        "Updates install into a new boot environment.".into(),
                    )
                }
                UpdateState::Installing => (
                    "Installing updates…".into(),
                    "You can keep working while this runs.".into(),
                ),
                UpdateState::Installed => (
                    "Updates installed".into(),
                    "Restart to finish updating.".into(),
                ),
                UpdateState::Failed(e) => ("Software Update".into(), e.clone()),
            };
            title.set_label(&t);
            detail.set_label(&d);
            detail.set_visible(!d.is_empty());
        })
    };
    show(&UpdateState::Idle);
    {
        let show = show.clone();
        check.connect_clicked(move |_| {
            show(&UpdateState::Checking);
            let (tx, rx) = async_channel::bounded(1);
            std::thread::spawn(move || {
                let out = wol_core::system::run_with_timeout(
                    &["wol", "update", "check", "--json"],
                    std::time::Duration::from_secs(120),
                );
                let _ = tx.send_blocking(out);
            });
            let show = show.clone();
            glib::spawn_future_local(async move {
                if let Ok(out) = rx.recv().await {
                    show(&parse_check(out.as_deref()));
                }
            });
        });
    }
    {
        let (show, ctx) = (show.clone(), ctx.clone());
        install.connect_clicked(move |_| {
            show(&UpdateState::Installing);
            let show = show.clone();
            ctx.act(
                Action::PkgUpgrade,
                Rc::new(move |r| {
                    show(&match r {
                        Ok(()) => UpdateState::Installed,
                        Err(e) => UpdateState::Failed(e.message().into()),
                    })
                }),
            );
        });
    }
    p
}

// ---- About ----

fn about(host: &Host, sys: &dyn System) -> gtk::Box {
    let p = w::vbox(w::S3);
    p.add_css_class("page");
    let mark = w::app_icon("wolos-mark", w::ICON_XL);
    mark.set_halign(gtk::Align::Center);
    p.append(&mark);
    p.append(&w::centered("WolOS", "large-title"));
    p.append(&w::centered(
        &format!("Version {}", env!("CARGO_PKG_VERSION")),
        "secondary",
    ));
    let or = |s: &str| {
        if s.is_empty() {
            "Unknown".to_string()
        } else {
            s.to_string()
        }
    };
    let mut rows = vec![
        info("Name", &or(&host.hostname)),
        info("Base", "FreeBSD 15.1"),
    ];
    if !host.processor.is_empty() {
        rows.push(info("Processor", &host.processor));
    }
    if !host.memory.is_empty() {
        rows.push(info("Memory", &host.memory));
    }
    if let Some(b) = crate::system::battery_label(sys.battery()) {
        rows.push(info("Battery", &b));
    }
    p.append(&w::section("", &rows));
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_names_from_full_names() {
        for (full, want) in [
            ("Alice Liddell", "alice"),
            ("Mad Hatter", "mad"),
            ("  Dinah ", "dinah"),
            ("José", "jos"),
            ("42 Wonder", ""),
        ] {
            assert_eq!(account_name(full), want, "{full}");
        }
    }

    #[test]
    fn add_user_password_rules() {
        assert_eq!(
            password_problem("", ""),
            Some("Use a password of at least 8 characters.")
        );
        assert_eq!(
            password_problem("longenough", "longenouhg"),
            Some("The passwords do not match.")
        );
        assert_eq!(password_problem("longenough", "longenough"), None);
    }

    #[test]
    fn update_check_states() {
        assert_eq!(parse_check(Some("[]\n")), UpdateState::UpToDate);
        assert_eq!(
            parse_check(Some(r#"["curl","git"]"#)),
            UpdateState::Available(vec!["curl".into(), "git".into()])
        );
        assert!(matches!(parse_check(None), UpdateState::Failed(_)));
    }
}
