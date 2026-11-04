// SPDX-License-Identifier: GPL-2.0-or-later
//! Desktop widgets (ADR 0010): a layer surface above the wallpaper and below windows, inside the
//! menu bar and Dock zones. Cards sit on a snapping grid saved in `widgets.json`; Edit Widgets
//! (desktop right-click, a card's menu, `wol shell show widgets`) raises the surface, shows remove
//! buttons and the gallery, and lets cards be dragged. Calm: one coalesced timer for every
//! widget, music and batteries are event-driven, and nothing runs while a fullscreen window
//! covers the desktop.
mod live_sources;
use super::widgets_builtin::{self as builtin, Data, SystemView, WeatherView};
use super::widgets_gallery::{Actions, Entry, Gallery};
use super::widgets_layout as lay;
use crate::kit::data as d;
use crate::kit::surface;
use crate::kit::widgets as w;
use crate::widgets::{
    self as wd, Now, calendar, grid::Grid, media, power, schedule::Schedule, timer, weather,
    worldclock,
};
use gtk::{gio, glib, prelude::*};
use serde_json::Value;
use shared::widgets::manifest::{Manifest, Source};
use shared::widgets::{
    Builtin, Desktop, Face, Kind, LUA_PREFIX, Options, Place, Placed, Size, Units,
};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use wol_core::widgets::Package;

/// What a card shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    Builtin(Data),
    Custom {
        manifest: Rc<Manifest>,
        dir: PathBuf,
        data: Value,
    },
    /// A sentence from `crate::copy`; `details` (paths, exit codes) only behind Show Details.
    Error {
        name: String,
        message: String,
        details: Vec<String>,
    },
    /// An http source the user has not allowed yet.
    Consent {
        name: String,
        host: String,
    },
    /// A Lua widget's tree (`ui::lua_widgets`).
    View {
        name: String,
        node: shared::view::Node,
    },
}

pub type Listener = Rc<dyn Fn(Change)>;
/// A card menu action, run with the board.
type BoardFn = Box<dyn Fn(&Rc<Board>)>;

/// Board -> provider notifications.
pub enum Change {
    Added(String),
    Removed(Box<Placed>),
    /// Size or options changed.
    Updated(String),
    /// The user allowed a widget's network host.
    Granted(String),
    Edit(bool),
    /// A Timer card's button.
    Timer(String, timer::Press),
    /// A button on a Lua widget: (file id, button id).
    Lua(String, u32),
    /// The whole layout was swapped (reload, a display came or went).
    Replaced,
}

struct Drag {
    id: String,
    x0: f64,
    y0: f64,
    target: (u16, u16),
}

/// Saves one display's part of the layout (`Desks`).
type Saver = Rc<dyn Fn(&Desktop)>;

struct St {
    file: Option<PathBuf>,
    desk: Desktop,
    grid: Grid,
    content: HashMap<String, Content>,
    cards: HashMap<String, gtk::Box>,
    removes: HashMap<String, gtk::Button>,
    packages: Vec<Rc<Package>>,
    edit: bool,
    drag: Option<Drag>,
    mark: Option<gtk::Box>,
}

/// The desktop: canvas of cards, drag and drop, menus, Edit Widgets and its gallery.
pub struct Board {
    pub root: gtk::Overlay,
    canvas: gtk::Fixed,
    pub gallery: Rc<Gallery>,
    st: RefCell<St>,
    listener: RefCell<Option<Listener>>,
    win: RefCell<Option<gtk::Window>>,
    /// The gallery's own Top-layer surface (`Spec::widgets_gallery`); None: it stays an overlay
    /// of `root` (goldens, dev). A desktop moved to Top at runtime stayed under windows (QA bug 5).
    gallery_win: RefCell<Option<gtk::Window>>,
    /// Gallery margins (start, end) from [`gallery_margins`], kept for a window attached later.
    gallery_side: Cell<(i32, i32)>,
    /// Edit Widgets turned on: the other transient panels close (`transient`).
    opened: RefCell<Option<Rc<dyn Fn()>>>,
    /// Saves through the per-display store (`Desks`) instead of writing `file` itself.
    saver: RefCell<Option<Saver>>,
    /// The display new widgets go on (its `shared::widgets::displays::Display::key`).
    stamp: RefCell<Option<String>>,
}

/// What the empty desktop paints: one pixel at 1/255 black, to everyone's eye nothing. A board
/// with no widget has nothing else to draw, and GTK never attaches a buffer to a window that
/// renders nothing: its frame clock then commits an empty layer surface every 16 ms, and Hyprland
/// answers each commit with a new `configure` and damage on the whole output (QA-100 H17-01: four
/// cores of compositor with one window open; one widget on the desktop stopped it). One drawn
/// pixel makes the first frame a real one, after which the surface is idle like any other. The
/// window stays mapped, so the bare desktop keeps its right-click menu and takes dropped files.
fn mark_presence(cr: &gtk::cairo::Context) {
    cr.rectangle(0.0, 0.0, 1.0, 1.0);
    cr.set_source_rgba(0.0, 0.0, 0.0, 1.0 / 255.0);
    let _ = cr.fill();
}

fn entries(packages: &[Rc<Package>]) -> Vec<Entry> {
    Builtin::ALL
        .into_iter()
        .map(Entry::Builtin)
        .chain(packages.iter().cloned().map(Entry::Package))
        .collect()
}

/// Sizes a placed widget may take.
fn sizes_of(kind: &Kind, packages: &[Rc<Package>], current: Size) -> Vec<Size> {
    match kind {
        Kind::Builtin(b) => builtin::sizes(*b).to_vec(),
        Kind::Custom(id) => packages
            .iter()
            .find(|p| &p.id == id)
            .and_then(|p| p.manifest.as_ref().ok())
            .map(|m| m.sizes.clone())
            .unwrap_or_else(|| vec![current]),
        Kind::Lua(_) => Size::ALL.to_vec(),
        Kind::Unknown(_) => vec![current],
    }
}

impl Board {
    pub fn new(desk: Desktop, file: Option<PathBuf>, packages: Vec<Package>) -> Rc<Board> {
        let packages: Vec<Rc<Package>> = packages.into_iter().map(Rc::new).collect();
        Rc::new_cyclic(|me: &Weak<Board>| {
            let m = me.clone();
            let probe = d::size_probe(move |wd, ht| {
                if let Some(b) = m.upgrade() {
                    b.resize(wd, ht);
                }
            });
            probe.set_draw_func(|_, cr, _, _| mark_presence(cr));
            let root = d::layers(&probe);
            let canvas = d::canvas();
            canvas.set_halign(gtk::Align::Fill);
            canvas.set_valign(gtk::Align::Fill);
            root.add_overlay(&canvas);
            let (m1, m2, m3, m4) = (me.clone(), me.clone(), me.clone(), me.clone());
            let gallery = Gallery::new(
                entries(&packages),
                Actions {
                    add: Box::new(move |k, s, p| m1.upgrade().map_or(Ok(()), |b| b.add(k, s, p))),
                    set_place: Box::new(move |id, p| {
                        if let Some(b) = m2.upgrade() {
                            b.set_place(id, p);
                        }
                    }),
                    done: Box::new(move || {
                        if let Some(b) = m3.upgrade() {
                            b.set_edit(false);
                        }
                    }),
                    search: Box::new(move |q, done| {
                        if m4.upgrade().is_some() {
                            search_places(q, done);
                        }
                    }),
                },
            );
            gallery.root.set_visible(false);
            root.add_overlay(&gallery.root);
            let grid = Grid::new(1, 1, d::card_metrics());
            Board {
                root,
                canvas,
                gallery,
                opened: RefCell::new(None),
                st: RefCell::new(St {
                    file,
                    desk,
                    grid,
                    content: HashMap::new(),
                    cards: HashMap::new(),
                    removes: HashMap::new(),
                    packages,
                    edit: false,
                    drag: None,
                    mark: None,
                }),
                listener: RefCell::new(None),
                win: RefCell::new(None),
                gallery_win: RefCell::new(None),
                gallery_side: Cell::new((0, 0)),
                saver: RefCell::new(None),
                stamp: RefCell::new(None),
            }
        })
        .wired()
    }

