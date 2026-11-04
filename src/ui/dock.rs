// SPDX-License-Identifier: GPL-2.0-or-later
//! Dock: pinned apps, running dots from compositor clients, separator, the Downloads stack, Trash,
//! and the enabled plugins' `[[dock]]` entries (ADR 0031), in the order of the layout's
//! `dock_items`, on the layout's Dock edge.
use super::plugins;
use crate::apps;
use crate::kit::{
    self,
    anim::{Animated, Clock, Motion},
    motion::Spring,
    surface::Spec,
    widgets as w,
};
use gtk::{gio, glib, prelude::*};
use shared::layout::{DockItem, Edge, Layout};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use wol_core::downloads::{self, Item};

/// Newest files the Downloads stack shows.
pub const STACK_ITEMS: usize = 12;

/// Downloads, as xdg-user-dirs names it.
pub fn downloads_dir() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let config = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{home}/.config"));
    let text = std::fs::read_to_string(format!("{config}/user-dirs.dirs")).ok();
    downloads::dir(text.as_deref(), &home)
}

fn file_icon(i: &Item) -> gtk::Image {
    if i.dir {
        return w::app_icon("folder", w::ICON_L);
    }
    let (ctype, _) = gio::content_type_guess(Some(i.name.as_str()), None::<&[u8]>);
    let img = gtk::Image::from_gicon(&gio::content_type_get_icon(&ctype));
    img.set_pixel_size(w::ICON_L);
    img
}

/// The stack's popover body (grid): newest files, a click opens one; Open in Files last.
pub fn stack_grid(items: &[Item], dir: &str, done: Rc<dyn Fn()>) -> gtk::Box {
    let b = w::vbox(w::S2);
    b.add_css_class("downloads-stack");
    b.append(&w::label("Downloads", "title"));
    if items.is_empty() {
        b.append(&w::label(crate::copy::DOCK_DOWNLOADS_EMPTY, "secondary"));
    }
    let grid = w::wrap_row(w::S2, 4);
    for i in items {
        let col = w::vbox(w::S1);
        col.append(&file_icon(i));
        let name = w::label(&i.name, "secondary");
        name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        name.set_max_width_chars(w::STACK_NAME_CHARS);
        name.add_css_class("may-ellipsize");
        col.append(&name);
        let btn = gtk::Button::builder()
            .child(&col)
            .tooltip_text(&i.name)
            .build();
        btn.add_css_class("stack-item");
        let (path, done) = (i.path.clone(), done.clone());
        btn.connect_clicked(move |_| {
            kit::spawn(&["gio".into(), "open".into(), path.clone()]);
            done();
        });
        grid.append(&btn);
    }
    b.append(&grid);
    let dir = dir.to_owned();
    let open = w::button("Open in Files", w::Kind::Default, move || {
        kit::spawn(&["nautilus".into(), dir.clone()]);
        done();
    });
    open.set_halign(gtk::Align::End);
    b.append(&open);
    b
}

/// Downloads tile: the popover lists the newest files, kept current by a directory monitor.
fn downloads_tile(dir: String, items: Rc<RefCell<Vec<Item>>>) -> gtk::Button {
    let pop = gtk::Popover::new();
    pop.add_css_class("dock-stack");
    let fill: Rc<dyn Fn()> = {
        let (pop, items, dir) = (pop.clone(), items.clone(), dir.clone());
        Rc::new(move || {
            let p2 = pop.clone();
            pop.set_child(Some(&stack_grid(
                &items.borrow(),
                &dir,
                Rc::new(move || p2.popdown()),
            )));
        })
    };
    let f = fill.clone();
    let p = pop.clone();
    let tile = w::DockTile::new("folder-download", "Downloads", move || {
        f();
        p.popup();
    });
    pop.set_parent(&tile.button);
    tile.button.connect_destroy(move |_| pop.unparent());
    tile.button
}

/// Reads Downloads off the UI thread now and whenever it changes.
fn watch_downloads(dir: &str, items: Rc<RefCell<Vec<Item>>>) -> Option<gio::FileMonitor> {
    let refresh: Rc<dyn Fn()> = {
        let dir = dir.to_owned();
        Rc::new(move || {
            let (tx, rx) = async_channel::bounded(1);
            let d = dir.clone();
            std::thread::spawn(move || {
                let _ = tx.send_blocking(downloads::read(&d, STACK_ITEMS));
            });
            let items = items.clone();
            glib::spawn_future_local(async move {
                if let Ok(v) = rx.recv().await {
                    *items.borrow_mut() = v;
                }
            });
        })
    };
    refresh();
    let m = gio::File::for_path(dir)
        .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
        .ok()?;
    m.connect_changed(move |_, _, _, _| refresh());
    Some(m)
}

/// Seconds a launched icon bounces at most when no window shows up.
pub const LAUNCH_TIMEOUT_SECS: u32 = 15;

/// What a click on a pinned app does: a running app comes forward, anything else launches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Click {
    Launch,
    /// `focuswindow` argument naming the app's windows by class (minimised ones come back to the
    /// current desktop: fork patch 0008).
    Focus(String),
}

pub fn click(p: &apps::Pin, running: bool) -> Click {
    if running {
        Click::Focus(focus_target(p))
    } else {
        Click::Launch
    }
}

/// Compositor window regex (RE2) for the pin's classes, the app-id and its program, matched
/// case-insensitively like `apps::is_running` (app-ids are mixed case, Xwayland classes vary).
pub fn focus_target(p: &apps::Pin) -> String {
    let escape = |s: &str| -> String {
        s.chars()
            .flat_map(|c| {
                if ".+*?()[]{}|^$\\".contains(c) {
                    vec!['\\', c]
                } else {
                    vec![c]
                }
            })
            .collect()
    };
    let mut alts = vec![escape(p.id)];
    if !p.class.is_empty() {
        // A web app: its windows' own class, never the browser's (`apps::is_running`).
        alts.push(escape(p.class));
    } else if let Some(prog) = p.exec.first().map(|e| e.rsplit('/').next().unwrap_or(e))
        && !prog.eq_ignore_ascii_case(p.id)
    {
        alts.push(escape(prog));
    }
    // A Wolkit window that asked for glass carries the suffix on its class (`appmenu::GLASS_SUFFIX`).
    format!("class:^(?i)({})(\\.glass)?$", alts.join("|"))
}

