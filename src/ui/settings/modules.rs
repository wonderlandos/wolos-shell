// SPDX-License-Identifier: GPL-2.0-or-later
//! Modules (ADR 0032, docs/modules/modules.md): the optional AI, developer, creative and console
//! stacks. The list is `wol modules list --json` (`Machine::modules`, off the UI thread); Install
//! and Remove are the meta package through wol-admind (`Action::PkgInstall` / `PkgRemove`, the
//! Authenticate sheet asks first); Install also takes the record's backend package for this
//! machine (`Status::backend`, as `wol modules install` does); Finish Setup is the user-side step, `wol modules enable <id>`
//! (a Change verb, capability `apps`), run off the UI thread. The page reloads after each.
//! "Could Not Be Added" lists what modules-install gave up on ([`GAVE_UP_FILE`]): a module offers
//! Retry, the same install as Install; a media format package its own `PkgInstall`.
use super::*;
use crate::i18n::{self, keys};
use wol_core::modules::Status;

/// What a module's row says and offers, from its state (`Status::state`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Offer {
    /// Installed and set up (or nothing to set up): "Ready", Remove in the menu.
    Ready,
    /// Installed, the user step not run yet: Finish Setup.
    FinishSetup,
    /// Queued by the installer for the next start online.
    Pending,
    /// Not installed: Install.
    Install,
}

/// The offer for one module.
pub fn offer(s: &Status) -> Offer {
    match (s.installed, s.enabled || !s.needs_enable, s.pending) {
        (true, true, _) => Offer::Ready,
        (true, false, _) => Offer::FinishSetup,
        (false, _, true) => Offer::Pending,
        (false, _, false) => Offer::Install,
    }
}

/// modules-install's list of what it gave up on after its tries: "m ID" / "p NAME" lines; a name
/// it installs later leaves it. Root's file, only read here.
pub const GAVE_UP_FILE: &str = wol_core::modules::GAVE_UP_FILE;
/// The most of that file read (bytes); a longer one is not read at all.
pub const GAVE_UP_MAX: u64 = 64 * 1024;
/// The most entries shown (each media format is one `pacman -Q` when read).
const GAVE_UP_ITEMS: usize = 32;

/// One entry of [`GAVE_UP_FILE`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GaveUp {
    /// A module id ("m"), else a media format package ("p").
    pub module: bool,
    pub name: String,
}

/// The entries of [`GAVE_UP_FILE`]'s text: lines of two words, a valid module id or package name;
/// anything else dropped, repeats once, at most [`GAVE_UP_ITEMS`].
pub fn parse_gave_up(text: &str) -> Vec<GaveUp> {
    let mut v: Vec<GaveUp> = Vec::new();
    for l in text.lines() {
        let mut w = l.split_whitespace();
        let (Some(kind), Some(name), None) = (w.next(), w.next(), w.next()) else {
            continue;
        };
        let module = match kind {
            "m" if wol_core::modules::valid_id(name) => true,
            "p" if wol_core::store::is_pkg_name(name) => false,
            _ => continue,
        };
        let g = GaveUp {
            module,
            name: name.into(),
        };
        if !v.contains(&g) {
            v.push(g);
        }
        if v.len() == GAVE_UP_ITEMS {
            break;
        }
    }
    v
}

/// The modules still given up on: listed in `gave_up`, in the registry and not installed (Retry or
/// Install worked since).
fn modules_given_up<'a>(list: &'a [Status], gave_up: &[GaveUp]) -> Vec<&'a Status> {
    list.iter()
        .filter(|s| !s.installed && gave_up.iter().any(|g| g.module && g.name == s.id))
        .collect()
}

/// "900 MB", "2.4 GB"; empty when the record has no size.
pub fn size_label(mb: u32) -> String {
    match mb {
        0 => String::new(),
        m if m < 1000 => format!("{m} MB"),
        m => {
            let gb = f64::from(m) / 1000.0;
            let s = format!("{gb:.1}");
            format!("{} GB", s.strip_suffix(".0").unwrap_or(&s))
        }
    }
}

