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
use wopkg::release::{PluginReason, plugin_fits, plugin_updates};

/// How long adding a plugin from a file waits for `wol plugin add` (it checks the package and copies it).
const ADD_WAIT: std::time::Duration = std::time::Duration::from_secs(60);

/// What the row's button does now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Not on this account.
    Get,
    /// Installed; the index has this newer version.
    Update(String),
    /// Not on this account, and it needs this WolOS version or later: shown, not offered.
    NeedsNewer(String),
    /// Installed and off.
    TurnOn,
    /// Installed and on.
    On,
    /// Installed and on, but this version asks for more than was allowed (the labels of what is
    /// new, lower case): Allow grants it, like Turn On does, and the plugin has it from then on.
    NeedsGrant(Vec<String>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub entry: Entry,
    pub state: State,
    /// An installed plugin whose newer index version needs a newer WolOS: (version, minimum). The
    /// installed one keeps working and keeps its switch; the row says what is held back.
    pub held: Option<(String, String)>,
}

impl Row {
    /// On this account (listed under Your Plugins).
    pub fn installed(&self) -> bool {
        can_remove(&self.state)
    }
}

/// The page's content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Listing {
    Loading,
    /// One sentence: why there is no list.
    Unavailable(String),
    Ready(Vec<Row>),
    /// No index (offline, not published, not trusted: the sentence), but this account's own
    /// plugins still have their rows, so they can be turned on, off or removed (QA-PLUG-66).
    Partial(Vec<Row>, String),
}

/// What an enabled plugin's installed version asks for beyond its grant: (id, permission ids).
pub type Missing = (String, Vec<String>);

/// Rows for the index, installed ones (updates first) before the rest, each group in index order.
/// `installed`: (id, version) of this account's plugins; `enabled`: ids that are on; `missing`:
/// what the enabled ones ask for beyond their grant; `os`: the running WolOS version. A plugin
/// this WolOS cannot run is not offered (`NeedsNewer`); an installed one stays on and off as it
/// was and only notes the version that is held back.
pub fn rows(
    index: &PluginIndex,
    installed: &[(String, String)],
    enabled: &[String],
    missing: &[Missing],
    os: &str,
) -> Vec<Row> {
    let updates = plugin_updates(os, installed, index);
    let mut v: Vec<Row> = index
        .plugins
        .iter()
        .map(|e| {
            let update = updates.iter().find(|u| u.id == e.id);
            let state = if !installed.iter().any(|(id, _)| *id == e.id) {
                if plugin_fits(os, &e.min_wolos) {
                    State::Get
                } else {
                    State::NeedsNewer(e.min_wolos.clone())
                }
            } else if let Some(u) = update.filter(|u| u.reason == PluginReason::Available) {
                State::Update(u.to.clone())
            } else {
                installed_state(&e.id, enabled, missing)
            };
            let held = update
                .filter(|u| u.reason == PluginReason::NeedsNewerWolos)
                .map(|u| (u.to.clone(), e.min_wolos.clone()));
            Row {
                entry: e.clone(),
                state,
                held,
            }
        })
        .collect();
    sort(&mut v);
    v
}

/// Installed ones (updates first) before the rest; a stable sort keeps index order in a group.
fn sort(v: &mut [Row]) {
    let rank = |s: &State| match s {
        State::Update(_) | State::NeedsGrant(_) => 0,
        State::TurnOn | State::On => 1,
        State::Get | State::NeedsNewer(_) => 2,
    };
    v.sort_by_key(|r| rank(&r.state));
}

/// The switch of a plugin on this account: off, on, or on but asking for more.
fn installed_state(id: &str, enabled: &[String], missing: &[Missing]) -> State {
    if !enabled.iter().any(|e| e == id) {
        return State::TurnOn;
    }
    match missing.iter().find(|(m, _)| m == id) {
        Some((_, ids)) if !ids.is_empty() => State::NeedsGrant(
            ids.iter()
                .filter_map(|p| shared::plugins::Permission::parse(p))
                .map(|p| p.label().to_lowercase())
                .collect(),
        ),
        _ => State::On,
    }
}

