// SPDX-License-Identifier: GPL-2.0-or-later
//! Kit windows drawn by libWolkit (ADR 0060, WOLKIT-1TO1): one rasteriser for every Kit surface.
//!
//! A `shared::view` tree (what a libwol app sends, what the web page at /kit.html shows) is raised
//! into a [`wolkit_render::page::Page`] and drawn by the CPU reference rasteriser
//! (`wolkit::cpu::raster`); the frame is painted 1:1 into a [`gtk::DrawingArea`]. The web page
//! draws the same page with the same fonts through the same rasteriser, so a Kit window and the
//! page are the same pixels (the shell golden `app-window-parts-*` IS the wolkit frame,
//! `crates/wolkit-render/tests/page.rs` checks it at zero differing pixels).
//!
//! This is the default drawing path for Kit app windows. `WOL_KIT_RENDER=gtk` is the fallback:
//! the tree drawn with GTK widgets (`ui::app_view`), which follows the same spec numbers but
//! rasterises with Cairo and Pango, so its pixels differ.
//!
//! The look follows the system theme live (`kit::style::look`/`on_look`, the same theme the GTK
//! parts load), and input goes to the page as wolkit input records; the page's events become the
//! same [`Act`]s the GTK path sends.
use super::app_view::{Act, Sink};
use crate::kit::style;
use gtk::glib::translate::IntoGlib;
use gtk::{cairo, glib, prelude::*};
use shared::view::Node;
use std::cell::RefCell;
use std::rc::{Rc, Weak};
use wolkit::window::{
    KeyInput, POINTER_DOWN, POINTER_LEAVE, POINTER_MOVE, POINTER_SCROLL, POINTER_UP, PointerInput,
};
use wolkit_render::Renderer;
use wolkit_render::cpu::CpuRenderer;
use wolkit_render::page::{Page, PageEvent};

/// The switch between the two drawing paths: unset or `wolkit` draws with libWolkit (the
/// default), `gtk` with GTK widgets.
pub const FLAG: &str = "WOL_KIT_RENDER";

/// Whether Kit app windows draw through libWolkit.
pub fn enabled() -> bool {
    enabled_for(crate::envv::var(FLAG).as_deref())
}

fn enabled_for(v: Option<&str>) -> bool {
    !matches!(v.map(str::trim), Some("gtk"))
}

/// The width an app window opens at when its content does not say (the libwol sample window).
pub const DEFAULT_WIDTH: i32 = 420;

/// Font files of the Kit page, the same bytes the web page loads (`site/public/kit/fonts`):
/// Inter Regular and SemiBold, static OTF. One set on both sides is what makes text identical.
pub const FONT_FILES: [&str; 2] = ["Inter-Regular.otf", "Inter-SemiBold.otf"];

/// Where the font files are looked for: `WOLKIT_PAGE_FONTS`, the installed copy, the checkout.
pub fn font_dirs() -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    if let Some(d) = crate::envv::var("WOLKIT_PAGE_FONTS") {
        v.push(d.into());
    }
    v.push("/usr/share/wolos/fonts/kit".into());
    v.push(concat!(env!("CARGO_MANIFEST_DIR"), "/../wolkit-render/fonts").into());
    v
}

/// The same two files, built into the shell: an installed system without the font directory
/// still draws Kit windows through libWolkit (the default path never falls back silently).
const BUILT_IN: [&[u8]; 2] = [
    include_bytes!("../../../wolkit-render/fonts/Inter-Regular.otf"),
    include_bytes!("../../../wolkit-render/fonts/Inter-SemiBold.otf"),
];

/// The font files' bytes from the first directory that has both, else the built-in copy.
pub fn fonts() -> Option<Vec<Vec<u8>>> {
    font_dirs()
        .into_iter()
        .find_map(|d| {
            FONT_FILES
                .iter()
                .map(|f| std::fs::read(d.join(f)).ok())
                .collect::<Option<Vec<_>>>()
        })
        .or_else(|| Some(BUILT_IN.iter().map(|b| b.to_vec()).collect()))
}