/// The row's subtitle: the summary and the download size.
fn subtitle(s: &Status) -> String {
    match (s.summary.is_empty(), size_label(s.size_mb)) {
        (_, size) if size.is_empty() => s.summary.clone(),
        (true, size) => size,
        (false, size) => format!("{} · {size}", s.summary),
    }
}

/// `wol modules enable <id>` off the UI thread (minutes: recipes and version managers); true
/// when it finished. The id is checked first: it goes into an argv, never a shell.
fn enable(id: &str) -> bool {
    if !wol_core::modules::valid_id(id) {
        return false;
    }
    // 30 min: a toolchain download on a slow line.
    let limit = std::time::Duration::from_secs(1800);
    wol_core::system::run_with_timeout(&["wol", "modules", "enable", id], limit).is_some()
}

/// Ids whose Install or Finish Setup is running from this page: one run per module, also across
/// the reloads another row's action makes.
type Busy = Rc<RefCell<Vec<String>>>;

/// A row's button that runs one job at a time: while `id` runs it is off with a spinner beside
/// it; `run` gets what to call when the job ended (the page reloads after it).
fn job_button(
    label: &str,
    kind: w::Kind,
    id: &str,
    busy: &Busy,
    run: impl Fn(Box<dyn FnOnce()>) + 'static,
) -> gtk::Widget {
    let line = w::hbox(w::S2);
    let spin = w::spinner();
    let b = w::button(label, kind, || {});
    let running = |on: bool| {
        spin.set_visible(on);
        spin.set_spinning(on);
        b.set_sensitive(!on);
    };
    running(busy.borrow().iter().any(|x| x == id));
    let (id, busy, spin2) = (id.to_string(), busy.clone(), spin.clone());
    b.connect_clicked(move |b| {
        if busy.borrow().contains(&id) {
            return;
        }
        busy.borrow_mut().push(id.clone());
        spin2.set_visible(true);
        spin2.set_spinning(true);
        b.set_sensitive(false);
        let (busy, id) = (busy.clone(), id.clone());
        run(Box::new(move || busy.borrow_mut().retain(|x| *x != id)));
    });
    line.append(&spin);
    line.append(&b);
    line.upcast()
}

/// After `wol modules enable`: what the status line says.
fn enable_said(ok: bool) -> &'static str {
    if ok {
        ""
    } else {
        i18n::t(keys::SETTINGS_MODULES_ENABLE_NOT_DONE)
    }
}

fn module_row(
    e: &Rc<Env>,
    s: &Status,
    reload: &Rc<dyn Fn()>,
    st: &gtk::Label,
    busy: &Busy,
) -> gtk::Widget {
    let sub = subtitle(s);
    let control: gtk::Widget = match offer(s) {
        Offer::Ready => {
            let line = w::hbox(w::S2);
            line.append(&w::label("Ready", "secondary"));
            let (ctx, st2, pkg, r) = (e.ctx.clone(), st.clone(), s.package.clone(), reload.clone());
            let remove: w::MenuItem = (
                "Remove",
                Box::new(move || {
                    let (st3, r) = (st2.clone(), r.clone());
                    ctx.act(
                        Action::PkgRemove(pkg.clone()),
                        Rc::new(move |res| {
                            say(&st3, res.as_ref().err().map_or("", |e| e.message()));
                            r();
                        }),
                    );
                }),
            );
            if let Some(more) = form::actions_button(vec![remove]) {
                line.append(&more);
            }
            line.upcast()
        }
        Offer::FinishSetup => {
            let (id, st2, r) = (s.id.clone(), st.clone(), reload.clone());
            job_button(
                "Finish Setup",
                w::Kind::Primary,
                &s.id,
                busy,
                move |ended| {
                    let (id, st3, r) = (id.clone(), st2.clone(), r.clone());
                    off_thread(
                        move || enable(&id),
                        move |ok| {
                            say(&st3, enable_said(ok));
                            ended();
                            r();
                        },
                    );
                },
            )
        }
        // Queued for the next connection: Install Now does not wait for it (the same path as
        // Install; audit6 ST6-2: the person is never left with a row that only says "Waiting").
        Offer::Pending => install_button(e, s, "Install Now", reload, st, busy),
        Offer::Install => install_button(e, s, "Install", reload, st, busy),
    };
    let sub = if offer(s) == Offer::Pending {
        i18n::t(keys::SETTINGS_MODULES_PENDING).to_string()
    } else {
        sub
    };
    w::list_row("view-app-grid-symbolic", &s.name, &sub, Some(&control)).upcast()
}

