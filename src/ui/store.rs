//! App Store (`wol-store`): macOS App Store layout on the kit AppWindow. Curated catalog from
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
use wol_core::package;
use wol_core::store::{self, App, Category};

pub const PAGES: [&str; 6] = ["Discover", "Create", "Work", "Play", "Develop", "Updates"];

/// Machine facts the store reads (injected so goldens do not depend on the build host).
#[derive(Clone, Debug)]
pub struct Env {
    /// XDG data dirs searched for `applications/<id>.desktop`.
    pub data_dirs: Vec<PathBuf>,
    /// Run `pacman -Ss` and `flatpak search` for non-catalog results.
    pub pkg_search: bool,
    /// Flathub apps installed (any scope).
    pub flatpaks: Vec<String>,
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
        "WolOS can't check who made this app. Install it only if you trust where it came from.",
    ),
    (
        "dialog-warning-symbolic",
        "Can't be installed",
        "This file is damaged, was changed after it was signed, or comes from a developer WolOS doesn't know.",
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
    open_details: RefCell<Option<OpenDetails>>,
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
    set(w::TileState::Installing);
    say(&sh.status, "");
    let poll = poll_progress(progress, set.clone());
    let st = sh.status.clone();
    sh.ctx.act(
        action,
        Rc::new(move |r: Result<(), ActionError>| {
            if let Some(id) = poll.borrow_mut().take() {
                id.remove();
            }
            match r {
                Ok(()) => installed.set(now_installed),
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
        kit::spawn(&["flatpak".into(), "run".into(), target.into()]);
    } else if let Some(d) = desktop {
        kit::spawn(&["gtk4-launch".into(), d.into()]);
    }
}

fn is_present(sh: &Shared, target: &str, flathub: bool) -> bool {
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

/// Whether a native package (catalog first, then a repo hit) already covers a Flathub app.
pub fn covered_natively(h: &wol_core::flatpak::Hit, pkgs: &[store::PkgHit]) -> bool {
    let guess = wol_core::flatpak::native_guess(h);
    let name = h.name.to_lowercase();
    store::search(&h.name)
        .iter()
        .any(|a| a.name.to_lowercase() == name || a.packages.contains(&guess.as_str()))
        || pkgs.iter().any(|p| p.name == guess)
}

/// Repo and Flathub hits under the catalog's, one tile per app.
fn show_more(
    sh: &Rc<Shared>,
    more: &gtk::Box,
    empty: &gtk::Widget,
    pkgs: &[store::PkgHit],
    flat: &[wol_core::flatpak::Hit],
) {
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
    let g = w::wrap_row(w::S4, 3);
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
                        format!(
                            "{} update{} available",
                            v.len(),
                            if v.len() == 1 { "" } else { "s" }
                        ),
                        "WolOS saves a restore point before updating.".into(),
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
        return show_more(sh, &more, empty.upcast_ref(), &found.0, &found.1);
    }
    let Some(argv) = store::search_argv(term).filter(|_| sh.env.pkg_search) else {
        return;
    };
    let flat_argv = wol_core::flatpak::search_argv(term);
    let term = term.to_owned();
    let (tx, rx) = async_channel::bounded(1);
    std::thread::spawn(move || {
        // The package index answers in milliseconds; the commands below are the fallback.
        if let Some(found) = crate::packages::search(&term) {
            let _ = tx.send_blocking(found);
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
            show_more(&sh, &more, empty.upcast_ref(), &pkgs, &flat);
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
    page.append(&w::section("What it uses", &rows));
    let installed = Rc::new(std::cell::Cell::new(s.button == "Open"));
    let (sh, id, path, label) = (sh.clone(), v.info.id.clone(), f.path.clone(), s.button);
    get.connect_clicked(move |b| {
        if installed.get() {
            kit::spawn(&["gtk4-launch".into(), id.clone()]);
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

/// What a tile opens: one app from the catalog, the repos or Flathub.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subject {
    /// Catalog id, package name or Flathub app id: what installs it.
    pub target: String,
    pub name: String,
    pub summary: String,
    pub flathub: bool,
    pub desktop: Option<&'static str>,
    /// The native package (`pacman -Si`, Arch's AppStream).
    pub pkg: Option<String>,
}

impl Subject {
    pub fn catalog(a: &'static App) -> Self {
        Subject {
            target: a.id.into(),
            name: a.name.into(),
            summary: a.summary.into(),
            flathub: false,
            desktop: Some(a.desktop),
            pkg: a.packages.first().map(|p| (*p).to_owned()),
        }
    }
    pub fn pkg(h: &store::PkgHit) -> Self {
        Subject {
            target: h.name.clone(),
            name: h.name.clone(),
            summary: h.summary.clone(),
            flathub: false,
            desktop: None,
            pkg: Some(h.name.clone()),
        }
    }
    pub fn flathub(h: &wol_core::flatpak::Hit) -> Self {
        Subject {
            target: h.id.clone(),
            name: h.name.clone(),
            summary: h.summary.clone(),
            flathub: true,
            desktop: None,
            pkg: None,
        }
    }
}

/// What the details page reads off the UI thread.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Loaded {
    pub details: Option<wol_core::appstream::Details>,
    /// The cached AppStream icon file.
    pub icon_file: Option<String>,
    pub size: Option<String>,
    /// Flatpak permissions it asks for.
    pub perms: Vec<wol_core::flatpak::Perm>,
    /// Installed for every account (a system Flatpak).
    pub system: bool,
}

fn capture(argv: &[String], secs: u64) -> Option<String> {
    let a: Vec<&str> = argv.iter().map(String::as_str).collect();
    wol_core::system::run_with_timeout(&a, std::time::Duration::from_secs(secs))
}

/// Blocking: AppStream, size, permissions and install scope for `s`.
pub fn load(s: &Subject) -> Loaded {
    use wol_core::{appstream, flatpak};
    let mut l = Loaded::default();
    if s.flathub {
        let home = std::env::var("HOME").unwrap_or_default();
        for (xml, icons) in appstream::flathub_paths(&home) {
            let Some(d) = std::fs::read_to_string(&xml)
                .ok()
                .and_then(|x| appstream::by_id(&x, &s.target))
            else {
                continue;
            };
            l.icon_file = d
                .icon
                .as_ref()
                .map(|(n, f)| format!("{icons}/{n}x{n}/{f}"))
                .filter(|f| std::path::Path::new(f).is_file());
            l.details = Some(d);
            break;
        }
        for scope in [flatpak::Scope::User, flatpak::Scope::System] {
            if l.size.is_none() {
                l.size = flatpak::remote_info_argv(scope, &s.target, false)
                    .and_then(|a| capture(&a, 20))
                    .and_then(|t| flatpak::parse_installed_size(&t));
            }
            if l.perms.is_empty() {
                l.perms = flatpak::remote_info_argv(scope, &s.target, true)
                    .and_then(|a| capture(&a, 20))
                    .map(|t| flatpak::metadata_perms(&t))
                    .unwrap_or_default();
            }
        }
        let sys = [
            "flatpak",
            "list",
            "--app",
            "--system",
            "--columns=application",
        ]
        .map(String::from);
        l.system = capture(&sys, 5).is_some_and(|t| flatpak::parse_ids(&t).contains(&s.target));
    } else if let Some(pkg) = &s.pkg {
        for xml in appstream::NATIVE_XML {
            let argv = ["gzip", "-dc", xml].map(String::from);
            if let Some(d) = capture(&argv, 20).and_then(|x| appstream::by_pkgname(&x, pkg)) {
                l.details = Some(d);
                break;
            }
        }
        l.size = store::info_argv(pkg)
            .and_then(|a| capture(&a, 20))
            .and_then(|t| store::parse_installed_size(&t));
    }
    l
}

/// Information rows: (title, value).
pub fn info_rows(s: &Subject, l: &Loaded) -> Vec<(&'static str, String)> {
    let mut v = Vec::new();
    if let Some(d) = l.details.as_ref().and_then(|d| d.developer.clone()) {
        v.push(("Developer", d));
    }
    if let Some(z) = &l.size {
        v.push(("Size", z.clone()));
    }
    v.push((
        "Source",
        if s.flathub {
            "Flathub"
        } else {
            "WolOS packages"
        }
        .into(),
    ));
    if l.system {
        v.push(("Installed for", "All accounts".into()));
    }
    v
}

/// Get button label for a tile state: "Get", "Open", "42%".
pub fn button_label(s: w::TileState) -> String {
    match s {
        w::TileState::Get => "Get".into(),
        w::TileState::Installing => "Installing…".into(),
        w::TileState::Progress(p) => format!("{p}%"),
        w::TileState::Open => "Open".into(),
    }
}

/// A screenshot decoded at the size it is shown (the Picture's natural size is then right).
fn thumb(path: &str) -> Option<gtk::gdk::Texture> {
    let pb =
        gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(path, w::SCREENSHOT_W, w::SCREENSHOT_H, true)
            .ok()?;
    let png = pb.save_to_bufferv("png", &[]).ok()?;
    gtk::gdk::Texture::from_bytes(&glib::Bytes::from_owned(png)).ok()
}

fn screenshot(t: &gtk::gdk::Texture) -> gtk::Widget {
    let pic = gtk::Picture::for_paintable(t);
    pic.set_content_fit(gtk::ContentFit::Contain);
    pic.set_size_request(t.width(), t.height());
    pic.add_css_class("store-screenshot");
    pic.upcast()
}

/// Screenshots row: cached files show at once, others download off the UI thread first.
fn screenshots(urls: &[String]) -> Option<gtk::Widget> {
    if urls.is_empty() {
        return None;
    }
    let row = w::hbox(w::S3);
    let cache = wol_core::appstream::cache_home();
    for url in urls.iter().take(6) {
        if url.starts_with('/') {
            if let Some(t) = thumb(url) {
                row.append(&screenshot(&t));
            }
            continue;
        }
        let Some(path) = wol_core::appstream::shot_cache(&cache, url) else {
            continue;
        };
        let slot = w::hbox(0);
        slot.set_size_request(w::SCREENSHOT_W, w::SCREENSHOT_H);
        slot.add_css_class("store-screenshot");
        row.append(&slot);
        let (tx, rx) = async_channel::bounded(1);
        let url = url.clone();
        std::thread::spawn(move || {
            let p = std::path::Path::new(&path);
            if !p.is_file() {
                if let Some(d) = p.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                let tmp = format!("{path}.part");
                let ok = wol_core::system::run_with_timeout(
                    &["curl", "-fsSL", "--max-time", "30", "-o", &tmp, "--", &url],
                    std::time::Duration::from_secs(35),
                )
                .is_some();
                if !(ok && std::fs::rename(&tmp, &path).is_ok()) {
                    let _ = std::fs::remove_file(&tmp);
                    return;
                }
            }
            // Decoded and scaled here, not on the UI thread.
            if let Some(t) = thumb(&path) {
                let _ = tx.send_blocking(t);
            }
        });
        glib::spawn_future_local(async move {
            if let Ok(t) = rx.recv().await {
                slot.append(&screenshot(&t));
            }
        });
    }
    let scroll = gtk::ScrolledWindow::builder()
        .child(&row)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .propagate_natural_height(true)
        .min_content_height(w::SCREENSHOT_H)
        .build();
    Some(scroll.upcast())
}

/// Details page: icon, name, summary, Get/Open (and Remove, Install for All Accounts),
/// screenshots, description, information and, for a Flatpak, what it can do.
fn fill_details(
    sh: &Rc<Shared>,
    page: &gtk::Box,
    s: &Subject,
    l: Option<Loaded>,
    back: Rc<dyn Fn()>,
) {
    while let Some(c) = page.first_child() {
        page.remove(&c);
    }
    say(&sh.status, "");
    let head = w::hbox(w::S1);
    head.add_css_class("subpage-header");
    head.append(&w::icon_button("go-previous-symbolic", "Back", move || {
        back()
    }));
    page.append(&head);
    let Some(l) = l else {
        let b = w::hbox(w::S3);
        b.append(&w::progress_ring());
        b.append(&w::label("Loading…", "secondary"));
        page.append(&b);
        let (tx, rx) = async_channel::bounded(1);
        let s2 = s.clone();
        std::thread::spawn(move || {
            let _ = tx.send_blocking(load(&s2));
        });
        let (sh, page, s, head) = (sh.clone(), page.clone(), s.clone(), head.clone());
        glib::spawn_future_local(async move {
            if let Ok(l) = rx.recv().await {
                // Still showing this app: replace the spinner, keep the back button.
                if page.first_child().as_ref() == Some(head.upcast_ref()) {
                    while let Some(c) = head.next_sibling() {
                        page.remove(&c);
                    }
                    details_body(&sh, &page, &s, &l);
                }
            }
        });
        return;
    };
    details_body(sh, page, s, &l);
}

fn details_body(sh: &Rc<Shared>, page: &gtk::Box, s: &Subject, l: &Loaded) {
    let d = l.details.clone().unwrap_or_default();
    let top = w::hbox(w::S6);
    let icon = match &l.icon_file {
        Some(f) => {
            let img = gtk::Image::from_file(f);
            img.set_pixel_size(w::HERO_ICON);
            img.upcast()
        }
        None => w::store_icon(s.desktop.unwrap_or(&s.target), w::HERO_ICON),
    };
    top.append(&icon);
    let text = w::vbox(w::S1);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    let name = w::label(
        if d.name.is_empty() { &s.name } else { &d.name },
        "large-title",
    );
    name.set_wrap(true);
    text.append(&name);
    let summary = if d.summary.is_empty() {
        &s.summary
    } else {
        &d.summary
    };
    let sl = w::label(summary, "secondary");
    sl.set_wrap(true);
    text.append(&sl);
    if let Some(dev) = &d.developer {
        text.append(&w::label(dev, "secondary"));
    }
    let buttons = w::hbox(w::S2);
    buttons.set_margin_top(w::S2);
    let get = gtk::Button::with_label("Get");
    get.add_css_class("get-button");
    let all = w::button("Install for All Accounts", w::Kind::Default, || {});
    let remove = w::button("Remove", w::Kind::Default, || {});
    buttons.append(&get);
    buttons.append(&all);
    buttons.append(&remove);
    text.append(&buttons);
    top.append(&text);
    page.append(&top);

    let installed = Rc::new(std::cell::Cell::new(is_present(sh, &s.target, s.flathub)));
    let system = Rc::new(std::cell::Cell::new(l.system));
    let set: Rc<dyn Fn(w::TileState)> = {
        let (get, all, remove, flathub) = (get.clone(), all.clone(), remove.clone(), s.flathub);
        Rc::new(move |st| {
            get.set_label(&button_label(st));
            get.set_sensitive(!st.busy());
            remove.set_visible(st == w::TileState::Open);
            remove.set_sensitive(!st.busy());
            all.set_visible(flathub && st == w::TileState::Get);
        })
    };
    set(if installed.get() {
        w::TileState::Open
    } else {
        w::TileState::Get
    });
    {
        let (sh, s, set, inst) = (sh.clone(), s.clone(), set.clone(), installed.clone());
        get.connect_clicked(move |_| {
            if inst.get() {
                return launch(&s.target, s.flathub, s.desktop);
            }
            run_store_action(
                &sh,
                install_action(&s.target, s.flathub, false),
                progress_path(&s.target, s.flathub),
                set.clone(),
                inst.clone(),
                true,
                s.name.clone(),
            );
        });
    }
    {
        let (sh, s, set, inst, sys) = (
            sh.clone(),
            s.clone(),
            set.clone(),
            installed.clone(),
            system.clone(),
        );
        all.connect_clicked(move |_| {
            sys.set(true);
            run_store_action(
                &sh,
                install_action(&s.target, true, true),
                progress_path(&s.target, false),
                set.clone(),
                inst.clone(),
                true,
                s.name.clone(),
            );
        });
    }
    {
        let (sh, s, set, inst, sys) = (
            sh.clone(),
            s.clone(),
            set.clone(),
            installed.clone(),
            system.clone(),
        );
        remove.connect_clicked(move |_| {
            run_store_action(
                &sh,
                remove_action(&s.target, s.flathub, sys.get()),
                None,
                set.clone(),
                inst.clone(),
                false,
                s.name.clone(),
            );
        });
    }

    if let Some(shots) = screenshots(&d.screenshots) {
        page.append(&shots);
    }
    if !d.description.is_empty() {
        let b = w::vbox(w::S2);
        for p in &d.description {
            let l = w::label(p, "");
            l.set_wrap(true);
            l.set_xalign(0.0);
            b.append(&l);
        }
        page.append(&b);
    }
    let rows: Vec<gtk::Widget> = info_rows(s, l)
        .into_iter()
        .map(|(t, v)| {
            let value = w::label(&v, "secondary");
            w::list_row("", t, "", Some(value.upcast_ref())).upcast()
        })
        .collect();
    page.append(&w::section("Information", &rows));
    if s.flathub && !l.perms.is_empty() {
        let rows: Vec<gtk::Widget> = l
            .perms
            .iter()
            .map(|p| {
                let title = if p.is_display() {
                    "Display".into()
                } else {
                    p.plain()
                };
                let sub = if p.is_display() {
                    p.plain()
                } else {
                    String::new()
                };
                w::list_row(crate::ui::flatpak_consent::perm_icon(p), &title, &sub, None).upcast()
            })
            .collect();
        page.append(&w::section("What it can do", &rows));
    }
}

pub fn content(sys: Rc<dyn System>, env: Env) -> Store {
    let aw = w::AppWindow::new("App Store", true, false);
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
        open_details: RefCell::new(None),
    });
    let search = aw.sidebar_search("Search");
    aw.source_item("starred-symbolic", "Discover", &discover(&sh));
    for (c, icon) in [
        (Category::Create, "applications-graphics-symbolic"),
        (Category::Work, "applications-office-symbolic"),
        (Category::Play, "applications-games-symbolic"),
        (Category::Develop, "applications-engineering-symbolic"),
    ] {
        aw.source_item(icon, c.name(), &category(&sh, c));
    }
    aw.source_item(
        "software-update-available-symbolic",
        "Updates",
        &updates(&sh),
    );
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
        aw,
        search,
        show_results,
        shared: sh.clone(),
        show_package,
        show_details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn details_information_rows() {
        let s = Subject::flathub(&wol_core::flatpak::Hit {
            id: "org.gimp.GIMP".into(),
            name: "GIMP".into(),
            summary: "Edit images".into(),
        });
        let l = Loaded {
            details: Some(wol_core::appstream::Details {
                developer: Some("The GIMP team".into()),
                ..Default::default()
            }),
            size: Some("312.4 MB".into()),
            system: true,
            ..Default::default()
        };
        assert_eq!(
            info_rows(&s, &l),
            [
                ("Developer", "The GIMP team".to_string()),
                ("Size", "312.4 MB".into()),
                ("Source", "Flathub".into()),
                ("Installed for", "All accounts".into()),
            ]
        );
        let native = Subject::catalog(store::get("gimp").unwrap());
        assert_eq!(native.pkg.as_deref(), Some("gimp"));
        assert_eq!(
            info_rows(&native, &Loaded::default()),
            [("Source", "WolOS packages".to_string())]
        );
    }

    #[test]
    fn get_button_shows_the_percentage() {
        assert_eq!(button_label(w::TileState::Get), "Get");
        assert_eq!(button_label(w::TileState::Progress(42)), "42%");
        assert_eq!(button_label(w::TileState::Open), "Open");
        assert!(w::TileState::Progress(0).busy() && w::TileState::Installing.busy());
        assert!(!w::TileState::Open.busy());
    }

    #[test]
    fn store_actions_by_scope() {
        assert_eq!(
            install_action("org.a.B", true, false),
            Action::FlatpakInstall("org.a.B".into())
        );
        assert!(matches!(
            install_action("org.a.B", true, true),
            Action::Admin(r) if r.action == shared::admin::AdminAction::FlatpakInstall
        ));
        assert!(matches!(
            remove_action("org.a.B", true, true),
            Action::Admin(r) if r.action == shared::admin::AdminAction::FlatpakRemove
        ));
        assert_eq!(
            install_action("gimp", false, true),
            Action::PkgInstall("gimp".into())
        );
        assert_eq!(
            progress_path("gimp", false).as_deref(),
            Some("/run/wolos/progress/gimp")
        );
    }

    #[test]
    fn native_packages_win_over_flathub() {
        let hit = |id: &str, name: &str| wol_core::flatpak::Hit {
            id: id.into(),
            name: name.into(),
            summary: String::new(),
        };
        let repo = [store::PkgHit {
            name: "drawing".into(),
            summary: String::new(),
        }];
        assert!(covered_natively(
            &hit("com.github.maoschanz.drawing", "Drawing"),
            &repo
        ));
        assert!(!covered_natively(
            &hit("com.github.maoschanz.drawing", "Drawing"),
            &[]
        ));
        let catalog = &store::CATALOG[0];
        assert!(covered_natively(
            &hit("org.example.Whatever", catalog.name),
            &[]
        ));
        assert!(!covered_natively(
            &hit("com.example.NoSuchApp", "No Such App Here"),
            &[]
        ));
    }
}
