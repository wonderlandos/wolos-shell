// SPDX-License-Identifier: GPL-2.0-or-later
//! The bar's status items (SP-7): one menu bar button per item of [`crate::tray`], drawn like
//! the bar's own status icons, in the status slot. Hidden while no app has an item, and while
//! `shell.json` says `"tray": false` (then no watcher runs either, so an app that would hide in
//! the tray keeps its window).
//!
//! A click activates the item (or opens its menu when the item is only a menu), a right click
//! opens its menu, a middle click is its second action, the wheel scrolls it. Menus are the
//! kit's menu popover; a submenu slides in within the same popover, with a row back.
use crate::kit::{fence, widgets as w};
use crate::tray::{self, Item, MenuNode, Toggle, Tray};
use gtk::{gdk, glib, prelude::*};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

struct State {
    /// [`start`] ran (the real shell, not a preview).
    started: bool,
    /// `shell.json` `"tray"` (default on).
    enabled: bool,
    /// The running watcher (None: off, starting, or another process owns the name).
    tray: Option<Tray>,
    starting: bool,
    /// Watchers being stopped: each sends (or drops) once its name is given up. A start waits
    /// for them, so turning the tray off and on again does not race its own name (stability S5).
    stopping: Vec<async_channel::Receiver<()>>,
    /// Bumped on every start: an older run's list is not applied.
    run: u64,
    items: Vec<Item>,
    /// Folders added to the icon theme (`IconThemePath`).
    theme_paths: Vec<String>,
}

/// The watcher's state: one per process (one session, one name on the bus). Plain data, so it is
/// no thread-local: the strips that show it are the menu bars' ([`super::menubar::tray_strips`]).
static STATE: Mutex<State> = Mutex::new(State {
    started: false,
    enabled: true,
    tray: None,
    starting: false,
    stopping: Vec::new(),
    run: 0,
    items: Vec::new(),
    theme_paths: Vec::new(),
});

/// Runs `f` on the state (briefly: never across an await).
fn state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    f(&mut STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner))
}

fn tray_now() -> Option<Tray> {
    state(|s| s.tray.clone())
}

/// Starts the session's watcher behind `shell.json` `"tray"` (once; later calls do nothing).
/// Only the shell's own bar calls this ([`super::menubar::build`]); previews draw an empty strip.
pub fn start() {
    if state(|s| std::mem::replace(&mut s.started, true)) {
        return;
    }
    fence::guard("tray", || super::ipc::on_reload(|p| set_enabled(p.tray)));
}

fn set_enabled(on: bool) {
    let (launch, stop) = state(|s| {
        s.enabled = on;
        let launch = on && s.tray.is_none() && !s.starting;
        let stop = if on { None } else { s.tray.take() };
        if !on {
            s.items.clear();
        }
        (launch, stop)
    });
    if let Some(t) = stop {
        let (done, wait) = async_channel::bounded(1);
        state(|s| s.stopping.push(wait));
        glib::spawn_future_local(async move {
            t.stop().await;
            let _ = done.try_send(());
        });
    }
    if launch {
        run();
    }
    render_all();
}

/// Previews and goldens: the bars made from now on show `items`, as if apps had sent them (bars
/// there are keep theirs). Ignored once the real watcher was asked for ([`start`]).
pub fn preview(items: Vec<Item>) {
    state(|s| {
        if !s.started {
            s.items = items;
        }
    });
}

