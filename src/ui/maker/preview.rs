// SPDX-License-Identifier: GPL-2.0-or-later
//! Plugin Maker live preview (ADR 0031 "Plugin Maker / Theme Studio"): the plugin being made,
//! loaded through the shell's own plugin host (`ui::plugins::Host`) in a sandbox, drawn on the
//! four surfaces it can reach: a menu bar strip with its items in place, a Dock strip, its
//! desktop cards and its Control Center tiles.
//!
//! - Same host, same rules: every entry is its own Lua state with the host's deadlines, and
//!   three failures in a row turn the plugin off ([`MAX_FAILURES`](shared::plugins::MAX_FAILURES));
//!   a failing entry shows its sentence where it would sit.
//! - Invariant (the sandbox): nothing acts on the person's desktop. [`PreviewServices`] runs no
//!   command and reads nothing from the web; state, toggles and the enable list live in a private
//!   folder ([`sandbox`]), never in the account's real stores.
use super::off_thread;
use crate::copy::maker as copy;
use crate::kit::{controls::ToggleTile, form, widgets as w};
use crate::ui::app_view::{Act, Sink, View};
use crate::ui::plugins::{self, Card, DockTile, Host, Paths, Shown, Snapshot, Source, Tile};
use gtk::prelude::*;
use shared::layout::{BarItem, Layout};
use shared::plugins::Kind;
use shared::view::Node;
use shared::widgets::Size;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use wol_core::plugins as cp;
use wol_lua::{Services, Stamp};

/// Kinds with Lua the preview runs (menu bar items and desktop cards; Dock entries and tiles
/// need none).
pub const KINDS: &[Kind] = &[Kind::Menubar, Kind::Widget];

/// The preview's private stores: under the session's runtime folder (or the temp folder), one
/// per app process, so a preview never shares state with the desktop or another Maker.
pub fn sandbox() -> Paths {
    sandbox_in(&sandbox_root())
}

/// The folder [`sandbox`] lives in, for this process.
pub fn sandbox_root() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(std::env::temp_dir);
    base.join(format!("wol-plugin-maker-{}", std::process::id()))
}

/// Removes a sandbox folder when dropped, on the dropping thread (so it is done before the
/// process exits; no detached thread is left to be killed at exit).
pub struct SandboxGuard(pub PathBuf);

impl Drop for SandboxGuard {
    fn drop(&mut self) {
        remove_sandbox(&self.0);
    }
}

/// Deletes what the plugin host writes in the sandbox under `root`, by name (RR-041: no
/// recursive delete): the enable list, each plugin's `state.json`, the toggle files, then the
/// folders bottom-up while they are empty. Nothing else goes: a folder holding a file the host
/// did not name (a stray scratch file) stays, inside the session's runtime folder.
pub fn remove_sandbox(root: &Path) {
    use std::fs;
    let p = sandbox_in(root);
    let enabled = cp::enabled_path(&p.config_home);
    let state = p.state_home.join(shared::plugins::STATE_DIR);
    let toggles = p.state_home.join(shared::plugins::TOGGLES_DIR);
    let _ = fs::remove_file(&enabled);
    // DirEntry::file_type does not follow symlinks: a link is removed, never entered.
    let entries = |d: &Path| -> Vec<fs::DirEntry> {
        fs::read_dir(d)
            .map(|r| r.flatten().collect())
            .unwrap_or_default()
    };
    for e in entries(&state) {
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            if let Some(id) = e.file_name().to_str() {
                let _ = fs::remove_file(cp::state_path(&p.state_home, id));
            }
            let _ = fs::remove_dir(e.path());
        }
    }
    for e in entries(&toggles) {
        if e.file_type().is_ok_and(|t| !t.is_dir()) {
            let _ = fs::remove_file(e.path());
        }
    }
    for leaf in [enabled.as_path(), state.as_path(), toggles.as_path()] {
        for d in leaf
            .ancestors()
            .filter(|d| *d != enabled && d.starts_with(root))
        {
            match fs::remove_dir(d) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => break,
                _ => {}
            }
        }
    }
}

