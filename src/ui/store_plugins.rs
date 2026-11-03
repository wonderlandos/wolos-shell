// SPDX-License-Identifier: GPL-2.0-or-later
//! App Store > Plugins (ADR 0031 W4, docs/modules/plugins.md "Distribution"): the signed plugin
//! index through wopkg's `plugins` source. Browse, Get (download, check, write into this
//! account's plugin folder), Turn On (`wol plugin enable`, which grants what the row lists) and
//! Update. Plugins follow the plugin index, on their own schedule: an OS release neither brings
//! nor holds them, and nothing here needs the administrator. Network and disk work run off the
//! UI thread; [`rows`] and [`button_text`] are the pure part.
use super::{Shared, note_row, say};
use crate::kit::widgets as w;
use gtk::{glib, prelude::*};
use std::path::PathBuf;
use std::rc::Rc;
use wopkg::index::plugins::{Entry, PluginIndex};
use wopkg::release::{PluginReason, plugin_updates};

/// What the row's button does now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Not on this account.
    Get,
    /// Installed; the index has this newer version.
    Update(String),
    /// A newer version needs this WolOS version or later.
    NeedsNewer(String),
    /// Installed and off.
    TurnOn,
    /// Installed and on.
    On,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub entry: Entry,
    pub state: State,
}

/// The page's content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Listing {
    Loading,
    /// One sentence: why there is no list.
    Unavailable(String),
    Ready(Vec<Row>),
}

/// Rows for the index, installed ones (updates first) before the rest, each group in index order.
/// `installed`: (id, version) of this account's plugins; `enabled`: ids that are on; `os`: the
/// running WolOS version.
pub fn rows(
    index: &PluginIndex,
    installed: &[(String, String)],
    enabled: &[String],
    os: &str,
) -> Vec<Row> {
    let updates = plugin_updates(os, installed, index);
    let mut v: Vec<Row> = index
        .plugins
        .iter()
        .map(|e| {
            let state = match (
                installed.iter().any(|(id, _)| *id == e.id),
                updates.iter().find(|u| u.id == e.id),
            ) {
                (false, _) => State::Get,
                (true, Some(u)) if u.reason == PluginReason::Available => {
                    State::Update(u.to.clone())
                }
                (true, Some(_)) => State::NeedsNewer(e.min_wolos.clone()),
                (true, None) if enabled.contains(&e.id) => State::On,
                (true, None) => State::TurnOn,
            };
            Row {
                entry: e.clone(),
                state,
            }
        })
        .collect();
    let rank = |s: &State| match s {
        State::Update(_) | State::NeedsNewer(_) => 0,
        State::TurnOn | State::On => 1,
        State::Get => 2,
    };
    v.sort_by_key(|r| rank(&r.state));
    v
}

pub fn button_text(s: &State) -> String {
    match s {
        State::Get => STORE_PLUGIN_GET.into(),
        State::Update(v) => STORE_PLUGIN_UPDATE_TO.replace("{v}", v),
        State::NeedsNewer(v) => STORE_PLUGIN_NEEDS.replace("{v}", v),
        State::TurnOn => STORE_PLUGIN_TURN_ON.into(),
        State::On => STORE_PLUGIN_TURN_OFF.into(),
    }
}

/// Whether the row's menu offers Remove: every plugin on this account can be removed.
pub fn can_remove(s: &State) -> bool {
    !matches!(s, State::Get)
}

/// The second line of a row: what it adds and what it asks for, in plain words.
pub fn asks(e: &Entry) -> String {
    let labels = e.permission_labels();
    if labels.is_empty() {
        STORE_PLUGIN_ASKS_NOTHING.replace("{s}", &e.summary)
    } else {
        STORE_PLUGIN_ASKS_TO
            .replace("{s}", &e.summary)
            .replace("{p}", &labels.join(", ").to_lowercase())
    }
}

// ---- Live data (blocking; called from worker threads only) ----

struct Paths {
    data: PathBuf,
    config: PathBuf,
    cache: PathBuf,
}

