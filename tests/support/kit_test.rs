// SPDX-License-Identifier: GPL-2.0-or-later
//! The Kit's widget tester (WolOS Design System, ADR 0058 draft): one component on a private
//! headless display, no app around it. Mount it in a frame (width, density, light or dark), act on
//! it the way a person does (Tab, Space, a click), and read back what the design system specifies:
//! the measured view tree (`kit::check::tree`, the layout rules of `crate::layout`), the
//! accessibility roles, relations and states (GTK's own test accessible API),
//! and the picture (the golden runner's Cairo render, compared with the same tolerance).
//!
//! The specs are data: `design/components/catalog.json` (written by `design/tools/components.py`
//! from `design/components/*.toml`) says which states, densities and variants a component has and
//! what each must measure; a test builds each case and this module checks it against the spec.
//! Include with `#[path = "support/kit_test.rs"] mod kit_test;` next to `support/broadway.rs`.
#![allow(dead_code)] // not every component test calls every helper
use gtk::{gdk, glib, prelude::*};
use shell::kit::{self, widgets as w};
use shell::theme::{self, Variant};
use std::path::{Path, PathBuf};

/// Broadway display range of this test binary (golden 40-239, others 240-859).
pub const DISPLAYS: u32 = 860;
/// Share of pixels that may differ (any channel by more than 24) before a golden fails: the golden
/// runner's tolerance (`tests/golden.rs`).
pub const TOLERANCE: f64 = 0.005;

/// One density of the design system (`density.<name>` in a spec): a class on the window, so a
/// whole window changes together and a component never picks its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Density {
    Compact,
    Standard,
    Comfortable,
}

impl Density {
    pub const ALL: [Density; 3] = [Density::Compact, Density::Standard, Density::Comfortable];
    pub fn name(self) -> &'static str {
        match self {
            Density::Compact => "compact",
            Density::Standard => "standard",
            Density::Comfortable => "comfortable",
        }
    }
    pub fn parse(s: &str) -> Option<Density> {
        Self::ALL.into_iter().find(|d| d.name() == s)
    }
    /// The window class; standard has none (it is today's kit.css).
    pub fn class(self) -> Option<&'static str> {
        match self {
            Density::Compact => Some("density-compact"),
            Density::Standard => None,
            Density::Comfortable => Some("density-comfortable"),
        }
    }
}

/// The density rules this proof of concept adds to the Kit stylesheet (work package DS-GTK-1 moves
/// them into kit.css): rows and their controls follow the window's density class; type never
/// changes with density. GTK's `min-height` is a minimum of the content box (padding and border
/// come on top), so a row of `row` px with `pad_y` above and below asks for `row - 2 * pad_y`.
/// Compact rows are 32 (24 + 4 + 4) and carry the small switch (24 x 16: content 20 x 12 + 2);
/// comfortable rows are 48 (32 + 8 + 8). Standard is today's kit.css, unchanged.
pub const DENSITY_CSS: &str = "\
.density-compact .list-row { min-height: 24px; padding-top: 4px; padding-bottom: 4px; }
.density-compact switch.toggle { min-width: 20px; min-height: 12px; }
.density-compact switch.toggle slider { min-width: 12px; min-height: 12px; }
.density-compact switch.toggle image { -gtk-icon-size: 12px; }
.density-comfortable .list-row { min-height: 32px; padding-top: 8px; padding-bottom: 8px; }
";

/// Where one case is drawn.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub width: i32,
    pub density: Density,
    pub variant: Variant,
}

impl Frame {
    pub fn tag(&self) -> String {
        format!(
            "{}-{}",
            self.density.name(),
            if self.variant == Variant::Light {
                "light"
            } else {
                "dark"
            }
        )
    }
}

/// The catalog of component specs (`design/components/catalog.json`).
pub fn catalog() -> serde_json::Value {
    let path = std::env::var_os("WOL_KIT_CATALOG").map_or_else(
        || {
            PathBuf::from(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../design/components/catalog.json"
            ))
        },
        PathBuf::from,
    );
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e} (run design/tools/components.py)", path.display()));
    serde_json::from_str(&text).expect("catalog.json is JSON")
}

/// One component's spec by id.
pub fn spec(id: &str) -> serde_json::Value {
    catalog()["components"]
        .as_array()
        .and_then(|a| a.iter().find(|c| c["id"] == id).cloned())
        .unwrap_or_else(|| panic!("no spec for {id} in design/components"))
}