/// [`sandbox`] under `root`.
pub fn sandbox_in(root: &Path) -> Paths {
    Paths {
        dirs: Vec::new(),
        config_home: root.join("config"),
        state_home: root.join("state"),
    }
}

/// What preview scripts reach: the clock and the machine, and the sandbox's state and toggles.
/// Commands, the web, panels and notifications do nothing.
pub struct PreviewServices {
    state_home: PathBuf,
    /// A fixed clock (goldens); None = this machine's.
    now: Option<Stamp>,
}

impl PreviewServices {
    pub fn new(paths: &Paths, now: Option<Stamp>) -> PreviewServices {
        PreviewServices {
            state_home: paths.state_home.clone(),
            now,
        }
    }
}

impl Services for PreviewServices {
    fn now(&self) -> Stamp {
        self.now.unwrap_or_else(|| wol_lua::Live.now())
    }
    fn system(&self) -> wol_lua::SystemInfo {
        wol_lua::Live.system()
    }
    fn notify(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn run(&self, _: &str, _: &[String], _: u32) -> Result<String, String> {
        Err(copy::PREVIEW_NO_RUN.into())
    }
    fn http_get(&self, _: &str, _: &str) -> Result<String, String> {
        Err(copy::PREVIEW_NO_WEB.into())
    }
    fn open(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn toggle_get(&self, plugin: &str, toggle: &str) -> bool {
        cp::toggle_get(&self.state_home, plugin, toggle)
    }
    fn toggle_set(&self, plugin: &str, toggle: &str, on: bool) -> Result<(), String> {
        cp::toggle_set(&self.state_home, plugin, toggle, on)
    }
    fn show(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn state_load(&self, plugin: &str) -> Option<String> {
        cp::load_state(&self.state_home, plugin)
    }
    fn state_save(&self, plugin: &str, json: &str) -> Result<(), String> {
        cp::save_state(&self.state_home, plugin, json)
    }
}

/// One read of the plugin folder: the snapshot the host takes, or the first sentence to fix.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Read {
    pub snapshot: Snapshot,
    pub problem: Option<String>,
}

/// Reads `dir` as one enabled plugin with every permission it declares (the sandbox grants
/// them; nothing it asks for acts). Blocks: run it off the GTK thread.
pub fn read(dir: &Path) -> Read {
    let plugin = cp::load(dir, false);
    let m = match &plugin.manifest {
        Ok(m) => m.clone(),
        Err(p) => {
            return Read {
                problem: Some(p.clone()),
                ..Read::default()
            };
        }
    };
    let mut enabled = cp::Enabled::default();
    enabled.enable(&m);
    let sources = m
        .scripts()
        .into_iter()
        .filter(|(kind, _, _)| KINDS.contains(kind))
        .map(|(kind, id, file)| Source {
            plugin: m.id.clone(),
            kind,
            id: id.to_owned(),
            text: std::fs::read_to_string(dir.join(file))
                .map_err(|_| format!("{file}: the file could not be read as text.")),
        })
        .collect();
    Read {
        snapshot: Snapshot {
            plugins: vec![plugin],
            enabled,
            sources,
        },
        problem: None,
    }
}

/// A menu bar item: its slot (0 left, 1 centre, 2 right), entry, and what it shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub slot: usize,
    pub plugin: String,
    pub id: String,
    pub shown: Shown,
}

/// What the four surfaces show of the plugin now.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Surfaces {
    pub bar: Vec<Item>,
    pub dock: Vec<DockTile>,
    pub cards: Vec<(Card, Shown)>,
    pub tiles: Vec<Tile>,
    /// Sentences the host raised (the plugin turned off after three failures).
    pub toasts: Vec<String>,
}

impl Surfaces {
    pub fn is_empty(&self) -> bool {
        self.bar.is_empty()
            && self.dock.is_empty()
            && self.cards.is_empty()
            && self.tiles.is_empty()
    }
}