/// Owns the watcher name and applies every list it sends until it stops.
fn run() {
    let run = state(|s| {
        s.starting = true;
        s.run += 1;
        s.run
    });
    glib::spawn_future_local(async move {
        let spawn: tray::Spawn = Arc::new(|f| {
            glib::MainContext::default().spawn(f);
        });
        // The last watcher gives its name up first.
        for w in state(|s| std::mem::take(&mut s.stopping)) {
            let _ = w.recv().await;
        }
        let mut tries = 1;
        let (t, rx) = loop {
            match Tray::serve(None, spawn.clone()).await {
                Ok(x) => break x,
                Err(e) if tray::serve_again(&e, tries) => {
                    tries += 1;
                    glib::timeout_future(tray::SERVE_GAP).await;
                }
                Err(e) => {
                    state(|s| s.starting = false);
                    crate::diag::say!("wol-shell: tray: {e}");
                    return;
                }
            }
        };
        let keep = state(|s| {
            s.starting = false;
            let keep = s.enabled && s.run == run;
            if keep {
                s.tray = Some(t.clone());
            }
            keep
        });
        if !keep {
            t.stop().await;
            return;
        }
        while let Ok(mut items) = rx.recv().await {
            while let Ok(newer) = rx.try_recv() {
                items = newer;
            }
            if !state(|s| {
                let current = s.run == run && s.enabled;
                if current {
                    s.items = items;
                }
                current
            }) {
                break;
            }
            render_all();
        }
    });
}

fn render_all() {
    let (items, on) = state(|s| (s.items.clone(), s.enabled));
    for st in super::menubar::tray_strips() {
        st.update(&items, on);
    }
}

/// One bar's items.
pub struct Strip {
    pub root: gtk::Box,
    slots: RefCell<Vec<Slot>>,
}

struct Slot {
    id: String,
    button: gtk::Button,
    item: Rc<RefCell<Item>>,
}

/// A bar's strip with the items there are now; it follows every change while the bar lives.
pub fn strip() -> Rc<Strip> {
    let st = fence::guard("tray", || {
        let st = Rc::new(Strip {
            root: w::hbox(0),
            slots: RefCell::default(),
        });
        let (items, on) = state(|s| (s.items.clone(), s.enabled));
        st.update(&items, on);
        st
    });
    // A strip that stopped as it was made is an empty one: the bar keeps its other items.
    st.unwrap_or_else(|| {
        let root = w::hbox(0);
        root.set_visible(false);
        Rc::new(Strip {
            root,
            slots: RefCell::default(),
        })
    })
}

impl Strip {
    /// Shows `items` (the passive ones left out): buttons kept by id, redrawn when their item
    /// changed, in registration order.
    fn update(&self, items: &[Item], on: bool) {
        let shown: Vec<&Item> = items.iter().filter(|i| i.shown()).collect();
        let mut slots = self.slots.borrow_mut();
        slots.retain(|s| {
            let keep = shown.iter().any(|i| i.id == s.id);
            if !keep {
                drop_popovers(&s.button);
                self.root.remove(&s.button);
            }
            keep
        });
        let mut order = Vec::with_capacity(shown.len());
        for it in &shown {
            match slots.iter().position(|s| s.id == it.id) {
                Some(n) => {
                    if *slots[n].item.borrow() != **it {
                        slots[n].item.replace((*it).clone());
                        paint(&slots[n].button, it);
                    }
                    order.push(n);
                }
                None => {
                    let item = Rc::new(RefCell::new((*it).clone()));
                    slots.push(Slot {
                        id: it.id.clone(),
                        button: button(item.clone()),
                        item,
                    });
                    order.push(slots.len() - 1);
                }
            }
        }
        let now: Vec<gtk::Widget> =
            std::iter::successors(self.root.first_child(), |c| c.next_sibling()).collect();
        let want: Vec<gtk::Widget> = order
            .iter()
            .map(|&n| slots[n].button.clone().upcast())
            .collect();
        if now != want {
            for c in &now {
                self.root.remove(c);
            }
            for c in &want {
                self.root.append(c);
            }
        }
        self.root.set_visible(on && !want.is_empty());
    }
}

/// Unparents the menus a button holds (before it goes, or before a new menu opens).
fn drop_popovers(b: &gtk::Button) {
    let mut c = b.first_child();
    while let Some(w) = c {
        c = w.next_sibling();
        if w.is::<gtk::Popover>() {
            w.unparent();
        }
    }
}