/// Named icons of a tree, read from the icon theme so the page can draw them (`icon:<name>`).
fn register_icons(node: &Node) {
    let mut names = Vec::new();
    collect_icons(node, &mut names);
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    let theme = gtk::IconTheme::for_display(&display);
    for name in names {
        let paintable = theme.lookup_icon(
            &name,
            &[],
            16,
            1,
            gtk::TextDirection::Ltr,
            gtk::IconLookupFlags::FORCE_SYMBOLIC,
        );
        let Some(path) = paintable.file().and_then(|f| f.path()) else {
            continue;
        };
        if let Ok(svg) = std::fs::read_to_string(&path) {
            wolkit::image::icon::register(&format!("{name}.svg"), &svg);
        }
    }
}

fn collect_icons(node: &Node, out: &mut Vec<String>) {
    match node {
        Node::Image { source, .. } if !source.contains('/') => {
            out.push(source.trim_start_matches("icon:").to_owned())
        }
        Node::Stack { children, .. }
        | Node::Row { children, .. }
        | Node::List { children }
        | Node::Card { children, .. }
        | Node::Tabs { children, .. }
        | Node::Disclosure { children, .. } => {
            for c in children {
                collect_icons(c, out);
            }
        }
        _ => {}
    }
}

/// The page's pixels (premultiplied RGBA) as a Cairo image (premultiplied ARGB32, native order).
fn surface(img: &wolkit_render::Image, scale: f64) -> Option<cairo::ImageSurface> {
    let (w, h) = (
        i32::try_from(img.width).ok()?,
        i32::try_from(img.height).ok()?,
    );
    let stride = cairo::Format::ARgb32.stride_for_width(img.width).ok()?;
    let row = usize::try_from(stride).ok()?;
    let width = usize::try_from(img.width).ok()?;
    let mut data = vec![0u8; row * usize::try_from(h).ok()?];
    for (y, out) in data.chunks_exact_mut(row).enumerate() {
        let src = img.data.get(y * width * 4..(y + 1) * width * 4)?;
        for (o, p) in out
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(src.as_chunks::<4>().0)
        {
            let argb = u32::from_be_bytes([p[3], p[0], p[1], p[2]]);
            o.copy_from_slice(&argb.to_ne_bytes());
        }
    }
    let s = cairo::ImageSurface::create_for_data(data, cairo::Format::ARgb32, w, h, stride).ok()?;
    s.set_device_scale(scale, scale);
    Some(s)
}

struct State {
    page: Page,
    tree: Node,
    sink: Rc<Sink>,
    frame: Option<cairo::ImageSurface>,
    /// (logical width, height, scale) the frame was drawn at.
    drawn_at: (i32, i32, i32),
    dirty: bool,
}

impl State {
    fn frame(&mut self, w: i32, h: i32, scale: i32) -> Option<&cairo::ImageSurface> {
        let at = (w.max(1), h.max(1), scale.max(1));
        if at != self.drawn_at {
            let u = |v: i32| u32::try_from(v).unwrap_or(1);
            self.page.resize(u(at.0), u(at.1), at.2 as f32);
            self.drawn_at = at;
            self.dirty = true;
        }
        if self.dirty || self.frame.is_none() {
            let img = self.page.draw(|f| CpuRenderer.render_image(f)).ok()?;
            self.frame = surface(&img, f64::from(at.2));
            self.dirty = false;
        }
        self.frame.as_ref()
    }