/// The packages a module's Install asks wol-admind for, in order: the record's backend package
/// for this machine first, then the meta package. In that order a failed backend leaves the
/// module not installed (Install or Retry stays on offer, and wol-admind keeps its gave-up line),
/// where the other order would leave a module that looks ready without it.
fn install_packages(s: &Status) -> Vec<String> {
    s.backend
        .iter()
        .chain(std::iter::once(&s.package))
        .cloned()
        .collect()
}

/// `PkgInstall` of each of `pkgs` in turn through wol-admind, stopping at the first that fails;
/// the error text goes to the status line and `done` gets whether all installed.
fn install_all(ctx: &Rc<Ctx>, mut pkgs: Vec<String>, st: &gtk::Label, done: Rc<dyn Fn(bool)>) {
    if pkgs.is_empty() {
        done(true);
        return;
    }
    let first = pkgs.remove(0);
    let (ctx2, st2) = (ctx.clone(), st.clone());
    ctx.act(
        Action::PkgInstall(first),
        Rc::new(move |res| {
            say(&st2, res.as_ref().err().map_or("", |e| e.message()));
            if res.is_ok() {
                install_all(&ctx2, pkgs.clone(), &st2, done.clone());
            } else {
                done(false);
            }
        }),
    );
}

/// A module's Install (or Retry, `label`): the backend package and the meta package through
/// wol-admind, then the user step when the module has one; one run per module (`busy`), the page
/// reloads after it.
fn install_button(
    e: &Rc<Env>,
    s: &Status,
    label: &str,
    reload: &Rc<dyn Fn()>,
    st: &gtk::Label,
    busy: &Busy,
) -> gtk::Widget {
    let (ctx, st2, pkgs, id, needs, r) = (
        e.ctx.clone(),
        st.clone(),
        install_packages(s),
        s.id.clone(),
        s.needs_enable,
        reload.clone(),
    );
    job_button(label, w::Kind::Default, &s.id, busy, move |ended| {
        let (st3, id, r) = (st2.clone(), id.clone(), r.clone());
        let ended = RefCell::new(Some(ended));
        let st4 = st3.clone();
        install_all(
            &ctx,
            pkgs.clone(),
            &st3,
            Rc::new(move |ok| {
                let Some(ended) = ended.borrow_mut().take() else {
                    return;
                };
                if ok && needs {
                    // The user step follows the packages, as `wol modules install` says.
                    let (st5, r2, id) = (st4.clone(), r.clone(), id.clone());
                    off_thread(
                        move || enable(&id),
                        move |ok| {
                            say(&st5, enable_said(ok));
                            ended();
                            r2();
                        },
                    );
                } else {
                    ended();
                    r();
                }
            }),
        );
    })
}

/// A media format package modules-install gave up on, with Retry: `PkgInstall` through
/// wol-admind (as the Store installs), one run per name; the page reloads after it.
fn media_row(
    e: &Rc<Env>,
    name: &str,
    reload: &Rc<dyn Fn()>,
    st: &gtk::Label,
    busy: &Busy,
) -> gtk::Widget {
    let (ctx, st2, pkg, r) = (e.ctx.clone(), st.clone(), name.to_string(), reload.clone());
    // "media:" keeps a package's run apart from a module id's (neither holds a colon).
    let key = format!("media:{name}");
    let retry = job_button("Retry", w::Kind::Default, &key, busy, move |ended| {
        let (st3, r) = (st2.clone(), r.clone());
        let ended = RefCell::new(Some(ended));
        ctx.act(
            Action::PkgInstall(pkg.clone()),
            Rc::new(move |res| {
                say(&st3, res.as_ref().err().map_or("", |e| e.message()));
                if let Some(ended) = ended.borrow_mut().take() {
                    ended();
                    r();
                }
            }),
        );
    });
    w::list_row(
        "view-app-grid-symbolic",
        &media_title(name),
        i18n::t(keys::SETTINGS_MODULES_GAVE_UP_MEDIA),
        Some(&retry),
    )
    .upcast()
}