fn paths() -> Paths {
    Paths {
        data: wol_core::widgets::data_home(),
        config: PathBuf::from(crate::kit::config_home()),
        cache: PathBuf::from(wol_core::appstream::cache_home())
            .join(wopkg::index::plugins::CACHE_DIR),
    }
}

fn keys() -> Vec<[u8; 32]> {
    wol_core::package::read_keys(std::path::Path::new(shared::package::KEYS_DIR))
}

/// This account's plugins (id, version) and the ids that are on.
fn on_this_account(p: &Paths) -> (Vec<(String, String)>, Vec<String>) {
    let user = vec![(p.data.join(shared::plugins::USER_DIR), false)];
    let enabled = wol_core::plugins::load_enabled(&p.config);
    let mut have = Vec::new();
    let mut on = Vec::new();
    for pl in wol_core::plugins::load_all(&user) {
        if let Ok(m) = &pl.manifest {
            have.push((m.id.clone(), m.version.clone()));
            if enabled.is_enabled(&m.id) {
                on.push(m.id.clone());
            }
        }
    }
    (have, on)
}

/// The list on this computer: [`load_in`] over this account's folders and the index URL.
fn load(fresh: bool) -> Listing {
    let net = wopkg::fetch::Net::new(false);
    load_in(
        &paths(),
        &keys(),
        &net,
        wopkg::index::plugins::DEFAULT_URL,
        fresh,
    )
}

/// The list: wopkg's own choice of a download or the verified cache
/// (`wopkg::index::plugins::load`: a download when `fresh`, else the cache, downloaded again
/// when missing, expired or stale), with this account's plugins on it. Blocks.
fn load_in(
    p: &Paths,
    keys: &[[u8; 32]],
    net: &wopkg::fetch::Net,
    url: &str,
    fresh: bool,
) -> Listing {
    match wopkg::index::plugins::load(net, url, &p.cache, keys, fresh) {
        Ok(index) => {
            let (have, on) = on_this_account(p);
            Listing::Ready(rows(&index, &have, &on, env!("CARGO_PKG_VERSION")))
        }
        Err(_) => Listing::Unavailable(STORE_PLUGINS_OFFLINE.into()),
    }
}

/// Get or Update: download, check (signature first), write into this account's plugin folder.
fn install(e: &Entry) -> bool {
    let p = paths();
    let net = wopkg::fetch::Net::new(false);
    wopkg::index::plugins::download(&net, e, &keys())
        .ok()
        .is_some_and(|v| {
            wol_core::package::install_plugin(&v, &p.data.join(shared::plugins::USER_DIR)).is_ok()
        })
}

/// `wol plugin enable|disable <id>`: the same path as Terminal, so grants and reloads match.
fn set_on(id: &str, on: bool) -> bool {
    let verb = if on { "enable" } else { "disable" };
    wol_core::system::run_with_timeout(
        &["wol", "plugin", verb, id],
        std::time::Duration::from_secs(20),
    )
    .is_some()
}

/// `wol plugin remove <id>`: deletes this account's plugin folder and its state.
fn remove(id: &str) -> bool {
    wol_core::system::run_with_timeout(
        &["wol", "plugin", "remove", id],
        std::time::Duration::from_secs(20),
    )
    .is_some()
}

// ---- Page ----

use crate::copy::{
    STORE_NOT_FINISHED as FAILED, STORE_PLUGIN_ASKS_NOTHING, STORE_PLUGIN_ASKS_TO, STORE_PLUGIN_BY,
    STORE_PLUGIN_GET, STORE_PLUGIN_NEEDS, STORE_PLUGIN_REMOVE, STORE_PLUGIN_TURN_OFF,
    STORE_PLUGIN_TURN_ON, STORE_PLUGIN_UPDATE_TO, STORE_PLUGINS_AVAILABLE, STORE_PLUGINS_CHECKED,
    STORE_PLUGINS_CREATE, STORE_PLUGINS_EMPTY, STORE_PLUGINS_INTRO as INTRO, STORE_PLUGINS_LOADING,
    STORE_PLUGINS_MINE, STORE_PLUGINS_NOT_YET, STORE_PLUGINS_OFFLINE,
    STORE_PLUGINS_SIGNED as SIGNED, STORE_PLUGINS_TITLE, STORE_PLUGINS_TRY_AGAIN,
};