/// Renders every entry once through `host` (which has taken `snap`), placed as the desktop
/// places them: menu bar items by the default layout, cards, Dock entries and tiles.
pub fn surfaces(host: &mut Host, snap: &Snapshot, state_home: &Path) -> Surfaces {
    let mut bar = Vec::new();
    for (slot, items) in host.bar(&Layout::default()).iter().enumerate() {
        for i in items {
            if let BarItem::Plugin { plugin, id } = i
                && let Some(shown) = host.render(Kind::Menubar, plugin, id)
            {
                bar.push(Item {
                    slot,
                    plugin: plugin.clone(),
                    id: id.clone(),
                    shown,
                });
            }
        }
    }
    let cards = host
        .cards()
        .into_iter()
        .filter_map(|c| {
            let shown = host.render(Kind::Widget, &c.plugin, &c.id)?;
            Some((c, shown))
        })
        .collect();
    Surfaces {
        bar,
        dock: host.dock_tiles(),
        cards,
        tiles: plugins::tiles_of(snap, state_home),
        toasts: host.take_toasts(),
    }
}

// ---- The preview view ----

/// The Preview page: the plugin on the four surfaces, rebuilt on every reload and click.
pub struct Preview {
    pub root: gtk::Box,
    body: gtk::Box,
    host: Rc<RefCell<Host>>,
    paths: Paths,
    snap: RefCell<Snapshot>,
    problem: RefCell<Option<String>>,
}

impl Preview {
    /// An empty preview whose scripts reach `services` and whose stores are `paths`.
    pub fn new(paths: Paths, services: Rc<dyn Services>) -> Rc<Preview> {
        let root = w::page("Preview");
        root.append(&w::footnote(copy::PREVIEW_ABOUT));
        let body = w::vbox(w::S6);
        root.append(&body);
        Rc::new(Preview {
            root,
            body,
            host: Rc::new(RefCell::new(Host::with_services(
                paths.clone(),
                KINDS,
                services,
            ))),
            paths,
            snap: RefCell::default(),
            problem: RefCell::default(),
        })
    }

    /// Takes a fresh read (every script loads again; the failure streak starts over) and draws.
    pub fn show(self: &Rc<Self>, r: Read) {
        self.host.borrow_mut().apply(r.snapshot.clone());
        self.snap.replace(r.snapshot);
        self.problem.replace(r.problem);
        self.draw();
    }

    /// Reads `dir` again off the GTK thread, then shows it.
    pub fn reload(self: &Rc<Self>, dir: &Path) {
        let dir = dir.to_path_buf();
        let me = Rc::downgrade(self);
        off_thread(
            move || read(&dir),
            move |r| {
                if let Some(p) = me.upgrade() {
                    p.show(r);
                }
            },
        );
    }

    fn draw(self: &Rc<Self>) {
        while let Some(c) = self.body.first_child() {
            self.body.remove(&c);
        }
        if let Some(p) = self.problem.borrow().clone() {
            self.body.append(&w::section(
                "",
                &[
                    form::notice(copy::PREVIEW_FIX_FIRST, &[]).upcast(),
                    w::list_row("", &p, "", None).upcast(),
                ],
            ));
            return;
        }
        let s = {
            let snap = self.snap.borrow();
            surfaces(&mut self.host.borrow_mut(), &snap, &self.paths.state_home)
        };
        for t in &s.toasts {
            self.body.append(&form::notice(t, &[]));
        }
        if s.is_empty() {
            self.body.append(&w::footnote(copy::PREVIEW_EMPTY));
            return;
        }
        if !s.bar.is_empty() {
            self.body
                .append(&surface("Menu Bar", &self.menubar(&s.bar)));
        }
        if !s.dock.is_empty() {
            self.body.append(&surface("Dock", &dock(&s.dock)));
        }
        if !s.cards.is_empty() {
            self.body.append(&surface("Desktop", &self.cards(&s.cards)));
        }
        if !s.tiles.is_empty() {
            self.body
                .append(&surface("Control Center", &self.tiles(&s.tiles)));
        }
    }

