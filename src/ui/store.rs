// SPDX-License-Identifier: GPL-2.0-or-later
//! App Store (`wol-store`): store layout on the kit AppWindow. Curated catalog from
//! `wol_core::store`, `pacman -Ss` for the rest, installs through wol-admind `pkg-install` with the
//! Authenticate sheet, updates from `wol update check`. `wol-store <file.wol>` (double-click) opens
//! the install page: wol-admind `wol-install` (ADR 0011). Slow work never runs on the UI thread.
use super::settings::{Ctx, UpdateState, parse_check, with_flatpaks};
use crate::kit::{self, widgets as w};
use crate::system::{Action, ActionError, System};
use gtk::{glib, prelude::*};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use wol_core::package;
use wol_core::store::{self, App, Category};

pub const PAGES: [&str; 8] = [
    "Discover", "Create", "Work", "Play", "Develop", "Kits", "Plugins", "Updates",
];

#[path = "store_plugins.rs"]
pub mod plugins;

mod details;
use details::fill_details;
pub use details::{Loaded, Subject, button_label, info_rows, load};

/// Machine facts the store reads (injected so goldens do not depend on the build host).
#[derive(Clone, Debug)]
pub struct Env {
    /// XDG data dirs searched for `applications/<id>.desktop`.
    pub data_dirs: Vec<PathBuf>,
    /// Run `pacman -Ss` and `flatpak search` for non-catalog results.
    pub pkg_search: bool,
    /// Flathub apps installed (any scope).
    pub flatpaks: Vec<String>,
    /// What is installed, for a kit's Added: None reads this computer's package database (off
    /// the GTK thread); goldens set it so they do not depend on the build host.
    pub installed: Option<wopkg::context::Installed>,
}

impl Env {
    pub fn read() -> Self {
        let mut dirs: Vec<PathBuf> = vec![PathBuf::from(
            std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
                format!("{}/.local/share", std::env::var("HOME").unwrap_or_default())
            }),
        )];
        dirs.extend(
            std::env::var("XDG_DATA_DIRS")
                .unwrap_or_else(|_| "/usr/share".into())
                .split(':')
                .map(PathBuf::from),
        );
        let list = wol_core::flatpak::installed_argv();
        let l: Vec<&str> = list.iter().map(String::as_str).collect();
        Env {
            data_dirs: dirs,
            pkg_search: true,
            flatpaks: wol_core::system::run_with_timeout(&l, std::time::Duration::from_secs(5))
                .map(|t| wol_core::flatpak::parse_ids(&t))
                .unwrap_or_default(),
            installed: None,
        }
    }
}

pub fn run() -> glib::ExitCode {
    let arg = std::env::args().nth(1);
    // A double-clicked `.wol` opens its own window, even when the App Store is already open.
    let file = arg.as_ref().is_some_and(|a| a.ends_with(".wol"));
    let flags = if file {
        gtk::gio::ApplicationFlags::NON_UNIQUE
    } else {
        gtk::gio::ApplicationFlags::empty()
    };
    let app = gtk::Application::builder()
        .application_id("org.wolos.store")
        .flags(flags)
        .build();
    app.connect_activate(move |app| {
        kit::style::install("");
        let win = gtk::ApplicationWindow::builder()
            .application(app)
            .title("App Store")
            .default_width(w::APP_SIZE.0)
            .default_height(w::APP_SIZE.1)
            .decorated(false)
            .build();
        let s = content(crate::system::default_system(), Env::read());
        match &arg {
            Some(p) if file => s.open_package(
                &std::fs::canonicalize(p).map_or_else(|_| p.clone(), |a| a.display().to_string()),
            ),
            Some(p) => s.aw.select(p),
            None => {}
        }
        win.set_child(Some(&s.aw.root));
        kit::snapshot_on_map(&win, "store");
        win.present();
    });
    app.run_with_args::<&str>(&[])
}

pub struct Store {
    pub aw: w::AppWindow,
    pub search: gtk::SearchEntry,
    show_results: Rc<dyn Fn(&str)>,
    shared: Rc<Shared>,
    /// The install page: `None` while the file is checked.
    show_package: Rc<dyn Fn(Option<PackageFile>)>,
    /// The details page: `None` reads the details off the UI thread first.
    show_details: Rc<dyn Fn(Subject, Option<Loaded>)>,
    /// The Community tab's lock.
    pub aur: Rc<AurGate>,
    /// Shows a listing on the Plugins page.
    show_plugins: Rc<dyn Fn(plugins::Listing)>,
}

impl Store {
    /// Results for `term` with these repo and Flathub hits, without searching (goldens).
    pub fn search_with(
        &self,
        term: &str,
        pkgs: Vec<store::PkgHit>,
        flat: Vec<wol_core::flatpak::Hit>,
    ) {
        *self.shared.preset.borrow_mut() = Some((pkgs, flat));
        self.search_now(term);
    }

    /// Shows results for `term` now (no typing delay): goldens and `wol-store search:<term>`.
    pub fn search_now(&self, term: &str) {
        self.search.set_text(term);
        (self.show_results)(term);
    }

    /// Install page for the `.wol` at absolute `path` (double-click): checked off the UI thread.
    pub fn open_package(&self, path: &str) {
        (self.show_package)(None);
        let (tx, rx) = async_channel::bounded(1);
        let p = path.to_string();
        std::thread::spawn(move || {
            let _ = tx.send_blocking(PackageFile::read(&p));
        });
        let show = self.show_package.clone();
        glib::spawn_future_local(async move {
            if let Ok(f) = rx.recv().await {
                show(Some(f));
            }
        });
    }

    /// Details page for `s`, its catalogue details read off the UI thread.
    pub fn open_details(&self, s: Subject) {
        (self.show_details)(s, None);
    }

    /// Details page with details already read (goldens).
    pub fn show_details(&self, s: Subject, l: Loaded) {
        (self.show_details)(s, Some(l));
    }

    /// The Plugins page with this listing, without downloading (goldens).
    pub fn show_plugins(&self, l: plugins::Listing) {
        (self.show_plugins)(l);
        self.aw.select("Plugins");
    }

    /// Install page for an already checked file (goldens).
    pub fn show_package(&self, f: PackageFile) {
        (self.show_package)(Some(f));
    }
}

/// A `.wol` file as the install page sees it (ADR 0011).
#[derive(Debug)]
pub struct PackageFile {
    pub path: String,
    /// [`package::verify`] with the trusted keys; the error text is never shown.
    pub verified: Result<package::Verified, String>,
    /// Version of the same app installed now.
    pub installed: Option<String>,
    /// `icon.png` from the file.
    pub icon: Option<Vec<u8>>,
}

impl PackageFile {
    /// Blocking: hashes the whole file.
    pub fn read(path: &str) -> Self {
        let keys = package::read_keys(std::path::Path::new(shared::package::KEYS_DIR));
        Self::read_with(
            path,
            &keys,
            std::path::Path::new(shared::package::RECORDS_DIR),
        )
    }

    pub fn read_with(path: &str, keys: &[[u8; 32]], records: &std::path::Path) -> Self {
        let p = std::path::Path::new(path);
        let verified = package::verify(p, keys);
        let (installed, icon) = match &verified {
            Ok(v) => (
                package::installed(records)
                    .into_iter()
                    .find(|i| i.id == v.info.id)
                    .map(|i| i.version),
                v.icon
                    .as_ref()
                    .and_then(|m| package::read_member(p, m, 4 << 20).ok()),
            ),
            Err(_) => (None, None),
        };
        PackageFile {
            path: path.into(),
            verified,
            installed,
            icon,
        }
    }
}