/// A media format package's row title: the formats it brings, as the Kits list names them
/// ("AAC, H.264"); the package name only when no kit lists it.
fn media_title(pkg: &str) -> String {
    let parts: Vec<&str> = wol_core::store::KITS
        .iter()
        .flat_map(|k| k.parts)
        .filter(|p| p.packages.contains(&pkg))
        .map(|p| p.name)
        .collect();
    if parts.is_empty() {
        pkg.to_string()
    } else {
        parts.join(", ")
    }
}

/// What the pane says with no rows: a list that could not be read is not an empty list.
fn empty_copy(list: &Option<Vec<Status>>) -> &'static str {
    if list.is_some() {
        i18n::t(keys::SETTINGS_MODULES_NONE)
    } else {
        i18n::t(keys::SETTINGS_MODULES_UNREADABLE)
    }
}

fn render(
    e: &Rc<Env>,
    into: &gtk::Box,
    list: Option<Vec<Status>>,
    gave_up: &[GaveUp],
    reload: &Rc<dyn Fn()>,
    st: &gtk::Label,
    busy: &Busy,
) {
    let none = empty_copy(&list);
    let Some(list) = list.filter(|l| !l.is_empty()) else {
        into.append(&w::section(
            "",
            &[w::list_row("view-app-grid-symbolic", none, "", None).upcast()],
        ));
        return;
    };
    let failed = modules_given_up(&list, gave_up);
    let (on, off): (Vec<&Status>, Vec<&Status>) = list
        .iter()
        .filter(|s| !failed.iter().any(|f| f.id == s.id))
        .partition(|s| matches!(offer(s), Offer::Ready | Offer::FinishSetup));
    if !on.is_empty() {
        let rows: Vec<gtk::Widget> = on
            .iter()
            .map(|s| module_row(e, s, reload, st, busy))
            .collect();
        into.append(&w::section("Installed", &rows));
    }
    if !off.is_empty() {
        let rows: Vec<gtk::Widget> = off
            .iter()
            .map(|s| module_row(e, s, reload, st, busy))
            .collect();
        into.append(&w::section("Available", &rows));
    }
    let mut rows: Vec<gtk::Widget> = failed
        .iter()
        .map(|s| {
            let retry = install_button(e, s, "Retry", reload, st, busy);
            w::list_row(
                "view-app-grid-symbolic",
                &s.name,
                i18n::t(keys::SETTINGS_MODULES_GAVE_UP),
                Some(&retry),
            )
            .upcast()
        })
        .collect();
    rows.extend(
        gave_up
            .iter()
            .filter(|g| !g.module)
            .map(|g| media_row(e, &g.name, reload, st, busy)),
    );
    if !rows.is_empty() {
        into.append(&w::section("Could Not Be Added", &rows));
    }
}

