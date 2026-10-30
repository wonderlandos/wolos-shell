//! Dock: pinned apps, running dots from Hyprland clients, separator, the Downloads stack, Trash.
use crate::apps::{self, PINS};
use crate::kit::{self, surface::Spec, widgets as w};
use gtk::{gio, glib, prelude::*};
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

/// The stack's popover body (macOS grid): newest files, a click opens one; Open in Files last.
pub fn stack_grid(items: &[Item], dir: &str, done: Rc<dyn Fn()>) -> gtk::Box {
    let b = w::vbox(w::S2);
    b.add_css_class("downloads-stack");
    b.append(&w::label("Downloads", "title"));
    if items.is_empty() {
        b.append(&w::label("Nothing downloaded yet.", "secondary"));
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
    tiles: Vec<w::DockTile>,
    /// Per tile: bumped on every start and stop, so a stale 60 s timer cannot stop a newer bounce.
    epochs: Vec<std::cell::Cell<u32>>,
    /// Newest files in Downloads (the stack).
    pub downloads: Rc<RefCell<Vec<Item>>>,
    monitor: RefCell<Option<gio::FileMonitor>>,
}

pub fn content(running: &HashSet<String>) -> Rc<Dock> {
    let root = w::hbox(w::S2);
    root.add_css_class("dock-shelf");
    root.add_css_class("no-row-check"); // tiles lift on hover by design
    root.set_halign(gtk::Align::Center);
    root.set_valign(gtk::Align::End);
    let tiles: Vec<w::DockTile> = PINS
        .iter()
        .map(|p| {
            let argv: Vec<String> = p.exec.iter().map(|s| s.to_string()).collect();
            w::DockTile::new(p.icon, p.name, move || kit::spawn(&argv))
        })
        .collect();
    for t in &tiles {
        root.append(&t.button);
        let tile = t.clone();
        t.button.connect_clicked(move |_| launched(&tile));
    }
    root.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    let downloads: Rc<RefCell<Vec<Item>>> = Default::default();
    root.append(&downloads_tile(downloads_dir(), downloads.clone()));
    root.append(
        &w::DockTile::new("wolos-trash", "Trash", || {
            kit::spawn(
                &apps::TRASH
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>(),
            )
        })
        .button,
    );
    let d = Rc::new(Dock {
        root,
        epochs: tiles.iter().map(|_| Default::default()).collect(),
        tiles,
        downloads,
        monitor: RefCell::new(None),
    });
    d.set_running(running);
    d
}

/// The Dock window, its tiles, and its keyboard access (⌃F2/⌃F3 style, see [`keyboard`]).
pub type Built = (gtk::ApplicationWindow, Rc<Dock>, Rc<dyn Fn(bool)>);

pub fn build(app: &gtk::Application) -> Built {
    let win = super::window(app, "dock", Spec::dock());
    let d = content(&crate::hypr::running_classes(&crate::hypr::clients()));
    *d.monitor.borrow_mut() = watch_downloads(&downloads_dir(), d.downloads.clone());
    // Autohide: the shelf slides away and a thin hot zone at the bottom edge brings it back.
    let reveal = gtk::Revealer::builder()
        .child(&d.root)
        .transition_type(gtk::RevealerTransitionType::SlideUp)
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
    {
        let (root, win, d) = (d.root.clone(), win.clone(), d.clone());
        super::ipc::on_reload(move |p| {
            hide.set(p.dock_autohide);
            delay.set(p.autohide_delay_ms());
            d.set_tile_size(p.tile_size());
            reveal.set_reveal_child(!p.dock_autohide);
            kit::surface::set_exclusive(&win, !p.dock_autohide);
            set_magnify(&root, p.dock_magnify);
        });
    }
    (win, d, keys)
}

/// ⌃F3 (`wol shell show dock`): the Dock shows and takes the keyboard, the first app focused;
/// arrows move, Return opens the app and gives the keyboard back, as does Esc.
fn keyboard(
    win: &gtk::ApplicationWindow,
    d: &Dock,
    reveal: &gtk::Revealer,
    hide: &Rc<std::cell::Cell<bool>>,
) -> Rc<dyn Fn(bool)> {
    let Some(first) = d.tiles.first() else {
        return Rc::new(|_| {});
    };
    let (reveal, hide) = (reveal.clone(), hide.clone());
    // Autohide: shown while it has the keyboard.
    let keys = kit::surface::keyboard_access(win, &first.button, move |on| {
        reveal.set_reveal_child(on || !hide.get())
    });
    let mut c = d.root.first_child();
    while let Some(w) = c {
        if let Some(b) = w.downcast_ref::<gtk::Button>() {
            let k = keys.clone();
            b.connect_clicked(move |_| k(false));
        }
        c = w.next_sibling();
    }
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

/// Settings > Dock > Magnification: tiles lift on hover only when on.
pub fn set_magnify(root: &gtk::Box, on: bool) {
    if on {
        root.remove_css_class("flat")
    } else {
        root.add_css_class("flat")
    }
}

impl Dock {
    /// Settings > Dock > Size.
    pub fn set_tile_size(&self, px: u32) {
        for t in &self.tiles {
            t.set_icon_size(px as i32);
        }
    }

    fn tile_for(&self, class: &str) -> Option<(usize, &w::DockTile)> {
        let classes = HashSet::from([class.to_lowercase()]);
        PINS.iter()
            .zip(&self.tiles)
            .position(|(p, _)| apps::is_running(p, &classes))
            .map(|i| (i, &self.tiles[i]))
    }

    /// A window of `class` asks for attention: its tile bounces until it is focused
    /// ([`Dock::focused`]) or [`ATTENTION_MAX_SECS`] pass.
    pub fn attention(self: &Rc<Self>, class: &str) {
        let Some((i, t)) = self.tile_for(class) else {
            return;
        };
        if t.is_attention() {
            return;
        }
        t.set_attention(true);
        let epoch = self.epochs[i].get().wrapping_add(1);
        self.epochs[i].set(epoch);
        let d = Rc::downgrade(self);
        glib::timeout_add_seconds_local_once(ATTENTION_MAX_SECS, move || {
            if let Some(d) = d.upgrade().filter(|d| d.epochs[i].get() == epoch) {
                d.tiles[i].set_attention(false);
            }
        });
    }

    /// The window of `class` came to the front: its bounce stops.
    pub fn focused(&self, class: &str) {
        if let Some((i, t)) = self.tile_for(class).filter(|(_, t)| t.is_attention()) {
            self.epochs[i].set(self.epochs[i].get().wrapping_add(1));
            t.set_attention(false);
        }
    }

    pub fn set_running(&self, classes: &HashSet<String>) {
        for (p, t) in PINS.iter().zip(&self.tiles) {
            t.set_running(apps::is_running(p, classes));
        }
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