/// A row's entry for a plugin on this account, from its own manifest (sideloaded, made in
/// Plugin Maker or with `wol plugin new`: not in the index).
pub fn local_entry(m: &shared::plugins::Manifest) -> Entry {
    Entry {
        id: m.id.clone(),
        name: m.name.clone(),
        version: m.version.clone(),
        summary: m.description.clone(),
        author: m.author.clone(),
        license: m.license.clone(),
        kinds: m.kinds().iter().map(|k| k.id().to_owned()).collect(),
        permissions: m.permissions.iter().map(|p| p.id().to_owned()).collect(),
        min_wolos: m.min_wolos.clone(),
        ..Entry::default()
    }
}

/// QA-PLUG-66: `rows` plus a row for each plugin on this account the index does not list
/// (`local`, from [`local_entry`]), so every installed plugin can be turned on, off or removed.
pub fn with_local(
    mut v: Vec<Row>,
    local: &[Entry],
    enabled: &[String],
    missing: &[Missing],
) -> Vec<Row> {
    for e in local {
        if v.iter().any(|r| r.entry.id == e.id) {
            continue;
        }
        v.push(Row {
            entry: e.clone(),
            state: installed_state(&e.id, enabled, missing),
            held: None,
        });
    }
    sort(&mut v);
    v
}

pub fn button_text(s: &State) -> String {
    match s {
        State::Get => STORE_PLUGIN_GET.into(),
        State::Update(v) => STORE_PLUGIN_UPDATE_TO.replace("{v}", v),
        State::NeedsNewer(v) => STORE_PLUGIN_NEEDS.replace("{v}", v),
        State::TurnOn => STORE_PLUGIN_TURN_ON.into(),
        State::On => STORE_PLUGIN_TURN_OFF.into(),
        State::NeedsGrant(_) => STORE_PLUGIN_ALLOW.into(),
    }
}