/// What the install page says about a [`PackageFile`] (no GTK: tested and copy-linted).
#[derive(Debug, PartialEq)]
pub struct PackageSheet {
    pub name: String,
    pub summary: String,
    /// "Version 3.1.0 · com.example.App".
    pub detail: String,
    /// Trust row: (icon, title, subtitle).
    pub trust: (&'static str, &'static str, &'static str),
    /// (icon, label) for each capability the app declares.
    pub capabilities: Vec<(&'static str, &'static str)>,
    /// wol-admind `wol-install` trust argument; `None`: the file can't be installed.
    pub install: Option<&'static str>,
    /// "Install", "Update", or "Open" when this version is installed already.
    pub button: &'static str,
}

/// Trust rows of the install page: signed by a trusted key, not signed, refused.
pub const TRUST_ROWS: [(&str, &str, &str); 3] = [
    (
        "security-high-symbolic",
        "Verified",
        "Signed by a developer WolOS trusts, and unchanged since.",
    ),
    (
        "dialog-warning-symbolic",
        "Not signed",
        "WolOS can’t check who made this app. Install it only if you trust where it came from.",
    ),
    (
        "dialog-warning-symbolic",
        "Can’t be installed",
        "This file is damaged, was changed after it was signed, or comes from a developer WolOS doesn’t know.",
    ),
];

/// Capability rows: icon and label for every `shared::package::CAPABILITIES` id.
pub fn capability_row(id: &str) -> Option<(&'static str, &'static str)> {
    let icon = match id {
        "network" => "network-wireless-symbolic",
        "files" => "folder-symbolic",
        "camera" => "camera-photo-symbolic",
        "microphone" => "microphone-sensitivity-high-symbolic",
        "location" => "preferences-desktop-locale-symbolic",
        "screen-capture" => "wolos-capture-screen-symbolic",
        "notifications" => "preferences-system-notifications-symbolic",
        "background" => "emblem-system-symbolic",
        "devices" => "drive-harddisk-symbolic",
        _ => return None,
    };
    let label = shared::package::CAPABILITIES.iter().find(|c| c.0 == id)?.1;
    Some((icon, label))
}

pub fn package_sheet(f: &PackageFile) -> PackageSheet {
    let file = std::path::Path::new(&f.path);
    let Ok(v) = &f.verified else {
        let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned());
        return PackageSheet {
            name: stem.unwrap_or_default(),
            summary: String::new(),
            detail: String::new(),
            trust: TRUST_ROWS[2],
            capabilities: vec![],
            install: None,
            button: "Install",
        };
    };
    let i = &v.info;
    let (trust, install) = match v.trust {
        package::Trust::Signed(_) => (TRUST_ROWS[0], "signed"),
        package::Trust::Unsigned => (TRUST_ROWS[1], "unsigned"),
    };
    PackageSheet {
        name: if i.name.is_empty() {
            i.id.clone()
        } else {
            i.name.clone()
        },
        summary: i.summary.clone(),
        detail: format!("Version {} · {}", i.version, i.id),
        trust,
        capabilities: i
            .capabilities
            .iter()
            .filter_map(|c| capability_row(c))
            .collect(),
        install: Some(install),
        button: match &f.installed {
            Some(v) if *v == i.version => "Open",
            Some(_) => "Update",
            None => "Install",
        },
    }
}

/// Search hits handed in instead of searched for (goldens).
type Found = (Vec<store::PkgHit>, Vec<wol_core::flatpak::Hit>);

/// Opens the details page for one app.
type OpenDetails = Rc<dyn Fn(Subject)>;

struct Shared {
    ctx: Rc<Ctx>,
    env: Env,
    preset: RefCell<Option<Found>>,
    /// One inline error line per window (HIG §3).
    status: gtk::Label,
    /// Opens the details page (set once the page exists).
    /// Tiles waiting to hear where their app comes from.
    badges: RefCell<Vec<(String, w::AppTile)>>,
    open_details: RefCell<Option<OpenDetails>>,
    /// What this window installed and removed: every tile, page and later search reads it
    /// before the start-up snapshot (QA-STORE-31, QA-STORE-64).
    changed: RefCell<Changes>,
}

/// Installs (true) and removals (false) this window made, last one per target wins.
#[derive(Default, Debug)]
pub struct Changes(Vec<(String, bool)>);

impl Changes {
    pub fn set(&mut self, target: &str, installed: bool) {
        self.0.retain(|(t, _)| t != target);
        self.0.push((target.to_owned(), installed));
    }
    pub fn get(&self, target: &str) -> Option<bool> {
        self.0.iter().find(|(t, _)| t == target).map(|(_, i)| *i)
    }
}

/// The app or package an install or remove action is about.
pub fn action_target(a: &Action) -> Option<&str> {
    match a {
        Action::PkgInstall(t)
        | Action::PkgRemove(t)
        | Action::FlatpakInstall(t)
        | Action::FlatpakRemove(t) => Some(t),
        Action::Admin(r) => r.args.first().map(String::as_str),
        _ => None,
    }
}

fn say(l: &gtk::Label, text: &str) {
    l.set_label(text);
    l.set_visible(!text.is_empty());
}

fn icon(a: &App, size: i32) -> gtk::Widget {
    w::store_icon(a.id, size)
}

/// Where wol-admind (system) or this account's Flatpak (user) publishes an install's percentage.
fn progress_path(target: &str, user: bool) -> Option<String> {
    let dir = if user {
        wol_core::progress::user_dir()?
    } else {
        wol_core::progress::SYSTEM_DIR.to_owned()
    };
    wol_core::progress::file_for(&dir, target)
}

/// Polls the progress file while an install runs; stops when dropped from the returned cell.
fn poll_progress(
    path: Option<String>,
    set: Rc<dyn Fn(w::TileState)>,
) -> Rc<RefCell<Option<glib::SourceId>>> {
    let id: Rc<RefCell<Option<glib::SourceId>>> = Default::default();
    if let Some(path) = path {
        *id.borrow_mut() = Some(glib::timeout_add_local(
            std::time::Duration::from_millis(250),
            move || {
                if let Some(p) = wol_core::progress::read(&path) {
                    set(w::TileState::Progress(p));
                }
                glib::ControlFlow::Continue
            },
        ));
    }
    id
}

/// Runs an install or remove `action`; `done(ok)` on the UI thread. Installs show progress.
#[allow(clippy::too_many_arguments)]
fn run_store_action(
    sh: &Rc<Shared>,
    action: Action,
    progress: Option<String>,
    set: Rc<dyn Fn(w::TileState)>,
    installed: Rc<std::cell::Cell<bool>>,
    now_installed: bool,
    name: String,
) {
    set(if now_installed {
        w::TileState::Installing
    } else {
        w::TileState::Removing
    });
    say(&sh.status, "");
    let poll = poll_progress(progress, set.clone());
    let st = sh.status.clone();
    let target = action_target(&action).map(str::to_owned);
    let shared = Rc::downgrade(sh);
    sh.ctx.act(
        action,
        Rc::new(move |r: Result<(), ActionError>| {
            if let Some(id) = poll.borrow_mut().take() {
                id.remove();
            }
            match r {
                Ok(()) => {
                    installed.set(now_installed);
                    if let (Some(t), Some(sh)) = (&target, shared.upgrade()) {
                        sh.changed.borrow_mut().set(t, now_installed);
                    }
                }
                Err(e) => say(
                    &st,
                    &if now_installed {
                        crate::copy::install(e, &name)
                    } else {
                        crate::copy::remove(e, &name)
                    },
                ),
            }
            set(if installed.get() {
                w::TileState::Open
            } else {
                w::TileState::Get
            });
        }),
    );
}

