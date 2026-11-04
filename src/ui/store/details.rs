// SPDX-License-Identifier: GPL-2.0-or-later
//! The App Store's details page: what a tile opens (`Subject`), what is loaded about it off the
//! GTK thread (`Loaded`), and the sheet that shows it with its screenshots, versions and actions.
use super::{
    Shared, aur_details, install_action, is_present, launch, progress_path, remove_action,
    run_store_action, say,
};
use crate::kit::widgets as w;
use gtk::{glib, prelude::*};
use std::cell::RefCell;
use std::rc::Rc;
use wol_core::store::{self, App};

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
    /// Only the community package collection has it: built here, reviewed by nobody.
    pub aur: bool,
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
            aur: false,
        }
    }
    pub fn aur(h: &store::PkgHit) -> Self {
        Subject {
            target: h.name.clone(),
            name: h.name.clone(),
            summary: h.summary.clone(),
            flathub: false,
            desktop: None,
            pkg: None,
            aur: true,
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
            aur: false,
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
            aur: false,
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
    /// Every place this app comes from, best first.
    pub sources: Vec<crate::packages::AppSource>,
    /// The version on this computer.
    pub installed: Option<String>,
    pub pinned: bool,
    /// The history entry that undoes its last change.
    pub undo: Option<String>,
}

pub(super) fn capture(argv: &[String], secs: u64) -> Option<String> {
    let a: Vec<&str> = argv.iter().map(String::as_str).collect();
    wol_core::system::run_with_timeout(&a, std::time::Duration::from_secs(secs))
}

/// Blocking: AppStream, size, permissions and install scope for `s`.
pub fn load(s: &Subject) -> Loaded {
    use wol_core::{appstream, flatpak};
    let mut l = Loaded::default();
    if !s.aur {
        l.sources = crate::packages::sources(s.pkg.as_deref().unwrap_or(&s.target));
        if let Some(i) = l.sources.iter().find(|x| x.installed) {
            l.installed = crate::packages::installed_version(&i.id, i.kind == "flatpak");
            l.pinned = crate::packages::pinned(&i.id);
            l.undo = crate::packages::last_change(&i.id);
        }
    }
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
        if s.aur {
            "Community"
        } else if s.flathub {
            "Flathub"
        } else {
            "System packages"
        }
        .into(),
    ));
    if l.system {
        v.push(("Installed for", "All accounts".into()));
    }
    v
}

/// Get button label for a tile state: "Get", "Open", "42%".
/// The button's text on a details page: an installed package with nothing to open says
/// "Installed" (QA-STORE-30), not an "Open" that does nothing.
pub fn open_label(s: w::TileState, can_open: bool) -> String {
    match s {
        w::TileState::Open if !can_open => "Installed".into(),
        other => button_label(other),
    }
}

pub fn button_label(s: w::TileState) -> String {
    match s {
        w::TileState::Get => "Get".into(),
        w::TileState::Installing => "Installing…".into(),
        w::TileState::Progress(p) => format!("{p}%"),
        w::TileState::Open => "Open".into(),
    }
}

/// A screenshot decoded at the size it is shown (the Picture's natural size is then right).
pub(super) fn thumb(path: &str) -> Option<gtk::gdk::Texture> {
    let pb =
        gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(path, w::SCREENSHOT_W, w::SCREENSHOT_H, true)
            .ok()?;
    let png = pb.save_to_bufferv("png", &[]).ok()?;
    gtk::gdk::Texture::from_bytes(&glib::Bytes::from_owned(png)).ok()
}

pub(super) fn screenshot(t: &gtk::gdk::Texture) -> gtk::Widget {
    let pic = gtk::Picture::for_paintable(t);
    pic.set_content_fit(gtk::ContentFit::Contain);
    pic.set_size_request(t.width(), t.height());
    pic.add_css_class("store-screenshot");
    pic.upcast()
}

/// Screenshots row: cached files show at once, others download off the UI thread first.
pub(super) fn screenshots(urls: &[String]) -> Option<gtk::Widget> {
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
pub(super) fn fill_details(
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

/// Run a wopkg verb off the main thread and say how it went.
pub(super) fn wopkg_action(sh: &Rc<Shared>, args: Vec<String>, done: String) {
    let status = sh.status.clone();
    let (tx, rx) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        let _ = tx.send_blocking(crate::packages::wopkg_verb(&a));
    });
    glib::spawn_future_local(async move {
        if let Ok(ok) = rx.recv().await {
            say(
                &status,
                if ok {
                    &done
                } else {
                    "That did not finish. Nothing was changed."
                },
            );
        }
    });
}