/// Strings of a spec array (`tests.states` and the like).
pub fn strings(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// The running tester: the display and where pictures and trees go.
pub struct Tester {
    _display: super::broadway::Display,
    pub out: PathBuf,
    pub golden: PathBuf,
    pub update: bool,
    /// The variant the stylesheet is applied for: applying it again is a second of nothing.
    themed: std::cell::Cell<Option<bool>>,
}

impl Tester {
    /// A private Broadway display, the test accessibility context and GTK; None (the test skips)
    /// without broadwayd. Call once, from the binary's only `#[test]`.
    pub fn start() -> Option<Tester> {
        let display = super::broadway::start(DISPLAYS)?;
        // SAFETY: the binary's only test, before gtk::init: no other thread reads the environment.
        // GTK_A11Y stays unset: under Broadway GTK falls back to its test accessibility context,
        // which keeps roles, relations and states for `gtk::test_accessible_has_*`; with
        // GTK_A11Y=test GTK 4.24.1 kept none (measured: every relation read back false).
        unsafe {
            std::env::remove_var("GTK_A11Y");
            std::env::set_var("WOL_FAKE_NOW", "1");
            std::env::set_var("GTK_THEME", "Adwaita");
        }
        gtk::init().ok()?;
        if let Some(s) = gtk::Settings::default() {
            s.set_gtk_cursor_blink(false);
        }
        gtk::IconTheme::for_display(&gdk::Display::default()?).add_search_path(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../design/icons/symbolic"
        ));
        let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("kit");
        std::fs::create_dir_all(&out).ok()?;
        let golden = std::env::var_os("WOL_KIT_GOLDEN_DIR").map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden-kit"),
            PathBuf::from,
        );
        Some(Tester {
            _display: display,
            out,
            golden,
            update: std::env::var_os("WOL_GOLDEN_UPDATE").is_some(),
            themed: std::cell::Cell::new(None),
        })
    }

    /// The Kit stylesheet on the given theme, with the density rules.
    pub fn theme(&self, variant: Variant) {
        if self.themed.replace(Some(variant == Variant::Light)) == Some(variant == Variant::Light) {
            return;
        }
        let id = if variant == Variant::Light {
            "wol-light"
        } else {
            "wol-dark"
        };
        let t = theme::resolve(Some(id), false);
        let extra = format!("window.preview-app {{ background: none; }}\n{DENSITY_CSS}");
        kit::style::apply(&t, &extra);
    }

    /// Shows `child` alone on an app page (the frame of the golden runner's kit- cases) and waits
    /// until it has drawn.
    pub fn mount(&self, child: &impl IsA<gtk::Widget>, f: Frame) -> Mounted {
        self.theme(f.variant);
        let win = gtk::Window::builder()
            .decorated(false)
            .default_width(f.width)
            .default_height(1)
            .build();
        win.add_css_class("preview-app");
        win.add_css_class("app");
        if let Some(c) = f.density.class() {
            win.add_css_class(c);
        }
        let page = w::vbox(w::S4);
        page.add_css_class("page");
        page.add_css_class("app-window");
        page.append(child);
        win.set_child(Some(&page));
        win.present();
        let m = Mounted {
            win,
            root: page.upcast(),
        };
        m.settle();
        m
    }
}

/// One mounted component.
pub struct Mounted {
    pub win: gtk::Window,
    pub root: gtk::Widget,
}

// Releases the private window the case was mounted in.
impl Drop for Mounted {
    fn drop(&mut self) {
        self.win.destroy();
    }
}

fn walk(w: &gtk::Widget, out: &mut Vec<gtk::Widget>) {
    out.push(w.clone());
    let mut c = w.first_child();
    while let Some(ch) = c {
        walk(&ch, out);
        c = ch.next_sibling();
    }
}

impl Mounted {
    /// Runs the main loop until idle, then two frames (focus rings and transitions settle).
    pub fn settle(&self) {
        super::broadway::pump(50);
        let ctx = glib::MainContext::default();
        while ctx.iteration(false) {}
        if let Some(fc) = self.win.frame_clock() {
            let start = fc.frame_counter();
            let until = std::time::Instant::now() + std::time::Duration::from_millis(500);
            while fc.frame_counter() < start + 2 && std::time::Instant::now() < until {
                self.win.queue_draw();
                super::broadway::pump(16);
            }
        }
    }

    /// Every widget under the page, depth first.
    pub fn all(&self) -> Vec<gtk::Widget> {
        let mut v = Vec::new();
        walk(&self.root, &mut v);
        v
    }

    /// Every widget of type `T`.
    pub fn find<T: IsA<gtk::Widget>>(&self) -> Vec<T> {
        self.all()
            .into_iter()
            .filter_map(|w| w.downcast::<T>().ok())
            .collect()
    }