fn install_action(target: &str, flathub: bool, system: bool) -> Action {
    match (flathub, system) {
        (true, false) => Action::FlatpakInstall(target.into()),
        (true, true) => Action::Admin(shared::admin::AdminRequest {
            action: shared::admin::AdminAction::FlatpakInstall,
            args: vec![target.into()],
        }),
        _ => Action::PkgInstall(target.into()),
    }
}

fn remove_action(target: &str, flathub: bool, system: bool) -> Action {
    match (flathub, system) {
        (true, false) => Action::FlatpakRemove(target.into()),
        (true, true) => Action::Admin(shared::admin::AdminRequest {
            action: shared::admin::AdminAction::FlatpakRemove,
            args: vec![target.into()],
        }),
        _ => Action::PkgRemove(target.into()),
    }
}

fn launch(target: &str, flathub: bool, desktop: Option<&str>) {
    if flathub {
        kit::spawn_app(&["flatpak".into(), "run".into(), target.into()]);
    } else if let Some(d) = desktop {
        kit::spawn_app(&["gtk4-launch".into(), launch_arg(d)]);
    }
}

/// What `gtk4-launch` is given for a catalog desktop id. The catalog's ids are desktop entry ids
/// without the file extension, and `org.telegram.desktop` ends in ".desktop" by itself: gtk4-launch
/// takes a name that ends in ".desktop" for the file name, finds no such file and says nothing
/// (live-1 H1). The extension is always added.
pub(crate) fn launch_arg(id: &str) -> String {
    format!("{id}.desktop")
}

/// The state a details page (or tile) opens in: Open when the app is on this computer, by the
/// catalog's desktop file or by what the package database says (`Loaded::installed`): a repo
/// package that is not in the catalog has no desktop file the Store knows, but it is there
/// (QA-STORE-30).
pub(crate) fn start_state(present: bool, installed_version: Option<&str>) -> w::TileState {
    if present || installed_version.is_some_and(|v| !v.is_empty()) {
        w::TileState::Open
    } else {
        w::TileState::Get
    }
}

/// Whether "Open" has something to start: a Flathub app, or a package with a desktop id. A
/// package without one is installed but is not an app to open (a library, a command-line tool).
pub(crate) fn can_open(flathub: bool, desktop: Option<&str>) -> bool {
    flathub || desktop.is_some()
}

fn is_present(sh: &Shared, target: &str, flathub: bool) -> bool {
    if let Some(now) = sh.changed.borrow().get(target) {
        return now;
    }
    if flathub {
        sh.env.flatpaks.iter().any(|f| f == target)
    } else {
        store::get(target).is_some_and(|a| store::is_installed(a, &sh.env.data_dirs))
    }
}

/// Get -> install (Authenticate sheet when needed) -> Open; Open launches the .desktop file.
#[allow(clippy::too_many_arguments)]
fn wire(
    sh: &Rc<Shared>,
    target: String,
    name: String,
    flathub: bool,
    desktop: Option<&'static str>,
    set: Rc<dyn Fn(w::TileState)>,
    button: &gtk::Button,
    menu_on: Option<&gtk::Box>,
) {
    let installed = Rc::new(std::cell::Cell::new(false));
    if is_present(sh, &target, flathub) {
        installed.set(true);
        set(w::TileState::Open);
    }
    if let Some(root) = menu_on {
        remove_menu(sh, &target, &name, flathub, &set, &installed, root);
    }
    let sh = sh.clone();
    button.connect_clicked(move |_| {
        if installed.get() {
            return launch(&target, flathub, desktop);
        }
        run_store_action(
            &sh,
            install_action(&target, flathub, false),
            progress_path(&target, flathub),
            set.clone(),
            installed.clone(),
            true,
            name.clone(),
        );
    });
}

/// Right-click on an installed tile: Remove (wol-admind `pkg-remove`, Authenticate sheet).
fn remove_menu(
    sh: &Rc<Shared>,
    target: &str,
    name: &str,
    flathub: bool,
    set: &Rc<dyn Fn(w::TileState)>,
    installed: &Rc<std::cell::Cell<bool>>,
    root: &gtk::Box,
) {
    let click = gtk::GestureClick::new();
    click.set_button(gtk::gdk::BUTTON_SECONDARY);
    let (sh, target, name, set, installed) = (
        sh.clone(),
        target.to_string(),
        name.to_string(),
        set.clone(),
        installed.clone(),
    );
    let r = root.clone();
    click.connect_pressed(move |_, _, x, y| {
        if !installed.get() {
            return;
        }
        let label = format!("Remove {name}");
        let (sh, target, name, set, inst) = (
            sh.clone(),
            target.clone(),
            name.clone(),
            set.clone(),
            installed.clone(),
        );
        let remove = Box::new(move || {
            run_store_action(
                &sh,
                remove_action(&target, flathub, false),
                None,
                set.clone(),
                inst.clone(),
                false,
                name.clone(),
            );
        });
        kit::data::popup_menu(&r, x, y, vec![(label, None, remove)]);
    });
    root.add_controller(click);
}

/// A click on the tile itself (not its button) opens the details page.
fn open_on_click(sh: &Rc<Shared>, root: &gtk::Box, s: Subject) {
    let click = gtk::GestureClick::new();
    click.set_button(gtk::gdk::BUTTON_PRIMARY);
    let sh = sh.clone();
    click.connect_released(move |g, _, _, _| {
        g.set_state(gtk::EventSequenceState::Claimed);
        let open = sh.open_details.borrow().clone();
        if let Some(open) = open {
            open(s.clone());
        }
    });
    root.add_controller(click);
}

/// Say where each new tile's app comes from, once the index has answered.
fn fill_badges(sh: &Rc<Shared>) {
    let tiles = std::mem::take(&mut *sh.badges.borrow_mut());
    if !sh.env.pkg_search || tiles.is_empty() {
        return;
    }
    let names: Vec<String> = tiles.iter().map(|t| t.0.clone()).collect();
    let (tx, rx) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = tx.send_blocking(crate::packages::origins(&names));
    });
    glib::spawn_future_local(async move {
        let Ok(found) = rx.recv().await else { return };
        for (name, t) in &tiles {
            if let Some((_, origin)) = found.iter().find(|f| f.0 == *name) {
                t.set_source(origin);
            }
        }
    });
}

fn tile(sh: &Rc<Shared>, a: &'static App) -> gtk::Widget {
    let t = w::AppTile::new(&icon(a, w::ICON_L), a.name, a.summary);
    let t2 = t.clone();
    wire(
        sh,
        a.id.into(),
        a.name.into(),
        false,
        Some(a.desktop),
        Rc::new(move |s| t2.set_state(s)),
        &t.action,
        Some(&t.root),
    );
    open_on_click(sh, &t.root, Subject::catalog(a));
    let pkg = a.packages.first().copied().unwrap_or(a.id);
    sh.badges.borrow_mut().push((pkg.to_owned(), t.clone()));
    t.root.upcast()
}