/// A tile click: a running app comes forward; otherwise it launches and the icon bounces until
/// its first window ([`launched`]).
fn clicked(t: &w::DockTile, p: &apps::Pin) {
    match click(p, t.is_running()) {
        Click::Focus(target) => {
            // Off the UI thread: a stalled compositor froze the shell per click (audit2 A2-10).
            let d = crate::hypr::Dispatch::FocusWindow(target);
            glib::spawn_future_local(async move {
                let _ = gio::spawn_blocking(move || crate::hypr::dispatch(&d)).await;
            });
        }
        Click::Launch => {
            kit::spawn_app(&p.exec.iter().map(|s| s.to_string()).collect::<Vec<_>>());
            launched(t);
        }
    }
}

/// CSS margin above the shelf (`.dock-shelf` margin-top in kit.css): room for tiles to lift and
/// bounce, part of the shelf's measured height, never reserved from windows.
pub const SHELF_LIFT: i32 = 24;

/// Screen space the Dock reserves: the shelf and its bottom margin, from the shelf's natural
/// height (both CSS margins included) minus the lift room. Not the layer window (hot zone + lift
/// + shelf), which left a band between zoomed windows and the shelf. Autohide reserves nothing.
pub fn exclusive_zone(shelf_natural_height: i32, autohide: bool) -> i32 {
    if autohide {
        0
    } else {
        (shelf_natural_height - SHELF_LIFT).max(0)
    }
}

/// Clicking a pinned app that is not running bounces its icon.
pub fn bounces_on_click(running: bool) -> bool {
    !running
}

fn launched(t: &w::DockTile) {
    if !bounces_on_click(t.is_running()) {
        return;
    }
    t.set_launching(true);
    let weak = t.downgrade();
    glib::timeout_add_seconds_local_once(LAUNCH_TIMEOUT_SECS, move || {
        // Still bouncing: no window came up; the bounce and the dot go.
        if let Some(t) = weak().filter(w::DockTile::is_launching) {
            t.set_launching(false);
            t.set_running(false);
        }
    });
}

/// Seconds an app that asks for attention bounces at most (it stops sooner when focused).
pub const ATTENTION_MAX_SECS: u32 = 60;

/// The attention bounce ends: the app was focused, or it has bounced for the cap.
pub fn attention_over(elapsed_secs: u32, focused: bool) -> bool {
    focused || elapsed_secs >= ATTENTION_MAX_SECS
}

pub struct Dock {
    pub root: gtk::Box,
    /// The tiles of the layout's items, rebuilt when `dock_items` change ([`Dock::set_layout`]).
    shelf: RefCell<Shelf>,
    /// Newest files in Downloads (the stack).
    pub downloads: Rc<RefCell<Vec<Item>>>,
    /// Downloads, read once when the Dock is built (`user-dirs.dirs`).
    downloads_dir: String,
    monitor: RefCell<Option<gio::FileMonitor>>,
    /// Plugin entries (ADR 0031): one box at the place of the first plugin item.
    plugin_box: gtk::Box,
    plugin_tiles: RefCell<Vec<w::DockTile>>,
    tile_px: std::cell::Cell<Option<u32>>,
    /// The Dock's plugin host (None in previews).
    host: RefCell<Option<Rc<RefCell<plugins::Host>>>>,
    /// What the shelf shows now: its edge and items (`shell.json`, audit5 S8-1/S9-5).
    layout: RefCell<(Edge, Vec<DockItem>)>,
    /// The running classes last seen, for tiles built after them.
    running: RefCell<HashSet<String>>,
    /// The kept apps that are not pinned apps (`DockItem::App` resolved from the installed
    /// entries off the GTK thread, [`read_layout`]).
    kept: RefCell<Vec<apps::Pin>>,
    /// Keyboard access ([`keyboard`]): a tile click gives the keyboard back.
    keys: RefCell<Option<Keys>>,
    /// Hover magnify (Settings > Dock > Magnification).
    pub magnify: Rc<Magnify>,
}

/// Scale of the tile under the pointer.
pub const MAGNIFY: f32 = 68.0 / 60.0;
/// Scale of the two tiles beside it: half a fen of growth where the hovered one has a whole fen.
pub const NEIGHBOUR: f32 = 64.0 / 60.0;
/// The hover follows the pointer: a spring (interruptible, a quick pass keeps its speed).
pub const MAGNIFY_SPRING: Spring = Spring::SNAPPY;

/// Hover magnify: the tile under the pointer grows to [`MAGNIFY`] on a spring and the one it
/// left springs back. The tile's content scales about its bottom centre at draw time ([`Lift`]:
/// no relayout, the shelf keeps its size); kit.css's hover scale is off on the shelf (`flat`).
/// Under reduce motion the scale jumps.
pub struct Magnify {
    on: std::cell::Cell<bool>,
    /// Hovered tiles so far: their lift and its spring.
    tiles: RefCell<Vec<(glib::WeakRef<gtk::Button>, Lift, Animated)>>,
    hovered: RefCell<Option<glib::WeakRef<gtk::Button>>>,
    /// The tiles beside the hovered one, at [`NEIGHBOUR`].
    beside: RefCell<Vec<glib::WeakRef<gtk::Button>>>,
    /// An explicit clock (goldens); else the tile's window clock.
    clock: RefCell<Option<Clock>>,
}

impl Default for Magnify {
    fn default() -> Self {
        Magnify {
            on: std::cell::Cell::new(true),
            tiles: Default::default(),
            hovered: Default::default(),
            beside: Default::default(),
            clock: Default::default(),
        }
    }
}

impl Magnify {
    /// Moves run on `clock` (goldens and tests drive it by hand).
    pub fn use_clock(&self, clock: &Clock) {
        self.clock.replace(Some(clock.clone()));
    }