/// Where an app can come from and what is on this computer: the source picker, then update, pin
/// and undo for what is installed. Nothing shows without an index.
pub(super) fn versions(
    sh: &Rc<Shared>,
    page: &gtk::Box,
    s: &Subject,
    l: &Loaded,
    chosen: &Rc<RefCell<(String, bool)>>,
) {
    if l.sources.is_empty() {
        return;
    }
    let mut rows: Vec<gtk::Widget> = l
        .sources
        .iter()
        .map(|x| {
            let icon = if x.installed {
                "object-select-symbolic"
            } else {
                "package-x-generic-symbolic"
            };
            w::list_row(icon, &x.origin, &x.version, None).upcast()
        })
        .collect();
    if l.sources.len() > 1 && l.installed.is_none() {
        let labels: Vec<String> = l
            .sources
            .iter()
            .map(|x| format!("{} {}", x.origin, x.version))
            .collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let pick = w::dropdown(&refs, 0);
        let (src, chosen) = (l.sources.clone(), chosen.clone());
        *chosen.borrow_mut() = (src[0].id.clone(), src[0].kind == "flatpak");
        pick.connect_selected_notify(move |d| {
            if let Some(x) = src.get(d.selected() as usize) {
                *chosen.borrow_mut() = (x.id.clone(), x.kind == "flatpak");
            }
        });
        rows.push(w::list_row("", "Get From", "", Some(pick.upcast_ref())).upcast());
    }
    if let (Some(now), Some(i)) = (&l.installed, l.sources.iter().find(|x| x.installed)) {
        rows.insert(0, w::list_row("", "On This Computer", now, None).upcast());
        let id = i.id.clone();
        let (sh2, id2) = (sh.clone(), id.clone());
        let keep = w::toggle(l.pinned, move |on| {
            let verb = if on { "pin" } else { "unpin" };
            let done = if on {
                "It stays at this version."
            } else {
                "It updates again."
            };
            wopkg_action(&sh2, vec![verb.into(), id2.clone()], done.into());
        });
        rows.push(w::list_row("", "Keep This Version", "", Some(keep.upcast_ref())).upcast());
        let buttons = w::hbox(w::S2);
        if i.version != *now {
            let (sh2, id2) = (sh.clone(), id.clone());
            buttons.append(&w::button("Update", w::Kind::Default, move || {
                wopkg_action(&sh2, vec!["update".into(), id2.clone()], "Updated.".into());
            }));
        }
        if let Some(entry) = l.undo.clone() {
            let sh2 = sh.clone();
            buttons.append(&w::button("Roll Back", w::Kind::Default, move || {
                wopkg_action(
                    &sh2,
                    vec!["rollback".into(), entry.clone()],
                    "Rolled back.".into(),
                );
            }));
        }
        page.append(&w::section("Versions", &rows));
        if buttons.first_child().is_some() {
            page.append(&buttons);
        }
        return;
    }
    let _ = s;
    page.append(&w::section("Versions", &rows));
}

pub(super) fn details_body(sh: &Rc<Shared>, page: &gtk::Box, s: &Subject, l: &Loaded) {
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

    if s.aur {
        buttons.set_visible(false);
        aur_details(page, s);
    }

    let chosen = Rc::new(RefCell::new((s.target.clone(), s.flathub)));
    versions(sh, page, s, l, &chosen);
    let start = super::start_state(is_present(sh, &s.target, s.flathub), l.installed.as_deref());
    let installed = Rc::new(std::cell::Cell::new(start == w::TileState::Open));
    let system = Rc::new(std::cell::Cell::new(l.system));
    let openable = super::can_open(s.flathub, s.desktop);
    let set: Rc<dyn Fn(w::TileState)> = {
        let (get, all, remove, flathub) = (get.clone(), all.clone(), remove.clone(), s.flathub);
        Rc::new(move |st| {
            get.set_label(&open_label(st, openable));
            get.set_sensitive(!st.busy() && (st != w::TileState::Open || openable));
            remove.set_visible(st == w::TileState::Open);
            remove.set_sensitive(!st.busy());
            all.set_visible(flathub && st == w::TileState::Get);
        })
    };
    set(start);
    {
        let (sh, s, set, inst, chosen) = (
            sh.clone(),
            s.clone(),
            set.clone(),
            installed.clone(),
            chosen.clone(),
        );
        get.connect_clicked(move |_| {
            if inst.get() {
                return launch(&s.target, s.flathub, s.desktop);
            }
            let (target, flathub) = chosen.borrow().clone();
            run_store_action(
                &sh,
                install_action(&target, flathub, false),
                progress_path(&target, flathub),
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
        page.append(&w::section("What It Can Do", &rows));
    }
}