    fn wired(self: Rc<Self>) -> Rc<Self> {
        // Right-click on the bare desktop.
        let menu = gtk::GestureClick::new();
        menu.set_button(gtk::gdk::BUTTON_SECONDARY);
        let me = Rc::downgrade(&self);
        menu.connect_pressed(move |_, _, x, y| {
            let Some(b) = me.upgrade() else { return };
            let edit = b.st.borrow().edit;
            let m2 = Rc::downgrade(&b);
            d::popup_menu(
                &b.canvas,
                x,
                y,
                vec![
                    (
                        if edit {
                            "Done Editing"
                        } else {
                            "Edit Widgets…"
                        }
                        .to_string(),
                        None,
                        Box::new(move || {
                            if let Some(b) = m2.upgrade() {
                                b.set_edit(!edit);
                            }
                        }),
                    ),
                    (
                        "Change Wallpaper…".to_string(),
                        None,
                        Box::new(|| {
                            crate::kit::spawn(&["wol-settings".into(), "Wallpaper".into()])
                        }),
                    ),
                ],
            );
        });
        self.canvas.add_controller(menu);
        // Edit Widgets: drag a card to another cell.
        let drag = gtk::GestureDrag::new();
        let me = Rc::downgrade(&self);
        drag.connect_drag_begin(move |g, x, y| {
            if let Some(b) = me.upgrade()
                && !b.drag_begin(x, y)
            {
                g.set_state(gtk::EventSequenceState::Denied);
            }
        });
        let me = Rc::downgrade(&self);
        drag.connect_drag_update(move |_, dx, dy| {
            if let Some(b) = me.upgrade() {
                b.drag_update(dx, dy);
            }
        });
        let me = Rc::downgrade(&self);
        drag.connect_drag_end(move |_, _, _| {
            if let Some(b) = me.upgrade() {
                b.drag_end();
            }
        });
        self.canvas.add_controller(drag);
        self
    }

    pub fn set_listener(&self, f: Listener) {
        *self.listener.borrow_mut() = Some(f);
    }

    pub fn set_window(&self, win: &gtk::Window) {
        *self.win.borrow_mut() = Some(win.clone());
    }

    /// Moves the gallery out of the desktop overlay into `win`, its own surface above windows:
    /// Edit Widgets presents it, Done hides it, the desktop stays on the Bottom layer.
    pub fn set_gallery_window(&self, win: &gtk::Window) {
        self.root.remove_overlay(&self.gallery.root);
        self.gallery.root.set_visible(true);
        win.set_child(Some(&self.gallery.root));
        *self.gallery_win.borrow_mut() = Some(win.clone());
        let (start, end) = self.gallery_side.get();
        self.side_gallery(start, end);
        input_only_on(win, &self.gallery.root);
    }

    /// Puts the gallery `start`/`end` px in from the desktop's sides: the layer margins of its
    /// own surface, else the overlay child's margins.
    fn side_gallery(&self, start: i32, end: i32) {
        self.gallery_side.set((start, end));
        match self.gallery_win.borrow().as_ref() {
            Some(win) => gallery_layer_margins(win, start, end),
            None => {
                self.gallery.root.set_margin_start(start);
                self.gallery.root.set_margin_end(end);
            }
        }
    }

    /// Shows or hides the gallery; its own surface is presented (mapped and raised, on the
    /// desktop's display) so it comes up above an app window such as Settings.
    fn show_gallery(&self, on: bool) {
        let Some(gw) = self.gallery_win.borrow().clone() else {
            self.gallery.root.set_visible(on);
            return;
        };
        if !on {
            gw.set_visible(false);
            return;
        }
        let mon = self
            .win
            .borrow()
            .as_ref()
            .and_then(surface::connector_of)
            .and_then(|c| {
                super::displays::monitors()
                    .into_iter()
                    .find(|(k, _)| *k == c)
                    .map(|(_, m)| m)
            });
        if mon.is_some() {
            surface::set_monitor(&gw, mon.as_ref());
        }
        gw.set_visible(true);
        gw.present();
    }

    fn notify(&self, c: Change) {
        let l = self.listener.borrow().clone();
        if let Some(l) = l {
            l(c);
        }
    }

    pub fn widgets(&self) -> Vec<Placed> {
        self.st.borrow().desk.widgets.clone()
    }

    pub fn desktop(&self) -> Desktop {
        self.st.borrow().desk.clone()
    }

    pub fn packages(&self) -> Vec<Rc<Package>> {
        self.st.borrow().packages.clone()
    }

    pub fn is_editing(&self) -> bool {
        self.st.borrow().edit
    }

    /// Saves go through `f` (one display's part of the layout).
    pub fn set_saver(&self, f: Rc<dyn Fn(&Desktop)>) {
        self.saver.replace(Some(f));
    }

    /// New widgets are placed on display `key`.
    pub fn set_stamp(&self, key: Option<String>) {
        self.stamp.replace(key);
    }

    fn save(&self) {
        let saver = self.saver.borrow().clone();
        if let Some(f) = saver {
            let desk = self.st.borrow().desk.clone();
            return f(&desk);
        }
        let st = self.st.borrow();
        if let Some(f) = &st.file
            && let Err(e) = wd::save(f, &st.desk)
        {
            eprintln!("wol-shell: {}: {e}", f.display());
        }
    }

    /// The work area changed size: recompute the grid and place every card.
    pub fn resize(self: &Rc<Self>, wd: i32, ht: i32) {
        {
            let mut st = self.st.borrow_mut();
            let g = Grid::new(wd, ht, d::card_metrics());
            if g == st.grid {
                return;
            }
            st.grid = g;
        }
        self.layout();
    }

    /// Display positions (the saved layout, moved inward on a smaller screen).
    fn shown(&self) -> Vec<Placed> {
        let st = self.st.borrow();
        let at = st.grid.arrange(&st.desk.widgets);
        st.desk
            .widgets
            .iter()
            .zip(at)
            .map(|(p, (col, row))| Placed {
                col,
                row,
                ..p.clone()
            })
            .collect()
    }

    /// Puts every card at its cell; builds cards that do not exist yet.
    fn layout(self: &Rc<Self>) {
        let shown = self.shown();
        let ids: HashSet<String> = shown.iter().map(|p| p.id.clone()).collect();
        let stale: Vec<String> = self
            .st
            .borrow()
            .cards
            .keys()
            .filter(|k| !ids.contains(*k))
            .cloned()
            .collect();
        for id in stale {
            self.drop_card(&id);
        }
        for p in &shown {
            if !self.st.borrow().cards.contains_key(&p.id) {
                self.build_card(p);
            }
            self.place(p);
        }
        self.clear_gallery(&shown);
    }

    /// Keeps the gallery in the free side of the desktop, so it never covers a card.
    fn clear_gallery(&self, shown: &[Placed]) {
        let g = self.st.borrow().grid;
        let total = 2 * g.m.margin + g.cols as i32 * g.m.pitch() - g.m.gap;
        let spans = shown.iter().map(|p| {
            let (x, _) = g.m.origin(p.col, p.row);
            (x, x + g.m.size(p.size).0)
        });
        let (start, end) = gallery_margins(spans, total, g.m.margin);
        self.side_gallery(start, end);
    }

    fn origin(&self, p: &Placed) -> (f64, f64) {
        let (x, y) = self.st.borrow().grid.m.origin(p.col, p.row);
        (x as f64, y as f64)
    }

    fn place(&self, p: &Placed) {
        let (x, y) = self.origin(p);
        let st = self.st.borrow();
        if let Some(c) = st.cards.get(&p.id) {
            if c.parent().is_some() {
                self.canvas.move_(c, x, y);
            } else {
                self.canvas.put(c, x, y);
            }
        }
        if let Some(r) = st.removes.get(&p.id) {
            let off = (w::S2 as f64) - (w::S4 as f64);
            if r.parent().is_some() {
                self.canvas.move_(r, x + off, y + off);
            } else {
                self.canvas.put(r, x + off, y + off);
            }
            r.insert_before(&self.canvas, None::<&gtk::Widget>);
        }
    }

    fn drop_card(&self, id: &str) {
        let mut st = self.st.borrow_mut();
        if let Some(c) = st.cards.remove(id) {
            self.canvas.remove(&c);
        }
        if let Some(r) = st.removes.remove(id) {
            self.canvas.remove(&r);
        }
    }