    /// Widgets carrying a CSS class.
    pub fn by_class(&self, class: &str) -> Vec<gtk::Widget> {
        self.all()
            .into_iter()
            .filter(|w| w.has_css_class(class))
            .collect()
    }

    /// The label showing exactly `text`.
    pub fn by_text(&self, text: &str) -> Option<gtk::Label> {
        self.find::<gtk::Label>()
            .into_iter()
            .find(|l| l.text() == text)
    }

    /// What Tab does from the window: GTK's own focus chain moves forward; the widget with the
    /// keyboard after it.
    pub fn tab(&self) -> Option<gtk::Widget> {
        self.win.child_focus(gtk::DirectionType::TabForward);
        self.settle();
        gtk::prelude::GtkWindowExt::focus(&self.win)
    }

    /// Nobody has the keyboard: the picture of a state at rest, without a focus ring.
    pub fn unfocus(&self) {
        gtk::prelude::GtkWindowExt::set_focus(&self.win, None::<&gtk::Widget>);
        // A field that took the focus selected its text (select on focus); at rest it shows none.
        for w in self.all() {
            if let Some(e) = w.dynamic_cast_ref::<gtk::Editable>() {
                e.select_region(0, 0);
            }
        }
        self.settle();
    }

    /// What Space and Enter do on a focused control: its `activate` signal, which GTK binds to
    /// those keys.
    pub fn activate(&self, w: &impl IsA<gtk::Widget>) {
        w.as_ref().activate();
        self.settle();
    }

    /// Bounds of `w` in the window, px.
    pub fn rect(&self, w: &impl IsA<gtk::Widget>) -> (i32, i32, i32, i32) {
        w.as_ref()
            .compute_bounds(&self.win)
            .map(|b| {
                (
                    b.x().round() as i32,
                    b.y().round() as i32,
                    b.width().round() as i32,
                    b.height().round() as i32,
                )
            })
            .unwrap_or_default()
    }

    /// The measured view tree, one line per drawn widget: path, then x,y w x h. Text, so a
    /// snapshot diff reads in a review.
    pub fn tree(&self) -> String {
        fn lines(n: &shell::layout::Node, depth: usize, out: &mut String) {
            let leaf = sorted_classes(n.path.rsplit(" › ").next().unwrap_or(&n.path));
            let r = n.rect;
            out.push_str(&format!(
                "{}{leaf} [{},{} {}x{}]{}\n",
                "  ".repeat(depth),
                r.x,
                r.y,
                r.w,
                r.h,
                if n.ring { " ring" } else { "" }
            ));
            for c in &n.children {
                lines(c, depth + 1, out);
            }
        }
        let mut s = String::new();
        lines(
            &kit::check::tree(&self.root, self.win.upcast_ref(), ""),
            0,
            &mut s,
        );
        s
    }

    /// The layout rules of the HIG (section 6) on the mounted component.
    pub fn issues(&self) -> Vec<String> {
        kit::check::issues(&self.win)
            .into_iter()
            .map(|i| i.message)
            .collect()
    }

    /// The accessibility of every widget that has a role other than generic: role, then the
    /// relations and states that are set. GTK has no reader for a relation's target, so the target
    /// is checked structurally by the component test.
    pub fn a11y(&self) -> String {
        use gtk::{AccessibleRelation as R, AccessibleRole, AccessibleState as S};
        let mut s = String::new();
        for wd in self.all().iter().filter(|w| w.is_drawable()) {
            let role = wd.accessible_role();
            if matches!(
                role,
                AccessibleRole::Generic | AccessibleRole::None | AccessibleRole::Presentation
            ) {
                continue;
            }
            let mut line = format!("{role:?}").to_lowercase();
            if let Some(l) = wd.downcast_ref::<gtk::Label>() {
                line.push_str(&format!(" {:?}", l.text().as_str()));
            }
            for (r, n) in [
                (R::LabelledBy, "labelled-by"),
                (R::DescribedBy, "described-by"),
            ] {
                if gtk::test_accessible_has_relation(wd, r) {
                    line.push_str(&format!(" {n}"));
                }
            }
            for (st, n) in [
                (S::Checked, "checked"),
                (S::Disabled, "disabled"),
                (S::Busy, "busy"),
            ] {
                if gtk::test_accessible_has_state(wd, st) {
                    line.push_str(&format!(" {n}"));
                }
            }
            if !wd.is_sensitive() {
                line.push_str(" insensitive");
            }
            s.push_str(&line);
            s.push('\n');
        }
        s
    }