    /// A menu bar strip: the app name on the left, the plugin's items in their slots, a clock.
    fn menubar(self: &Rc<Self>, items: &[Item]) -> gtk::Box {
        let strip = w::hbox(0);
        strip.add_css_class("menubar-strip");
        strip.append(&w::icon("wolos-glyph-symbolic", w::ICON));
        strip.append(&w::label(super::APP_NAME, "app-name"));
        let slot = |n: usize| -> Vec<gtk::Widget> {
            items
                .iter()
                .filter(|i| i.slot == n)
                .map(|i| self.bar_item(i))
                .collect()
        };
        for i in slot(0) {
            strip.append(&i);
        }
        strip.append(&w::spacer());
        for i in slot(1) {
            strip.append(&i);
        }
        strip.append(&w::spacer());
        for i in slot(2) {
            strip.append(&i);
        }
        strip
    }

    /// One item: its tree (a click opens its popover), or its sentence in its place.
    fn bar_item(self: &Rc<Self>, i: &Item) -> gtk::Widget {
        let node = match &i.shown {
            Ok(n) => n.clone(),
            Err(m) => {
                let l = w::label(m, "secondary");
                l.add_css_class("may-ellipsize");
                l.set_ellipsize(gtk::pango::EllipsizeMode::End);
                l.set_max_width_chars(SENTENCE_CHARS);
                l.set_tooltip_text(Some(m));
                return l.upcast();
            }
        };
        let view = View::new(&node, Sink::new(|_| {}));
        view.widget().set_can_target(false);
        let b = w::menubar_item(view.widget());
        b.set_tooltip_text(Some(&self.host.borrow().name(&i.plugin)));
        let (me, plugin, id) = (Rc::downgrade(self), i.plugin.clone(), i.id.clone());
        b.connect_clicked(move |b| {
            if let Some(p) = me.upgrade() {
                p.popover(b, &plugin, &id);
            }
        });
        b.upcast()
    }

    /// The item's popover; its buttons run the script's handlers, then the preview redraws.
    fn popover(self: &Rc<Self>, at: &gtk::Button, plugin: &str, id: &str) {
        let r = self.host.borrow_mut().popover(plugin, id);
        let node = match r {
            Some(Ok(Some(n))) => n,
            Some(Ok(None)) | None => return,
            Some(Err(m)) => Node::Label {
                text: m,
                role: shared::view::Role::Secondary,
            },
        };
        let pop = gtk::Popover::new();
        pop.set_parent(at);
        pop.add_css_class("menu");
        let (me, p, i, close) = (
            Rc::downgrade(self),
            plugin.to_owned(),
            id.to_owned(),
            pop.downgrade(),
        );
        let view = View::new(
            &node,
            Sink::new(move |a| {
                if let (Act::Clicked(b), Some(me)) = (a, me.upgrade()) {
                    me.host.borrow_mut().click(Kind::Menubar, &p, &i, b);
                    if let Some(pop) = close.upgrade() {
                        pop.popdown();
                    }
                    me.draw();
                }
            }),
        );
        let body = view.widget().clone();
        body.set_margin_top(w::S2);
        body.set_margin_bottom(w::S2);
        body.set_margin_start(w::S2);
        body.set_margin_end(w::S2);
        pop.set_child(Some(&body));
        pop.connect_closed(|p| p.unparent());
        pop.popup();
    }

    /// Each card at its first size; a button runs the handler, then the preview redraws.
    fn cards(self: &Rc<Self>, cards: &[(Card, Shown)]) -> gtk::FlowBox {
        let row = w::wrap_row(w::S3, 3);
        for (c, shown) in cards {
            let size = c.sizes.first().copied().unwrap_or(Size::Small);
            let card = match shown {
                Ok(node) => {
                    let (me, p, i) = (Rc::downgrade(self), c.plugin.clone(), c.id.clone());
                    crate::ui::lua_widgets::card(size, node, true, move |b| {
                        if let Some(me) = me.upgrade() {
                            me.host.borrow_mut().click(Kind::Widget, &p, &i, b);
                            me.draw();
                        }
                    })
                }
                Err(m) => crate::ui::widgets_layout::error_card(
                    size,
                    &c.name,
                    copy::PREVIEW_FIX_FIRST,
                    std::slice::from_ref(m),
                ),
            };
            row.append(&card);
        }
        row
    }