/// Runs `work` off the UI thread, then `done(result)` on it.
fn off_ui<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(T) + 'static,
) {
    let (tx, rx) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = tx.send_blocking(work());
    });
    glib::spawn_future_local(async move {
        if let Ok(v) = rx.recv().await {
            done(v);
        }
    });
}

/// Shows a listing on the page.
type Show = Rc<dyn Fn(Listing)>;

/// The Plugins page, and the function that shows a listing on it.
pub(super) fn page(sh: &Rc<Shared>) -> (gtk::Box, Rc<dyn Fn(Listing)>) {
    let p = w::page(STORE_PLUGINS_TITLE);
    let intro = w::label(INTRO, "secondary");
    intro.set_wrap(true);
    p.append(&intro);
    p.append(&crate::ui::maker::launch_button(
        STORE_PLUGINS_CREATE,
        &crate::ui::maker::ARGV,
    ));
    let body = w::vbox(w::S6);
    p.append(&body);
    let show: Rc<std::cell::RefCell<Option<Show>>> = Default::default();
    let reload: Rc<dyn Fn(bool)> = {
        let show = show.clone();
        Rc::new(move |fresh| {
            let show = show.clone();
            off_ui(
                move || load(fresh),
                move |l| {
                    if let Some(s) = show.borrow().as_ref() {
                        s(l);
                    }
                },
            );
        })
    };
    let f: Rc<dyn Fn(Listing)> = {
        let (sh, body, reload) = (sh.clone(), body.clone(), reload.clone());
        Rc::new(move |l: Listing| fill(&sh, &body, l, &reload))
    };
    *show.borrow_mut() = Some(f.clone());
    f(Listing::Loading);
    if sh.env.pkg_search {
        let (reload, once) = (reload.clone(), std::cell::Cell::new(false));
        p.connect_map(move |_| {
            if !once.replace(true) {
                reload(false);
            }
        });
    } else {
        f(Listing::Unavailable(STORE_PLUGINS_NOT_YET.into()));
    }
    (p, f)
}

fn fill(sh: &Rc<Shared>, body: &gtk::Box, l: Listing, reload: &Rc<dyn Fn(bool)>) {
    while let Some(c) = body.first_child() {
        body.remove(&c);
    }
    let rows = match l {
        Listing::Loading => {
            body.append(&w::label(STORE_PLUGINS_LOADING, "secondary"));
            return;
        }
        Listing::Unavailable(why) => {
            let l = w::label(&why, "secondary");
            l.set_wrap(true);
            body.append(&l);
            let r = reload.clone();
            let again = w::button(STORE_PLUGINS_TRY_AGAIN, w::Kind::Default, move || r(true));
            again.set_halign(gtk::Align::Start);
            body.append(&again);
            return;
        }
        Listing::Ready(rows) => rows,
    };
    let (mine, other): (Vec<&Row>, Vec<&Row>) = rows.iter().partition(|r| r.state != State::Get);
    if !mine.is_empty() {
        let v: Vec<gtk::Widget> = mine.iter().map(|r| row(sh, r, reload)).collect();
        body.append(&w::section(STORE_PLUGINS_MINE, &v));
    }
    if other.is_empty() && mine.is_empty() {
        body.append(&w::label(STORE_PLUGINS_EMPTY, "secondary"));
    } else if !other.is_empty() {
        let v: Vec<gtk::Widget> = other.iter().map(|r| row(sh, r, reload)).collect();
        body.append(&w::section(STORE_PLUGINS_AVAILABLE, &v));
    }
    body.append(&w::section(
        "",
        &[note_row(
            "security-high-symbolic",
            STORE_PLUGINS_CHECKED,
            SIGNED,
        )],
    ));
}