fn pkg_tile(sh: &Rc<Shared>, h: &store::PkgHit) -> gtk::Widget {
    let t = w::AppTile::new(&w::store_icon(&h.name, w::ICON_L), &h.name, &h.summary);
    let t2 = t.clone();
    wire(
        sh,
        h.name.clone(),
        h.name.clone(),
        false,
        None,
        Rc::new(move |s| t2.set_state(s)),
        &t.action,
        Some(&t.root),
    );
    open_on_click(sh, &t.root, Subject::pkg(h));
    sh.badges.borrow_mut().push((h.name.clone(), t.clone()));
    t.root.upcast()
}

/// A Flathub app: same tile, a Flathub badge under the name, installed for this account.
fn flathub_tile(sh: &Rc<Shared>, h: &wol_core::flatpak::Hit) -> gtk::Widget {
    let t = w::AppTile::new(&w::store_icon(&h.id, w::ICON_L), &h.name, &h.summary);
    t.set_source("Flathub");
    let t2 = t.clone();
    wire(
        sh,
        h.id.clone(),
        h.name.clone(),
        true,
        None,
        Rc::new(move |s| t2.set_state(s)),
        &t.action,
        Some(&t.root),
    );
    open_on_click(sh, &t.root, Subject::flathub(h));
    t.root.upcast()
}

/// A community package: no Get button here, because building one is a decision made in Terminal.
fn aur_tile(sh: &Rc<Shared>, h: &store::PkgHit) -> gtk::Widget {
    let t = w::AppTile::new(&w::store_icon(&h.name, w::ICON_L), &h.name, &h.summary);
    t.set_source("Community");
    t.action.set_visible(false);
    open_on_click(sh, &t.root, Subject::aur(h));
    t.root.upcast()
}

/// What the states and flags mean, under the community results.
fn legend() -> gtk::Box {
    use crate::packages::{FLAGS, SEVERITIES, STATES};
    let b = w::vbox(w::S6);
    let states: Vec<gtk::Widget> = STATES.iter().map(|(n, i, t)| note_row(i, n, t)).collect();
    b.append(&w::section("What the Check Can Say", &states));
    let levels: Vec<gtk::Widget> = SEVERITIES
        .iter()
        .map(|(n, t)| note_row(severity_icon(n), n, t))
        .collect();
    b.append(&w::section("How Serious a Flag Is", &levels));
    let flags: Vec<gtk::Widget> = FLAGS
        .iter()
        .map(|(n, l, t)| note_row(severity_icon(l), n, &format!("{l}. {t}")))
        .collect();
    b.append(&w::section("The Flags", &flags));
    b
}

fn severity_icon(level: &str) -> &'static str {
    match level {
        "High" => "dialog-warning-symbolic",
        "Medium" => "view-more-symbolic",
        _ => "help-about-symbolic",
    }
}

/// Why the Community tab wants a password.
const AUR_WHY: &str =
    "Community packages are not reviewed by WolOS. Enter your password to open them.";

/// The Community tab: the warning always, the packages only after wol-admind has checked a password.
pub struct AurGate {
    locked: gtk::Box,
    open: gtk::Box,
    asking: std::cell::Cell<bool>,
}

impl AurGate {
    fn is_open(&self) -> bool {
        self.open.is_visible()
    }

    /// wol-admind said yes. Lasts until this window closes.
    pub fn show_open(&self) {
        self.locked.set_visible(false);
        self.open.set_visible(true);
    }
}

/// Asks wol-admind, which checks the password every time. Anything but yes leaves the tab locked.
fn ask_aur(sh: &Rc<Shared>, gate: &Rc<AurGate>) {
    if gate.is_open() || gate.asking.replace(true) {
        return;
    }
    say(&sh.status, "");
    let was = sh.ctx.set_why(AUR_WHY);
    let (ctx, st, gate) = (sh.ctx.clone(), sh.status.clone(), gate.clone());
    sh.ctx.act(
        Action::Admin(shared::admin::AdminRequest {
            action: shared::admin::AdminAction::AurOpen,
            args: vec![],
        }),
        Rc::new(move |r: Result<(), ActionError>| {
            gate.asking.set(false);
            ctx.set_why(&was);
            match r {
                Ok(()) => gate.show_open(),
                // Cancel (the sheet's `Cancelled`; `Denied` from a sheet that gave up): the tab
                // stays locked, and that is the whole answer.
                Err(ActionError::Denied | ActionError::Cancelled) => {}
                Err(e) => say(&st, e.message()),
            }
        }),
    );
}

/// The Community page: apart from the catalogue and from search, its risk before any name.
fn aur_page(sh: &Rc<Shared>) -> (gtk::Box, Rc<AurGate>) {
    let p = w::page("Community");
    p.append(&w::section(
        "Before You Open",
        &[note_row(
            "dialog-warning-symbolic",
            "Community Packages",
            crate::packages::AUR_RISK,
        )],
    ));
    let locked = w::vbox(w::S2);
    locked.append(&w::label(
        "Community packages stay locked until you enter your password.",
        "secondary",
    ));
    let open = w::vbox(w::S2);
    open.set_visible(false);
    let gate = Rc::new(AurGate {
        locked: locked.clone(),
        open: open.clone(),
        asking: Default::default(),
    });
    let (sh2, g2) = (sh.clone(), gate.clone());
    let again = w::button("Enter Password", w::Kind::Default, move || {
        ask_aur(&sh2, &g2)
    });
    again.set_halign(gtk::Align::Start);
    locked.append(&again);
    p.append(&locked);
    // Previews have no package search and nothing to unlock.
    if sh.env.pkg_search {
        let (sh2, g2) = (sh.clone(), gate.clone());
        p.connect_map(move |_| ask_aur(&sh2, &g2));
    }
    let found = w::vbox(w::S2);
    let entry = gtk::SearchEntry::new();
    entry.set_placeholder_text(Some("Search community packages"));
    let (sh2, f3) = (sh.clone(), found.clone());
    entry.connect_search_changed(move |e| {
        while let Some(c) = f3.first_child() {
            f3.remove(&c);
        }
        let term = e.text().trim().to_string();
        if term.is_empty() {
            return;
        }
        let (tx, rx) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let _ = tx.send_blocking(crate::packages::aur_search(&term));
        });
        let (sh, found) = (sh2.clone(), f3.clone());
        glib::spawn_future_local(async move {
            if let Ok(hits) = rx.recv().await {
                found.append(&grid(hits.iter().take(30).map(|h| aur_tile(&sh, h))));
            }
        });
    });
    open.append(&entry);
    open.append(&found);
    open.append(&legend());
    p.append(&open);
    (p, gate)
}

