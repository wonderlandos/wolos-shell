//! App Store (`wol-store`): macOS App Store layout on the kit AppWindow. Curated catalog from
//! `wol_core::store`, `pkg search` for the rest, installs through wol-admind `pkg-install` with the
//! Authenticate sheet, updates from `wol update check`. Slow work never runs on the UI thread.
use super::settings::{Ctx, UpdateState, parse_check};
use crate::kit::{self, widgets as w};
use crate::system::{Action, ActionError, System};
use gtk::{glib, prelude::*};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use wol_core::store::{self, App, Category};

pub const PAGES: [&str; 6] = ["Discover", "Create", "Work", "Play", "Develop", "Updates"];

/// Machine facts the store reads (injected so goldens do not depend on the build host).
#[derive(Clone, Debug)]
pub struct Env {
    /// XDG data dirs searched for `applications/<id>.desktop`.
    pub data_dirs: Vec<PathBuf>,
    /// Run `pkg search` for non-catalog results.
    pub pkg_search: bool,
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
                .unwrap_or_else(|_| "/usr/local/share:/usr/share".into())
                .split(':')
                .map(PathBuf::from),
        );
        Env {
            data_dirs: dirs,
            pkg_search: true,
        }
    }
}

pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("org.wolos.store")
        .build();
    app.connect_activate(|app| {
        kit::style::install("");
        let win = gtk::ApplicationWindow::builder()
            .application(app)
            .title("App Store")
            .default_width(w::APP_SIZE.0)
            .default_height(w::APP_SIZE.1)
            .decorated(false)
            .build();
        let s = content(crate::system::default_system(), Env::read());
        if let Some(p) = std::env::args().nth(1) {
            s.aw.select(&p);
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
}

impl Store {
    /// Shows results for `term` now (no typing delay): goldens and `wol-store search:<term>`.
    pub fn search_now(&self, term: &str) {
        self.search.set_text(term);
        (self.show_results)(term);
    }
}

struct Shared {
    ctx: Rc<Ctx>,
    env: Env,
    /// One inline error line per window (HIG §3).
    status: gtk::Label,
}

fn say(l: &gtk::Label, text: &str) {
    l.set_label(text);
    l.set_visible(!text.is_empty());
}

fn icon(a: &App, size: i32) -> gtk::Widget {
    w::store_icon(a.id, size)
}

/// Get -> install (Authenticate sheet when needed) -> Open; Open launches the .desktop file.
fn wire(
    sh: &Rc<Shared>,
    target: String,
    name: String,
    desktop: Option<&'static str>,
    set: Rc<dyn Fn(w::TileState)>,
    button: &gtk::Button,
) {
    let installed = Rc::new(std::cell::Cell::new(false));
    if let Some(a) = store::get(&target)
        && store::is_installed(a, &sh.env.data_dirs)
    {
        installed.set(true);
        set(w::TileState::Open);
    }
    let sh = sh.clone();
    button.connect_clicked(move |_| {
        if installed.get() {
            if let Some(d) = desktop {
                kit::spawn(&["gtk4-launch".into(), d.into()]);
            }
            return;
        }
        set(w::TileState::Installing);
        say(&sh.status, "");
        let (set, st, inst, name) = (
            set.clone(),
            sh.status.clone(),
            installed.clone(),
            name.clone(),
        );
        sh.ctx.act(
            Action::PkgInstall(target.clone()),
            Rc::new(move |r: Result<(), ActionError>| match r {
                Ok(()) => {
                    inst.set(true);
                    set(w::TileState::Open);
                }
                Err(e) => {
                    set(w::TileState::Get);
                    say(&st, &crate::copy::install(e, &name));
                }
            }),
        );
    });
}

fn tile(sh: &Rc<Shared>, a: &'static App) -> gtk::Widget {
    let t = w::AppTile::new(&icon(a, w::ICON_L), a.name, a.summary);
    let t2 = t.clone();
    wire(
        sh,
        a.id.into(),
        a.name.into(),
        Some(a.desktop),
        Rc::new(move |s| t2.set_state(s)),
        &t.action,
    );
    t.root.upcast()
}

fn pkg_tile(sh: &Rc<Shared>, h: &store::PkgHit) -> gtk::Widget {
    let t = w::AppTile::new(&w::store_icon(&h.name, w::ICON_L), &h.name, &h.summary);
    let t2 = t.clone();
    wire(
        sh,
        h.name.clone(),
        h.name.clone(),
        None,
        Rc::new(move |s| t2.set_state(s)),
        &t.action,
    );
    t.root.upcast()
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
        Some(a.desktop),
        Rc::new(move |s| {
            g2.set_sensitive(s != w::TileState::Installing);
            g2.set_label(match s {
                w::TileState::Get => "Get",
                w::TileState::Installing => "Installing…",
                w::TileState::Open => "Open",
            });
        }),
        &get,
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
                let _ = tx.send_blocking(out);
            });
            let show = show.clone();
            glib::spawn_future_local(async move {
                if let Ok(out) = rx.recv().await {
                    show(parse_check(out.as_deref()));
                }
            });
        });
    }
    p
}

/// Search results: catalog hits now, `pkg search` hits when they arrive.
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
    if !sh.env.pkg_search {
        return;
    }
    let (tx, rx) = async_channel::bounded(1);
    let t = term.to_string();
    std::thread::spawn(move || {
        let out = wol_core::system::run_with_timeout(
            &["pkg", "search", "--", &t],
            std::time::Duration::from_secs(15),
        );
        let _ = tx.send_blocking(out.map(|o| store::parse_pkg_search(&o)).unwrap_or_default());
    });
    let sh = sh.clone();
    glib::spawn_future_local(async move {
        let Ok(found) = rx.recv().await else { return };
        if found.is_empty() || more.parent().is_none() {
            return;
        }
        empty.set_visible(false);
        more.append(&w::label("More packages", "title"));
        more.append(&grid(found.iter().take(30).map(|h| pkg_tile(&sh, h))));
    });
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
        status: status.clone(),
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
    }
}