    /// The plugin's tiles as Control Center draws them; a click flips the sandbox's toggle.
    fn tiles(self: &Rc<Self>, tiles: &[Tile]) -> gtk::Box {
        let row = w::hbox(w::S2);
        row.set_halign(gtk::Align::Start);
        for t in tiles {
            let tile = ToggleTile::new(&t.icon, &t.label);
            tile.set_on(t.on);
            let (me, t) = (Rc::downgrade(self), t.clone());
            tile.button.connect_clicked(move |_| {
                let Some(me) = me.upgrade() else { return };
                let _ = cp::toggle_set(&me.paths.state_home, &t.plugin, &t.toggle, !t.on);
                me.draw();
            });
            tile.button.set_width_request(TILE);
            tile.button.set_height_request(TILE);
            tile.button.set_hexpand(false);
            row.append(&tile.button);
        }
        row
    }
}

/// One surface: its title over what the plugin shows there (drawn as the surface draws it, so
/// no card around it).
fn surface(title: &str, child: &impl IsA<gtk::Widget>) -> gtk::Box {
    let b = w::vbox(w::S2);
    b.append(&w::label(title, "title"));
    b.append(child);
    b
}

/// The plugin's Dock entries next to two apps every Dock has, as the Dock places them (after the
/// apps). The preview launches nothing.
fn dock(tiles: &[DockTile]) -> gtk::Box {
    let shelf = w::hbox(w::S2);
    shelf.add_css_class("dock-shelf");
    shelf.add_css_class("no-row-check"); // tiles lift on hover by design
    shelf.set_halign(gtk::Align::Center);
    for (icon, name) in NEIGHBOURS {
        shelf.append(&w::DockTile::new(icon, name, || {}).button);
    }
    for t in tiles {
        shelf.append(&w::DockTile::new(&t.icon, &t.name, || {}).button);
    }
    shelf
}