/// The page's reload, set once it exists (rows call it after their action).
type Slot = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let p = w::page("Modules");
    let body = w::vbox(w::S6);
    p.append(&body);
    p.append(&w::footnote(i18n::t(keys::SETTINGS_MODULES_ABOUT)));
    let status = status_label();
    p.append(&status);
    let reload: Slot = Rc::default();
    let busy: Busy = Rc::default();
    let load: Rc<dyn Fn()> = {
        let (e, body, st, slot) = (
            Rc::downgrade(e),
            body.downgrade(),
            status.clone(),
            reload.clone(),
        );
        Rc::new(move || {
            let (Some(e), Some(body)) = (e.upgrade(), body.upgrade()) else {
                return;
            };
            clear(&body);
            let m = e.machine();
            let (e2, st, slot, busy) = (e.clone(), st.clone(), slot.clone(), busy.clone());
            later(
                &body,
                move || (m.modules(), m.modules_gave_up()),
                move |into, (list, gave_up)| {
                    let none: Rc<dyn Fn()> = Rc::new(|| {});
                    let again = slot.borrow().clone().unwrap_or(none);
                    render(&e2, into, list, &gave_up, &again, &st, &busy)
                },
            );
        })
    };
    *reload.borrow_mut() = Some(load.clone());
    load();
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::copy::settings as copy;

    fn st(installed: bool, enabled: bool, pending: bool, needs_enable: bool) -> Status {
        Status {
            installed,
            enabled,
            pending,
            needs_enable,
            ..Status::default()
        }
    }

    #[test]
    fn each_state_offers_its_next_step() {
        assert_eq!(offer(&st(true, true, false, true)), Offer::Ready);
        assert_eq!(
            offer(&st(true, false, false, false)),
            Offer::Ready,
            "nothing to set up"
        );
        assert_eq!(offer(&st(true, false, false, true)), Offer::FinishSetup);
        assert_eq!(offer(&st(false, false, true, true)), Offer::Pending);
        assert_eq!(offer(&st(false, false, false, true)), Offer::Install);
    }

    #[test]
    fn install_takes_the_backend_package_before_the_meta_package() {
        let m = |backend: Option<&str>| Status {
            package: "wolos-module-ai-local".into(),
            backend: backend.map(String::from),
            ..Status::default()
        };
        assert_eq!(
            install_packages(&m(Some("ollama-cuda"))),
            ["ollama-cuda", "wolos-module-ai-local"]
        );
        assert_eq!(
            install_packages(&m(None)),
            ["wolos-module-ai-local"],
            "no backend (cpu, or a record without one): the meta package alone"
        );
    }

    #[test]
    fn an_unreadable_list_is_not_an_empty_one() {
        assert_eq!(empty_copy(&None), copy::MODULES_UNREADABLE);
        assert_eq!(empty_copy(&Some(Vec::new())), copy::MODULES_NONE);
    }

    #[test]
    fn sizes_read_in_megabytes_or_gigabytes() {
        assert_eq!(size_label(0), "");
        assert_eq!(size_label(900), "900 MB");
        assert_eq!(size_label(2400), "2.4 GB");
        assert_eq!(size_label(3000), "3 GB");
    }

    #[test]
    fn a_media_format_reads_as_the_formats_it_brings() {
        assert_eq!(media_title("lame"), "MP3");
        assert_eq!(
            media_title("gst-libav"),
            "AAC, H.264, HEVC (H.265), AC3 (Dolby Digital)"
        );
        assert_eq!(media_title("not-in-a-kit"), "not-in-a-kit");
    }

    #[test]
    fn the_gave_up_list_keeps_only_well_formed_names() {
        let g = parse_gave_up(
            "m creative\np gst-libav\nm creative\nm Bad_Id\np -S\nx dev\nm dev extra\n\np lame",
        );
        let names: Vec<(bool, &str)> = g.iter().map(|g| (g.module, g.name.as_str())).collect();
        assert_eq!(
            names,
            [(true, "creative"), (false, "gst-libav"), (false, "lame")]
        );
        let many: String = (0..100).map(|i| format!("p pkg{i}\n")).collect();
        assert_eq!(parse_gave_up(&many).len(), GAVE_UP_ITEMS);
    }

    #[test]
    fn a_module_given_up_on_shows_until_it_is_installed() {
        let mut list = vec![
            Status {
                id: "creative".into(),
                ..Status::default()
            },
            Status {
                id: "dev".into(),
                installed: true,
                ..Status::default()
            },
        ];
        let g = parse_gave_up("m creative\nm dev\nm gone\np creative\n");
        let ids = |l: &[Status]| -> Vec<String> {
            modules_given_up(l, &g)
                .iter()
                .map(|s| s.id.clone())
                .collect()
        };
        assert_eq!(ids(&list), ["creative"], "installed or unknown ids are not");
        list[0].installed = true;
        assert!(ids(&list).is_empty(), "Retry worked: the row goes");
    }

    #[test]
    fn enable_refuses_an_id_that_is_not_a_module_name() {
        assert!(!enable("../etc"));
        assert!(!enable("dev; rm"));
    }
}