    pub fn is_on(&self) -> bool {
        self.on.get()
    }

    /// Off: every tile springs back and none grows.
    pub fn set_on(&self, on: bool) {
        self.on.set(on);
        if !on {
            self.hover(None);
        }
    }

    /// The pointer is over `tile` (None: over no tile, or gone).
    pub fn hover(&self, tile: Option<&gtk::Button>) {
        let tile = tile.filter(|_| self.on.get());
        let before = self.hovered.borrow().as_ref().and_then(|w| w.upgrade());
        if before.as_ref() == tile {
            return;
        }
        let near: Vec<gtk::Button> = tile.map(neighbours).unwrap_or_default();
        // What had a scale and has none now springs back.
        let old: Vec<gtk::Button> = before
            .into_iter()
            .chain(self.beside.borrow().iter().filter_map(|w| w.upgrade()))
            .collect();
        for b in old {
            if Some(&b) != tile && !near.contains(&b) {
                self.spring(&b, 1.0);
            }
        }
        for n in &near {
            self.spring(n, NEIGHBOUR);
        }
        if let Some(t) = tile {
            self.spring(t, MAGNIFY);
        }
        self.beside
            .replace(near.iter().map(|b| b.downgrade()).collect());
        self.hovered.replace(tile.map(|t| t.downgrade()));
    }

    /// The scale `tile` draws at now (1 for one never hovered).
    pub fn scale_of(&self, tile: &gtk::Button) -> f32 {
        self.tiles
            .borrow()
            .iter()
            .find(|(b, _, _)| b.upgrade().as_ref() == Some(tile))
            .map_or(1.0, |(_, l, _)| l.scale())
    }

    fn spring(&self, tile: &gtk::Button, to: f32) {
        let found = self
            .tiles
            .borrow()
            .iter()
            .find(|(b, _, _)| b.upgrade().as_ref() == Some(tile))
            .map(|(_, _, a)| a.clone());
        let a = match found {
            Some(a) => a,
            None if to == 1.0 => return,
            None => {
                let Some(lift) = Lift::wrap(tile) else { return };
                let clock = self
                    .clock
                    .borrow()
                    .clone()
                    .unwrap_or_else(|| Clock::of(tile));
                let a = {
                    let l = lift.clone();
                    Animated::on_clock(&clock, 1.0, move |v| l.set_scale(v))
                };
                let mut t = self.tiles.borrow_mut();
                t.retain(|(b, _, _)| b.upgrade().is_some());
                t.push((tile.downgrade(), lift, a.clone()));
                a
            }
        };
        a.retarget(to, Motion::Spring(MAGNIFY_SPRING));
    }
}