fn row(sh: &Rc<Shared>, r: &Row, reload: &Rc<dyn Fn(bool)>) -> gtk::Widget {
    let kind = match r.state {
        State::Get | State::Update(_) => w::Kind::Primary,
        _ => w::Kind::Default,
    };
    let b = w::button(&button_text(&r.state), kind, || {});
    b.set_sensitive(!matches!(r.state, State::NeedsNewer(_)));
    let (sh2, entry, state, again) = (sh.clone(), r.entry.clone(), r.state.clone(), reload.clone());
    b.connect_clicked(move |b| {
        b.set_sensitive(false);
        say(&sh2.status, "");
        let (e, s) = (entry.clone(), state.clone());
        let (st, reload, b) = (sh2.status.clone(), again.clone(), b.clone());
        off_ui(
            move || match s {
                State::Get | State::Update(_) => install(&e),
                State::TurnOn => set_on(&e.id, true),
                State::On => set_on(&e.id, false),
                State::NeedsNewer(_) => false,
            },
            move |ok| {
                if !ok {
                    say(&st, FAILED);
                    b.set_sensitive(true);
                }
                reload(false);
            },
        );
    });
    // The name alone: an installed row's version is the button's business ("Update to 1.3").
    let sub = match r.entry.author.as_str() {
        "" => asks(&r.entry),
        a => STORE_PLUGIN_BY
            .replace("{a}", a)
            .replace("{s}", &asks(&r.entry)),
    };
    let row = w::list_row(
        "wolos-widgets-symbolic",
        &r.entry.name,
        "",
        Some(b.upcast_ref()),
    );
    if let Some(t) = row
        .first_child()
        .and_then(|c| c.next_sibling())
        .and_downcast::<gtk::Box>()
    {
        let l = w::label(&sub, "secondary");
        l.set_wrap(true);
        t.append(&l);
    }
    if can_remove(&r.state) {
        remove_on_secondary_click(sh, &row, &r.entry, reload);
    }
    row.upcast()
}