    /// The page's events as the Kit host's acts.
    fn send(&self, ev: &[PageEvent]) {
        use wolkit::event::kind as k;
        for e in ev {
            let act = match e.kind {
                k::CLICK => match self.page.pick_for(e.node) {
                    // The design-system parts keep their state in the app's tree: the click asks
                    // for the next state and the app's next tree shows it.
                    Some(wolkit_render::view::Pick::Toggled { id, on }) => Act::Toggled(*id, *on),
                    Some(wolkit_render::view::Pick::Picked { id, key }) => {
                        Act::Picked(*id, key.clone())
                    }
                    None => Act::Clicked(e.node),
                },
                k::TOGGLE => Act::Toggled(e.node, self.page.checked(e.node)),
                k::SLIDER => match self.page.slider_value(e.node) {
                    Some(v) => Act::Slid(e.node, f64::from(v)),
                    None => continue,
                },
                k::TEXT_CHANGED => {
                    Act::Edited(e.node, self.page.text_of(e.node).unwrap_or_default())
                }
                _ => continue,
            };
            self.sink.act(act);
        }
    }
}

/// A view tree drawn by libWolkit in one GTK widget.
pub struct Host {
    pub area: gtk::DrawingArea,
    state: Rc<RefCell<State>>,
    /// The size the window opens at: [`DEFAULT_WIDTH`] and the content's height there.
    pub natural: (i32, i32),
}

fn mods(m: gtk::gdk::ModifierType) -> u32 {
    use gtk::gdk::ModifierType as M;
    use wolkit::window::{MOD_ALT, MOD_CTRL, MOD_SHIFT, MOD_SUPER};
    (if m.contains(M::SHIFT_MASK) {
        MOD_SHIFT
    } else {
        0
    }) | (if m.contains(M::CONTROL_MASK) {
        MOD_CTRL
    } else {
        0
    }) | (if m.contains(M::ALT_MASK) { MOD_ALT } else { 0 })
        | (if m.contains(M::SUPER_MASK) {
            MOD_SUPER
        } else {
            0
        })
}

impl Host {
    /// None without the Kit fonts (the caller falls back to GTK widgets).
    pub fn new(tree: &Node, sink: Rc<Sink>) -> Option<Host> {
        let look = style::look();
        let mut page = Page::new(
            u32::try_from(DEFAULT_WIDTH).unwrap_or(420),
            1,
            1.0,
            look.dark,
            fonts()?,
        )
        .ok()?;
        page.set_theme(look.dark, look.accent);
        register_icons(tree);
        page.mount(tree).ok()?;
        let area = gtk::DrawingArea::new();
        area.set_hexpand(true);
        area.set_vexpand(true);
        area.set_focusable(true);
        // No content size: GTK would take it as the minimum too and a window could not narrow
        // below it; the window opens at [`Host::natural`] instead (`ui::app_host`).
        let natural = (DEFAULT_WIDTH, natural_height(&mut page, DEFAULT_WIDTH));
        let state = Rc::new(RefCell::new(State {
            page,
            tree: tree.clone(),
            sink,
            frame: None,
            drawn_at: (0, 0, 0),
            dirty: true,
        }));
        {
            let st = Rc::downgrade(&state);
            area.set_draw_func(move |a, cr, w, h| {
                let Some(st) = st.upgrade() else { return };
                let mut st = st.borrow_mut();
                if let Some(s) = st.frame(w, h, a.scale_factor()) {
                    // 1:1: the frame is the widget's pixels, no filter, no blend over anything.
                    let _ = cr.set_source_surface(s, 0.0, 0.0);
                    cr.set_operator(cairo::Operator::Source);
                    let _ = cr.paint();
                }
            });
        }
        let host = Host {
            area,
            state,
            natural,
        };
        host.input();
        host.follow_theme();
        Some(host)
    }