/// The tiles on each side of `tile` (separators and other widgets are skipped).
fn neighbours(tile: &gtk::Button) -> Vec<gtk::Button> {
    let step = |from: Option<gtk::Widget>, back: bool| {
        let mut at = from;
        while let Some(w) = at {
            if w.has_css_class("dock-tile") {
                return w.downcast::<gtk::Button>().ok();
            }
            at = if back {
                w.prev_sibling()
            } else {
                w.next_sibling()
            };
        }
        None
    };
    [
        step(tile.prev_sibling(), true),
        step(tile.next_sibling(), false),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The Dock tile under (`x`, `y`) of `root`.
fn tile_at(root: &gtk::Box, x: f64, y: f64) -> Option<gtk::Button> {
    let mut w = root.pick(x, y, gtk::PickFlags::DEFAULT);
    while let Some(x) = w {
        if x == *root.upcast_ref::<gtk::Widget>() {
            return None;
        }
        if x.has_css_class("dock-tile")
            && let Ok(b) = x.clone().downcast::<gtk::Button>()
        {
            return Some(b);
        }
        w = x.parent();
    }
    None
}

mod lift {
    use gtk::{glib, graphene, prelude::*, subclass::prelude::*};
    use std::cell::Cell;

    pub struct Lift {
        pub scale: Cell<f32>,
    }

    impl Default for Lift {
        fn default() -> Self {
            Lift {
                scale: Cell::new(1.0),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Lift {
        const NAME: &'static str = "WolDockLift";
        type Type = super::Lift;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_layout_manager_type::<gtk::BinLayout>();
        }
    }

    impl ObjectImpl for Lift {
        fn dispose(&self) {
            while let Some(c) = self.obj().first_child() {
                c.unparent();
            }
        }
    }

    impl WidgetImpl for Lift {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let w = self.obj();
            let Some(child) = w.first_child() else { return };
            let s = self.scale.get();
            if (s - 1.0).abs() < 1e-4 {
                return w.snapshot_child(&child, snapshot);
            }
            let (cw, ch) = (w.width() as f32, w.height() as f32);
            snapshot.save();
            snapshot.translate(&graphene::Point::new(cw / 2.0, ch));
            snapshot.scale(s, s);
            snapshot.translate(&graphene::Point::new(-cw / 2.0, -ch));
            w.snapshot_child(&child, snapshot);
            snapshot.restore();
        }
    }
}

glib::wrapper! {
    /// Draws its one child scaled about its bottom centre (a transform at draw time: layout and
    /// input stay where they are).
    pub struct Lift(ObjectSubclass<lift::Lift>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Lift {
    /// Puts a lift between `button` and its content (once; the existing one after that).
    pub fn wrap(button: &gtk::Button) -> Option<Lift> {
        let child = button.child()?;
        if let Ok(l) = child.clone().downcast::<Lift>() {
            return Some(l);
        }
        let l: Lift = glib::Object::new();
        button.set_child(None::<&gtk::Widget>);
        child.set_parent(&l);
        button.set_child(Some(&l));
        Some(l)
    }

    pub fn scale(&self) -> f32 {
        use gtk::subclass::prelude::ObjectSubclassIsExt;
        self.imp().scale.get()
    }

    pub fn set_scale(&self, s: f32) {
        use gtk::subclass::prelude::ObjectSubclassIsExt;
        if self.imp().scale.replace(s) != s {
            self.queue_draw();
        }
    }
}

/// The tiles built from the layout's items.
#[derive(Default)]
struct Shelf {
    tiles: Vec<w::DockTile>,
    /// The pin of each tile in `tiles` (same order).
    pins: Vec<apps::Pin>,
    /// Per tile: bumped on every start and stop, so a stale 60 s timer cannot stop a newer bounce.
    epochs: Vec<Rc<std::cell::Cell<u32>>>,
}

pub fn content(running: &HashSet<String>) -> Rc<Dock> {
    content_with(running, &Layout::default().dock_items, Edge::Bottom)
}

/// Where a Dock on `edge` sits along it: centred, against the edge.
pub fn align(edge: Edge) -> (gtk::Align, gtk::Align) {
    use gtk::Align as A;
    match edge {
        Edge::Bottom => (A::Center, A::End),
        Edge::Top => (A::Center, A::Start),
        Edge::Left => (A::Start, A::Center),
        Edge::Right => (A::End, A::Center),
    }
}

/// A Dock with `items` (the layout's `dock_items`, plugin items in place) on `edge`.
pub fn content_with(running: &HashSet<String>, items: &[DockItem], edge: Edge) -> Rc<Dock> {
    let root = gtk::Box::new(kit::surface::orientation(edge), w::S2);
    root.add_css_class("dock-shelf");
    root.add_css_class("no-row-check"); // tiles lift on hover by design
    let plugin_box = gtk::Box::new(kit::surface::orientation(edge), w::S2);
    plugin_box.set_visible(false);
    let d = Rc::new(Dock {
        root,
        shelf: Default::default(),
        downloads: Default::default(),
        downloads_dir: downloads_dir(),
        monitor: RefCell::new(None),
        plugin_box,
        plugin_tiles: RefCell::new(Vec::new()),
        tile_px: Default::default(),
        host: RefCell::new(None),
        layout: RefCell::new((edge, Vec::new())),
        running: RefCell::new(running.clone()),
        kept: RefCell::new(Vec::new()),
        keys: RefCell::new(None),
        magnify: Default::default(),
    });
    // The hover scale is the spring's (Magnify), not kit.css's.
    d.root.add_css_class("flat");
    {
        // Enter too: a pointer that lands on a tile (a warp, a window closing under it) moves
        // nothing before it is there.
        let motion = gtk::EventControllerMotion::new();
        let at = {
            let (m, root) = (d.magnify.clone(), d.root.downgrade());
            move |x: f64, y: f64| {
                if let Some(r) = root.upgrade() {
                    m.hover(tile_at(&r, x, y).as_ref());
                }
            }
        };
        let at2 = at.clone();
        motion.connect_enter(move |_, x, y| at2(x, y));
        motion.connect_motion(move |_, x, y| at(x, y));
        let m = d.magnify.clone();
        motion.connect_leave(move |_| m.hover(None));
        d.root.add_controller(motion);
    }
    d.root.add_controller(drop_target());
    d.set_edge(edge);
    d.fill(items);
    d
}

/// Labels of the Dock's own menu and its drop.
pub const KEEP_IN_DOCK: &str = "Keep in Dock";
pub const REMOVE_FROM_DOCK: &str = "Remove from Dock";

/// `shell.json` (its text now: `old`) with its Dock items changed by `change` (None: nothing to
/// do), saved through the change journal like a Settings change; the Dock (any process's) follows
/// through `ipc::watch_prefs`. Ok(false) when nothing changed. Writes a file: call it off the GTK
/// thread, like the read of `old` ([`shell_file`]).
pub fn change_items(
    old: Option<String>,
    change: impl FnOnce(&[DockItem]) -> Option<Vec<DockItem>>,
) -> std::io::Result<bool> {
    let mut l = wol_core::prefs::layout(old.as_deref());
    let Some(items) = change(&l.dock_items) else {
        return Ok(false);
    };
    l.dock_items = items;
    let text = wol_core::prefs::with_layout(old.as_deref(), &l).map_err(std::io::Error::other)?;
    wol_core::journal::Journal::for_config(std::path::Path::new(&kit::config_home())).save(
        "shell",
        "dock",
        &[(wol_core::prefs::SHELL_FILE, Some(&text))],
        wol_core::journal::now(),
    )?;
    Ok(true)
}

/// Keeps the installed app `id` (desktop id without `.desktop`) in the Dock, after the others;
/// `old` is `shell.json` now ([`change_items`]).
pub fn keep(old: Option<String>, id: &str) -> std::io::Result<bool> {
    change_items(old, |items| apps::keep_in_dock(items, apps::here(), id))
}

/// Takes the kept app `id` off the Dock (the app stays installed).
pub fn unkeep(old: Option<String>, id: &str) -> std::io::Result<bool> {
    change_items(old, |items| apps::remove_from_dock(items, id))
}

/// [`keep`] (or [`unkeep`]) off the GTK thread: reads `shell.json` and saves the change there.
pub fn keep_later(id: String, on: bool) {
    glib::spawn_future_local(async move {
        let _ = gio::spawn_blocking(move || {
            let old = std::fs::read_to_string(shell_file()).ok();
            if on { keep(old, &id) } else { unkeep(old, &id) }
        })
        .await;
    });
}

/// An app dropped on the shelf (a Search row, a `.desktop` file from Files) is kept
/// ([`apps::dropped_app`]); the save runs off the GTK thread.
fn drop_target() -> gtk::DropTarget {
    let t = gtk::DropTarget::new(glib::Type::INVALID, gtk::gdk::DragAction::COPY);
    t.set_types(&[String::static_type(), gtk::gdk::FileList::static_type()]);
    t.connect_drop(|_, v, _, _| dropped(v));
    t
}

/// What the shelf's drop does with `v` (a string or a file list): keeps the app it names, the
/// save off the GTK thread. False when it names none.
pub fn dropped(v: &glib::Value) -> bool {
    let text = if let Ok(s) = v.get::<String>() {
        s
    } else if let Ok(f) = v.get::<gtk::gdk::FileList>() {
        f.files()
            .first()
            .and_then(|f| f.path())
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    } else {
        String::new()
    };
    let Some(id) = apps::dropped_app(&text) else {
        return false;
    };
    keep_later(id, true);
    true
}

/// Right-click on a kept app's tile: Remove from Dock.
fn removable(button: &gtk::Button, id: &'static str) {
    let click = gtk::GestureClick::new();
    click.set_button(gtk::gdk::BUTTON_SECONDARY);
    let b = button.clone();
    click.connect_pressed(move |_, _, x, y| {
        kit::data::popup_menu(
            &b,
            x,
            y,
            vec![(
                REMOVE_FROM_DOCK.to_string(),
                None,
                Box::new(move || keep_later(id.to_string(), false)),
            )],
        );
    });
    button.add_controller(click);
}

/// How many of `items` are kept apps that are not pinned apps (they need the installed entries).
fn kept_count(items: &[DockItem]) -> usize {
    items
        .iter()
        .filter(|i| matches!(i, DockItem::App { id } if !apps::here().iter().any(|p| p.id == id)))
        .count()
}

/// The image of a Dock tile (its first child's first child).
fn tile_image(t: &w::DockTile) -> Option<gtk::Image> {
    t.button.child()?.first_child().and_downcast::<gtk::Image>()
}

/// The Dock window, its tiles, and its keyboard access (⌃F2/⌃F3 style, see [`keyboard`]).
/// Keyboard access: `keys(true)` takes the keyboard, `keys(false)` gives it back.
pub type Keys = Rc<dyn Fn(bool)>;

pub type Built = (gtk::ApplicationWindow, Rc<Dock>, Keys);

/// The layout in `shell.json` (`text`), as the Dock uses it: its edge (Bottom when the layout
/// names none) and its items. The menu bar reads the same file (`menubar::read_layout`).
pub fn layout_of(text: Option<&str>) -> (Layout, Edge) {
    let l = wol_core::prefs::layout(text);
    let edge = l.dock.unwrap_or(Edge::Bottom);
    (l, edge)
}

pub fn shell_file() -> std::path::PathBuf {
    std::path::Path::new(&kit::config_home()).join(wol_core::prefs::SHELL_FILE)
}

pub fn build(app: &gtk::Application) -> Built {
    // Built on the default layout; `shell.json` is read off the GTK thread right after (the first
    // `on_reload` below) and moves and refills the Dock like the menu bar (audit5 S8-1/S9-5).
    let edge = Edge::Bottom;
    let win = super::window(app, "dock", Spec::dock(edge));
    crate::kit::surface::survive_output_loss(&win, Spec::dock(edge));
    // Running dots come from `ipc::hypr_events` (its reseed reads the clients off the UI thread).
    let d = content_with(&HashSet::new(), &Layout::default().dock_items, edge);
    let layout = Rc::new(RefCell::new(Layout::default()));
    *d.monitor.borrow_mut() = watch_downloads(&d.downloads_dir, d.downloads.clone());
    // Autohide: the shelf slides away and a thin hot zone at the Dock's edge brings it back.
    let reveal = gtk::Revealer::builder()
        .child(&d.root)
        .transition_type(kit::surface::reveal_from(edge))
        .reveal_child(true)
        .build();
    let zone = w::vbox(0);
    zone.add_css_class("dock-hotzone");
    zone.append(&reveal);
    let hide = Rc::new(std::cell::Cell::new(false));
    let delay = Rc::new(std::cell::Cell::new(0u32));
    let epoch = Rc::new(std::cell::Cell::new(0u32));
    let motion = gtk::EventControllerMotion::new();
    {
        let (r, h) = (reveal.clone(), hide.clone());
        let (r2, dl, ep, ep2) = (r.clone(), delay.clone(), epoch.clone(), epoch.clone());
        motion.connect_enter(move |_, _, _| show_after(&r2, dl.get(), &ep));
        motion.connect_leave(move |_| {
            ep2.set(ep2.get().wrapping_add(1));
            if h.get() {
                r.set_reveal_child(false)
            }
        });
    }
    zone.add_controller(motion);
    win.set_child(Some(&zone));
    let keys = keyboard(&win, &d, &reveal, &hide);
    // Reserve the shelf, not the layer window: zoomed windows end at the shelf ([`exclusive_zone`]).
    // Measured across the shelf (its height on the bottom, its width on a side) when it maps (CSS
    // applied), after a tile size change and after a layout change.
    let reserve: Rc<dyn Fn()> = {
        let (win, dd, hide) = (win.clone(), Rc::downgrade(&d), hide.clone());
        Rc::new(move || {
            let Some(d) = dd.upgrade() else { return };
            let (_, natural, _, _) = d.root.measure(across(d.edge()), -1);
            kit::surface::set_exclusive_px(&win, exclusive_zone(natural, hide.get()));
        })
    };
    {
        let r = reserve.clone();
        d.root.connect_map(move |_| r());
    }
    {
        // Plugin entries: loaded off the GTK thread, again when the plugin folder changes.
        let host = Rc::new(RefCell::new(plugins::Host::new(
            plugins::Paths::user(),
            &[],
        )));
        let (dd, h, l) = (Rc::downgrade(&d), Rc::downgrade(&host), layout.clone());
        plugins::watch(
            &host,
            &kit::lifetime::Lifetime::of_window(&win),
            Rc::new(move || {
                if let (Some(d), Some(h)) = (dd.upgrade(), h.upgrade()) {
                    d.set_plugins(&h.borrow(), &l.borrow());
                }
            }),
        );
        d.host.replace(Some(host));
    }
    {
        let d = d.clone();
        let (w, rv) = (win.downgrade(), reveal.downgrade());
        super::ipc::on_reload(move |p| {
            read_layout(&d, &layout, w.clone(), rv.clone(), reserve.clone());
            hide.set(p.dock_autohide);
            delay.set(p.autohide_delay_ms());
            d.set_tile_size(p.tile_size());
            reveal.set_reveal_child(!p.dock_autohide);
            reserve();
            set_magnify(&d, p.dock_magnify);
        });
    }
    (win, d, keys)
}

/// Reads `shell.json` off the GTK thread (at start and on every reload) and applies it like the
/// menu bar: a new Dock edge moves the window and turns the shelf, new `dock_items` rebuild it,
/// and the plugin entries take their places in the new order (audit5 S8-1/S9-5).
fn read_layout(
    d: &Rc<Dock>,
    layout: &Rc<RefCell<Layout>>,
    win: glib::WeakRef<gtk::ApplicationWindow>,
    reveal: glib::WeakRef<gtk::Revealer>,
    reserve: Rc<dyn Fn()>,
) {
    let (d, layout) = (Rc::downgrade(d), layout.clone());
    glib::spawn_future_local(async move {
        let (text, kept) = gio::spawn_blocking(|| {
            let text = std::fs::read_to_string(shell_file()).ok();
            let items = layout_of(text.as_deref()).0.dock_items;
            let installed = if kept_count(&items) > 0 {
                crate::desktop::load_all()
            } else {
                Vec::new()
            };
            (text, apps::kept_pins(&items, apps::here(), &installed))
        })
        .await
        .unwrap_or_default();
        let (l, edge) = layout_of(text.as_deref());
        let Some(d) = d.upgrade() else { return };
        let kept_changed = d.set_kept(kept);
        if d.edge() != edge {
            if let Some(w) = win.upgrade() {
                kit::surface::reanchor(&w, Spec::dock(edge));
            }
            if let Some(r) = reveal.upgrade() {
                r.set_transition_type(kit::surface::reveal_from(edge));
            }
        }
        let moved = d.edge() != edge;
        let rebuilt = d.set_layout(edge, &l.dock_items) || (kept_changed && d.refill());
        let reorder = rebuilt || *layout.borrow() != l;
        *layout.borrow_mut() = l;
        if reorder && let Some(h) = d.host.borrow().clone() {
            d.set_plugins(&h.borrow(), &layout.borrow());
        }
        if moved || rebuilt {
            reserve();
        }
    });
}

/// ⌃F3 (`wol shell show dock`): the Dock shows and takes the keyboard, its first entry focused;
/// arrows move, Return opens the app and gives the keyboard back, as does Esc. The shelf, not
/// one tile, takes the focus: GTK hands it to its first button, also after a rebuild.
fn keyboard(
    win: &gtk::ApplicationWindow,
    d: &Dock,
    reveal: &gtk::Revealer,
    hide: &Rc<std::cell::Cell<bool>>,
) -> Keys {
    let (reveal, hide) = (reveal.clone(), hide.clone());
    // Autohide: shown while it has the keyboard.
    let keys = kit::surface::keyboard_access(win, &d.root, move |on| {
        reveal.set_reveal_child(on || !hide.get())
    });
    give_back_on_click(&d.root, &keys);
    d.keys.replace(Some(keys.clone()));
    keys
}

/// The Dock shows once the pointer has rested at the edge for `delay_ms` (0: at once). Leaving
/// bumps `epoch`, which cancels a show still waiting.
fn show_after(reveal: &gtk::Revealer, delay_ms: u32, epoch: &Rc<std::cell::Cell<u32>>) {
    let mine = epoch.get().wrapping_add(1);
    epoch.set(mine);
    if delay_ms == 0 {
        return reveal.set_reveal_child(true);
    }
    let (r, e) = (reveal.downgrade(), epoch.clone());
    glib::timeout_add_local_once(
        std::time::Duration::from_millis(u64::from(delay_ms)),
        move || {
            if let (Some(r), true) = (r.upgrade(), e.get() == mine) {
                r.set_reveal_child(true)
            }
        },
    );
}

/// Settings > Dock > Magnification: tiles lift on hover only when on ([`Magnify`]).
pub fn set_magnify(d: &Dock, on: bool) {
    d.magnify.set_on(on);
}

impl Dock {
    /// Settings > Dock > Size.
    pub fn set_tile_size(&self, px: u32) {
        self.tile_px.set(Some(px));
        let shelf = self.shelf.borrow();
        for t in shelf.tiles.iter().chain(self.plugin_tiles.borrow().iter()) {
            t.set_icon_size(px as i32);
        }
    }

    /// The edge the shelf sits on now.
    pub fn edge(&self) -> Edge {
        self.layout.borrow().0
    }

    /// The layout's items the shelf shows now.
    pub fn items(&self) -> Vec<DockItem> {
        self.layout.borrow().1.clone()
    }

    /// Turns the shelf for a Dock on `edge`: its direction, its place, and the separators across it.
    pub fn set_edge(&self, edge: Edge) {
        self.layout.borrow_mut().0 = edge;
        let orientation = kit::surface::orientation(edge);
        self.root.set_orientation(orientation);
        self.plugin_box.set_orientation(orientation);
        let (h, v) = align(edge);
        self.root.set_halign(h);
        self.root.set_valign(v);
        let mut c = self.root.first_child();
        while let Some(w) = c {
            if let Some(sep) = w.downcast_ref::<gtk::Separator>() {
                sep.set_orientation(across(edge));
            }
            c = w.next_sibling();
        }
    }

    /// A new layout from `shell.json` (audit5 S8-1/S9-5): the shelf turns to `edge`, and is built
    /// again from `items` when they changed. True when the items were rebuilt.
    pub fn set_layout(&self, edge: Edge, items: &[DockItem]) -> bool {
        if self.edge() != edge {
            self.set_edge(edge);
        }
        if self.layout.borrow().1 == items {
            return false;
        }
        self.fill(items);
        true
    }

    /// The kept apps that are not pinned apps; true when they changed (the shelf needs a
    /// [`Dock::refill`]).
    pub fn set_kept(&self, kept: Vec<apps::Pin>) -> bool {
        if *self.kept.borrow() == kept {
            return false;
        }
        self.kept.replace(kept);
        true
    }

    /// Builds the shelf again from the items it shows. Always true.
    pub fn refill(&self) -> bool {
        let items = self.items();
        self.fill(&items);
        true
    }

    /// The pinned and kept apps the shelf shows now, in order.
    pub fn pins(&self) -> Vec<apps::Pin> {
        self.shelf.borrow().pins.clone()
    }

    /// Builds the shelf from `items`, in their order, on the current edge: the pinned apps, the
    /// separators, the Downloads stack, Trash, and the plugin box at the first plugin item.
    fn fill(&self, items: &[DockItem]) {
        while let Some(c) = self.root.first_child() {
            self.root.remove(&c);
        }
        let first_plugin = items
            .iter()
            .position(|i| matches!(i, DockItem::Plugin { .. }));
        let mut shelf = Shelf::default();
        let root = &self.root;
        let kept = self.kept.borrow();
        let mut pin = |p: &apps::Pin, is_kept: bool| {
            if shelf.pins.iter().any(|q| q.id == p.id) {
                return;
            }
            let t = w::DockTile::new(p.icon, p.name, || {});
            if p.icon.starts_with('/')
                && let Some(img) = tile_image(&t)
            {
                // A web app's own icon file (wol_core::webapps::icon_path), not a theme name.
                img.set_from_file(Some(p.icon));
            }
            if is_kept {
                removable(&t.button, p.id);
            }
            let (tile, pp) = (t.clone(), p.clone());
            t.button.connect_clicked(move |_| clicked(&tile, &pp));
            root.append(&t.button);
            shelf.pins.push(p.clone());
            shelf.epochs.push(Default::default());
            shelf.tiles.push(t);
        };
        for (n, item) in items.iter().enumerate() {
            match item {
                DockItem::Apps => apps::here().iter().for_each(|p| pin(p, false)),
                DockItem::App { id } => {
                    if let Some(p) = apps::here().iter().find(|p| p.id == id) {
                        pin(p, false);
                    } else if let Some(p) = kept.iter().find(|p| p.id == id) {
                        pin(p, true);
                    }
                }
                DockItem::Separator => root.append(&gtk::Separator::new(across(self.edge()))),
                DockItem::Downloads => root.append(&downloads_tile(
                    self.downloads_dir.clone(),
                    self.downloads.clone(),
                )),
                DockItem::Trash => root.append(
                    &w::DockTile::new("wolos-trash", "Trash", || {
                        kit::spawn(
                            &apps::TRASH
                                .iter()
                                .map(|s| s.to_string())
                                .collect::<Vec<_>>(),
                        )
                    })
                    .button,
                ),
                DockItem::Plugin { .. } if Some(n) == first_plugin => root.append(&self.plugin_box),
                DockItem::Plugin { .. } => {}
            }
        }
        // No plugin item in the layout: the box sits after the apps (new entries go there).
        if self.plugin_box.parent().is_none() {
            let after = shelf
                .tiles
                .last()
                .map(|t| t.button.clone().upcast::<gtk::Widget>());
            root.insert_child_after(&self.plugin_box, after.as_ref());
        }
        if let Some(px) = self.tile_px.get() {
            for t in &shelf.tiles {
                t.set_icon_size(i32::try_from(px).unwrap_or(w::DOCK_TILE));
            }
        }
        {
            let running = self.running.borrow();
            for (p, t) in shelf.pins.iter().zip(&shelf.tiles) {
                t.set_running(apps::is_running(p, &running));
            }
        }
        self.shelf.replace(shelf);
        self.layout.borrow_mut().1 = items.to_vec();
        if let Some(k) = self.keys.borrow().clone() {
            give_back_on_click(&self.root, &k);
        }
    }

    /// The plugin entries of `host` in the order `layout` gives them (`plugins::Host::dock`).
    pub fn set_plugins(&self, host: &plugins::Host, layout: &Layout) {
        while let Some(c) = self.plugin_box.first_child() {
            self.plugin_box.remove(&c);
        }
        let all = host.dock_tiles();
        let tiles: Vec<w::DockTile> = host
            .dock(layout)
            .iter()
            .filter_map(|i| match i {
                DockItem::Plugin { plugin, id } => {
                    all.iter().find(|t| t.plugin == *plugin && t.id == *id)
                }
                _ => None,
            })
            .map(plugins::dock_tile)
            .collect();
        for t in &tiles {
            if let Some(px) = self.tile_px.get() {
                t.set_icon_size(i32::try_from(px).unwrap_or(w::DOCK_TILE));
            }
            self.plugin_box.append(&t.button);
        }
        self.plugin_box.set_visible(!tiles.is_empty());
        self.plugin_tiles.replace(tiles);
    }

    /// The tile of the app `class` belongs to, and its bounce epoch.
    fn tile_for(&self, class: &str) -> Option<(w::DockTile, Rc<std::cell::Cell<u32>>)> {
        let classes = HashSet::from([class.to_lowercase()]);
        let s = self.shelf.borrow();
        let i = s.pins.iter().position(|p| apps::is_running(p, &classes))?;
        Some((s.tiles[i].clone(), s.epochs[i].clone()))
    }

    /// A window of `class` asks for attention: its tile bounces until it is focused
    /// ([`Dock::focused`]) or [`ATTENTION_MAX_SECS`] pass.
    pub fn attention(&self, class: &str) {
        let Some((t, epochs)) = self.tile_for(class) else {
            return;
        };
        if t.is_attention() {
            return;
        }
        t.set_attention(true);
        let epoch = epochs.get().wrapping_add(1);
        epochs.set(epoch);
        let weak = t.downgrade();
        glib::timeout_add_seconds_local_once(ATTENTION_MAX_SECS, move || {
            if let (Some(t), true) = (weak(), epochs.get() == epoch) {
                t.set_attention(false);
            }
        });
    }

    /// The window of `class` came to the front: its bounce stops.
    pub fn focused(&self, class: &str) {
        for t in &self.shelf.borrow().tiles {
            t.set_focused(false);
        }
        if let Some((t, _)) = self.tile_for(class) {
            t.set_focused(true);
        }
        if let Some((t, epochs)) = self.tile_for(class).filter(|(t, _)| t.is_attention()) {
            epochs.set(epochs.get().wrapping_add(1));
            t.set_attention(false);
        }
    }

    pub fn set_running(&self, classes: &HashSet<String>) {
        self.running.replace(classes.clone());
        let s = self.shelf.borrow();
        for (p, t) in s.pins.iter().zip(&s.tiles) {
            t.set_running(apps::is_running(p, classes));
        }
    }
}

/// The direction across a shelf on `edge` (its separators).
fn across(edge: Edge) -> gtk::Orientation {
    match kit::surface::orientation(edge) {
        gtk::Orientation::Horizontal => gtk::Orientation::Vertical,
        _ => gtk::Orientation::Horizontal,
    }
}

/// Every button on the shelf gives the keyboard back when clicked (`keys(false)`).
fn give_back_on_click(root: &gtk::Box, keys: &Keys) {
    let mut c = root.first_child();
    while let Some(w) = c {
        if let Some(b) = w.downcast_ref::<gtk::Button>() {
            let k = keys.clone();
            b.connect_clicked(move |_| k(false));
        }
        c = w.next_sibling();
    }
}

#[cfg(test)]
mod launch_tests {
    use super::*;

    /// A launched app bounces until its first window maps (it shows as running), or the timeout.
    #[test]
    fn the_icon_bounces_until_the_app_runs() {
        assert!(bounces_on_click(false));
        assert!(!bounces_on_click(true), "a running app just comes forward");
        const { assert!(LAUNCH_TIMEOUT_SECS >= 5 && LAUNCH_TIMEOUT_SECS <= 30) };
        let css = include_str!("../kit/kit.css");
        let rule = css
            .lines()
            .find(|l| l.starts_with("button.dock-tile.launching"))
            .expect("a .launching rule");
        assert!(
            rule.contains("animation:") && rule.contains("infinite"),
            "{rule}"
        );
        assert!(css.contains("@keyframes dock-bounce"));
    }
}

#[cfg(test)]
mod click_tests {
    use super::*;

    /// Live: clicking the Dock icon of a running app started a second instance (WolOS brings
    /// the app forward). The focus target names every class the running dot accepts.
    #[test]
    fn a_click_focuses_a_running_app_and_launches_the_rest() {
        let files = apps::Pin {
            id: "org.gnome.Nautilus",
            name: "Files",
            icon: "wolos-files",
            exec: &["nautilus"],
            class: "",
        };
        assert_eq!(click(&files, false), Click::Launch);
        assert_eq!(
            click(&files, true),
            Click::Focus("class:^(?i)(org\\.gnome\\.Nautilus|nautilus)(\\.glass)?$".into())
        );
        let same = apps::Pin {
            id: "helium",
            name: "Browser",
            icon: "wolos-browser",
            exec: &["/usr/bin/helium"],
            class: "",
        };
        assert_eq!(
            focus_target(&same),
            "class:^(?i)(helium)(\\.glass)?$",
            "program = id: once"
        );
        for p in apps::PINS {
            let t = focus_target(p);
            assert!(
                t.starts_with("class:^(?i)(") && t.ends_with(")(\\.glass)?$"),
                "{t}"
            );
        }
    }

    /// Live: zoomed windows stopped 36 px above the shelf: the Dock reserved its whole layer
    /// window (hot zone, lift room, shelf). The zone is the shelf plus its bottom margin.
    #[test]
    fn the_dock_reserves_the_shelf_not_its_lift_room() {
        let css = include_str!("../kit/kit.css");
        let shelf = css
            .lines()
            .skip_while(|l| !l.starts_with(".dock-shelf {"))
            .take(3)
            .collect::<String>();
        // The CSS margin, top right bottom left, in pixels (units stripped: no px literals here).
        let margin: Vec<i32> = shelf
            .split(" margin:")
            .nth(1)
            .and_then(|m| m.split(';').next())
            .unwrap_or_default()
            .split_whitespace()
            .map(|v| {
                v.trim_end_matches(char::is_alphabetic)
                    .parse()
                    .unwrap_or(-1)
            })
            .collect();
        assert_eq!(margin, [SHELF_LIFT, 32, 8, 32], "{shelf}");
        assert_eq!(
            exclusive_zone(24 + 4 + 64 + 8, false),
            76,
            "shelf + bottom margin"
        );
        assert_eq!(exclusive_zone(100, true), 0, "autohide reserves nothing");
        assert_eq!(exclusive_zone(10, false), 0, "never negative");
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    /// audit5 S9-5: the Dock stayed bottom-centre with the default items while the bar followed
    /// `shell.json`. It now reads the same layout: a left Dock is anchored left, shelf vertical.
    #[test]
    fn the_dock_follows_the_layout_in_shell_json() {
        let l = Layout {
            dock: Some(Edge::Left),
            dock_items: vec![DockItem::Trash],
            ..Layout::default()
        };
        let text = wol_core::prefs::with_layout(None, &l).expect("a valid layout");
        let (got, edge) = layout_of(Some(&text));
        assert_eq!(edge, Edge::Left);
        assert_eq!(got.dock_items, [DockItem::Trash]);
        assert_eq!(
            Spec::dock(edge).anchor,
            [false, false, true, false],
            "left only"
        );
        assert_eq!(
            kit::surface::orientation(edge),
            gtk::Orientation::Vertical,
            "a vertical shelf"
        );
        assert_eq!(align(edge), (gtk::Align::Start, gtk::Align::Center));
        let (none, bottom) = layout_of(None);
        assert_eq!((none, bottom), (Layout::default(), Edge::Bottom));
    }
}

#[cfg(test)]
mod attention_tests {
    use super::*;

    /// An app asking for attention bounces until it is focused, or for at most 60 s.
    #[test]
    fn attention_bounce_lasts_until_focus_or_sixty_seconds() {
        assert_eq!(ATTENTION_MAX_SECS, 60);
        assert!(!attention_over(0, false));
        assert!(!attention_over(59, false));
        assert!(attention_over(60, false), "stops at the cap");
        assert!(attention_over(3, true), "focus ends it at once");
        let css = include_str!("../kit/kit.css");
        let rule = css
            .lines()
            .find(|l| l.starts_with("button.dock-tile.attention"))
            .expect("an .attention rule");
        assert!(rule.contains("infinite"), "{rule}: bounces until stopped");
    }
}