/// What reading a package found: its state, its flags, each sentence, and the graph if drawn.
pub fn audit_section(a: &crate::packages::Audit) -> gtk::Box {
    use crate::packages::STATES;
    let state = STATES.iter().find(|s| s.0 == a.state).unwrap_or(&STATES[3]);
    let mut rows: Vec<gtk::Widget> = vec![note_row(state.1, state.0, state.2)];
    rows.extend(a.flags.iter().map(|f| {
        let times = if f.count > 1 {
            format!(" Found {} times.", f.count)
        } else {
            String::new()
        };
        note_row(
            severity_icon(&f.severity),
            &f.title,
            &format!("{}.{times}", f.severity),
        )
    }));
    let b = w::vbox(w::S2);
    b.append(&w::section("What the Check Found", &rows));
    if !a.findings.is_empty() {
        let paths: Vec<gtk::Widget> = a
            .findings
            .iter()
            .map(|(t, f)| note_row("dialog-warning-symbolic", t, f))
            .collect();
        b.append(&w::section("Where It Was Found", &paths));
    }
    if let Some(g) = &a.graph {
        let pic = gtk::Picture::for_filename(g);
        pic.set_content_fit(gtk::ContentFit::ScaleDown);
        pic.set_can_shrink(true);
        pic.set_size_request(-1, w::SCREENSHOT_H);
        b.append(&pic);
    }
    b
}

/// A community package's part of a details page: the risk, the way to install it, and the check on request.
fn aur_details(page: &gtk::Box, s: &Subject) {
    let how = format!(
        "To install it, run wopkg install {} in Terminal, where you can read what it does first.",
        s.target
    );
    let rows = [
        note_row(
            "dialog-warning-symbolic",
            "Community Package",
            crate::packages::AUR_RISK,
        ),
        note_row("package-x-generic-symbolic", "How to Install", &how),
    ];
    page.append(&w::section("Before You Install", &rows));
    let found = w::vbox(w::S2);
    let (f2, target) = (found.clone(), s.target.clone());
    let read = w::button("Read This Package", w::Kind::Default, move || {
        let (tx, rx) = async_channel::bounded(1);
        let t = target.clone();
        std::thread::spawn(move || {
            let _ = tx.send_blocking(crate::packages::audit(&t));
        });
        let f = f2.clone();
        glib::spawn_future_local(async move {
            if let Ok(a) = rx.recv().await {
                while let Some(c) = f.first_child() {
                    f.remove(&c);
                }
                f.append(&audit_section(&a));
            }
        });
    });
    read.set_halign(gtk::Align::Start);
    page.append(&read);
    page.append(&found);
}

/// Whether a native package (catalog first, then a repo hit) already covers a Flathub app: by
/// the id's last part or by the app's name, either case (QA-STORE-70: Arch `ponysay` and
/// Flathub "Ponysay" were two tiles).
pub fn covered_natively(h: &wol_core::flatpak::Hit, pkgs: &[store::PkgHit]) -> bool {
    let guess = wol_core::flatpak::native_guess(h);
    let name = h.name.to_lowercase();
    store::search(&h.name)
        .iter()
        .any(|a| a.name.to_lowercase() == name || a.packages.contains(&guess.as_str()))
        || pkgs
            .iter()
            .any(|p| p.name == guess || p.name.to_lowercase() == name)
}

/// The Flathub hits no native one covers ([`covered_natively`]), for every search path (the
/// package index's answer had no such filter).
pub fn flathub_beyond_native(
    pkgs: &[store::PkgHit],
    flat: Vec<wol_core::flatpak::Hit>,
) -> Vec<wol_core::flatpak::Hit> {
    flat.into_iter()
        .filter(|h| !covered_natively(h, pkgs))
        .collect()
}

/// Repo hits the catalog's tiles for `term` do not already show: one app, one tile.
pub fn beyond_catalog<'a>(term: &str, pkgs: &'a [store::PkgHit]) -> Vec<&'a store::PkgHit> {
    let shown = store::search(term);
    pkgs.iter()
        .filter(|p| !shown.iter().any(|a| a.packages.contains(&p.name.as_str())))
        .collect()
}

/// Repo and Flathub hits under the catalog's, one tile per app.
fn show_more(
    sh: &Rc<Shared>,
    term: &str,
    more: &gtk::Box,
    empty: &gtk::Widget,
    pkgs: &[store::PkgHit],
    flat: &[wol_core::flatpak::Hit],
) {
    let pkgs = beyond_catalog(term, pkgs);
    if !pkgs.is_empty() || !flat.is_empty() {
        empty.set_visible(false);
    }
    if !pkgs.is_empty() {
        more.append(&w::label("More packages", "title"));
        more.append(&grid(pkgs.iter().take(30).map(|h| pkg_tile(sh, h))));
    }
    if !flat.is_empty() {
        more.append(&w::label("From Flathub", "title"));
        more.append(&grid(flat.iter().take(30).map(|h| flathub_tile(sh, h))));
    }
}

fn grid(tiles: impl IntoIterator<Item = gtk::Widget>) -> gtk::FlowBox {
    let g = w::wrap_row(w::S6, 3);
    g.set_halign(gtk::Align::Fill);
    for t in tiles {
        g.append(&t);
    }
    g
}

fn shelf(title: &str, g: &gtk::FlowBox) -> gtk::Box {
    let b = w::vbox(w::S2);
    b.append(&w::label(title, "title"));
    b.append(g);
    b
}

/// Page banner for Discover (`None`) or a category, from `store::hero`.
fn hero(sh: &Rc<Shared>, page: Option<Category>) -> Option<gtk::Widget> {
    let h = store::hero(page);
    let a = store::get(h.app)?;
    let get = gtk::Button::with_label("Get");
    get.add_css_class("get-button");
    let card = w::hero_banner(
        h.colors,
        a.name,
        h.headline,
        h.subhead,
        &icon(a, w::HERO_ICON),
        &get,
    );
    let g2 = get.clone();
    wire(
        sh,
        a.id.into(),
        a.name.into(),
        false,
        Some(a.desktop),
        Rc::new(move |s| {
            g2.set_sensitive(!s.busy());
            g2.set_label(&button_label(s));
        }),
        &get,
        None,
    );
    Some(card.upcast())
}

fn discover(sh: &Rc<Shared>) -> gtk::Box {
    let p = w::page("Discover");
    if let Some(h) = hero(sh, None) {
        p.append(&h);
    }
    for (title, ids) in store::ROWS {
        let g = grid(
            ids.iter()
                .filter_map(|id| store::get(id))
                .map(|a| tile(sh, a)),
        );
        p.append(&shelf(title, &g));
    }
    p
}

fn category(sh: &Rc<Shared>, c: Category) -> gtk::Box {
    let p = w::page(c.name());
    if let Some(h) = hero(sh, Some(c)) {
        p.append(&h);
    }
    p.append(&grid(store::in_category(c).map(|a| tile(sh, a))));
    if c == Category::Develop {
        let tools = store::TOOLCHAINS.iter().map(|(name, summary)| {
            pkg_tile(
                sh,
                &store::PkgHit {
                    name: (*name).into(),
                    summary: (*summary).into(),
                },
            )
        });
        p.append(&shelf("SDKs and Toolchains", &grid(tools)));
    }
    p
}

/// What a kit's Get button says: like an app's, but a kit has nothing to open.
pub fn kit_label(s: w::TileState) -> String {
    match s {
        w::TileState::Open => "Added".into(),
        s => button_label(s),
    }
}

/// A part's row icon: what kind of thing it lets people play or read.
pub fn part_icon(k: &store::Kit, p: &store::KitPart) -> &'static str {
    if k.source != store::KitSource::Packages {
        "applications-graphics-symbolic"
    } else if p
        .packages
        .iter()
        .any(|n| n.starts_with("ttf-") || n.starts_with("noto-"))
    {
        "preferences-desktop-locale-symbolic"
    } else if p
        .packages
        .iter()
        .any(|n| matches!(*n, "x264" | "x265" | "libde265"))
    {
        "video-display-symbolic"
    } else {
        "audio-x-generic-symbolic"
    }
}