/// A secondary click on an installed row offers Remove (like the app rows on the Apps page).
fn remove_on_secondary_click(
    sh: &Rc<Shared>,
    row: &impl IsA<gtk::Widget>,
    e: &Entry,
    reload: &Rc<dyn Fn(bool)>,
) {
    let click = gtk::GestureClick::new();
    click.set_button(gtk::gdk::BUTTON_SECONDARY);
    let (sh, id, name, reload) = (sh.clone(), e.id.clone(), e.name.clone(), reload.clone());
    let r = row.clone().upcast::<gtk::Widget>();
    click.connect_pressed(move |_, _, x, y| {
        let (st, id, reload) = (sh.status.clone(), id.clone(), reload.clone());
        let go = Box::new(move || {
            say(&st, "");
            let id = id.clone();
            let (st, reload) = (st.clone(), reload.clone());
            off_ui(
                move || remove(&id),
                move |ok| {
                    if !ok {
                        say(&st, FAILED);
                    }
                    reload(false);
                },
            );
        });
        let label = STORE_PLUGIN_REMOVE.replace("{n}", &name);
        crate::kit::data::popup_menu(&r, x, y, vec![(label, None, go)]);
    });
    row.add_controller(click);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(id: &str, v: &str, min: &str) -> Entry {
        Entry {
            id: id.into(),
            name: id.into(),
            version: v.into(),
            min_wolos: min.into(),
            summary: "Does a thing".into(),
            permissions: vec!["http".into()],
            ..Default::default()
        }
    }

    #[test]
    fn rows_put_your_plugins_and_updates_first() {
        let index = PluginIndex {
            plugins: vec![
                e("a.new", "1.0", ""),
                e("b.off", "1.0", ""),
                e("c.on", "1.0", ""),
                e("d.update", "2.0", ""),
                e("e.later", "2.0", "9.9"),
            ],
        };
        let have: Vec<(String, String)> = [
            ("b.off", "1.0"),
            ("c.on", "1.0"),
            ("d.update", "1.0"),
            ("e.later", "1.0"),
        ]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
        let got: Vec<(String, State)> = rows(&index, &have, &["c.on".into()], "0.12.0")
            .into_iter()
            .map(|r| (r.entry.id, r.state))
            .collect();
        assert_eq!(
            got,
            [
                ("d.update".into(), State::Update("2.0".into())),
                ("e.later".into(), State::NeedsNewer("9.9".into())),
                ("b.off".into(), State::TurnOn),
                ("c.on".into(), State::On),
                ("a.new".into(), State::Get),
            ]
        );
    }

    /// The Store's load over a verified cache (no download: it is fresh), with this account's
    /// plugins on it; a plugin removed from the folder (what `wol plugin remove` deletes) is
    /// offered as Get again.
    #[test]
    fn the_cached_list_shows_what_this_account_has() {
        let root = std::env::temp_dir().join(format!("wol-store-plugins-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let p = Paths {
            data: root.join("data"),
            config: root.join("config"),
            cache: root.join("cache"),
        };
        let seed = [7u8; 32];
        let keys = [wol_core::package::public_of(&seed)];
        let entry = serde_json::json!({
            "id": "alice.demo", "name": "Demo", "version": "0.1", "summary": "Says hello",
            "author": "Alice", "license": "MIT", "kinds": ["menubar"], "permissions": [],
            "min_wolos": "0.12", "url": "https://plugins.wolos.org/alice.demo-0.1.wolplug",
            "sha256": "ab".repeat(32), "size": 4096
        });
        let text =
            serde_json::to_vec(&serde_json::json!({"format": 1, "plugins": [entry]})).unwrap();
        std::fs::create_dir_all(&p.cache).unwrap();
        // wopkg's cache names (index::plugins INDEX_FILE and SIG_FILE).
        std::fs::write(p.cache.join("index.json"), &text).unwrap();
        std::fs::write(
            p.cache.join("index.json.sig"),
            wol_core::package::sign(&seed, &text),
        )
        .unwrap();
        let user = p.data.join(shared::plugins::USER_DIR);
        crate::ui::maker::editor::create(&user, "alice.demo", &[shared::plugins::Kind::Menubar])
            .unwrap();
        let net = wopkg::fetch::Net::new(false);
        // Nothing listens here: the list must come from the cache.
        let url = "http://127.0.0.1:9/index.json";
        let state = |l: Listing| match l {
            Listing::Ready(r) => r.into_iter().map(|r| (r.entry.id, r.state)).collect(),
            other => panic!("{other:?}"),
        };
        let got: Vec<(String, State)> = state(load_in(&p, &keys, &net, url, false));
        assert_eq!(got, [("alice.demo".to_string(), State::TurnOn)]);
        assert!(
            on_this_account(&p)
                .0
                .iter()
                .any(|(id, _)| id == "alice.demo")
        );
        std::fs::remove_dir_all(user.join("alice.demo")).unwrap();
        assert!(on_this_account(&p).0.is_empty(), "a removed plugin is gone");
        let got: Vec<(String, State)> = state(load_in(&p, &keys, &net, url, false));
        assert_eq!(got, [("alice.demo".to_string(), State::Get)]);
        // A list that is not signed by a known key is not shown.
        assert_eq!(
            load_in(&p, &[[0; 32]], &net, url, false),
            Listing::Unavailable(STORE_PLUGINS_OFFLINE.into())
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn buttons_and_rows_read_plainly() {
        assert_eq!(button_text(&State::Get), "Get");
        assert_eq!(button_text(&State::Update("1.3".into())), "Update to 1.3");
        assert_eq!(
            button_text(&State::NeedsNewer("0.13".into())),
            "Needs WolOS 0.13"
        );
        assert_eq!(button_text(&State::TurnOn), "Turn On");
        assert_eq!(button_text(&State::On), "Turn Off");
        assert!(!can_remove(&State::Get));
        assert!(can_remove(&State::TurnOn) && can_remove(&State::On));
        assert!(can_remove(&State::Update("2".into())));
        let x = e("a.b", "1", "");
        assert_eq!(
            asks(&x),
            "Does a thing. Asks to: read data from the web (each site asks once)."
        );
        let none = Entry {
            permissions: vec![],
            ..x
        };
        assert_eq!(asks(&none), "Does a thing. Asks for nothing.");
        for s in [INTRO, SIGNED, FAILED] {
            assert!(crate::copy::lint(s).is_empty(), "{s}");
        }
    }
}
