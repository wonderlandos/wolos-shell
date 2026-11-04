// SPDX-License-Identifier: GPL-2.0-or-later
//! Choosers for preferences panes: Looks previews and preset cards, colour wells, the storage usage
//! bar, key caps, and file pickers. Drawn colours come from the active theme's tokens ([`Tokens`]),
//! never literals; layout and style live in kit.css "Pickers".
use super::widgets::{self as w, S1, S2};
use gtk::{gdk, gio, glib, prelude::*};
use shared::theme::Color;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use wol_core::looks::Looks;

/// Preset card preview (16:10).
pub const PRESET_THUMB: (i32, i32) = (176, 110);
/// Large live preview at the top of Effects & Looks.
pub const LOOKS_HERO: (i32, i32) = (320, 200);
/// Colour well diameter.
pub const WELL: i32 = 22;
/// Characters a preset description wraps at.
pub const PRESET_CHARS: i32 = 22;

/// Theme colours the drawn previews use.
#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub surface: Color,
    pub card: Color,
    pub line: Color,
    pub text: Color,
    pub dim: Color,
    pub accent: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
    pub glass: Color,
}

impl Tokens {
    pub fn of(t: &crate::theme::Theme) -> Self {
        let c = |k: &str| {
            t.tokens
                .get(k)
                .and_then(|v| Color::parse(v))
                .unwrap_or(Color::WHITE)
        };
        Tokens {
            surface: c("surface"),
            card: c("card"),
            line: c("line"),
            text: c("text"),
            dim: c("dim"),
            accent: c("accent"),
            success: c("success"),
            warning: c("warning"),
            danger: c("danger"),
            glass: c("glass"),
        }
    }

    /// The theme on screen now (saved theme, else the light/dark default).
    pub fn current() -> Self {
        let dark = gtk::Settings::default()
            .map(|s| s.is_gtk_application_prefer_dark_theme())
            .unwrap_or(false);
        Self::of(&crate::theme::resolve(
            super::style::active_id().as_deref(),
            dark,
        ))
    }
}

/// Shipped wallpapers: installed, else the repo's design tree (dev, tests): the earlier pictures
/// and the brand set (the Moon Gate, the places).
pub const WALLPAPER_DIRS: [&str; 3] = [
    "/usr/share/wolos/wallpapers",
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../design/wallpapers"),
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../design/brand/wallpapers"),
];
/// Day, dusk and night pictures of the Dynamic wallpaper, in that order.
pub const DYNAMIC: [&str; 3] = ["wonderland-day", "wonderland-dusk", "wonderland-night"];
/// Saved choice meaning "the Dynamic wallpaper".
pub const DYNAMIC_CHOICE: &str = "dynamic";
/// Saved choice meaning "the Moon Gate" (also the default): day in Light, night in Dark, in the
/// active theme's dye (`themes/gate-<theme>-*`, made at image build) or the signature dye.
pub const GATE_CHOICE: &str = "gate";
/// Saved choice prefix of a place (`place:cachoeira`): day in Light, night in Dark.
pub const PLACE_PREFIX: &str = "place:";
/// The places in picker order: (id, name). Each shows when its pictures are installed.
pub const PLACES: [(&str, &str); 4] = [
    ("cachoeira", "Cachoeira Station"),
    ("brasilia", "Brasília"),
    ("estaiada", "Ponte Estaiada"),
    ("rioniteroi", "Rio–Niterói Bridge"),
];
/// The themes whose gate is the signature (`wolos-gate-*`), not a generated dye.
const SIGNATURE_THEMES: [&str; 2] = ["wol-light", "wol-dark"];

/// What the Moon Gate and the places follow: the active theme and its light or dark side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Look {
    pub theme: String,
    pub theme_name: String,
    pub night: bool,
}

impl Look {
    pub fn current() -> Look {
        let dark = gtk::Settings::default()
            .map(|s| s.is_gtk_application_prefer_dark_theme())
            .unwrap_or(false);
        let t = crate::theme::resolve(super::style::active_id().as_deref(), dark);
        Look {
            night: t.variant == crate::theme::Variant::Dark,
            theme: t.id,
            theme_name: t.name,
        }
    }
}

/// `day` or `night`.
fn mode(night: bool) -> &'static str {
    if night { "night" } else { "day" }
}