    fn render(self: &Rc<Self>, p: &Placed) -> gtk::Box {
        let st = self.st.borrow();
        let c = match (st.content.get(&p.id), &p.kind) {
            (Some(Content::Builtin(data)), _) => {
                let me = Rc::downgrade(self);
                let id = p.id.clone();
                let (me2, id2) = (me.clone(), id.clone());
                let hooks = builtin::Hooks {
                    choose_city: Rc::new(move || {
                        if let Some(b) = me.upgrade() {
                            b.choose_city(&id);
                        }
                    }),
                    timer: Rc::new(move |press| {
                        if let Some(b) = me2.upgrade() {
                            b.notify(Change::Timer(id2.clone(), press));
                        }
                    }),
                    note: (!st.edit).then(|| self.note_hooks(&p.id)),
                };
                builtin::card(data, p.size, &hooks)
            }
            (
                Some(Content::Custom {
                    manifest,
                    dir,
                    data,
                }),
                Kind::Custom(pkg),
            ) => {
                let cache = wd::cache_home().join("wolos/widgets").join(pkg);
                let cx = lay::Ctx {
                    dir,
                    cache: &cache,
                    interactive: !st.edit,
                };
                lay::card(manifest, p.size, data, &cx)
            }
            (
                Some(Content::Error {
                    name,
                    message,
                    details,
                }),
                _,
            ) => lay::error_card(p.size, name, message, details),
            (Some(Content::View { node, .. }), Kind::Lua(file)) => {
                let (me, file) = (Rc::downgrade(self), file.clone());
                super::lua_widgets::card(p.size, node, !st.edit, move |button| {
                    if let Some(b) = me.upgrade() {
                        b.notify(Change::Lua(file.clone(), button));
                    }
                })
            }
            (Some(Content::Consent { name, host }), _) => {
                let me = Rc::downgrade(self);
                let id = p.id.clone();
                lay::consent_card(p.size, name, host, move || {
                    if let Some(b) = me.upgrade() {
                        b.grant(&id);
                    }
                })
            }
            // Loading: the empty glass, so nothing jumps when data arrives.
            _ => d::card(p.size, &d::fill()),
        };
        if st.edit
            && let Some(body) = c.first_child()
        {
            body.set_can_target(false);
        }
        c
    }

    fn build_card(self: &Rc<Self>, p: &Placed) {
        let c = self.render(p);
        let click = gtk::GestureClick::new();
        click.set_button(gtk::gdk::BUTTON_SECONDARY);
        let (me, id) = (Rc::downgrade(self), p.id.clone());
        click.connect_pressed(move |g, _, x, y| {
            g.set_state(gtk::EventSequenceState::Claimed);
            if let Some(b) = me.upgrade() {
                b.card_menu(&id, x, y);
            }
        });
        c.add_controller(click);
        let edit = self.st.borrow().edit;
        let mut st = self.st.borrow_mut();
        st.cards.insert(p.id.clone(), c);
        if edit && !st.removes.contains_key(&p.id) {
            let r = d::corner_button("list-remove-symbolic", "Remove Widget");
            let (me, id) = (Rc::downgrade(self), p.id.clone());
            r.connect_clicked(move |_| {
                if let Some(b) = me.upgrade() {
                    b.remove(&id);
                }
            });
            st.removes.insert(p.id.clone(), r);
        }
    }

    /// Rebuilds one card (new data, size or mode) where it stands.
    fn refresh_card(self: &Rc<Self>, id: &str) {
        let Some(p) = self.shown().into_iter().find(|p| p.id == id) else {
            return;
        };
        if let Some(old) = self.st.borrow_mut().cards.remove(id) {
            self.canvas.remove(&old);
        }
        self.build_card(&p);
        self.place(&p);
    }

    /// New content for a widget; unchanged content changes nothing on screen.
    pub fn set_content(self: &Rc<Self>, id: &str, c: Content) {
        {
            let mut st = self.st.borrow_mut();
            if st.content.get(id) == Some(&c) {
                return;
            }
            st.content.insert(id.to_string(), c);
        }
        if self.st.borrow().drag.as_ref().is_some_and(|d| d.id == id) {
            return;
        }
        self.refresh_card(id);
    }

    pub fn content(&self, id: &str) -> Option<Content> {
        self.st.borrow().content.get(id).cloned()
    }

    pub fn add(
        self: &Rc<Self>,
        kind: Kind,
        size: Size,
        place: Option<Place>,
    ) -> Result<(), String> {
        let shown = self.shown();
        let id = {
            let mut st = self.st.borrow_mut();
            let Some((col, row)) = st.grid.free_spot(&shown, None, size, (0, 0)) else {
                return Err(crate::copy::WIDGET_NO_ROOM.into());
            };
            let id = st.desk.new_id(&kind);
            let mut options = Options::default();
            if kind == Kind::Builtin(Builtin::Weather) {
                options.place = place;
            }
            st.desk.widgets.push(Placed {
                id: id.clone(),
                kind,
                size,
                col,
                row,
                options,
                display: self.stamp.borrow().clone(),
            });
            id
        };
        self.save();
        self.layout();
        self.notify(Change::Added(id));
        Ok(())
    }

    pub fn remove(self: &Rc<Self>, id: &str) {
        let removed = {
            let mut st = self.st.borrow_mut();
            st.content.remove(id);
            st.desk.remove(id)
        };
        self.drop_card(id);
        self.save();
        if let Some(p) = removed {
            self.notify(Change::Removed(Box::new(p)));
        }
    }

    fn update(self: &Rc<Self>, id: &str, f: impl FnOnce(&mut Placed, &Grid, &[Placed])) {
        let shown = self.shown();
        {
            let mut st = self.st.borrow_mut();
            let grid = st.grid;
            let Some(p) = st.desk.get_mut(id) else { return };
            f(p, &grid, &shown);
        }
        self.save();
        self.layout();
        self.refresh_card(id);
        self.notify(Change::Updated(id.to_string()));
    }

    fn set_size(self: &Rc<Self>, id: &str, size: Size) {
        self.update(id, |p, grid, shown| {
            let at = shown
                .iter()
                .find(|s| s.id == p.id)
                .map_or((p.col, p.row), |s| (s.col, s.row));
            let spot = if grid.is_free(shown, Some(&p.id), at.0, at.1, size) {
                Some(at)
            } else {
                grid.free_spot(shown, Some(&p.id), size, at)
            };
            if let Some((c, r)) = spot {
                p.size = size;
                (p.col, p.row) = (c, r);
            }
        });
    }

    pub fn set_place(self: &Rc<Self>, id: &str, place: Place) {
        self.update(id, |p, _, _| p.options.place = Some(place));
    }

    /// A note on the desktop: its text saved without rebuilding the card (the caret stays), and
    /// the desktop surface taking the keyboard while it is written in.
    fn note_hooks(self: &Rc<Self>, id: &str) -> builtin::NoteHooks {
        let (me, id) = (Rc::downgrade(self), id.to_string());
        let me2 = me.clone();
        builtin::NoteHooks {
            text: Rc::new(move |t| {
                let Some(b) = me.upgrade() else { return };
                let changed = {
                    let mut st = b.st.borrow_mut();
                    match st.desk.get_mut(&id) {
                        Some(p) if p.options.text.as_deref().unwrap_or("") != t => {
                            p.options.text = (!t.is_empty()).then_some(t);
                            true
                        }
                        _ => false,
                    }
                };
                if changed {
                    b.save();
                }
            }),
            keyboard: Rc::new(move |on| {
                let Some(b) = me2.upgrade() else { return };
                if let Some(win) = b.win.borrow().as_ref() {
                    surface::set_keyboard(
                        win,
                        if on {
                            surface::INVOKED
                        } else {
                            surface::Keyboard::None
                        },
                    );
                }
            }),
        }
    }

    fn set_options(self: &Rc<Self>, id: &str, f: impl FnOnce(&mut Options)) {
        self.update(id, |p, _, _| f(&mut p.options));
    }

    fn grant(self: &Rc<Self>, id: &str) {
        let host = match self.content(id) {
            Some(Content::Consent { host, .. }) => host,
            _ => return,
        };
        {
            let mut st = self.st.borrow_mut();
            let Some(Kind::Custom(pkg)) = st.desk.get(id).map(|p| p.kind.clone()) else {
                return;
            };
            st.desk.grant(&pkg, &host);
        }
        self.save();
        self.notify(Change::Granted(id.to_string()));
    }

    /// Edit Widgets on or off.
    pub fn set_edit(self: &Rc<Self>, on: bool) {
        if self.st.borrow().edit == on {
            return;
        }
        self.st.borrow_mut().edit = on;
        if on && let Some(f) = self.opened.borrow().clone() {
            f();
        }
        if !on {
            let rs: Vec<gtk::Button> = self
                .st
                .borrow_mut()
                .removes
                .drain()
                .map(|(_, r)| r)
                .collect();
            for r in rs {
                self.canvas.remove(&r);
            }
        }
        let ids: Vec<String> = self.widgets().into_iter().map(|p| p.id).collect();
        for id in ids {
            self.refresh_card(&id);
        }
        // The desktop stays on the Bottom layer; the gallery's own surface comes and goes.
        self.show_gallery(on);
        self.notify(Change::Edit(on));
    }