    /// Shows `tree` (a new version from the app).
    pub fn update(&self, tree: &Node) {
        let mut st = self.state.borrow_mut();
        if st.tree == *tree {
            return;
        }
        register_icons(tree);
        let (w, h, s) = st.drawn_at;
        // A fresh page keeps the layout honest when the shape changed; the size and look carry over.
        let look = style::look();
        if let Some(mut p) = fonts().and_then(|f| {
            Page::new(
                u32::try_from(w.max(DEFAULT_WIDTH)).unwrap_or(420),
                u32::try_from(h.max(1)).unwrap_or(1),
                s.max(1) as f32,
                look.dark,
                f,
            )
            .ok()
        }) {
            p.set_theme(look.dark, look.accent);
            if p.mount(tree).is_ok() {
                st.page = p;
                st.tree = tree.clone();
                st.drawn_at = (0, 0, 0);
                st.dirty = true;
            }
        }
        drop(st);
        self.area.queue_draw();
    }

    fn follow_theme(&self) {
        let (st, area) = (Rc::downgrade(&self.state), self.area.downgrade());
        style::on_look(move |look| {
            let (Some(st), Some(area)) = (st.upgrade(), area.upgrade()) else {
                return false;
            };
            let mut s = st.borrow_mut();
            if s.page.set_theme(look.dark, look.accent) {
                s.dirty = true;
                area.queue_draw();
            }
            true
        });
    }