/// Whether a kit is already here: a packages kit when every one of its packages is installed, a
/// wopkg kit when its recipe is. `pacman` and `recipes` come from wopkg's installed view.
pub fn kit_installed(
    k: &store::Kit,
    pacman: &std::collections::HashSet<String>,
    recipes: &std::collections::HashSet<String>,
) -> bool {
    match k.source {
        store::KitSource::Packages => {
            let p = k.packages();
            !p.is_empty() && p.iter().all(|n| pacman.contains(*n))
        }
        store::KitSource::Wopkg(r) => recipes.contains(r),
    }
}

/// [`kit_installed`] against `set` ([`Env::installed`]), else on this computer (reads the package
/// database folders; worker thread only).
fn kit_installed_here(k: &store::Kit, set: Option<&wopkg::context::Installed>) -> bool {
    if let Some(i) = set {
        return kit_installed(k, &i.pacman, &i.recipes);
    }
    wopkg::context::Context::new(Default::default())
        .map(|c| {
            let i = c.installed();
            kit_installed(k, &i.pacman, &i.recipes)
        })
        .unwrap_or(false)
}

/// Get on a kit: one wol-admind `pkg-install <kit id>` (one approval for every part), or a wopkg
/// install for this account. Nothing is ever ticked or installed before the person clicks.
fn wire_kit(sh: &Rc<Shared>, k: &'static store::Kit, b: &gtk::Button) {
    let b2 = b.clone();
    let set: Rc<dyn Fn(w::TileState)> = Rc::new(move |s| {
        b2.set_sensitive(!s.busy() && s != w::TileState::Open);
        b2.set_label(&kit_label(s));
    });
    // The start state: worked out off the GTK thread, so a kit got before says Added.
    {
        let (tx, rx) = async_channel::bounded(1);
        let injected = sh.env.installed.clone();
        std::thread::spawn(move || {
            let _ = tx.send_blocking(kit_installed_here(k, injected.as_ref()));
        });
        let (set, b) = (set.clone(), b.clone());
        glib::spawn_future_local(async move {
            // Only over the untouched Get: a click made meanwhile owns the button.
            if rx.recv().await.unwrap_or(false)
                && b.is_sensitive()
                && b.label().as_deref() == Some(kit_label(w::TileState::Get).as_str())
            {
                set(w::TileState::Open);
            }
        });
    }
    let sh = sh.clone();
    b.connect_clicked(move |_| match k.wopkg_args() {
        None => run_store_action(
            &sh,
            install_action(k.id, false, false),
            progress_path(k.id, false),
            set.clone(),
            Rc::new(std::cell::Cell::new(false)),
            true,
            k.name.into(),
        ),
        Some(args) => {
            set(w::TileState::Installing);
            say(&sh.status, "");
            let (tx, rx) = async_channel::bounded(1);
            std::thread::spawn(move || {
                let a: Vec<&str> = args.iter().map(String::as_str).collect();
                let _ = tx.send_blocking(crate::packages::wopkg_verb(&a));
            });
            let (set, st) = (set.clone(), sh.status.clone());
            glib::spawn_future_local(async move {
                let ok = rx.recv().await.unwrap_or(false);
                if !ok {
                    say(&st, crate::copy::STORE_NOT_FINISHED);
                }
                set(if ok {
                    w::TileState::Open
                } else {
                    w::TileState::Get
                });
            });
        }
    });
}

/// Kits (ADR 0032 B): a set of parts in one tile and one click, the most wanted part first.
fn kits(sh: &Rc<Shared>) -> gtk::Box {
    let p = w::page("Kits");
    let intro = w::label(crate::copy::STORE_KITS_INTRO, "secondary");
    intro.set_wrap(true);
    p.append(&intro);
    // Only kits that can apply here: a wopkg kit without its recipe is not shown.
    let dirs = store::recipe_dirs(std::env::var("WOPKG_RECIPES").ok().as_deref());
    for k in store::shown_kits(&dirs) {
        let head = w::hbox(w::S3);
        head.append(&w::app_icon(k.icon, w::ICON_L));
        let text = w::vbox(0);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);
        text.append(&w::label(k.name, "title"));
        let sum = w::label(k.summary, "secondary");
        sum.set_wrap(true);
        text.append(&sum);
        head.append(&text);
        let get = gtk::Button::with_label(&kit_label(w::TileState::Get));
        get.add_css_class("get-button");
        get.set_valign(gtk::Align::Center);
        wire_kit(sh, k, &get);
        head.append(&get);
        p.append(&head);
        let parts: Vec<gtk::Widget> = k
            .parts
            .iter()
            .map(|part| note_row(part_icon(k, part), part.name, part.why))
            .collect();
        p.append(&w::section("", &parts));
        let note = w::label(k.note, "secondary");
        note.set_wrap(true);
        p.append(&note);
    }
    p
}

fn updates(sh: &Rc<Shared>) -> gtk::Box {
    let p = w::page("Updates");
    let ic = w::icon("software-update-available-symbolic", w::ICON_M);
    ic.add_css_class("accent-icon");
    let title = w::label("", "title");
    title.set_wrap(true);
    let detail = w::label("", "secondary");
    detail.set_wrap(true);
    let text = w::vbox(0);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    text.append(&title);
    text.append(&detail);
    let ring = w::progress_ring();
    let action = gtk::Button::with_label("Check");
    action.add_css_class("get-button");
    action.set_valign(gtk::Align::Center);
    let head = w::hbox(w::S3);
    head.add_css_class("list-row");
    head.add_css_class("hero-row");
    head.append(&ic);
    head.append(&text);
    head.append(&ring);
    head.append(&action);
    let list = w::vbox(0);
    p.append(&w::section("", &[head.upcast()]));
    let list_section = w::section("Packages", &[list.clone().upcast()]);
    p.append(&list_section);
    let state = Rc::new(RefCell::new(UpdateState::Idle));
    let show: Rc<dyn Fn(UpdateState)> = {
        let (title, detail, ring, action, list, ls, state) = (
            title.clone(),
            detail.clone(),
            ring.clone(),
            action.clone(),
            list.clone(),
            list_section.clone(),
            state.clone(),
        );
        Rc::new(move |s: UpdateState| {
            let busy = matches!(s, UpdateState::Checking | UpdateState::Installing);
            ring.set_visible(busy);
            action.set_visible(!busy && s != UpdateState::Installed);
            action.set_label(if matches!(s, UpdateState::Available(_)) {
                "Update All"
            } else {
                "Check"
            });
            while let Some(c) = list.first_child() {
                list.remove(&c);
            }
            ls.set_visible(matches!(s, UpdateState::Available(_)));
            let (t, d) = match &s {
                UpdateState::Idle => (
                    "Check for updates".to_string(),
                    "See what is new for your apps.".to_string(),
                ),
                UpdateState::Checking => ("Checking for updates…".into(), String::new()),
                UpdateState::UpToDate => ("All apps are up to date".into(), String::new()),
                UpdateState::Available(v) => {
                    for n in v.iter().take(50) {
                        list.append(&w::list_row("package-x-generic-symbolic", n, "", None));
                    }
                    (
                        crate::copy::updates_available(v.len()),
                        crate::copy::UPDATE_RESTORE_POINT.into(),
                    )
                }
                UpdateState::Installing => (
                    "Installing updates…".into(),
                    "You can keep using your apps.".into(),
                ),
                UpdateState::Installed => (
                    "Updates installed".into(),
                    "Restart to finish updating.".into(),
                ),
                UpdateState::Failed(e) => ("Updates".into(), e.clone()),
            };
            title.set_label(&t);
            detail.set_label(&d);
            detail.set_visible(!d.is_empty());
            *state.borrow_mut() = s;
        })
    };
    show(UpdateState::Idle);
    {
        let (show, ctx) = (show.clone(), sh.ctx.clone());
        action.connect_clicked(move |_| {
            if matches!(*state.borrow(), UpdateState::Available(_)) {
                show(UpdateState::Installing);
                let show = show.clone();
                return ctx.act(
                    Action::PkgUpgrade,
                    Rc::new(move |r| {
                        show(match r {
                            Ok(()) => UpdateState::Installed,
                            Err(e) => UpdateState::Failed(crate::copy::update(e).into()),
                        })
                    }),
                );
            }
            show(UpdateState::Checking);
            let (tx, rx) = async_channel::bounded(1);
            std::thread::spawn(move || {
                let out = wol_core::system::run_with_timeout(
                    &["wol", "update", "check", "--json"],
                    std::time::Duration::from_secs(120),
                );
                let ua = wol_core::flatpak::updates_argv();
                let a: Vec<&str> = ua.iter().map(String::as_str).collect();
                let flat =
                    wol_core::system::run_with_timeout(&a, std::time::Duration::from_secs(60))
                        .map(|t| wol_core::flatpak::parse_ids(&t))
                        .unwrap_or_default();
                let _ = tx.send_blocking((out, flat));
            });
            let show = show.clone();
            glib::spawn_future_local(async move {
                if let Ok((out, flat)) = rx.recv().await {
                    show(with_flatpaks(parse_check(out.as_deref()), &flat));
                }
            });
        });
    }
    p
}