    fn choose_city(self: &Rc<Self>, id: &str) {
        self.set_edit(true);
        self.gallery
            .select(&Kind::Builtin(Builtin::Weather), Some(id.to_string()));
    }

    pub fn on_edit(&self, f: Rc<dyn Fn()>) {
        *self.opened.borrow_mut() = Some(f);
    }

    /// Socket: `show|hide|toggle widgets` (Edit Widgets).
    pub fn command(self: &Rc<Self>, c: &crate::protocol::Command) -> bool {
        match edit_after(c, self.is_editing()) {
            Some(on) => {
                self.set_edit(on);
                true
            }
            None => false,
        }
    }

    /// widgets.json or the package folders changed (`wol shell reload`).
    pub fn replace(self: &Rc<Self>, desk: Desktop, packages: Vec<Package>) {
        let packages: Vec<Rc<Package>> = packages.into_iter().map(Rc::new).collect();
        let ids: Vec<String> = self.st.borrow().cards.keys().cloned().collect();
        for id in ids {
            self.drop_card(&id);
        }
        {
            let mut st = self.st.borrow_mut();
            st.desk = desk;
            st.packages = packages.clone();
            st.content.clear();
        }
        self.gallery.set_entries(entries(&packages));
        self.layout();
        self.notify(Change::Replaced);
    }

    fn card_menu(self: &Rc<Self>, id: &str, x: f64, y: f64) {
        let Some(p) = self.st.borrow().desk.get(id).cloned() else {
            return;
        };
        let packages = self.packages();
        let mut rows: Vec<w::MenuRow> = Vec::new();
        let me = Rc::downgrade(self);
        let act = |f: BoardFn| -> Box<dyn Fn()> {
            let me = me.clone();
            Box::new(move || {
                if let Some(b) = me.upgrade() {
                    f(&b);
                }
            })
        };
        let tick = |on: bool| on.then_some("✓");
        for s in sizes_of(&p.kind, &packages, p.size) {
            let id = p.id.clone();
            rows.push((
                s.label().to_string(),
                tick(s == p.size),
                act(Box::new(move |b| b.set_size(&id, s))),
            ));
        }
        match &p.kind {
            Kind::Builtin(Builtin::Clock) => {
                rows.push((String::new(), None, Box::new(|| {})));
                let face = p.options.face.unwrap_or(Face::Analog);
                for (label, f) in [("Analog", Face::Analog), ("Digital", Face::Digital)] {
                    let id = p.id.clone();
                    rows.push((
                        label.into(),
                        tick(face == f),
                        act(Box::new(move |b| b.set_options(&id, |o| o.face = Some(f)))),
                    ));
                }
            }
            Kind::Builtin(Builtin::Timer) => {
                rows.push((String::new(), None, Box::new(|| {})));
                let mode = timer::Mode::parse(p.options.timer.as_deref());
                for m in timer::MODES {
                    let id = p.id.clone();
                    rows.push((
                        m.label(),
                        tick(m == mode),
                        act(Box::new(move |b| {
                            b.set_options(&id, |o| o.timer = Some(m.id()))
                        })),
                    ));
                }
            }
            Kind::Builtin(Builtin::WorldClock) => {
                rows.push((String::new(), None, Box::new(|| {})));
                let chosen = worldclock::zones(p.options.cities.as_deref());
                for (name, zone) in worldclock::CITIES {
                    let id = p.id.clone();
                    rows.push((
                        name.into(),
                        tick(chosen.iter().any(|z| z == zone)),
                        act(Box::new(move |b| {
                            b.set_options(&id, |o| {
                                o.cities = Some(worldclock::toggle(o.cities.as_deref(), zone))
                            })
                        })),
                    ));
                }
            }
            Kind::Builtin(Builtin::Weather) => {
                rows.push((String::new(), None, Box::new(|| {})));
                let id = p.id.clone();
                rows.push((
                    "Choose City…".into(),
                    None,
                    act(Box::new(move |b| b.choose_city(&id))),
                ));
                let units = p.options.units.unwrap_or_else(local_units);
                for (label, u) in [
                    ("Celsius", Units::Celsius),
                    ("Fahrenheit", Units::Fahrenheit),
                ] {
                    let id = p.id.clone();
                    rows.push((
                        label.into(),
                        tick(units == u),
                        act(Box::new(move |b| b.set_options(&id, |o| o.units = Some(u)))),
                    ));
                }
            }
            _ => {}
        }
        rows.push((String::new(), None, Box::new(|| {})));
        if !self.is_editing() {
            rows.push((
                "Edit Widgets…".into(),
                None,
                act(Box::new(|b| b.set_edit(true))),
            ));
        }
        let id = p.id.clone();
        rows.push((
            "Remove Widget".into(),
            None,
            act(Box::new(move |b| b.remove(&id))),
        ));
        let card = self.st.borrow().cards.get(&p.id).cloned();
        if let Some(c) = card {
            d::popup_menu(&c, x, y, rows);
        }
    }

    // ---- Drag and drop (Edit Widgets) ----

    fn card_at(&self, x: f64, y: f64) -> Option<String> {
        let mut hit = self.canvas.pick(x, y, gtk::PickFlags::DEFAULT);
        let st = self.st.borrow();
        while let Some(wg) = hit {
            if let Some((id, _)) = st
                .cards
                .iter()
                .find(|(_, c)| c.upcast_ref::<gtk::Widget>() == &wg)
            {
                return Some(id.clone());
            }
            if st
                .removes
                .values()
                .any(|r| r.upcast_ref::<gtk::Widget>() == &wg)
            {
                return None;
            }
            hit = wg.parent();
        }
        None
    }

    fn drag_begin(self: &Rc<Self>, x: f64, y: f64) -> bool {
        if !self.is_editing() {
            return false;
        }
        let Some(id) = self.card_at(x, y) else {
            return false;
        };
        let Some(p) = self.shown().into_iter().find(|p| p.id == id) else {
            return false;
        };
        let (x0, y0) = self.origin(&p);
        let mark = d::drop_mark(p.size);
        self.canvas.put(&mark, x0, y0);
        let mut st = self.st.borrow_mut();
        if let Some(c) = st.cards.get(&id) {
            mark.insert_before(&self.canvas, Some(c));
            c.add_css_class("dragging");
            c.insert_before(&self.canvas, None::<&gtk::Widget>);
        }
        if let Some(r) = st.removes.get(&id) {
            r.set_visible(false);
        }
        st.mark = Some(mark);
        st.drag = Some(Drag {
            id,
            x0,
            y0,
            target: (p.col, p.row),
        });
        true
    }

    fn drag_update(&self, dx: f64, dy: f64) {
        let shown = self.shown();
        let mut st = self.st.borrow_mut();
        let grid = st.grid;
        let (cw, ch) = (self.canvas.width() as f64, self.canvas.height() as f64);
        let Some(dr) = st.drag.as_ref() else { return };
        let id = dr.id.clone();
        let Some(p) = shown.iter().find(|p| p.id == id) else {
            return;
        };
        let (pw, ph) = grid.m.size(p.size);
        let nx = (dr.x0 + dx).clamp(0.0, (cw - pw as f64).max(0.0));
        let ny = (dr.y0 + dy).clamp(0.0, (ch - ph as f64).max(0.0));
        if let Some(c) = st.cards.get(&id) {
            self.canvas.move_(c, nx, ny);
        }
        let (c, r) = grid.snap(nx, ny, p.size);
        let target = if grid.is_free(&shown, Some(&id), c, r, p.size) {
            (c, r)
        } else {
            grid.drop_at(&shown, &id, c, r)
        };
        let (mx, my) = grid.m.origin(target.0, target.1);
        if let Some(m) = st.mark.as_ref() {
            self.canvas.move_(m, mx as f64, my as f64);
        }
        if let Some(dr) = st.drag.as_mut() {
            dr.target = target;
        }
    }

    fn drag_end(self: &Rc<Self>) {
        let (dr, mark) = {
            let mut st = self.st.borrow_mut();
            (st.drag.take(), st.mark.take())
        };
        if let Some(m) = mark {
            self.canvas.remove(&m);
        }
        let Some(dr) = dr else { return };
        {
            let mut st = self.st.borrow_mut();
            if let Some(c) = st.cards.get(&dr.id) {
                c.remove_css_class("dragging");
            }
            if let Some(r) = st.removes.get(&dr.id) {
                r.set_visible(true);
            }
            if let Some(p) = st.desk.get_mut(&dr.id) {
                (p.col, p.row) = dr.target;
            }
        }
        self.save();
        self.layout();
    }
}