/// `16x10` for a display taller than 16:9, else `4k`.
fn size_for(aspect: f64) -> &'static str {
    if aspect < 1.7 { "16x10" } else { "4k" }
}

fn first_shipped(names: &[String]) -> Option<PathBuf> {
    names.iter().find_map(|n| {
        WALLPAPER_DIRS
            .iter()
            .map(|d| Path::new(d).join(n))
            .find(|p| p.exists())
    })
}

/// Shipped wallpaper `<name>.svg`.
pub fn shipped_wallpaper(name: &str) -> Option<PathBuf> {
    first_shipped(&[format!("{name}.svg")])
}

/// The Moon Gate for `theme` (its generated dye, else the signature), day or night, for a
/// display of `aspect` (width / height).
pub fn gate_file(theme: &str, night: bool, aspect: f64) -> Option<PathBuf> {
    let (m, s) = (mode(night), size_for(aspect));
    let mut names = Vec::new();
    if !SIGNATURE_THEMES.contains(&theme) && !theme.contains('/') {
        names.push(format!("themes/gate-{theme}-{m}-{s}.svg"));
    }
    names.push(format!("wolos-gate-{m}-{s}.svg"));
    first_shipped(&names)
}

/// A place's picture, day or night.
pub fn place_file(id: &str, night: bool, aspect: f64) -> Option<PathBuf> {
    first_shipped(&[format!("{id}-{}-{}.svg", mode(night), size_for(aspect))])
}

/// The same picture's other side, when installed: `…-day-…` and `…-night-…` (the gate, a theme's
/// gate, a place).
pub fn other_side(p: &Path) -> Option<PathBuf> {
    let name = p.file_name()?.to_str()?;
    let other = if name.contains("-day-") {
        name.replacen("-day-", "-night-", 1)
    } else if name.contains("-night-") {
        name.replacen("-night-", "-day-", 1)
    } else {
        return None;
    };
    Some(p.with_file_name(other)).filter(|o| o.exists())
}

/// Dynamic wallpaper picture for a local hour: day 07–17, dusk 17–20, night otherwise.
pub fn dynamic_name(hour: u8) -> &'static str {
    match hour {
        7..=16 => DYNAMIC[0],
        17..=19 => DYNAMIC[1],
        _ => DYNAMIC[2],
    }
}