/// Search results: catalog hits now, `pacman -Ss` hits when they arrive.
fn fill_search(sh: &Rc<Shared>, page: &gtk::Box, term: &str) {
    while let Some(c) = page.first_child() {
        page.remove(&c);
    }
    page.append(&w::label(&format!("Results for “{term}”"), "large-title"));
    let hits = store::search(term);
    if !hits.is_empty() {
        page.append(&grid(hits.into_iter().map(|a| tile(sh, a))));
    }
    let more = w::vbox(w::S2);
    page.append(&more);
    let empty = w::empty_state(
        "system-search-symbolic",
        "No results",
        "Try a different word, like “photo” or “music”.",
        None,
    );
    empty.set_visible(store::search(term).is_empty());
    page.append(&empty);
    if let Some(found) = sh.preset.borrow().clone() {
        return show_more(sh, term, &more, empty.upcast_ref(), &found.0, &found.1);
    }
    let Some(argv) = store::search_argv(term).filter(|_| sh.env.pkg_search) else {
        return;
    };
    let flat_argv = wol_core::flatpak::search_argv(term);
    let term = term.to_owned();
    let shown_for = term.clone();
    let (tx, rx) = async_channel::bounded(1);
    std::thread::spawn(move || {
        // The package index answers in milliseconds; the commands below are the fallback.
        if let Some((pkgs, flat)) = crate::packages::search(&term) {
            let flat = flathub_beyond_native(&pkgs, flat);
            let _ = tx.send_blocking((pkgs, flat));
            return;
        }
        let run = |argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            wol_core::system::run_with_timeout(&a, std::time::Duration::from_secs(15))
        };
        let pkgs = run(&argv)
            .map(|o| store::parse_pkg_search(&o))
            .unwrap_or_default();
        let flat = flat_argv
            .and_then(|a| run(&a))
            .map(|o| wol_core::flatpak::parse_search(&o, |h| covered_natively(h, &pkgs)))
            .unwrap_or_default();
        let _ = tx.send_blocking((pkgs, flat));
    });
    let sh = sh.clone();
    glib::spawn_future_local(async move {
        let Ok((pkgs, flat)) = rx.recv().await else {
            return;
        };
        if more.parent().is_some() {
            show_more(&sh, &shown_for, &more, empty.upcast_ref(), &pkgs, &flat);
            fill_badges(&sh);
        }
    });
}

/// List row whose subtitle wraps (trust sentences in narrow windows).
fn note_row(icon: &str, title: &str, subtitle: &str) -> gtk::Widget {
    let row = w::list_row(icon, title, "", None);
    if let Some(t) = row
        .first_child()
        .and_then(|c| c.next_sibling())
        .and_downcast::<gtk::Box>()
    {
        let l = w::label(subtitle, "secondary");
        l.set_wrap(true);
        t.append(&l);
    }
    row.upcast()
}

/// The file's own `icon.png`, else its store icon (the generic package icon when none ships).
fn package_icon(f: &PackageFile, size: i32) -> gtk::Widget {
    let png = f
        .icon
        .as_ref()
        .and_then(|b| gtk::gdk::Texture::from_bytes(&glib::Bytes::from(b)).ok());
    if let Some(t) = png {
        let img = gtk::Image::from_paintable(Some(&t));
        img.set_pixel_size(size);
        return img.upcast();
    }
    let name = match &f.verified {
        Ok(v) if !v.info.icon.is_empty() => v.info.icon.as_str(),
        Ok(v) => v.info.id.as_str(),
        Err(_) => "",
    };
    w::store_icon(name, size)
}

/// Install page: icon, name, Install, who made it (trust) and what it uses. Install -> Open.
fn fill_package(sh: &Rc<Shared>, page: &gtk::Box, f: Option<PackageFile>) {
    while let Some(c) = page.first_child() {
        page.remove(&c);
    }
    say(&sh.status, "");
    let Some(f) = f else {
        let b = w::hbox(w::S3);
        b.append(&w::progress_ring());
        b.append(&w::label("Checking the app…", "secondary"));
        page.append(&b);
        return;
    };
    let s = package_sheet(&f);
    let head = w::hbox(w::S6);
    head.append(&package_icon(&f, w::HERO_ICON));
    let text = w::vbox(w::S1);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    let name = w::label(&s.name, "large-title");
    name.set_wrap(true);
    text.append(&name);
    for t in [&s.summary, &s.detail] {
        if !t.is_empty() {
            let l = w::label(t, "secondary");
            l.set_wrap(true);
            text.append(&l);
        }
    }
    let get = gtk::Button::with_label(s.button);
    get.add_css_class("get-button");
    get.set_halign(gtk::Align::Start);
    get.set_margin_top(w::S2);
    // A refused file has no button: nothing on the page can install it.
    get.set_visible(s.install.is_some());
    text.append(&get);
    head.append(&text);
    page.append(&head);
    page.append(&w::section(
        "",
        &[note_row(s.trust.0, s.trust.1, s.trust.2)],
    ));
    let (Ok(v), Some(trust)) = (&f.verified, s.install) else {
        return;
    };
    let rows: Vec<gtk::Widget> = if s.capabilities.is_empty() {
        vec![
            w::list_row(
                "object-select-symbolic",
                "Nothing beyond its own windows",
                "",
                None,
            )
            .upcast(),
        ]
    } else {
        s.capabilities
            .iter()
            .map(|(i, l)| w::list_row(i, l, "", None).upcast())
            .collect()
    };
    page.append(&w::section("What It Uses", &rows));
    let installed = Rc::new(std::cell::Cell::new(s.button == "Open"));
    let (sh, id, path, label) = (sh.clone(), v.info.id.clone(), f.path.clone(), s.button);
    get.connect_clicked(move |b| {
        if installed.get() {
            kit::spawn_app(&["gtk4-launch".into(), id.clone()]);
            return;
        }
        b.set_sensitive(false);
        b.set_label("Installing…");
        say(&sh.status, "");
        let (b, st, inst, name) = (
            b.clone(),
            sh.status.clone(),
            installed.clone(),
            s.name.clone(),
        );
        sh.ctx.act(
            Action::Admin(shared::admin::AdminRequest {
                action: shared::admin::AdminAction::WolInstall,
                args: vec![path.clone(), trust.into()],
            }),
            Rc::new(move |r: Result<(), ActionError>| {
                b.set_sensitive(true);
                match r {
                    Ok(()) => {
                        inst.set(true);
                        b.set_label("Open");
                    }
                    Err(e) => {
                        b.set_label(label);
                        say(&st, &crate::copy::install(e, &name));
                    }
                }
            }),
        );
    });
}