/// Whether the row's menu offers Remove: every plugin on this account can be removed.
pub fn can_remove(s: &State) -> bool {
    !matches!(s, State::Get | State::NeedsNewer(_))
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

/// A row's second line: [`asks`], or for a plugin that asks for more than it was allowed, what is
/// new; then what an installed one is held back from.
pub fn sub_line(r: &Row) -> String {
    let mut line = match &r.state {
        State::NeedsGrant(new) if !new.is_empty() => STORE_PLUGIN_ASKS_MORE
            .replace("{s}", &r.entry.summary)
            .replace("{p}", &new.join(", ")),
        _ => asks(&r.entry),
    };
    if let Some((v, min)) = &r.held {
        line = STORE_PLUGIN_HELD
            .replace("{s}", &line)
            .replace("{v}", v)
            .replace("{m}", min);
    }
    line
}

// ---- Live data (blocking; called from worker threads only) ----

/// Where this account keeps its plugins, their switches and the verified plugin list.
pub(crate) struct Paths {
    pub(crate) data: PathBuf,
    pub(crate) config: PathBuf,
    pub(crate) cache: PathBuf,
}

pub(crate) fn paths() -> Paths {
    Paths {
        data: wol_core::widgets::data_home(),
        config: PathBuf::from(crate::kit::config_home()),
        cache: PathBuf::from(wol_core::appstream::cache_home())
            .join(wopkg::index::plugins::CACHE_DIR),
    }
}

/// The keys the plugin index and `.wolplug` downloads verify against: the WolOS keys folder and
/// its `hub/` (the Hub's index key), never used for system updates (HUB-KEYDIR).
pub(crate) fn keys() -> Vec<[u8; 32]> {
    wol_core::package::read_keys_for(
        std::path::Path::new(shared::package::KEYS_DIR),
        wol_core::package::Purpose::Plugins,
    )
}

/// This account's plugins (id, version), the ids that are on, and what those ask for beyond their
/// grant (a newer version that wants more: `Enabled::missing`, the same test `wol plugin list`
/// uses).
pub(crate) fn on_this_account(p: &Paths) -> (Vec<(String, String)>, Vec<String>, Vec<Missing>) {
    let (have, on, more, _) = on_this_account_with_entries(p);
    (have, on, more)
}

/// [`on_this_account`], and each plugin's [`local_entry`].
fn on_this_account_with_entries(
    p: &Paths,
) -> (Vec<(String, String)>, Vec<String>, Vec<Missing>, Vec<Entry>) {
    let user = vec![(p.data.join(shared::plugins::USER_DIR), false)];
    let enabled = wol_core::plugins::load_enabled(&p.config);
    let mut have = Vec::new();
    let mut on = Vec::new();
    let mut more = Vec::new();
    let mut local = Vec::new();
    for pl in wol_core::plugins::load_all(&user) {
        if let Ok(m) = &pl.manifest {
            have.push((m.id.clone(), m.version.clone()));
            local.push(local_entry(m));
            if enabled.is_enabled(&m.id) {
                on.push(m.id.clone());
                let new: Vec<String> = enabled
                    .missing(m)
                    .iter()
                    .map(|p| p.id().to_owned())
                    .collect();
                if !new.is_empty() {
                    more.push((m.id.clone(), new));
                }
            }
        }
    }
    (have, on, more, local)
}

/// The WolOS release this computer runs, for `min_wolos` (QA-PLUG-57): the slot's release file,
/// else os-release's `VERSION_ID`, else this program's own version (a checkout). Two small
/// reads: call it off the GTK thread.
pub(crate) fn running_os() -> String {
    wol_core::update::check::running_release(env!("CARGO_PKG_VERSION"))
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
    let (have, on, more, local) = on_this_account_with_entries(p);
    match wopkg::index::plugins::load(net, url, &p.cache, keys, fresh) {
        Ok(index) => {
            let v = rows(&index, &have, &on, &more, &running_os());
            Listing::Ready(with_local(v, &local, &on, &more))
        }
        // e2e D5: every failure read "check that you are online", also with the network up and
        // no list on the server.
        Err(e) if local.is_empty() => Listing::Unavailable(unavailable(e.status).into()),
        Err(e) => Listing::Partial(
            with_local(Vec::new(), &local, &on, &more),
            unavailable(e.status).into(),
        ),
    }
}

/// Why there is no list, in the Store's words: no list on the server, a list that does not
/// check, or the server could not be reached.
fn unavailable(status: wopkg::ffi::Status) -> &'static str {
    match status {
        wopkg::ffi::Status::NotFound => STORE_PLUGINS_NOT_PUBLISHED,
        wopkg::ffi::Status::Untrusted => STORE_PLUGINS_UNTRUSTED,
        _ => STORE_PLUGINS_OFFLINE,
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
/// Enable on a plugin that is on again grants what its manifest asks for now (Allow).
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

/// What `wol plugin add` said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Added {
    /// Added; the plugin's id.
    Done(String),
    /// Not signed by the plugin index: it needs the person's yes (`--yes`).
    NotSigned,
    /// Not a plugin WolOS can add, or it did not run.
    Refused,
}

/// `wol plugin add --json [--yes] -- <file>`: the Terminal's verb, with the file after `--` so a
/// name that starts with dashes is a file.
pub fn add_argv(file: &str, yes: bool) -> Vec<String> {
    let mut v: Vec<String> = ["wol", "plugin", "add", "--json"]
        .into_iter()
        .map(String::from)
        .collect();
    if yes {
        v.push("--yes".into());
    }
    v.extend(["--".into(), file.into()]);
    v
}

/// The answer of that line: exit 0 with the id in its JSON; exit 64 (usage) is the verb refusing
/// an unsigned file without `--yes`; anything else is a file it will not add.
pub fn added(code: i32, out: &str) -> Added {
    match code {
        0 => serde_json::from_str::<serde_json::Value>(out)
            .ok()
            .and_then(|v| v.get("id").and_then(|i| i.as_str()).map(String::from))
            .map_or(Added::Refused, Added::Done),
        64 => Added::NotSigned,
        _ => Added::Refused,
    }
}

/// Runs the add line. Blocks.
fn add_file(file: &str, yes: bool) -> Added {
    let argv = add_argv(file, yes);
    let a: Vec<&str> = argv.iter().map(String::as_str).collect();
    match wol_core::system::run_full(&a, None, ADD_WAIT) {
        Ok((code, out, _)) => added(code, &out),
        Err(_) => Added::Refused,
    }
}

/// Add From File…: pick a `.wolplug`; a signed one is added, an unsigned one asks first.
fn add_from_file(sh: &Rc<Shared>, anchor: &gtk::Button, reload: &Rc<dyn Fn(bool)>) {
    let (sh, reload, anchor2) = (sh.clone(), reload.clone(), anchor.clone());
    crate::kit::pickers::choose_file_of(
        anchor,
        STORE_PLUGINS_ADD_FILE,
        STORE_PLUGINS_ADD_FILE_KIND,
        &["wolplug"],
        move |path| {
            let file = path.display().to_string();
            let (sh2, reload2, anchor3, file2) =
                (sh.clone(), reload.clone(), anchor2.clone(), file.clone());
            off_ui(
                move || add_file(&file, false),
                move |r| after_add(&sh2, &anchor3, &reload2, &file2, r),
            );
        },
    );
}

fn after_add(
    sh: &Rc<Shared>,
    anchor: &gtk::Button,
    reload: &Rc<dyn Fn(bool)>,
    file: &str,
    r: Added,
) {
    match r {
        Added::Done(id) => {
            say(&sh.status, &STORE_PLUGINS_ADDED.replace("{id}", &id));
            reload(false);
        }
        Added::Refused => say(&sh.status, STORE_PLUGINS_ADD_REFUSED),
        Added::NotSigned => {
            let d = gtk::AlertDialog::builder()
                .modal(true)
                .message(STORE_PLUGINS_UNSIGNED_TITLE)
                .detail(STORE_PLUGINS_UNSIGNED_DETAIL)
                .buttons([
                    crate::i18n::tr(STORE_PLUGINS_ADD_CANCEL),
                    STORE_PLUGINS_ADD_ANYWAY,
                ])
                .cancel_button(0)
                .default_button(0)
                .build();
            let win = anchor.root().and_downcast::<gtk::Window>();
            let (sh, reload, anchor, file) =
                (sh.clone(), reload.clone(), anchor.clone(), file.to_string());
            d.choose(win.as_ref(), gtk::gio::Cancellable::NONE, move |c| {
                if c == Ok(1) {
                    let (sh2, reload2, anchor2, file2) =
                        (sh.clone(), reload.clone(), anchor.clone(), file.clone());
                    off_ui(
                        move || add_file(&file, true),
                        move |r| after_add(&sh2, &anchor2, &reload2, &file2, r),
                    );
                }
            });
        }
    }
}

// ---- Page ----

use crate::copy::visual::{
    STORE_PLUGINS_ADD_ANYWAY, STORE_PLUGINS_ADD_CANCEL, STORE_PLUGINS_ADD_FILE,
    STORE_PLUGINS_ADD_FILE_KIND, STORE_PLUGINS_ADD_REFUSED, STORE_PLUGINS_ADDED,
    STORE_PLUGINS_UNSIGNED_DETAIL, STORE_PLUGINS_UNSIGNED_TITLE,
};
use crate::copy::{
    STORE_NOT_FINISHED as FAILED, STORE_PLUGIN_ALLOW, STORE_PLUGIN_ASKS_MORE,
    STORE_PLUGIN_ASKS_NOTHING, STORE_PLUGIN_ASKS_TO, STORE_PLUGIN_BY, STORE_PLUGIN_GET,
    STORE_PLUGIN_HELD, STORE_PLUGIN_NEEDS, STORE_PLUGIN_REMOVE, STORE_PLUGIN_TURN_OFF,
    STORE_PLUGIN_TURN_ON, STORE_PLUGIN_UPDATE_TO, STORE_PLUGINS_AVAILABLE, STORE_PLUGINS_CHECKED,
    STORE_PLUGINS_CREATE, STORE_PLUGINS_EMPTY, STORE_PLUGINS_INTRO as INTRO, STORE_PLUGINS_LOADING,
    STORE_PLUGINS_MINE, STORE_PLUGINS_NOT_PUBLISHED, STORE_PLUGINS_NOT_YET, STORE_PLUGINS_OFFLINE,
    STORE_PLUGINS_SIGNED as SIGNED, STORE_PLUGINS_TITLE, STORE_PLUGINS_TRY_AGAIN,
    STORE_PLUGINS_UNTRUSTED,
};

/// Runs `work` off the UI thread, then `done(result)` on it.
pub(super) fn off_ui<T: Send + 'static>(
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
    let buttons = w::hbox(w::S2);
    buttons.set_halign(gtk::Align::Start);
    buttons.append(&crate::ui::maker::launch_button(
        STORE_PLUGINS_CREATE,
        &crate::ui::maker::ARGV,
    ));
    let add = w::button(STORE_PLUGINS_ADD_FILE, w::Kind::Default, || {});
    buttons.append(&add);
    p.append(&buttons);
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
    {
        let (sh, reload) = (sh.clone(), reload.clone());
        add.connect_clicked(move |b| add_from_file(&sh, b, &reload));
    }
    f(Listing::Loading);
    if sh.env.pkg_search {
        // Every time the page is shown, not only the first (QA-PLUG-56: a plugin made or removed
        // since was not there until the Store was closed); the cached list makes it cheap.
        let reload = reload.clone();
        p.connect_map(move |_| reload(false));
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
        Listing::Ready(rows) => (rows, None),
        Listing::Partial(rows, why) => (rows, Some(why)),
    };
    let (rows, why) = rows;
    let (mine, other): (Vec<&Row>, Vec<&Row>) = rows.iter().partition(|r| r.installed());
    if !mine.is_empty() {
        let v: Vec<gtk::Widget> = mine.iter().map(|r| row(sh, r, reload)).collect();
        body.append(&w::section(STORE_PLUGINS_MINE, &v));
    }
    if let Some(why) = why {
        // No index: this account's plugins above, then why the rest is missing.
        let l = w::label(&why, "secondary");
        l.set_wrap(true);
        body.append(&l);
        let r = reload.clone();
        let again = w::button(STORE_PLUGINS_TRY_AGAIN, w::Kind::Default, move || r(true));
        again.set_halign(gtk::Align::Start);
        body.append(&again);
    } else if other.is_empty() && mine.is_empty() {
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
        State::Get | State::Update(_) | State::NeedsGrant(_) => w::Kind::Primary,
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
                State::TurnOn | State::NeedsGrant(_) => set_on(&e.id, true),
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
        "" => sub_line(r),
        a => STORE_PLUGIN_BY
            .replace("{a}", a)
            .replace("{s}", &sub_line(r)),
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
                e("f.future", "1.0", "9.9"),
                e("g.broken", "1.0", "soon"),
                e("h.asks", "1.0", ""),
            ],
        };
        let have: Vec<(String, String)> = [
            ("b.off", "1.0"),
            ("c.on", "1.0"),
            ("d.update", "1.0"),
            ("e.later", "1.0"),
            ("h.asks", "1.0"),
        ]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
        let enabled: Vec<String> = vec!["c.on".into(), "h.asks".into()];
        // h.asks is on and its installed version asks for notifications beyond its grant (ST6-1).
        let more = vec![("h.asks".to_string(), vec!["notifications".to_string()])];
        let list = rows(&index, &have, &enabled, &more, "0.12.0");
        let got: Vec<(String, State)> = list
            .iter()
            .map(|r| (r.entry.id.clone(), r.state.clone()))
            .collect();
        assert_eq!(
            got,
            [
                ("d.update".into(), State::Update("2.0".into())),
                (
                    "h.asks".into(),
                    State::NeedsGrant(vec!["show notifications".into()])
                ),
                ("b.off".into(), State::TurnOn),
                ("c.on".into(), State::On),
                // Installed, its newer version held back: still On/Off as before (ST6-3).
                ("e.later".into(), State::TurnOn),
                ("a.new".into(), State::Get),
                // Not installed and not runnable here: not offered, whatever the minimum says.
                ("f.future".into(), State::NeedsNewer("9.9".into())),
                ("g.broken".into(), State::NeedsNewer("soon".into())),
            ]
        );
        let later = list.iter().find(|r| r.entry.id == "e.later").unwrap();
        assert_eq!(later.held, Some(("2.0".into(), "9.9".into())));
        assert!(
            sub_line(later).contains("Version 2.0 needs WolOS 9.9"),
            "{}",
            sub_line(later)
        );
        assert!(list.iter().filter(|r| r.held.is_some()).count() == 1);
        // Your Plugins: installed ones only.
        let mine: Vec<&str> = list
            .iter()
            .filter(|r| r.installed())
            .map(|r| r.entry.id.as_str())
            .collect();
        assert_eq!(mine, ["d.update", "h.asks", "b.off", "c.on", "e.later"]);
    }

    /// ST6-1: a plugin update that asks for a new permission is not left "On" with the new one
    /// silently refused: the row says what is new and Allow runs the enable path.
    #[test]
    fn an_update_that_asks_for_more_needs_a_grant() {
        let index = PluginIndex {
            plugins: vec![e("alice.weather", "1.3", "0.12")],
        };
        let have = vec![("alice.weather".to_string(), "1.3".to_string())];
        let on = vec!["alice.weather".to_string()];
        // Nothing missing: On.
        for none in [vec![], vec![("alice.weather".to_string(), vec![])]] {
            let r = rows(&index, &have, &on, &none, "0.12.0");
            assert_eq!(r[0].state, State::On, "{none:?}");
        }
        let more = vec![(
            "alice.weather".to_string(),
            vec!["notifications".to_string(), "bogus".to_string()],
        )];
        let r = rows(&index, &have, &on, &more, "0.12.0");
        let want = State::NeedsGrant(vec!["show notifications".into()]);
        assert_eq!(r[0].state, want);
        assert_eq!(button_text(&r[0].state), "Allow");
        assert!(can_remove(&r[0].state) && r[0].installed());
        assert_eq!(
            sub_line(&r[0]),
            "Does a thing. Asks for more now: show notifications."
        );
        // Off: Turn On grants what the manifest declares, so no separate grant.
        let r = rows(&index, &have, &[], &more, "0.12.0");
        assert_eq!(r[0].state, State::TurnOn);
        // An update to install comes first; Allow follows it.
        let have = vec![("alice.weather".to_string(), "1.2".to_string())];
        let r = rows(&index, &have, &on, &more, "0.12.0");
        assert_eq!(r[0].state, State::Update("1.3".into()));
    }

    /// The real check of what is missing: `Enabled::missing` over a plugin folder whose manifest
    /// asks for more than the saved grant.
    #[test]
    fn what_an_enabled_plugin_asks_beyond_its_grant_is_found_on_disk() {
        let root = std::env::temp_dir().join(format!("wol-store-grant-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let p = Paths {
            data: root.join("data"),
            config: root.join("config"),
            cache: root.join("cache"),
        };
        let user = p.data.join(shared::plugins::USER_DIR);
        // A Toggle plugin asks for `toggles`; it was turned on when it asked for nothing.
        crate::ui::maker::editor::create(&user, "alice.demo", &[shared::plugins::Kind::Toggle])
            .unwrap();
        std::fs::create_dir_all(&p.config).unwrap();
        let mut enabled = wol_core::plugins::Enabled::default();
        enabled.enable(&shared::plugins::Manifest {
            id: "alice.demo".into(),
            ..Default::default()
        });
        wol_core::plugins::save_enabled(&p.config, &enabled).unwrap();
        let (have, on, more) = on_this_account(&p);
        assert_eq!(have.len(), 1);
        assert_eq!(on, ["alice.demo"]);
        assert_eq!(
            more,
            [("alice.demo".to_string(), vec!["toggles".to_string()])]
        );
        // Allowing it (what `wol plugin enable` saves) clears it.
        let m = wol_core::plugins::load_all(&[(user.clone(), false)])
            .remove(0)
            .manifest
            .unwrap();
        enabled.enable(&m);
        wol_core::plugins::save_enabled(&p.config, &enabled).unwrap();
        assert!(on_this_account(&p).2.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    /// e2e D5: every failure read "check that you are online", also with the network up and no
    /// list on the server. Each reason has its own sentence.
    #[test]
    fn a_missing_or_unchecked_list_is_not_called_offline() {
        use wopkg::ffi::Status;
        assert_eq!(unavailable(Status::NotFound), STORE_PLUGINS_NOT_PUBLISHED);
        assert_eq!(unavailable(Status::Untrusted), STORE_PLUGINS_UNTRUSTED);
        assert_eq!(unavailable(Status::Unavailable), STORE_PLUGINS_OFFLINE);
        for t in [STORE_PLUGINS_NOT_PUBLISHED, STORE_PLUGINS_UNTRUSTED] {
            assert!(!t.contains("online"), "{t}");
        }
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
        // A list that is not signed by a known key is not shown, and says so (e2e D5: not
        // "check that you are online").
        assert_eq!(
            load_in(&p, &[[0; 32]], &net, url, false),
            Listing::Unavailable(STORE_PLUGINS_UNTRUSTED.into())
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
        assert!(!can_remove(&State::NeedsNewer("0.13".into())));
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

    /// QA-PLUG-56: the page reads its list every time it is shown, not once per Store session.
    #[test]
    fn the_page_reloads_each_time_it_is_shown() {
        let src = include_str!("store_plugins.rs");
        let page = &src[src.find("pub(super) fn page(").unwrap()..];
        let page = &page[..page.find("\nfn fill(").unwrap()];
        assert!(
            page.contains("p.connect_map(move |_| reload(false));"),
            "{page}"
        );
        assert!(!page.contains("once"), "no once-only flag");
    }

    /// QA-PLUG-57: `min_wolos` is held against the release file the computer runs, not the
    /// version of the program asking.
    #[test]
    fn min_wolos_reads_the_release_file() {
        use wol_core::update::check::release_of;
        let release = r#"{"version":"0.12.1","snapshot":"2026/11/02"}"#;
        let own = env!("CARGO_PKG_VERSION");
        assert_eq!(release_of(Some(release), None, own), "0.12.1");
        let os = "ID=wolos\nVERSION_ID=\"0.11.3\"\n";
        assert_eq!(release_of(None, Some(os), own), "0.11.3");
        assert_eq!(release_of(None, None, own), own);
        // A plugin asking for 0.13 is held back on 0.12.1 whatever this program is built as.
        let on = release_of(Some(release), None, own);
        assert!(!plugin_fits(&on, "0.13"));
        assert!(plugin_fits(&on, "0.12"));
        assert!(!running_os().is_empty());
    }

    /// QA-PLUG-66: a plugin on this account that the index does not list (made in Plugin Maker,
    /// `wol plugin new`, a sideloaded .wolplug) has its row, with its switch and Remove; with no
    /// index at all those rows still show, with the reason and Try Again.
    #[test]
    fn rows_include_installed_not_in_index() {
        let root = std::env::temp_dir().join(format!("wol-store-local-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let p = Paths {
            data: root.join("data"),
            config: root.join("config"),
            cache: root.join("cache"),
        };
        let user = p.data.join(shared::plugins::USER_DIR);
        crate::ui::maker::editor::create(&user, "alice.mine", &[shared::plugins::Kind::Menubar])
            .unwrap();
        // Index rows plus the local one, which is off.
        let (have, on, more, local) = on_this_account_with_entries(&p);
        let index = PluginIndex {
            plugins: vec![e("alice.weather", "1.3", "")],
        };
        let v = with_local(
            rows(&index, &have, &on, &more, "0.12.0"),
            &local,
            &on,
            &more,
        );
        let ids: Vec<(&str, &State)> = v.iter().map(|r| (r.entry.id.as_str(), &r.state)).collect();
        assert_eq!(
            ids,
            [
                ("alice.mine", &State::TurnOn),
                ("alice.weather", &State::Get)
            ]
        );
        assert!(v[0].installed(), "Remove is offered");
        // No index reachable and no cache: the account's plugins still have rows.
        let net = wopkg::fetch::Net::new(false);
        match load_in(&p, &keys(), &net, "http://127.0.0.1:9/index.json", true) {
            Listing::Partial(rows, why) => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].entry.id, "alice.mine");
                assert_eq!(rows[0].state, State::TurnOn);
                assert!(!why.is_empty());
            }
            other => panic!("{other:?}"),
        }
        let _ = std::fs::remove_dir_all(root);
    }

    /// Add From File runs the Terminal's `wol plugin add`, the file after `--`; the exit code
    /// tells signed (0 with the id), not signed (64: asks first, then `--yes`) and refused.
    #[test]
    fn add_from_file_is_the_terminal_verb() {
        assert_eq!(
            add_argv("/home/a/Downloads/clock.wolplug", false),
            [
                "wol",
                "plugin",
                "add",
                "--json",
                "--",
                "/home/a/Downloads/clock.wolplug"
            ]
        );
        assert_eq!(
            add_argv("--odd.wolplug", true),
            [
                "wol",
                "plugin",
                "add",
                "--json",
                "--yes",
                "--",
                "--odd.wolplug"
            ]
        );
        assert_eq!(
            added(0, r#"{"id":"alice.clock","version":"1.0","signed":true}"#),
            Added::Done("alice.clock".into())
        );
        assert_eq!(
            added(0, "not json"),
            Added::Refused,
            "success without an id is no success"
        );
        assert_eq!(added(64, ""), Added::NotSigned);
        for code in [1, 65, 66, 69, 77, -1] {
            assert_eq!(added(code, ""), Added::Refused, "{code}");
        }
        for s in [
            crate::copy::visual::STORE_PLUGINS_ADDED.replace("{id}", "alice.clock"),
            crate::copy::visual::STORE_PLUGINS_ADD_REFUSED.into(),
            crate::copy::visual::STORE_PLUGINS_UNSIGNED_DETAIL.into(),
        ] {
            assert!(
                crate::copy::lint(&s).is_empty(),
                "{s}: {:?}",
                crate::copy::lint(&s)
            );
        }
    }
}
