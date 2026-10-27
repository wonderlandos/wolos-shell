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

mod about;
mod accounts;
mod appearance;
mod displays;
mod keyboard;
mod lock;
mod menubar_dock;
mod network;
mod software_update;
mod sound;

pub use accounts::{account_name, password_problem};
pub use software_update::{UpdateState, parse_check};

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
        join: w::toolbar_sheet(&aw, &w::vbox(0)),
        user: w::toolbar_sheet(&aw, &w::vbox(0)),
    };
    // Added last: the Authenticate sheet sits above the others.
    ctx.place(&aw);

    aw.source_section("Personal");
    aw.source_item(
        "preferences-desktop-appearance-symbolic",
        "Appearance",
        &appearance::page(),
    );
    aw.source_item(
        "user-desktop-symbolic",
        "Menu Bar & Dock",
        &menubar_dock::page(),
    );
    aw.source_item("input-keyboard-symbolic", "Keyboard", &keyboard::page());
    aw.source_item("system-lock-screen-symbolic", "Lock & Idle", &lock::page());
    aw.source_section("Hardware");
    aw.source_item("video-display-symbolic", "Displays", &displays::page(&ctx));
    aw.source_item("audio-speakers-symbolic", "Sound", &sound::page(&ctx));
    aw.source_item(
        "network-wireless-symbolic",
        "Network",
        &network::page(&ctx, &sheets.join),
    );
    aw.source_section("System");
    aw.source_item(
        "system-users-symbolic",
        "Accounts",
        &accounts::page(&ctx, host, &sheets.user),
    );
    aw.source_item(
        "software-update-available-symbolic",
        "Software Update",
        &software_update::page(&ctx),
    );
    aw.source_item("help-about-symbolic", "About", &about::page(host, &*sys));
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