pub fn content(sys: Arc<dyn System>, env: Env) -> Store {
    let aw = w::AppWindow::new(true, false);
    // One ruler of content (576) in a window of 900, 840 when the pane is wide enough for a third tile.
    aw.set_column_wide(
        crate::kit::column::PANE_COLUMN,
        (
            crate::kit::column::STORE_WIDE.0,
            crate::kit::column::STORE_WIDE.1,
        ),
    );
    let ctx = Ctx::new(sys, "App Store");
    let status = w::label("", "secondary");
    status.set_wrap(true);
    status.set_visible(false);
    status.add_css_class("store-status");
    let sh = Rc::new(Shared {
        ctx,
        env,
        preset: RefCell::new(None),
        status: status.clone(),
        badges: RefCell::new(Vec::new()),
        changed: RefCell::default(),
        open_details: RefCell::new(None),
    });
    let search = aw.sidebar_search("Search");
    let sh2 = sh.clone();
    glib::idle_add_local_once(move || fill_badges(&sh2));
    aw.source_item("starred-symbolic", "Discover", &discover(&sh));
    for (c, icon) in [
        (Category::Create, "applications-graphics-symbolic"),
        (Category::Work, "applications-office-symbolic"),
        (Category::Play, "applications-games-symbolic"),
        (Category::Develop, "applications-engineering-symbolic"),
    ] {
        aw.source_item(icon, c.name(), &category(&sh, c));
    }
    aw.source_item("view-app-grid-symbolic", "Kits", &kits(&sh));
    let (plugins_page, show_plugins) = plugins::page(&sh);
    aw.source_item("wolos-widgets-symbolic", "Plugins", &plugins_page);
    aw.source_item(
        "software-update-available-symbolic",
        "Updates",
        &updates(&sh),
    );
    let (aur, aur_gate) = aur_page(&sh);
    aw.source_item("package-x-generic-symbolic", "Community", &aur);
    let results = w::vbox(w::S6);
    results.add_css_class("page");
    aw.hidden_page("Search", &results);
    let show_results: Rc<dyn Fn(&str)> = {
        let (sh, results, content, side) = (
            sh.clone(),
            results.clone(),
            aw.content.clone(),
            aw.sidebar.clone(),
        );
        Rc::new(move |term: &str| {
            side.unselect_all();
            fill_search(&sh, &results, term);
            content.set_visible_child_name("Search");
        })
    };
    let package_page = w::vbox(w::S6);
    package_page.add_css_class("page");
    aw.hidden_page("Package", &package_page);
    let show_package: Rc<dyn Fn(Option<PackageFile>)> = {
        let (sh, page, content, side) = (
            sh.clone(),
            package_page.clone(),
            aw.content.clone(),
            aw.sidebar.clone(),
        );
        Rc::new(move |f| {
            side.unselect_all();
            fill_package(&sh, &page, f);
            content.set_visible_child_name("Package");
        })
    };
    let details_page = w::vbox(w::S6);
    details_page.add_css_class("page");
    aw.hidden_page("Details", &details_page);
    let show_details: Rc<dyn Fn(Subject, Option<Loaded>)> = {
        let (sh, page, content, side) = (
            sh.clone(),
            details_page.clone(),
            aw.content.clone(),
            aw.sidebar.clone(),
        );
        let back_to: Rc<RefCell<String>> = Default::default();
        Rc::new(move |subj, loaded| {
            let now = content.visible_child_name().map(|n| n.to_string());
            if now.as_deref() != Some("Details") {
                *back_to.borrow_mut() = now.unwrap_or_else(|| "Discover".into());
            }
            let back: Rc<dyn Fn()> = {
                let (content, back_to, side) = (content.clone(), back_to.clone(), side.clone());
                Rc::new(move || {
                    let to = back_to.borrow().clone();
                    content.set_visible_child_name(&to);
                    if to != "Search" {
                        let mut i = 0;
                        while let Some(r) = side.row_at_index(i) {
                            if r.widget_name() == to.as_str() {
                                side.select_row(Some(&r));
                            }
                            i += 1;
                        }
                    }
                })
            };
            side.unselect_all();
            fill_details(&sh, &page, &subj, loaded, back);
            content.set_visible_child_name("Details");
        })
    };
    {
        let sd = show_details.clone();
        *sh.open_details.borrow_mut() = Some(Rc::new(move |subj| sd(subj, None)));
    }
    // Error line pinned under the content (inline, one sentence).
    w::toolbar_place(&aw, &w::sheet(&status));
    sh.ctx.place(&aw);
    {
        let (sh, results, content, sidebar) = (
            sh.clone(),
            results.clone(),
            aw.content.clone(),
            aw.sidebar.clone(),
        );
        let timer: Rc<RefCell<Option<glib::SourceId>>> = Default::default();
        search.connect_search_changed(move |e| {
            if let Some(t) = timer.borrow_mut().take() {
                t.remove();
            }
            let term = e.text().trim().to_string();
            if term.is_empty() {
                match sidebar.selected_row() {
                    Some(r) => content.set_visible_child_name(&r.widget_name()),
                    None => sidebar.select_row(sidebar.row_at_index(1).as_ref()),
                }
                return;
            }
            let (sh, results, content, timer2) =
                (sh.clone(), results.clone(), content.clone(), timer.clone());
            *timer.borrow_mut() = Some(glib::timeout_add_local_once(
                std::time::Duration::from_millis(250),
                move || {
                    timer2.borrow_mut().take();
                    fill_search(&sh, &results, &term);
                    content.set_visible_child_name("Search");
                },
            ));
        });
    }
    // The status sheet shows itself when there is something to say.
    if let Some(r) = status
        .ancestor(gtk::Revealer::static_type())
        .and_downcast::<gtk::Revealer>()
    {
        status.connect_visible_notify(move |l| r.set_reveal_child(l.is_visible()));
    }
    Store {
        aur: aur_gate,
        show_plugins,
        aw,
        search,
        show_results,
        shared: sh.clone(),
        show_package,
        show_details,
    }
}

#[cfg(test)]
mod tests;