/// The item's picture: its icon by name (the WolOS theme or the app's own folder), a file, the
/// picture it sent, else the package icon.
fn icon(it: &Item) -> gtk::Widget {
    let name = it.icon_name.as_str();
    if let Some(display) = gdk::Display::default() {
        let theme = gtk::IconTheme::for_display(&display);
        if !it.theme_path.is_empty()
            && state(|s| {
                let new = !s.theme_paths.contains(&it.theme_path);
                if new {
                    s.theme_paths.push(it.theme_path.clone());
                }
                new
            })
        {
            theme.add_search_path(&it.theme_path);
        }
        if !name.is_empty() && !name.starts_with('/') && theme.has_icon(name) {
            return w::icon(name, w::ICON).upcast();
        }
    }
    let texture: Option<gdk::Texture> = if name.starts_with('/') {
        gdk::Texture::from_filename(name).ok()
    } else {
        None
    };
    let texture = texture.or_else(|| {
        it.pixmap.as_ref().map(|p| {
            gdk::MemoryTexture::new(
                p.width,
                p.height,
                gdk::MemoryFormat::R8g8b8a8,
                &glib::Bytes::from(&p.rgba),
                usize::try_from(p.width).map_or(0, |w| w * 4),
            )
            .upcast()
        })
    });
    match texture {
        Some(t) => {
            let img = gtk::Image::from_paintable(Some(&t));
            img.set_pixel_size(w::ICON);
            img.set_valign(gtk::Align::Center);
            img.upcast()
        }
        None => w::app_icon(w::PACKAGE_ICON, w::ICON).upcast(),
    }
}

fn paint(b: &gtk::Button, it: &Item) {
    b.set_child(Some(&icon(it)));
    b.set_tooltip_text(Some(&it.tooltip));
}

/// A bar button for `item`; its handlers read the item as it is when they run.
fn button(item: Rc<RefCell<Item>>) -> gtk::Button {
    let b = w::menubar_item(&gtk::Image::new());
    paint(&b, &item.borrow());
    {
        let item = item.clone();
        b.connect_clicked(move |b| {
            let it = item.borrow().clone();
            if it.is_menu && it.menu.is_some() {
                open_menu(b, it.id);
                return;
            }
            let Some(t) = tray_now() else { return };
            let wb = b.downgrade();
            glib::spawn_future_local(async move {
                // An item that does not take the click shows its menu instead.
                if t.activate(&it.id).await.is_err()
                    && it.menu.is_some()
                    && let Some(b) = wb.upgrade()
                {
                    open_menu(&b, it.id);
                }
            });
        });
    }
    let click = gtk::GestureClick::new();
    click.set_button(0);
    {
        let (item, wb) = (item.clone(), b.downgrade());
        click.connect_pressed(move |g, _, _, _| {
            let (Some(b), Some(t)) = (wb.upgrade(), tray_now()) else {
                return;
            };
            let it = item.borrow().clone();
            match g.current_button() {
                gdk::BUTTON_SECONDARY if it.menu.is_some() => open_menu(&b, it.id),
                gdk::BUTTON_SECONDARY => {
                    glib::spawn_future_local(async move {
                        let _ = t.context_menu(&it.id).await;
                    });
                }
                gdk::BUTTON_MIDDLE => {
                    glib::spawn_future_local(async move {
                        let _ = t.secondary_activate(&it.id).await;
                    });
                }
                _ => {}
            }
        });
    }
    b.add_controller(click);
    let scroll = gtk::EventControllerScroll::new(
        gtk::EventControllerScrollFlags::BOTH_AXES | gtk::EventControllerScrollFlags::DISCRETE,
    );
    scroll.connect_scroll(move |_, dx, dy| {
        if let (Some((delta, dir)), Some(t)) = (tray::scroll_args(dx, dy), tray_now()) {
            let id = item.borrow().id.clone();
            glib::spawn_future_local(async move {
                let _ = t.scroll(&id, delta, dir).await;
            });
        }
        glib::Propagation::Stop
    });
    b.add_controller(scroll);
    b
}