    fn input(&self) {
        let pointer = |st: &Weak<RefCell<State>>, area: &gtk::DrawingArea, p: PointerInput| {
            let Some(st) = st.upgrade() else { return };
            let mut s = st.borrow_mut();
            let ev = s.page.pointer(p);
            s.dirty = true;
            s.send(&ev);
            drop(s);
            animate(&st, area);
            area.queue_draw();
        };
        let motion = gtk::EventControllerMotion::new();
        {
            let (st, area) = (Rc::downgrade(&self.state), self.area.clone());
            motion.connect_motion(move |c, x, y| {
                pointer(
                    &st,
                    &area,
                    PointerInput {
                        kind: POINTER_MOVE,
                        x: x as f32,
                        y: y as f32,
                        button: 0,
                        mods: mods(c.current_event_state()),
                    },
                );
            });
        }
        {
            let (st, area) = (Rc::downgrade(&self.state), self.area.clone());
            motion.connect_leave(move |_| {
                pointer(
                    &st,
                    &area,
                    PointerInput {
                        kind: POINTER_LEAVE,
                        x: 0.0,
                        y: 0.0,
                        button: 0,
                        mods: 0,
                    },
                );
            });
        }
        self.area.add_controller(motion);
        let click = gtk::GestureClick::new();
        click.set_button(0);
        {
            let (st, area) = (Rc::downgrade(&self.state), self.area.clone());
            click.connect_pressed(move |g, _, x, y| {
                area.grab_focus();
                pointer(
                    &st,
                    &area,
                    PointerInput {
                        kind: POINTER_DOWN,
                        x: x as f32,
                        y: y as f32,
                        button: g.current_button(),
                        mods: mods(g.current_event_state()),
                    },
                );
            });
        }
        {
            let (st, area) = (Rc::downgrade(&self.state), self.area.clone());
            click.connect_released(move |g, _, x, y| {
                pointer(
                    &st,
                    &area,
                    PointerInput {
                        kind: POINTER_UP,
                        x: x as f32,
                        y: y as f32,
                        button: g.current_button(),
                        mods: mods(g.current_event_state()),
                    },
                );
            });
        }
        self.area.add_controller(click);
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        {
            let (st, area) = (Rc::downgrade(&self.state), self.area.clone());
            scroll.connect_scroll(move |_, dx, dy| {
                // wolkit takes the deltas in the x and y of the record, logical pixels.
                pointer(
                    &st,
                    &area,
                    PointerInput {
                        kind: POINTER_SCROLL,
                        x: (dx * 40.0) as f32,
                        y: (dy * 40.0) as f32,
                        button: 0,
                        mods: 0,
                    },
                );
                glib::Propagation::Stop
            });
        }
        self.area.add_controller(scroll);
        let keys = gtk::EventControllerKey::new();
        let key = |st: &Weak<RefCell<State>>,
                   area: &gtk::DrawingArea,
                   keyval: gtk::gdk::Key,
                   m: gtk::gdk::ModifierType,
                   pressed: bool| {
            let Some(st) = st.upgrade() else { return false };
            let mut s = st.borrow_mut();
            let mods = mods(m);
            let plain = mods
                & (wolkit::window::MOD_CTRL | wolkit::window::MOD_ALT | wolkit::window::MOD_SUPER)
                == 0;
            let ev = match keyval.to_unicode().filter(|c| !c.is_control() && plain) {
                Some(c) if pressed && c != ' ' => s.page.text(&c.to_string()),
                Some(c) if !pressed && c != ' ' => Vec::new(),
                _ => s.page.key(KeyInput {
                    keysym: keyval.into_glib(),
                    pressed,
                    mods,
                }),
            };
            let handled = !ev.is_empty();
            s.dirty = true;
            s.send(&ev);
            drop(s);
            animate(&st, area);
            area.queue_draw();
            handled
        };
        {
            let (st, area) = (Rc::downgrade(&self.state), self.area.clone());
            keys.connect_key_pressed(move |_, k, _, m| {
                if key(&st, &area, k, m, true) {
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
        }
        {
            let (st, area) = (Rc::downgrade(&self.state), self.area.clone());
            keys.connect_key_released(move |_, k, _, m| {
                key(&st, &area, k, m, false);
            });
        }
        self.area.add_controller(keys);
    }
}

/// Moves the page's animations on with the frame clock for a short while after input (a switch
/// thumb slides); with GTK animations off the page jumps to the end at once.
fn animate(st: &Rc<RefCell<State>>, area: &gtk::DrawingArea) {
    let on = gtk::Settings::default().is_none_or(|s| s.is_gtk_enable_animations());
    if !on {
        let mut s = st.borrow_mut();
        s.page.advance(1_000_000_000);
        s.dirty = true;
        return;
    }
    let st = Rc::downgrade(st);
    // (first frame, previous frame) times in microseconds.
    let times = std::cell::Cell::new(None::<(i64, i64)>);
    area.add_tick_callback(move |a, clock| {
        let Some(st) = st.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let now = clock.frame_time();
        let (first, prev) = times.get().unwrap_or((now, now));
        times.set(Some((first, now)));
        let mut s = st.borrow_mut();
        s.page
            .advance(u64::try_from((now - prev).max(0)).unwrap_or(0) * 1000);
        s.dirty = true;
        a.queue_draw();
        if now - first > 600_000 {
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}

/// Height the page's content needs at `width` (its last part's bottom plus the page's bottom
/// padding), capped like the GTK path's scrolled window (`APP_MAX_HEIGHT`).
fn natural_height(page: &mut Page, width: i32) -> i32 {
    let w = u32::try_from(width).unwrap_or(420);
    page.resize(w, 4096, 1.0);
    let h = page.content_height().unwrap_or(380.0);
    i32::try_from(wolkit::num::round_u32(h.ceil()))
        .unwrap_or(super::app_view::APP_MAX_HEIGHT)
        .clamp(1, super::app_view::APP_MAX_HEIGHT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wolkit_is_the_default_and_gtk_the_fallback() {
        assert!(enabled_for(None));
        assert!(enabled_for(Some("wolkit")));
        assert!(!enabled_for(Some("gtk")));
        assert!(!enabled_for(Some(" gtk\n")));
    }

    #[test]
    fn the_kit_fonts_are_in_the_checkout() {
        assert!(fonts().is_some(), "{:?}", font_dirs());
    }

    #[test]
    fn the_built_in_fonts_are_the_checkout_files() {
        // One set of bytes on every side (the web page loads the same files).
        let dir = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../wolkit-render/fonts"
        ));
        for (f, b) in FONT_FILES.iter().zip(BUILT_IN) {
            assert_eq!(std::fs::read(dir.join(f)).unwrap(), b, "{f}");
        }
    }
}