fn local_units() -> Units {
    let l = ["LC_ALL", "LC_MEASUREMENT", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .unwrap_or_default();
    weather::units_for_locale(&l)
}

/// Geocoding off the main loop; `done` runs on it.
/// Edit Widgets after the socket command `c`, None when `c` is not for `widgets`: `hide widgets`
/// is Done, like the gallery's button and Esc.
fn edit_after(c: &crate::protocol::Command, editing: bool) -> Option<bool> {
    (c.target == "widgets").then(|| crate::protocol::apply(c.verb, editing))
}

/// The gallery surface's left/right layer margins (it spans the bottom edge, `Spec::widgets_gallery`).
#[cfg(feature = "layer-shell")]
fn gallery_layer_margins(win: &gtk::Window, start: i32, end: i32) {
    use gtk4_layer_shell::{Edge, LayerShell};
    if surface::layered() {
        LayerShell::set_margin(win, Edge::Left, start);
        LayerShell::set_margin(win, Edge::Right, end);
    }
}

/// Dev toplevels: the window is the gallery's size, no margins.
#[cfg(not(feature = "layer-shell"))]
fn gallery_layer_margins(_win: &gtk::Window, _start: i32, _end: i32) {}

/// Gallery margins (start, end): the gallery sits on the wider side clear of every card span.
fn gallery_margins(spans: impl Iterator<Item = (i32, i32)>, total: i32, pad: i32) -> (i32, i32) {
    let (lo, hi) = spans.fold((i32::MAX, i32::MIN), |(lo, hi), (a, b)| {
        (lo.min(a), hi.max(b))
    });
    if lo > hi {
        return (0, 0);
    }
    if total - hi >= lo {
        (hi + pad, 0)
    } else {
        (0, total - lo + pad)
    }
}

fn search_places(q: String, done: super::widgets_gallery::Found) {
    glib::spawn_future_local(async move {
        let r = gio::spawn_blocking(move || {
            wol_core::widgets::http_get(
                &weather::geocode_url(&q),
                wol_core::widgets::HTTP_TIMEOUT,
                wol_core::widgets::MAX_OUTPUT,
            )
            .map(|b| weather::parse_places(&String::from_utf8_lossy(&b)))
            .map_err(|e| e.message())
        })
        .await
        .unwrap_or_else(|_| Err(String::new()))
        .map_err(|why| {
            eprintln!("wol-shell: city search: {why}");
            crate::copy::CITY_SEARCH.to_string()
        });
        done(r);
    });
}

// ---- Live data ----

/// Local date and time (`WOL_FAKE_NOW`: the golden clock).
pub fn local_now() -> Now {
    if std::env::var_os("WOL_FAKE_NOW").is_some() {
        return builtin::sample_now();
    }
    let Ok(t) = glib::DateTime::now_local() else {
        return builtin::sample_now();
    };
    Now {
        y: t.year(),
        month: t.month() as u32,
        day: t.day_of_month() as u32,
        weekday: (t.day_of_week() as u32).saturating_sub(1),
        hour: t.hour() as u32,
        minute: t.minute() as u32,
    }
}

/// The World Clock rows for `zones` now (`WOL_FAKE_NOW`: the golden cities).
fn city_times(zones: Option<&[String]>) -> Vec<worldclock::CityTime> {
    if std::env::var_os("WOL_FAKE_NOW").is_some() {
        return builtin::sample_cities();
    }
    let (Ok(utc), Ok(here)) = (glib::DateTime::now_utc(), glib::DateTime::now_local()) else {
        return Vec::new();
    };
    let secs = |t: &glib::DateTime| (t.utc_offset().as_microseconds() / 1_000_000) as i32;
    worldclock::zones(zones)
        .iter()
        .filter_map(|z| {
            // An unknown zone falls back to UTC: skip it rather than show a wrong time.
            #[allow(deprecated)]
            let tz = glib::TimeZone::new(Some(z));
            if tz.identifier() != z.as_str() {
                return None;
            }
            let there = utc.to_timezone(&tz).ok()?;
            Some(worldclock::CityTime::at(
                &worldclock::city(z),
                utc.to_unix(),
                secs(&here),
                secs(&there),
            ))
        })
        .collect()
}

/// An event time as local wall-clock time.
fn to_local(wh: &calendar::When) -> Option<calendar::DateTime> {
    use calendar::When;
    let t = match wh {
        When::Date { .. } => return None,
        When::Floating(at) => return Some(*at),
        When::Utc(a) => {
            glib::DateTime::from_utc(a.y, a.m as i32, a.d as i32, a.hh as i32, a.mm as i32, 0.0)
                .ok()?
        }
        When::Zoned { tzid, at: a } => {
            let tz = glib::TimeZone::new(Some(tzid));
            glib::DateTime::new(
                &tz,
                a.y,
                a.m as i32,
                a.d as i32,
                a.hh as i32,
                a.mm as i32,
                0.0,
            )
            .ok()?
        }
    }
    .to_local()
    .ok()?;
    Some(calendar::DateTime {
        y: t.year(),
        m: t.month() as u32,
        d: t.day_of_month() as u32,
        hh: t.hour() as u32,
        mm: t.minute() as u32,
    })
}

/// Today in UTC `YYYYMMDDTHHMMSSZ` (EDS range).
fn today_utc(n: &Now) -> Option<(String, String)> {
    let start = glib::DateTime::new(
        &glib::TimeZone::local(),
        n.y,
        n.month as i32,
        n.day as i32,
        0,
        0,
        0.0,
    )
    .ok()?;
    let end = start.add_days(1).ok()?;
    let f = |t: &glib::DateTime| {
        t.to_utc()
            .ok()
            .and_then(|u| u.format("%Y%m%dT%H%M%SZ").ok())
            .map(|s| s.to_string())
    };
    Some((f(&start)?, f(&end)?))
}

/// Seconds between refreshes.
const MINUTE: u64 = 60;
const SYSTEM_EVERY: u64 = 10;
const EVENTS_EVERY: u64 = 15 * 60;
const BATTERY_EVERY: u64 = 2 * 60;
/// CPU samples a System chart shows (5 minutes).
const CPU_SAMPLES: usize = 30;

/// A desktop is covered (its widgets pause) by a maximized or full-screen window.
fn covers(mode: crate::hypr::Fullscreen) -> bool {
    mode != crate::hypr::Fullscreen::None
}

struct Live {
    board: Rc<Board>,
    sys: Rc<dyn crate::system::System>,
    schedule: RefCell<Schedule>,
    scheduled: RefCell<HashSet<String>>,
    timer: RefCell<Option<glib::SourceId>>,
    paused: Cell<bool>,
    wakeups: Cell<u64>,
    started: std::time::Instant,
    stats: RefCell<Option<std::sync::Arc<std::sync::Mutex<wol_core::system::stats::StatsSource>>>>,
    cpu: RefCell<wd::History>,
    system: RefCell<Option<SystemView>>,
    /// None = no calendar service.
    events: RefCell<Option<Vec<calendar::RawEvent>>>,
    events_day: Cell<u32>,
    music: RefCell<Option<media::Playing>>,
    music_on: Cell<bool>,
    /// None = UPower not asked yet.
    upower: Cell<Option<bool>>,
    batteries: RefCell<Vec<power::Device>>,
    weather: RefCell<HashMap<String, WeatherView>>,
    custom: RefCell<HashMap<String, Value>>,
    /// Lua widget files and their last render; the folder watch once one is placed.
    lua: RefCell<super::lua_widgets::Lua>,
    lua_watch: RefCell<Option<gio::FileMonitor>>,
    /// Timer widgets' counters by widget id (this session only).
    timers: RefCell<HashMap<String, timer::Counter>>,
}

/// Milliseconds on the monotonic clock (Timer widgets).
fn mono_ms() -> u64 {
    (glib::monotonic_time() / 1000) as u64
}

fn timing() -> bool {
    std::env::var_os("WOL_SHELL_TIMING").is_some()
}

fn place_key(p: &Place, u: Units) -> String {
    format!("{:.3},{:.3},{u:?}", p.lat, p.lon)
}

impl Live {
    fn units(p: &Placed) -> Units {
        p.options.units.unwrap_or_else(local_units)
    }

    /// Content for one placed widget from what is known now.
    fn content(&self, p: &Placed, now: &Now) -> Option<Content> {
        Some(match &p.kind {
            Kind::Builtin(b) => Content::Builtin(match b {
                Builtin::Clock => Data::Clock {
                    now: *now,
                    face: p.options.face.unwrap_or(Face::Analog),
                },
                Builtin::Calendar => Data::Calendar {
                    now: *now,
                    events: self
                        .events
                        .borrow()
                        .as_ref()
                        .map(|e| calendar::upcoming(e, now.at(), to_local)),
                },
                Builtin::Weather => Data::Weather(match &p.options.place {
                    None => WeatherView::NoPlace,
                    Some(pl) => self
                        .weather
                        .borrow()
                        .get(&place_key(pl, Self::units(p)))
                        .cloned()
                        .unwrap_or_else(|| WeatherView::Loading(pl.clone())),
                }),
                Builtin::Music => Data::Music(self.music.borrow().clone()),
                Builtin::Battery => Data::Batteries {
                    devices: self.batteries.borrow().clone(),
                    minutes: self.sys.battery_time(),
                },
                Builtin::System => Data::System(self.system.borrow().clone()?),
                Builtin::WorldClock => Data::WorldClock(city_times(p.options.cities.as_deref())),
                Builtin::Timer => Data::Timer(self.counter(p, |c| c.view(mono_ms()))),
                Builtin::Note => Data::Note(p.options.text.clone().unwrap_or_default()),
            }),
            Kind::Custom(id) => {
                let pkg = self.board.packages().into_iter().find(|k| &k.id == id);
                let Some(pkg) = pkg else {
                    return Some(Content::Error {
                        name: id.clone(),
                        message: crate::copy::WIDGET_MISSING.into(),
                        details: vec![format!(
                            "Package folder: ~/.local/share/{}/{id}",
                            shared::widgets::USER_PACKAGES_DIR
                        )],
                    });
                };
                let m = match &pkg.manifest {
                    Err(problems) => {
                        return Some(Content::Error {
                            name: pkg.name().into(),
                            message: crate::copy::widget_problems(pkg.name()),
                            details: lay::problem_lines(problems),
                        });
                    }
                    Ok(m) => m,
                };
                if let Some(host) = m.source.as_ref().and_then(Source::host)
                    && !self.board.desktop().allows(id, &host)
                {
                    return Some(Content::Consent {
                        name: m.name.clone(),
                        host,
                    });
                }
                let data = match (&m.source, self.custom.borrow().get(&p.id)) {
                    (None, _) => Value::Null,
                    (Some(_), Some(v)) => v.clone(),
                    (Some(_), None) => return None,
                };
                Content::Custom {
                    manifest: Rc::new(m.clone()),
                    dir: pkg.dir.clone(),
                    data,
                }
            }
            Kind::Lua(file) => self.lua.borrow().content(file)?,
            Kind::Unknown(k) => Content::Error {
                name: k.clone(),
                message: crate::copy::WIDGET_NEWER.into(),
                details: vec![format!("Widget kind: {k}")],
            },
        })
    }

    /// `f` on the counter of Timer widget `p`; a new mode starts a new counter.
    fn counter<T>(&self, p: &Placed, f: impl FnOnce(&mut timer::Counter) -> T) -> T {
        let mode = timer::Mode::parse(p.options.timer.as_deref());
        let mut all = self.timers.borrow_mut();
        let c = all
            .entry(p.id.clone())
            .or_insert_with(|| timer::Counter::new(mode));
        if c.mode != mode {
            *c = timer::Counter::new(mode);
        }
        f(c)
    }

    /// Settles Timer widgets that ran out (with a notification) and redraws them.
    fn tick_timers(self: &Rc<Self>) {
        let now = mono_ms();
        let mut ended = Vec::new();
        for c in self.timers.borrow_mut().values_mut() {
            if let Some(ph) = c.tick(now) {
                ended.push(timer::finished(c.mode, ph));
            }
        }
        for (summary, body) in ended {
            crate::kit::spawn(&[
                "notify-send".into(),
                "-a".into(),
                "Timer".into(),
                summary,
                body,
            ]);
        }
        self.show(|p| p.kind == Kind::Builtin(Builtin::Timer));
        self.sync();
    }

    /// Refreshes the cards of widgets matching `which`.
    fn show(&self, which: impl Fn(&Placed) -> bool) {
        let now = local_now();
        for p in self.board.widgets().iter().filter(|p| which(p)) {
            if let Some(c) = self.content(p, &now) {
                self.board.set_content(&p.id, c);
            }
        }
    }

    fn show_kind(&self, b: Builtin) {
        self.show(|p| p.kind == Kind::Builtin(b));
    }

    fn has(&self, b: Builtin) -> bool {
        self.board
            .widgets()
            .iter()
            .any(|p| p.kind == Kind::Builtin(b))
    }

    fn schedule(&self, id: &str, period: u64) {
        if self.scheduled.borrow_mut().insert(id.to_string()) {
            self.schedule.borrow_mut().set(id, period, wd::unix_now());
        }
    }

    fn unschedule(&self, id: &str) {
        if self.scheduled.borrow_mut().remove(id) {
            self.schedule.borrow_mut().remove(id);
        }
    }

    /// Starts what the widgets on the desktop need and stops what nothing needs any more.
    fn sync(self: &Rc<Self>) {
        let ws = self.board.widgets();
        let has = |b: Builtin| ws.iter().any(|p| p.kind == Kind::Builtin(b));
        if has(Builtin::Clock) || has(Builtin::Calendar) || has(Builtin::WorldClock) {
            self.schedule("minute", MINUTE);
        } else {
            self.unschedule("minute");
        }
        if has(Builtin::Calendar) {
            if !self.scheduled.borrow().contains("events") {
                self.schedule("events", EVENTS_EVERY);
                self.read_events();
            }
        } else {
            self.unschedule("events");
        }
        if has(Builtin::System) {
            if !self.scheduled.borrow().contains("system") {
                self.schedule("system", SYSTEM_EVERY);
                self.read_system();
            }
        } else {
            self.unschedule("system");
            *self.stats.borrow_mut() = None;
        }
        // Every second, only while a timer runs.
        if self.timers.borrow().values().any(|c| c.running()) {
            self.schedule("timer", 1);
        } else {
            self.unschedule("timer");
        }
        if has(Builtin::Weather) {
            self.schedule("weather", weather::REFRESH_SECS);
        } else {
            self.unschedule("weather");
        }
        if has(Builtin::Music) && !self.music_on.replace(true) {
            self.watch_music();
        }
        if has(Builtin::Battery) {
            match self.upower.get() {
                None => self.watch_batteries(),
                Some(false) => self.schedule("battery", BATTERY_EVERY),
                Some(true) => {}
            }
        } else {
            self.unschedule("battery");
        }
        let custom: HashSet<String> = ws
            .iter()
            .filter(|p| matches!(p.kind, Kind::Custom(_)))
            .map(|p| format!("custom:{}", p.id))
            .collect();
        let gone: Vec<String> = self
            .scheduled
            .borrow()
            .iter()
            .filter(|k| k.starts_with("custom:") && !custom.contains(*k))
            .cloned()
            .collect();
        for k in gone {
            self.unschedule(&k);
        }
        self.sync_lua(&ws);
        self.arm();
    }

    /// Lua widgets: read the folder when the first one is placed and watch it from then on;
    /// each placed file renders at its own refresh.
    fn sync_lua(self: &Rc<Self>, ws: &[Placed]) {
        let files: HashSet<String> = ws
            .iter()
            .filter_map(|p| match &p.kind {
                Kind::Lua(f) => Some(f.clone()),
                _ => None,
            })
            .collect();
        if !files.is_empty() && self.lua_watch.borrow().is_none() {
            self.lua.borrow_mut().poll();
            let dir = gio::File::for_path(wol_lua::user_dir());
            if let Ok(m) =
                dir.monitor_directory(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
            {
                let me = Rc::downgrade(self);
                m.connect_changed(move |_, _, _, _| {
                    if let Some(l) = me.upgrade() {
                        let changed = l.lua.borrow_mut().poll();
                        for f in changed {
                            l.show_lua(&f);
                        }
                        l.sync();
                    }
                });
                *self.lua_watch.borrow_mut() = Some(m);
            }
        }
        for f in &files {
            let key = format!("{LUA_PREFIX}{f}");
            match self.lua.borrow_mut().refresh(f) {
                Some(r) => self.schedule(&key, u64::from(r)),
                None => self.unschedule(&key),
            }
        }
        let gone: Vec<String> = self
            .scheduled
            .borrow()
            .iter()
            .filter(|k| k.starts_with(LUA_PREFIX) && !files.contains(&k[LUA_PREFIX.len()..]))
            .cloned()
            .collect();
        for k in gone {
            self.unschedule(&k);
        }
    }

    fn show_lua(&self, file: &str) {
        self.show(|p| matches!(&p.kind, Kind::Lua(f) if f == file));
    }

    fn arm(self: &Rc<Self>) {
        if let Some(t) = self.timer.borrow_mut().take() {
            t.remove();
        }
        if self.paused.get() {
            return;
        }
        let now_ms = glib::real_time() / 1000;
        let now = (now_ms / 1000) as u64;
        let Some(wait) = self.schedule.borrow().wait(now) else {
            return;
        };
        // Just past the deadline: a clock never shows the minute that just ended.
        let due_ms = (now + wait) as i64 * 1000 + 50;
        let delay = (due_ms - now_ms).max(0) as u64;
        let me = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(std::time::Duration::from_millis(delay), move || {
            if let Some(l) = me.upgrade() {
                *l.timer.borrow_mut() = None;
                l.wake();
            }
        });
        *self.timer.borrow_mut() = Some(id);
    }

    fn wake(self: &Rc<Self>) {
        let due = self.schedule.borrow_mut().take_due(wd::unix_now());
        if !due.is_empty() {
            let n = self.wakeups.get() + 1;
            self.wakeups.set(n);
            if timing() {
                let mins = self.started.elapsed().as_secs_f64() / 60.0;
                eprintln!(
                    "wol-shell: widgets wake {due:?} ({n} in {mins:.1} min, {:.1}/h)",
                    n as f64 / mins.max(1.0 / 60.0) * 60.0
                );
            }
        }
        for id in due {
            self.run(&id);
        }
        self.arm();
    }

    fn run(self: &Rc<Self>, id: &str) {
        match id {
            "minute" => {
                let now = local_now();
                if self.events_day.get() != now.day && self.has(Builtin::Calendar) {
                    self.read_events();
                }
                self.show(|p| {
                    matches!(
                        p.kind,
                        Kind::Builtin(Builtin::Clock | Builtin::Calendar | Builtin::WorldClock)
                    )
                });
            }
            "system" => self.read_system(),
            "timer" => self.tick_timers(),
            "events" => self.read_events(),
            "weather" => self.refresh_weather(None, false),
            "battery" => {
                *self.batteries.borrow_mut() = power::builtin(&*self.sys);
                self.show_kind(Builtin::Battery);
            }
            c if c.starts_with("custom:") => self.run_custom(&c["custom:".len()..]),
            c if c.starts_with(LUA_PREFIX) => {
                let file = &c[LUA_PREFIX.len()..];
                self.lua.borrow_mut().render(file);
                self.show_lua(file);
            }
            _ => {}
        }
    }

    fn set_paused(self: &Rc<Self>, on: bool) {
        if self.paused.replace(on) == on {
            return;
        }
        if timing() {
            eprintln!(
                "wol-shell: widgets {}",
                if on { "paused (fullscreen)" } else { "resumed" }
            );
        }
        if on {
            if let Some(t) = self.timer.borrow_mut().take() {
                t.remove();
            }
        } else {
            // What came due while covered runs now.
            self.wake();
        }
    }
}

fn read_disk() -> (Desktop, PathBuf, Vec<Package>) {
    let file = wd::desktop_path(&wol_core::theme::config_home());
    let (desk, err) = wd::load(&file);
    if let Some(e) = err {
        eprintln!("wol-shell: {e} (the file is kept as widgets.json.bad on the next change)");
    }
    let packages = wol_core::widgets::load_packages(&wol_core::widgets::package_dirs(
        &wol_core::widgets::data_home(),
    ));
    (desk, file, packages)
}

/// A desktop widget surface for `desk`, with its live data, ended with `life` (its window).
fn surface_for(
    app: &gtk::Application,
    sys: Rc<dyn crate::system::System>,
    desk: Desktop,
    packages: Vec<Package>,
) -> (gtk::ApplicationWindow, Rc<Board>) {
    let board = Board::new(desk, None, packages);
    let win = super::window(app, "desktop", surface::Spec::desktop());
    win.set_child(Some(&board.root));
    board.set_window(win.upcast_ref());
    super::drop_shelf::accept_drops(&win);
    let keys = gtk::EventControllerKey::new();
    let b2 = Rc::downgrade(&board);
    keys.connect_key_pressed(move |_, k, _, _| match b2.upgrade() {
        Some(b) if k == gtk::gdk::Key::Escape && b.is_editing() => {
            b.set_edit(false);
            glib::Propagation::Stop
        }
        _ => glib::Propagation::Proceed,
    });
    win.add_controller(keys);
    let gallery = gallery_window(app, &board);
    board.set_gallery_window(gallery.upcast_ref());
    // A display's board goes with its display (`Desks::split`); its gallery with it.
    win.connect_destroy(move |_| gallery.destroy());
    let live = Rc::new(Live {
        board: board.clone(),
        sys,
        schedule: Default::default(),
        scheduled: Default::default(),
        timer: Default::default(),
        paused: Cell::new(false),
        wakeups: Cell::new(0),
        started: std::time::Instant::now(),
        stats: Default::default(),
        cpu: RefCell::new(wd::History::new(CPU_SAMPLES)),
        system: Default::default(),
        events: Default::default(),
        events_day: Cell::new(0),
        music: Default::default(),
        music_on: Cell::new(false),
        upower: Cell::new(None),
        batteries: Default::default(),
        weather: Default::default(),
        custom: Default::default(),
        lua: RefCell::new(super::lua_widgets::Lua::user()),
        lua_watch: Default::default(),
        timers: Default::default(),
    });
    let l2 = Rc::downgrade(&live);
    board.set_listener(Rc::new(move |c| {
        if let Some(l) = l2.upgrade() {
            l.on_change(c);
        }
    }));
    live.start_all();
    live.watch_fullscreen();
    // The providers live as long as the surface; its timer goes with it.
    win.connect_destroy(move |_| {
        if let Some(t) = live.timer.borrow_mut().take() {
            t.remove();
        }
    });
    (win, board)
}

/// The Edit Widgets gallery's own surface for `board` (one per display, like the board): Top
/// layer, keyboard on demand, hidden until Edit Widgets. Esc and a compositor close are Done.
fn gallery_window(app: &gtk::Application, board: &Rc<Board>) -> gtk::ApplicationWindow {
    // `panel`: a transparent window around the glass gallery (kit.css).
    let gw = super::window(app, "panel", surface::Spec::widgets_gallery());
    gw.add_css_class("widgets-gallery");
    let b = Rc::downgrade(board);
    surface::on_escape(&gw, move || {
        if let Some(b) = b.upgrade() {
            b.set_edit(false);
        }
    });
    // Its display went away, or the compositor closed it: Done, and the window stays for the next
    // Edit Widgets (not destroyed).
    let b = Rc::downgrade(board);
    gw.connect_close_request(move |w| {
        match b.upgrade() {
            Some(b) if b.is_editing() => b.set_edit(false),
            _ => w.set_visible(false),
        }
        glib::Propagation::Stop
    });
    gw
}

/// The gallery surface spans the free width; clicks beside the glass go through to the desktop:
/// the input region follows `part`'s bounds after each frame the window draws (none while idle),
/// set only on a change.
fn input_only_on(win: &gtk::Window, part: &gtk::Box) {
    if !surface::layered() {
        return;
    }
    let part = part.downgrade();
    win.connect_realize(move |win| {
        let (Some(clock), Some(s)) = (win.frame_clock(), win.surface()) else {
            return;
        };
        let (w, part, last) = (win.downgrade(), part.clone(), Cell::new(None));
        clock.connect_after_paint(move |_| {
            let (Some(win), Some(part)) = (w.upgrade(), part.upgrade()) else {
                return;
            };
            let Some(b) = part.compute_bounds(&win) else {
                return;
            };
            let (x, y) = (b.x().floor() as i32, b.y().floor() as i32);
            let r = (
                x,
                y,
                b.width().ceil() as i32 + 1,
                b.height().ceil() as i32 + 1,
            );
            if last.replace(Some(r)) != Some(r) {
                let rect = gtk::cairo::RectangleInt::new(r.0, r.1, r.2, r.3);
                s.set_input_region(Some(&gtk::cairo::Region::create_rectangle(&rect)));
            }
        });
    });
}

/// One display's board and the widget ids it holds in the saved layout.
struct Part {
    win: Option<gtk::ApplicationWindow>,
    board: Rc<Board>,
    owned: std::collections::BTreeSet<String>,
}

/// The desktop widgets of every display, kept in one `widgets.json` (`shared::widgets::displays`):
/// the main display's board lives for the session (its window moves with the main display), the
/// others come and go with their display. Each board saves its part; the parts fold into the file.
pub struct Desks {
    app: gtk::Application,
    sys: Rc<dyn crate::system::System>,
    file: PathBuf,
    master: RefCell<Desktop>,
    /// By connector; "" is the main display's board until the display list is known.
    parts: RefCell<HashMap<String, Part>>,
    main: Rc<Board>,
}

fn display_list() -> Vec<(shared::widgets::displays::Display, gtk::gdk::Monitor)> {
    super::displays::monitors()
        .into_iter()
        .map(|(c, m)| {
            let description = m.description().map(|d| d.to_string()).unwrap_or_default();
            (
                shared::widgets::displays::Display {
                    connector: c,
                    description,
                },
                m,
            )
        })
        .collect()
}

impl Desks {
    fn save_part(&self, connector: &str, part: &Desktop) {
        let mut master = self.master.borrow_mut();
        let mut parts = self.parts.borrow_mut();
        let Some(p) = parts.get_mut(connector) else {
            return;
        };
        p.owned = shared::widgets::displays::merge(&mut master, &p.owned, part);
        if let Err(e) = wd::save(&self.file, &master) {
            eprintln!("wol-shell: {}: {e}", self.file.display());
        }
    }

    fn saver(self: &Rc<Self>, connector: &str) -> Rc<dyn Fn(&Desktop)> {
        let (me, c) = (Rc::downgrade(self), connector.to_string());
        Rc::new(move |d: &Desktop| {
            if let Some(me) = me.upgrade() {
                me.save_part(&c, d);
            }
        })
    }

    /// The display list changed (or the file was reloaded): every board gets its part again.
    fn split(self: &Rc<Self>) {
        use shared::widgets::displays as dp;
        let list = display_list();
        let ds: Vec<dp::Display> = list.iter().map(|x| x.0.clone()).collect();
        let master = self.master.borrow().clone();
        let (_, _, packages) = read_disk();
        let mut parts = self.parts.borrow_mut();
        // Gone displays take their board with them (their widgets wait in the file).
        let keep: HashSet<String> = ds.iter().skip(1).map(|d| d.connector.clone()).collect();
        parts.retain(|c, p| {
            let stay = c.is_empty() || keep.contains(c);
            if !stay && let Some(w) = &p.win {
                w.destroy();
            }
            stay
        });
        // The main board is filed under "" whichever display is main.
        let main_part = dp::subset(&master, &ds, 0);
        let main = parts.entry(String::new()).or_insert_with(|| Part {
            win: None,
            board: self.main.clone(),
            owned: Default::default(),
        });
        main.owned = main_part.widgets.iter().map(|p| p.id.clone()).collect();
        self.main.set_stamp(ds.first().map(dp::Display::key));
        self.main.replace(main_part, packages.clone());
        for (i, (d, mon)) in list.iter().enumerate().skip(1) {
            let part = dp::subset(&master, &ds, i);
            let owned = part.widgets.iter().map(|p| p.id.clone()).collect();
            if let Some(p) = parts.get_mut(&d.connector) {
                p.board.replace(part, packages.clone());
                p.board.set_stamp(Some(d.key()));
                p.owned = owned;
                continue;
            }
            let (win, board) = surface_for(&self.app, self.sys.clone(), part, packages.clone());
            surface::set_monitor(&win, Some(mon));
            board.set_stamp(Some(d.key()));
            board.set_saver(self.saver(&d.connector));
            win.present();
            parts.insert(
                d.connector.clone(),
                Part {
                    win: Some(win),
                    board,
                    owned,
                },
            );
        }
    }
}

/// The main display's desktop widget surface, and the per-display boards beside it.
pub fn build(
    app: &gtk::Application,
    sys: Rc<dyn crate::system::System>,
) -> (gtk::ApplicationWindow, Rc<Board>) {
    let (desk, file, packages) = read_disk();
    let (win, board) = surface_for(app, sys.clone(), Desktop::default(), packages);
    crate::kit::surface::survive_output_loss(&win, surface::Spec::desktop());
    let desks = Rc::new(Desks {
        app: app.clone(),
        sys,
        file,
        master: RefCell::new(desk),
        parts: Default::default(),
        main: board.clone(),
    });
    board.set_saver(desks.saver(""));
    desks.split();
    if let Some(d) = gtk::gdk::Display::default() {
        let weak = Rc::downgrade(&desks);
        d.monitors().connect_items_changed(move |_, _, _, _| {
            if let Some(me) = weak.upgrade() {
                me.split();
            }
        });
    }
    // `wol shell reload`: widgets.json and the package folders again.
    let first = Cell::new(true);
    let weak = Rc::downgrade(&desks);
    super::ipc::on_reload(move |_| {
        if first.replace(false) {
            return;
        }
        if let Some(me) = weak.upgrade() {
            me.master.replace(read_disk().0);
            me.split();
        }
    });
    // The store lives as long as the main surface.
    win.connect_destroy(move |_| {
        let _ = &desks;
    });
    (win, board)
}

#[cfg(test)]
mod gallery_tests {
    use super::{gallery_margins, mark_presence};

    /// QA-100 H17-01: the empty desktop draws a pixel (alpha 1 of 255), so GTK has a frame to attach.
    #[test]
    fn the_empty_desktop_paints_a_pixel_for_the_buffer() {
        let mut s = gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, 4, 4).unwrap();
        let blank = {
            let cr = gtk::cairo::Context::new(&s).unwrap();
            drop(cr);
            s.flush();
            s.data().unwrap().iter().all(|b| *b == 0)
        };
        assert!(blank, "a fresh surface is transparent");
        mark_presence(&gtk::cairo::Context::new(&s).unwrap());
        s.flush();
        let d = s.data().unwrap();
        let alphas: Vec<u8> = d.chunks(4).map(|px| px[3]).collect();
        assert_eq!(alphas[0], 1, "the first pixel is at alpha 1/255");
        assert_eq!(
            alphas.iter().filter(|a| **a != 0).count(),
            1,
            "and only that pixel"
        );
    }

    #[test]
    fn the_gallery_never_covers_a_card() {
        assert_eq!(gallery_margins([(24, 368)].into_iter(), 1920, 24), (392, 0));
        assert_eq!(
            gallery_margins([(1500, 1896)].into_iter(), 1920, 24),
            (0, 444)
        );
        assert_eq!(gallery_margins(std::iter::empty(), 1920, 24), (0, 0));
    }

    /// QA hunt F1: Esc ends Edit Widgets even while Settings has the keyboard. The gallery is a
    /// `wol-panel-*` layer, so while it is mapped wol-hypr forwards every Esc as `hide transient`
    /// (`wm_gates::SwitchKeys`, never consumed), and that command runs `transient::also_close`,
    /// which ui/mod.rs points at `Board::set_edit(false)`. Its own Esc handler covers the case
    /// where the gallery has the keyboard (an on-demand layer gets it when it maps).
    #[test]
    fn esc_reaches_the_gallery_without_its_keyboard() {
        use crate::kit::surface::Spec;
        assert!(Spec::widgets_gallery().namespace.starts_with("wol-panel-"));
        let esc = crate::protocol::parse("hide transient").unwrap();
        assert!(super::super::transient::closes_all(&esc));
    }
}

/// QA bug 5: `wol shell hide widgets` is Done (the gallery's button and Esc call `set_edit(false)`
/// too); other targets are not the board's.
#[cfg(test)]
mod edit_command_tests {
    use super::edit_after;
    use crate::protocol::parse;

    #[test]
    fn hide_widgets_is_done() {
        let c = |l: &str| parse(l).expect(l);
        assert_eq!(edit_after(&c("hide widgets"), true), Some(false));
        assert_eq!(edit_after(&c("hide widgets"), false), Some(false));
        assert_eq!(edit_after(&c("show widgets"), false), Some(true));
        assert_eq!(edit_after(&c("toggle widgets"), true), Some(false));
        assert_eq!(edit_after(&c("hide transient"), true), None);
    }
}

#[cfg(test)]
mod fullscreen_tests {
    use super::covers;
    use crate::hypr::{self, Fullscreen};

    #[test]
    fn each_display_follows_its_own_output() {
        let modes = [
            ("A".to_string(), Fullscreen::Maximized),
            ("B".to_string(), Fullscreen::None),
        ];
        hypr::set_monitor_fullscreen(modes.into_iter().collect());
        assert!(covers(hypr::monitor_fullscreen(Some("A"))));
        assert!(!covers(hypr::monitor_fullscreen(Some("B"))), "B stays live");
    }
}