/// Apps beside the plugin's Dock entries, so it is seen in place.
const NEIGHBOURS: [(&str, &str); 2] = [("wolos-files", "Files"), ("wolos-terminal", "Terminal")];
/// Characters of a failing item's sentence shown in the strip (the tooltip has it whole).
const SENTENCE_CHARS: i32 = 40;
/// Control Center tile side in px (the grid's four-column cell, `control_center::tiles`).
const TILE: i32 = 72;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::maker::editor;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wol-maker-pv-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const NOON: Stamp = Stamp {
        year: 2026,
        month: 11,
        day: 2,
        weekday: 1,
        hour: 12,
        minute: 5,
        second: 0,
    };

    fn host_for(root: &Path, dir: &Path) -> (Host, Read, Paths) {
        let paths = sandbox_in(&root.join("sandbox"));
        let r = read(dir);
        let mut h = Host::with_services(
            paths.clone(),
            KINDS,
            Rc::new(PreviewServices::new(&paths, Some(NOON))),
        );
        h.apply(r.snapshot.clone());
        (h, r, paths)
    }

    #[test]
    fn a_scaffolded_plugin_shows_on_all_four_surfaces() {
        let root = tmp("four");
        let dir = editor::create(
            &root,
            "alice.demo",
            &[Kind::Menubar, Kind::Widget, Kind::Dock, Kind::Tile],
        )
        .unwrap();
        let (mut h, r, paths) = host_for(&root, &dir);
        assert_eq!(r.problem, None);
        let s = surfaces(&mut h, &r.snapshot, &paths.state_home);
        assert_eq!(s.bar.len(), 1, "{s:?}");
        assert_eq!(s.bar[0].slot, 2, "side = \"right\" sits in the right slot");
        let bar = format!("{:?}", s.bar[0].shown.as_ref().unwrap());
        assert!(
            bar.contains("12:05"),
            "the item rendered with the clock: {bar}"
        );
        assert_eq!(s.cards.len(), 1);
        let card = format!("{:?}", s.cards[0].1.as_ref().unwrap());
        assert!(card.contains("Count: 0"), "{card}");
        assert_eq!(s.dock.len(), 1);
        assert_eq!(s.dock[0].name, "Demo");
        assert_eq!(s.tiles.len(), 1);
        assert!(!s.tiles[0].on);
        assert!(s.toasts.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_failing_entry_shows_its_sentence_and_three_strikes_turn_it_off() {
        let root = tmp("strikes");
        let dir = editor::create(&root, "alice.bad", &[Kind::Menubar]).unwrap();
        std::fs::write(
            dir.join("bar.lua"),
            "return wol.menubar { render = function() error('no data yet') end }",
        )
        .unwrap();
        let (mut h, r, paths) = host_for(&root, &dir);
        for n in 1..=2 {
            let s = surfaces(&mut h, &r.snapshot, &paths.state_home);
            let why = s.bar[0].shown.clone().unwrap_err();
            assert!(why.contains("no data yet"), "{n}: {why}");
            assert!(s.toasts.is_empty(), "{n}");
        }
        let s = surfaces(&mut h, &r.snapshot, &paths.state_home);
        assert!(s.bar[0].shown.is_err());
        assert_eq!(s.toasts.len(), 1, "the third failure turns it off: {s:?}");
        let s = surfaces(&mut h, &r.snapshot, &paths.state_home);
        assert!(s.bar.is_empty(), "off: it leaves the bar");
        // The off switch went to the sandbox, never to the account's plugins.json.
        assert!(cp::enabled_path(&paths.config_home).is_file());
        assert!(cp::enabled_path(&paths.config_home).starts_with(root.join("sandbox")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_slow_entry_is_stopped_at_the_host_deadline() {
        let root = tmp("slow");
        let dir = editor::create(&root, "alice.slow", &[Kind::Menubar]).unwrap();
        std::fs::write(
            dir.join("bar.lua"),
            "return wol.menubar { render = function() local t = {} while true do table.move(t, 1, 2^20 - 1, 2) end; end }",
        )
        .unwrap();
        let (mut h, r, paths) = host_for(&root, &dir);
        let start = std::time::Instant::now();
        let s = surfaces(&mut h, &r.snapshot, &paths.state_home);
        assert!(start.elapsed() < 2 * plugins::DEADLINES.render);
        assert_eq!(s.bar[0].shown, Err(wol_lua::TOO_LONG.to_owned()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_sandbox_runs_nothing_and_keeps_state_to_itself() {
        let root = tmp("sandbox");
        let paths = sandbox_in(&root);
        let s = PreviewServices::new(&paths, None);
        assert_eq!(
            s.run("alice.x", &["rm".into()], 1),
            Err(copy::PREVIEW_NO_RUN.to_owned())
        );
        assert_eq!(
            s.http_get("alice.x", "https://example.com/"),
            Err(copy::PREVIEW_NO_WEB.to_owned())
        );
        s.toggle_set("alice.x", "switch", true).unwrap();
        assert!(s.toggle_get("alice.x", "switch"));
        assert!(cp::toggle_path(&paths.state_home, "alice.x", "switch").starts_with(&root));
        s.state_save("alice.x", "{\"n\":1}").unwrap();
        assert_eq!(s.state_load("alice.x").as_deref(), Some("{\"n\":1}"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_broken_folder_gives_its_first_sentence_and_no_entries() {
        let root = tmp("broken");
        let dir = editor::create(&root, "alice.gone", &[Kind::Widget]).unwrap();
        std::fs::remove_file(dir.join("card.lua")).unwrap();
        let r = read(&dir);
        assert_eq!(
            r.problem.as_deref(),
            Some("plugin.toml names card.lua, but the folder has no such file.")
        );
        assert!(r.snapshot.plugins.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}