    /// The window as drawn (the golden runner's Cairo render: the same on every machine).
    pub fn render(&self) -> gdk::Texture {
        self.settle();
        let (wd, ht) = (self.win.width(), self.win.height());
        let p = gtk::WidgetPaintable::new(Some(&self.win));
        let mut node = None;
        for _ in 0..20 {
            let snap = gtk::Snapshot::new();
            p.snapshot(&snap, wd as f64, ht as f64);
            node = snap.to_node();
            if node.is_some() {
                break;
            }
            self.win.queue_draw();
            super::broadway::pump(100);
        }
        let node = node.expect("a render node");
        let r = gtk::gsk::CairoRenderer::new();
        r.realize(None::<&gdk::Surface>).expect("cairo renderer");
        let t = r.render_texture(
            node,
            Some(&gtk::graphene::Rect::new(0.0, 0.0, wd as f32, ht as f32)),
        );
        r.unrealize();
        t
    }

    /// Compares the picture and the view tree with `kit-<name>.png` and `kit-<name>.tree` in the
    /// golden folder (written with `WOL_GOLDEN_UPDATE=1`); the problems, or none.
    pub fn golden(&self, t: &Tester, name: &str) -> Vec<String> {
        let mut bad = Vec::new();
        let tex = self.render();
        let png = t.out.join(format!("kit-{name}.png"));
        tex.save_to_png(&png).expect("write the render");
        let tree = self.tree();
        std::fs::write(t.out.join(format!("kit-{name}.tree")), &tree).expect("write the tree");
        let (gold_png, gold_tree) = (
            t.golden.join(format!("kit-{name}.png")),
            t.golden.join(format!("kit-{name}.tree")),
        );
        if t.update {
            std::fs::create_dir_all(&t.golden).expect("the golden folder");
            std::fs::copy(&png, &gold_png).expect("update the golden");
            std::fs::write(&gold_tree, &tree).expect("update the tree");
            return bad;
        }
        match gdk::Texture::from_filename(&gold_png) {
            Ok(g) => {
                let d = diff(&tex, &g);
                if d > TOLERANCE {
                    bad.push(format!(
                        "{name}: differs from its golden by {:.2}% of pixels",
                        d * 100.0
                    ));
                }
            }
            Err(_) => bad.push(format!("{name}: no golden (run with WOL_GOLDEN_UPDATE=1)")),
        }
        match std::fs::read_to_string(&gold_tree) {
            Ok(g) if g == tree => {}
            Ok(_) => bad.push(format!(
                "{name}: the view tree changed (diff {} {})",
                gold_tree.display(),
                t.out.join(format!("kit-{name}.tree")).display()
            )),
            Err(_) => bad.push(format!(
                "{name}: no tree snapshot (run with WOL_GOLDEN_UPDATE=1)"
            )),
        }
        bad
    }
}

/// `Box.vertical.page.app-window` with its classes in alphabetical order: GTK keeps a node's classes
/// in the order their names were first seen by the process, which depends on which cases ran before
/// (a run of one component printed `page.app-window`, a full run `app-window.page`).
fn sorted_classes(leaf: &str) -> String {
    let mut parts = leaf.split('.');
    let head = parts.next().unwrap_or("");
    let mut classes: Vec<&str> = parts.collect();
    classes.sort_unstable();
    std::iter::once(head)
        .chain(classes)
        .collect::<Vec<_>>()
        .join(".")
}

fn pixels(t: &gdk::Texture) -> (i32, i32, Vec<u8>) {
    let mut d = gdk::TextureDownloader::new(t);
    d.set_format(gdk::MemoryFormat::R8g8b8a8);
    let (bytes, stride) = d.download_bytes();
    let (w, h) = (t.width(), t.height());
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h as usize {
        out.extend_from_slice(&bytes[y * stride..y * stride + w as usize * 4]);
    }
    (w, h, out)
}

/// Share of differing pixels, or 1.0 on a size mismatch (the golden runner's measure).
pub fn diff(a: &gdk::Texture, b: &gdk::Texture) -> f64 {
    let ((aw, ah, pa), (bw, bh, pb)) = (pixels(a), pixels(b));
    if (aw, ah) != (bw, bh) {
        return 1.0;
    }
    let bad = pa
        .chunks(4)
        .zip(pb.chunks(4))
        .filter(|(x, y)| x.iter().zip(*y).any(|(p, q)| p.abs_diff(*q) > 24))
        .count();
    bad as f64 / (aw * ah) as f64
}