/// The saved choice (`$XDG_CONFIG_HOME/wolos/wallpaper`, also written by `wol wallpaper set`):
/// a path, [`DYNAMIC_CHOICE`], [`GATE_CHOICE`], a place, or None for the default (the gate).
pub fn wallpaper_choice() -> Option<String> {
    std::fs::read_to_string(Path::new(&super::config_home()).join(shared::theme::WALLPAPER_FILE))
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// The default picture: the Moon Gate, else (an image without it) the earlier dusk picture.
fn default_wallpaper(look: &Look, aspect: f64) -> Option<PathBuf> {
    gate_file(&look.theme, look.night, aspect).or_else(|| shipped_wallpaper(DYNAMIC[1]))
}

/// Picture the desktop shows at `hour` in `look` on a display of `aspect`: the saved file when it
/// exists, Dynamic by time of day, the gate or a place by light and dark, else the default.
pub fn wallpaper_for(choice: Option<&str>, hour: u8, look: &Look, aspect: f64) -> Option<PathBuf> {
    match choice {
        Some(DYNAMIC_CHOICE) => shipped_wallpaper(dynamic_name(hour)),
        Some(c) if c.starts_with(PLACE_PREFIX) => {
            place_file(&c[PLACE_PREFIX.len()..], look.night, aspect)
                .or_else(|| default_wallpaper(look, aspect))
        }
        Some(p) if p != GATE_CHOICE && Path::new(p).is_file() => Some(PathBuf::from(p)),
        _ => default_wallpaper(look, aspect),
    }
}

/// The wallpaper's name as the Wallpaper page shows it: "Moon Gate, Nord, Night".
pub fn wallpaper_name(choice: Option<&str>, look: &Look, hour: u8) -> String {
    let side = if look.night { "Night" } else { "Day" };
    match choice {
        Some(DYNAMIC_CHOICE) => {
            let n = dynamic_name(hour);
            let part = n.rsplit('-').next().unwrap_or(n);
            let mut c = part.chars();
            let part: String = c
                .next()
                .map(|f| f.to_uppercase().chain(c).collect())
                .unwrap_or_default();
            format!("Dynamic, {part}")
        }
        Some(c) if c.starts_with(PLACE_PREFIX) => {
            let id = &c[PLACE_PREFIX.len()..];
            match PLACES.iter().find(|p| p.0 == id) {
                Some((_, name)) => format!("{name}, {side}"),
                None => format!("Moon Gate, {}, {side}", look.theme_name),
            }
        }
        Some(p) if p != GATE_CHOICE && Path::new(p).is_file() => Path::new(p)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        _ => format!("Moon Gate, {}, {side}", look.theme_name),
    }
}

/// The desktop wallpaper now, for a display of `aspect`.
pub fn desktop_wallpaper_for(aspect: f64) -> Option<PathBuf> {
    wallpaper_for(
        wallpaper_choice().as_deref(),
        super::now().hour,
        &Look::current(),
        aspect,
    )
}

/// The desktop wallpaper now (a 16:9 display).
pub fn desktop_wallpaper() -> Option<PathBuf> {
    desktop_wallpaper_for(16.0 / 9.0)
}

fn rgba(cr: &gtk::cairo::Context, c: Color, alpha: f64) {
    cr.set_source_rgba(
        c.r as f64 / 255.0,
        c.g as f64 / 255.0,
        c.b as f64 / 255.0,
        c.a * alpha,
    );
}

/// Black at `a` (shadows, dimming): a physical constant, not a theme colour.
fn shade(cr: &gtk::cairo::Context, a: f64) {
    cr.set_source_rgba(0.0, 0.0, 0.0, a);
}

/// Focused-window border paint: one colour, or a gradient across the box at `angle` degrees.
fn border_paint(cr: &gtk::cairo::Context, l: &Looks, x: f64, y: f64, bw: f64, bh: f64) {
    let colors: Vec<Color> = l
        .border_colors
        .iter()
        .filter_map(|c| Color::parse(c))
        .collect();
    match colors.as_slice() {
        [] => {}
        [one] => rgba(cr, *one, 1.0),
        many => {
            let a = f64::from(l.border_angle).to_radians();
            let (cx, cy) = (x + bw / 2.0, y + bh / 2.0);
            let r = (bw.max(bh)) / 2.0;
            let g = gtk::cairo::LinearGradient::new(
                cx - a.cos() * r,
                cy + a.sin() * r,
                cx + a.cos() * r,
                cy - a.sin() * r,
            );
            for (i, c) in many.iter().enumerate() {
                g.add_color_stop_rgba(
                    i as f64 / (many.len() - 1) as f64,
                    c.r as f64 / 255.0,
                    c.g as f64 / 255.0,
                    c.b as f64 / 255.0,
                    c.a,
                );
            }
            let _ = cr.set_source(&g);
        }
    }
}

/// One miniature window: shadow, frosted body, title bar with the window controls (right, quiet),
/// text lines, border.
#[allow(clippy::too_many_arguments)]
fn draw_window(
    cr: &gtk::cairo::Context,
    t: &Tokens,
    l: &Looks,
    (x, y, ww, wh): (f64, f64, f64, f64),
    k: f64,
    active: bool,
) {
    let r = (f64::from(l.rounding) * k * 0.9).min(ww.min(wh) / 3.0);
    if l.shadow && l.shadow_range > 0 {
        let spread = f64::from(l.shadow_range) * k * 0.35;
        for i in (1..=6).rev() {
            let e = spread * i as f64 / 6.0;
            shade(cr, if active { 0.05 } else { 0.035 });
            w::rounded(
                cr,
                x - e,
                y - e + spread * 0.25,
                ww + 2.0 * e,
                wh + 2.0 * e,
                r + e,
            );
            let _ = cr.fill();
        }
    }
    let opacity = f64::from(if active {
        l.active_opacity
    } else {
        l.inactive_opacity
    });
    // Glass: the blurred backdrop reads as a pale veil under a see-through window.
    if l.blur && opacity < 1.0 {
        rgba(cr, t.surface, 0.55 + f64::from(l.vibrancy) * 0.2);
        w::rounded(cr, x, y, ww, wh, r);
        let _ = cr.fill();
    }
    rgba(cr, t.card, opacity);
    w::rounded(cr, x, y, ww, wh, r);
    let _ = cr.fill();
    // Title bar.
    let bar = (wh * 0.16).clamp(6.0, 18.0);
    let _ = cr.save();
    w::rounded(cr, x, y, ww, wh, r);
    cr.clip();
    rgba(cr, t.surface, opacity);
    cr.rectangle(x, y, ww, bar);
    let _ = cr.fill();
    // Close rightmost, then zoom, minimise: one quiet circle each (the text colour at 28%, as
    // `button.light` in kit.css), never the status colours.
    let dot = (bar * 0.22).max(1.2);
    for i in 0..3 {
        rgba(cr, if active { t.text } else { t.dim }, 0.28);
        cr.arc(
            x + ww - r.max(bar * 0.5) - f64::from(i) * dot * 3.0,
            y + bar / 2.0,
            dot,
            0.0,
            std::f64::consts::TAU,
        );
        let _ = cr.fill();
    }
    // Content: three text lines and an accent button.
    let pad = (ww * 0.08).max(3.0);
    let line = (wh * 0.05).clamp(1.5, 5.0);
    rgba(cr, t.dim, 0.55 * opacity);
    for (i, f) in [0.62, 0.48, 0.55].into_iter().enumerate() {
        w::rounded(
            cr,
            x + pad,
            y + bar + pad + i as f64 * line * 2.4,
            (ww - 2.0 * pad) * f,
            line,
            line / 2.0,
        );
        let _ = cr.fill();
    }
    if active {
        rgba(cr, t.accent, 1.0);
        let (bw, bh) = (ww * 0.26, line * 2.2);
        w::rounded(cr, x + ww - pad - bw, y + wh - pad - bh, bw, bh, bh / 2.0);
        let _ = cr.fill();
    }
    if !active && l.dim_inactive {
        shade(cr, f64::from(l.dim_strength) * 0.8);
        cr.rectangle(x, y, ww, wh);
        let _ = cr.fill();
    }
    let _ = cr.restore();
    if l.border_size > 0 {
        let bw = (f64::from(l.border_size) * k * 1.2).max(1.0);
        if active {
            border_paint(cr, l, x, y, ww, wh);
        } else if let Some(c) = Color::parse(&l.inactive_border) {
            rgba(cr, c, 1.0);
        }
        cr.set_line_width(bw);
        w::rounded(cr, x, y, ww, wh, r);
        let _ = cr.stroke();
    }
}

/// The whole miniature desktop: menu bar, two tiled windows (left unfocused), Dock.
fn draw_desktop(cr: &gtk::cairo::Context, wd: f64, ht: f64, t: &Tokens, l: &Looks) {
    let l = l.normalized();
    // Scale from a 1440 px wide screen, exaggerated so gaps and corners stay visible small.
    let k = (wd / 1440.0) * 4.0;
    let glass = (t.glass.a * f64::from(l.glass)).clamp(0.15, 1.0);
    let mb = (ht * 0.07).max(5.0);
    rgba(cr, t.glass, glass / t.glass.a.max(0.01));
    cr.rectangle(0.0, 0.0, wd, mb);
    let _ = cr.fill();
    let dock_h = (ht * 0.1).max(7.0);
    let dock_w = wd * 0.42;
    let dock_y = ht - dock_h - ht * 0.03;
    let gap = f64::from(l.gaps) * k * 0.6;
    let (top, bottom) = (mb + gap, dock_y - gap.max(ht * 0.02));
    let inner = gap;
    let avail = wd - 2.0 * gap - inner;
    let left = avail * 0.46;
    let wh = (bottom - top).max(8.0);
    draw_window(cr, t, &l, (gap, top, left, wh), k, false);
    draw_window(
        cr,
        t,
        &l,
        (gap + left + inner, top, avail - left, wh),
        k,
        true,
    );
    rgba(cr, t.glass, glass / t.glass.a.max(0.01));
    w::rounded(
        cr,
        (wd - dock_w) / 2.0,
        dock_y,
        dock_w,
        dock_h,
        dock_h * 0.35,
    );
    let _ = cr.fill();
    rgba(cr, t.accent, 1.0);
    let icon = dock_h * 0.55;
    for i in 0..5 {
        let ix = (wd - dock_w) / 2.0 + dock_w * (0.12 + 0.19 * i as f64) - icon / 2.0;
        w::rounded(
            cr,
            ix,
            dock_y + (dock_h - icon) / 2.0,
            icon,
            icon,
            icon * 0.25,
        );
        let _ = cr.fill();
    }
}

/// Live miniature of a look over the desktop wallpaper (loaded off the UI thread).
#[derive(Clone)]
pub struct LooksPreview {
    pub root: gtk::Overlay,
    area: gtk::DrawingArea,
    looks: Rc<RefCell<Looks>>,
}

impl LooksPreview {
    pub fn new((pw, ph): (i32, i32), looks: &Looks, tokens: Tokens) -> Self {
        let root = gtk::Overlay::new();
        root.add_css_class("looks-preview");
        root.set_size_request(pw, ph);
        root.set_halign(gtk::Align::Center);
        root.set_valign(gtk::Align::Start);
        root.set_overflow(gtk::Overflow::Hidden);
        root.set_child(Some(&w::vbox(0)));
        let pic = gtk::Picture::new();
        pic.set_content_fit(gtk::ContentFit::Cover);
        pic.set_can_shrink(true);
        root.add_overlay(&pic);
        if let Some(f) = desktop_wallpaper() {
            glib::spawn_future_local(async move {
                let r = gio::spawn_blocking(move || {
                    gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(&f, pw * 2, -1, true)
                        .ok()
                        .map(|pb| gdk::Texture::for_pixbuf(&pb))
                })
                .await;
                if let Ok(Some(t)) = r {
                    pic.set_paintable(Some(&t));
                }
            });
        }
        let area = gtk::DrawingArea::new();
        let looks = Rc::new(RefCell::new(looks.clone()));
        let l2 = looks.clone();
        area.set_draw_func(move |_, cr, wd, ht| {
            draw_desktop(cr, f64::from(wd), f64::from(ht), &tokens, &l2.borrow())
        });
        root.add_overlay(&area);
        LooksPreview { root, area, looks }
    }

    pub fn set(&self, looks: &Looks) {
        *self.looks.borrow_mut() = looks.clone();
        self.area.queue_draw();
    }
}

/// A Looks preset: live miniature, name and one-line description; checked = accent ring. Group
/// several with `set_group`.
pub struct PresetCard {
    pub button: gtk::ToggleButton,
    pub preview: LooksPreview,
}

impl PresetCard {
    pub fn new(
        name: &str,
        description: &str,
        looks: &Looks,
        tokens: Tokens,
        selected: bool,
    ) -> Self {
        let preview = LooksPreview::new(PRESET_THUMB, looks, tokens);
        let b = w::vbox(S1);
        b.append(&preview.root);
        let n = w::label(name, "");
        n.add_css_class("preset-name");
        b.append(&n);
        // Whole words only: descriptions are at most two short lines (`looks::DESCRIPTION_CHARS`).
        let d = w::label(description, "secondary");
        d.set_wrap(true);
        d.set_wrap_mode(gtk::pango::WrapMode::Word);
        d.set_max_width_chars(PRESET_CHARS);
        d.set_width_chars(PRESET_CHARS);
        d.set_visible(!description.is_empty());
        b.append(&d);
        let button = gtk::ToggleButton::builder()
            .child(&b)
            .active(selected)
            .tooltip_text(name)
            .build();
        button.add_css_class("preview-tile");
        button.add_css_class("preset-card");
        PresetCard { button, preview }
    }
}

/// Colour well: a circle of the colour; click for the palette and a hex field.
pub struct ColorWell {
    pub button: gtk::MenuButton,
    color: Rc<RefCell<String>>,
    area: gtk::DrawingArea,
}

fn draw_well(cr: &gtk::cairo::Context, wd: f64, ht: f64, c: Option<Color>, ring: Color) {
    let r = wd.min(ht) / 2.0;
    // Transparent colours sit on a two-tone disc so they stay visible.
    if c.is_none_or(|c| c.a < 1.0) {
        rgba(cr, ring, 0.35);
        cr.arc(wd / 2.0, ht / 2.0, r, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
    }
    if let Some(c) = c {
        rgba(cr, c, 1.0);
        cr.arc(wd / 2.0, ht / 2.0, r - 1.0, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
    }
    rgba(cr, ring, 1.0);
    cr.set_line_width(1.0);
    cr.arc(wd / 2.0, ht / 2.0, r - 0.5, 0.0, std::f64::consts::TAU);
    let _ = cr.stroke();
}

impl ColorWell {
    /// `swatches`: (name, `#rrggbb[aa]`) offered in the popover. `changed` gets a valid colour.
    pub fn new(
        color: &str,
        swatches: &[(String, String)],
        tokens: Tokens,
        changed: impl Fn(String) + 'static,
    ) -> Self {
        let color = Rc::new(RefCell::new(color.to_string()));
        let area = gtk::DrawingArea::builder()
            .content_width(WELL)
            .content_height(WELL)
            .build();
        let c2 = color.clone();
        area.set_draw_func(move |_, cr, wd, ht| {
            draw_well(
                cr,
                f64::from(wd),
                f64::from(ht),
                Color::parse(&c2.borrow()),
                tokens.line.over(tokens.card),
            )
        });
        let button = gtk::MenuButton::new();
        button.set_child(Some(&area));
        button.add_css_class("color-well");
        button.set_valign(gtk::Align::Center);
        button.set_tooltip_text(Some(&color.borrow()));
        let pop = gtk::Popover::new();
        pop.add_css_class("menu");
        let body = w::vbox(S2);
        body.add_css_class("color-palette");
        let grid = w::wrap_row(S1, 8);
        let changed = Rc::new(changed);
        let field = w::text_field("#rrggbb");
        field.set_text(&color.borrow());
        field.set_width_chars(10);
        let set: Rc<dyn Fn(String)> = {
            let (color, area, button, changed, field) = (
                color.clone(),
                area.clone(),
                button.downgrade(),
                changed.clone(),
                field.clone(),
            );
            Rc::new(move |hex: String| {
                if !wol_core::looks::is_color(&hex) {
                    return;
                }
                *color.borrow_mut() = hex.clone();
                area.queue_draw();
                if field.text() != hex {
                    field.set_text(&hex);
                }
                if let Some(b) = button.upgrade() {
                    b.set_tooltip_text(Some(&hex));
                }
                changed(hex);
            })
        };
        for (name, hex) in swatches {
            let Some(c) = Color::parse(hex) else { continue };
            let s = w::accent_swatch(c, c, name, false);
            let (set, hex, pop) = (set.clone(), hex.clone(), pop.downgrade());
            s.connect_clicked(move |_| {
                set(hex.clone());
                if let Some(p) = pop.upgrade() {
                    p.popdown();
                }
            });
            grid.insert(&s, -1);
        }
        body.append(&grid);
        let s2 = set.clone();
        field.connect_activate(move |f| s2(f.text().trim().to_ascii_lowercase()));
        body.append(&field);
        pop.set_child(Some(&body));
        button.set_popover(Some(&pop));
        ColorWell {
            button,
            color,
            area,
        }
    }

    /// Shows `hex` without reporting a change.
    pub fn set(&self, hex: &str) {
        *self.color.borrow_mut() = hex.to_string();
        self.button.set_tooltip_text(Some(hex));
        self.area.queue_draw();
    }
}

/// Stacked usage bar (largest first) with a legend: (name, bytes label, fraction of the whole).
/// Colours cycle through accent, warning, success, danger, dim; free space stays the track.
pub fn usage_bar(parts: &[(String, String, f64)]) -> gtk::Box {
    /// Bar resolution: one cell = 1 %.
    const CELLS: i32 = 100;
    let b = w::vbox(S2);
    b.add_css_class("usage");
    let bar = gtk::Grid::new();
    bar.set_column_homogeneous(true);
    bar.add_css_class("usage-bar");
    let mut at = 0;
    for (i, (_, _, f)) in parts.iter().enumerate() {
        let n = ((f * f64::from(CELLS)).round() as i32).clamp(1, CELLS - at);
        if at >= CELLS || n <= 0 {
            break;
        }
        let seg = w::hbox(0);
        seg.add_css_class("usage-segment");
        seg.add_css_class(&format!("usage-{}", i % 5));
        bar.attach(&seg, at, 0, n, 1);
        at += n;
    }
    if at < CELLS {
        let rest = w::hbox(0);
        rest.add_css_class("usage-free");
        bar.attach(&rest, at, 0, CELLS - at, 1);
    }
    b.append(&bar);
    let legend = w::wrap_row(S2, 3);
    legend.add_css_class("usage-legend");
    legend.set_halign(gtk::Align::Fill);
    for (i, (name, size, _)) in parts.iter().enumerate() {
        let item = w::hbox(S1);
        item.set_halign(gtk::Align::Start);
        let dot = w::hbox(0);
        dot.add_css_class("usage-dot");
        dot.add_css_class(&format!("usage-{}", i % 5));
        dot.set_valign(gtk::Align::Center);
        item.append(&dot);
        item.append(&w::label(name, ""));
        item.append(&w::label(size, "secondary"));
        legend.insert(&item, -1);
    }
    b.append(&legend);
    b
}

/// One key as printed on the keyboard ("⌘", "Space").
pub fn keycap(text: &str) -> gtk::Label {
    let l = w::centered(text, "keycap");
    l.set_valign(gtk::Align::Center);
    l
}

/// What each modifier symbol means on this keyboard, shown once above shortcut lists.
pub fn key_legend(items: &[(&str, &str)]) -> gtk::Widget {
    let row = w::wrap_row(S2, 2);
    row.add_css_class("key-legend");
    row.set_halign(gtk::Align::Fill);
    for (cap, meaning) in items {
        let item = w::hbox(S2);
        item.set_halign(gtk::Align::Start);
        item.append(&keycap(cap));
        item.append(&w::label(meaning, "secondary"));
        row.insert(&item, -1);
    }
    row.upcast()
}

/// Native file chooser: open (`save` false) or save a JSON file; `done` gets the path.
pub fn choose_file(
    parent: &impl IsA<gtk::Widget>,
    title: &str,
    save: Option<&str>,
    done: impl FnOnce(PathBuf) + 'static,
) {
    let filter = gtk::FileFilter::new();
    filter.add_suffix("json");
    filter.set_name(Some("Looks presets"));
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let d = gtk::FileDialog::builder()
        .title(title)
        .modal(true)
        .filters(&filters)
        .build();
    let win = parent.root().and_downcast::<gtk::Window>();
    let done = RefCell::new(Some(done));
    let finish = move |r: Result<gio::File, glib::Error>| {
        if let (Ok(f), Some(done)) = (r, done.borrow_mut().take())
            && let Some(p) = f.path()
        {
            done(p)
        }
    };
    match save {
        Some(name) => {
            d.set_initial_name(Some(name));
            d.save(win.as_ref(), gio::Cancellable::NONE, finish);
        }
        None => d.open(win.as_ref(), gio::Cancellable::NONE, finish),
    }
}

/// Native picture chooser (Wallpaper > Add Photo…).
pub fn choose_picture(parent: &impl IsA<gtk::Widget>, done: impl FnOnce(PathBuf) + 'static) {
    let filter = gtk::FileFilter::new();
    filter.add_pixbuf_formats();
    filter.set_name(Some("Pictures"));
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let d = gtk::FileDialog::builder()
        .title("Choose a Picture")
        .modal(true)
        .filters(&filters)
        .build();
    let win = parent.root().and_downcast::<gtk::Window>();
    let done = RefCell::new(Some(done));
    d.open(win.as_ref(), gio::Cancellable::NONE, move |r| {
        if let (Ok(f), Some(done)) = (r, done.borrow_mut().take())
            && let Some(p) = f.path()
        {
            done(p)
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_wallpaper_follows_the_hour() {
        for (h, want) in [
            (0, "wonderland-night"),
            (6, "wonderland-night"),
            (7, "wonderland-day"),
            (16, "wonderland-day"),
            (17, "wonderland-dusk"),
            (19, "wonderland-dusk"),
            (20, "wonderland-night"),
        ] {
            assert_eq!(dynamic_name(h), want, "{h}");
        }
        let day = wallpaper_for(Some(DYNAMIC_CHOICE), 9, &look("nord", false), 1.77).unwrap();
        assert!(day.ends_with("wonderland-day.svg"), "{day:?}");
    }

    fn look(theme: &str, night: bool) -> Look {
        Look {
            theme: theme.into(),
            theme_name: theme.into(),
            night,
        }
    }

    fn gate_shipped() -> bool {
        shipped_wallpaper("wolos-gate-day-4k").is_some()
    }

    /// The default is the Moon Gate: day in Light, night in Dark, 16:10 on a 16:10 display; a
    /// missing photo falls back to it.
    #[test]
    fn the_gate_is_the_default_and_follows_light_and_dark() {
        if !gate_shipped() {
            eprintln!("skip: no design/brand tree");
            return;
        }
        let day = wallpaper_for(None, 9, &look("wol-light", false), 1.77).unwrap();
        assert!(day.ends_with("wolos-gate-day-4k.svg"), "{day:?}");
        let night = wallpaper_for(Some(GATE_CHOICE), 9, &look("wol-dark", true), 1.6).unwrap();
        assert!(night.ends_with("wolos-gate-night-16x10.svg"), "{night:?}");
        let missing = wallpaper_for(Some("/nonexistent.png"), 9, &look("wol-dark", true), 1.77);
        assert!(missing.unwrap().ends_with("wolos-gate-night-4k.svg"));
    }

    /// A theme's gate is its generated dye when the image made it, else the signature.
    #[test]
    fn a_theme_takes_its_gate_dye_when_generated() {
        if !gate_shipped() {
            eprintln!("skip: no design/brand tree");
            return;
        }
        let f = gate_file("nord", true, 1.77).unwrap();
        let dyed = Path::new(WALLPAPER_DIRS[2]).join("themes/gate-nord-night-4k.svg");
        if dyed.exists() {
            assert_eq!(f, dyed);
        } else {
            assert!(f.ends_with("wolos-gate-night-4k.svg"), "{f:?}");
        }
        assert!(
            gate_file("wol-dark", true, 1.77)
                .unwrap()
                .ends_with("wolos-gate-night-4k.svg")
        );
        assert!(
            gate_file("../x", false, 1.77)
                .unwrap()
                .ends_with("wolos-gate-day-4k.svg")
        );
    }

    #[test]
    fn places_follow_light_and_dark() {
        if place_file("cachoeira", false, 1.77).is_none() {
            eprintln!("skip: no design/brand tree");
            return;
        }
        let n = wallpaper_for(Some("place:cachoeira"), 9, &look("nord", true), 1.77).unwrap();
        assert!(n.ends_with("cachoeira-night-4k.svg"), "{n:?}");
        let gone =
            wallpaper_for(Some("place:holambra"), 9, &look("wol-light", false), 1.77).unwrap();
        assert!(
            gone.ends_with("wolos-gate-day-4k.svg"),
            "an unknown place is the gate: {gone:?}"
        );
    }

    #[test]
    fn the_other_side_is_the_same_picture_at_night_or_by_day() {
        if !gate_shipped() {
            eprintln!("skip: no design/brand tree");
            return;
        }
        let day = shipped_wallpaper("wolos-gate-day-4k").unwrap();
        let night = other_side(&day).unwrap();
        assert!(night.ends_with("wolos-gate-night-4k.svg"), "{night:?}");
        assert_eq!(other_side(&night), Some(day));
        assert_eq!(other_side(Path::new("/x/wonderland-dusk.svg")), None);
        assert_eq!(
            other_side(Path::new("/nonexistent/gate-nord-day-4k.svg")),
            None
        );
    }

    #[test]
    fn the_page_names_the_wallpaper() {
        let l = Look {
            theme: "nord".into(),
            theme_name: "Nord".into(),
            night: true,
        };
        assert_eq!(wallpaper_name(None, &l, 9), "Moon Gate, Nord, Night");
        assert_eq!(
            wallpaper_name(Some("gate"), &l, 9),
            "Moon Gate, Nord, Night"
        );
        assert_eq!(
            wallpaper_name(Some("place:cachoeira"), &l, 9),
            "Cachoeira Station, Night"
        );
        assert_eq!(wallpaper_name(Some("dynamic"), &l, 18), "Dynamic, Dusk");
        assert_eq!(
            wallpaper_name(Some("/nonexistent.png"), &l, 9),
            "Moon Gate, Nord, Night"
        );
    }
}