/// Reads item `id`'s menu and opens it under `b`; an item without one draws its own.
fn open_menu(b: &gtk::Button, id: String) {
    let Some(t) = tray_now() else { return };
    let wb = b.downgrade();
    glib::spawn_future_local(async move {
        match t.menu(&id).await {
            Ok(Some(root)) if !root.children.is_empty() => {
                if let Some(b) = wb.upgrade() {
                    show_menu(&b, &t, &id, &root);
                }
            }
            _ => {
                let _ = t.context_menu(&id).await;
            }
        }
    });
}

fn show_menu(anchor: &gtk::Button, t: &Tray, id: &str, root: &MenuNode) {
    drop_popovers(anchor);
    let pop = gtk::Popover::new();
    pop.add_css_class("menu");
    pop.set_has_arrow(false);
    let stack = gtk::Stack::new();
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    stack.set_interpolate_size(true);
    stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    page(&stack, &pop, t, id, root, "root", None);
    stack.set_visible_child_name("root");
    pop.set_child(Some(&stack));
    pop.set_parent(anchor);
    pop.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || {
            if p.parent().is_some() {
                p.unparent();
            }
        });
    });
    pop.popup();
}

/// Menu row: a kit menu item holding `parts`.
fn row(parts: &[gtk::Widget]) -> gtk::Button {
    let r = w::hbox(w::S2);
    for p in parts {
        r.append(p);
    }
    let it = gtk::Button::builder().child(&r).build();
    it.add_css_class("menu-item");
    it
}

/// Adds `node`'s rows as page `name`; a submenu is a page of its own, `back` = (page, label) of
/// the row that goes up again.
fn page(
    stack: &gtk::Stack,
    pop: &gtk::Popover,
    t: &Tray,
    id: &str,
    node: &MenuNode,
    name: &str,
    back: Option<(&str, &str)>,
) {
    let col = w::vbox(0);
    if let Some((up, label)) = back {
        let r = row(&[
            w::icon("pan-start-symbolic", w::ICON).upcast(),
            w::label(label, "").upcast(),
        ]);
        let (stack, up) = (stack.clone(), up.to_string());
        r.connect_clicked(move |_| stack.set_visible_child_name(&up));
        col.append(&r);
        col.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    }
    let toggles = node.children.iter().any(|c| c.toggle != Toggle::None);
    for c in &node.children {
        if c.separator {
            col.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
            continue;
        }
        let mut parts: Vec<gtk::Widget> = Vec::new();
        if toggles {
            let mark = w::icon("object-select-symbolic", w::ICON);
            mark.set_opacity(match c.toggle {
                Toggle::Check(true) | Toggle::Radio(true) => 1.0,
                _ => 0.0,
            });
            parts.push(mark.upcast());
        }
        let text = w::label(&c.label, "");
        text.set_hexpand(true);
        text.set_xalign(0.0);
        parts.push(text.upcast());
        if c.children.is_empty() {
            let r = row(&parts);
            let (pop, t, id, row_id) = (pop.clone(), t.clone(), id.to_string(), c.id);
            r.connect_clicked(move |_| {
                pop.popdown();
                let (t, id) = (t.clone(), id.clone());
                glib::spawn_future_local(async move {
                    let _ = t.menu_clicked(&id, row_id).await;
                });
            });
            r.set_sensitive(c.enabled);
            col.append(&r);
        } else {
            let sub = format!("{name}-{}", c.id);
            page(stack, pop, t, id, c, &sub, Some((name, &c.label)));
            parts.push(w::icon("pan-end-symbolic", w::ICON).upcast());
            let r = row(&parts);
            let stack = stack.clone();
            r.connect_clicked(move |_| stack.set_visible_child_name(&sub));
            r.set_sensitive(c.enabled);
            col.append(&r);
        }
    }
    stack.add_named(&col, Some(name));
}
