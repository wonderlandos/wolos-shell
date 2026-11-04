// SPDX-License-Identifier: GPL-2.0-or-later
//! Theme Studio (ADR 0031 "Plugin Maker / Theme Studio"): a `theme.json` (or the theme in use)
//! changed with two sliders per token group, lightness and color, in OKLab ([`to_oklab`],
//! [`shift`]); live contrast checks from the theme contract (`shared::theme::CONTRAST_RULES`,
//! the same composition `shared::theme::validate` measures); previews from the theme gallery
//! card and the terminal palette (`wol_core::theme::terminal`); "Use Wallpaper Colors" through
//! `wol_core::theme::generate::from_samples`; saved as a user theme under
//! `$XDG_CONFIG_HOME/wolos/themes/<id>/theme.json`.
//!
//! Invariant: a theme is saved only when `validate` finds nothing, so every saved theme loads in
//! the catalogue and reads everywhere. Files are read and written off the GTK thread.
use super::off_thread;
use crate::copy::maker as copy;
use crate::kit::widgets::{rgba as paint, rounded};
use crate::kit::{data as d, form, widgets as w};
use crate::theme::Theme;
use gtk::{gio, prelude::*};
use shared::theme::{CONTRAST_RULES, Color, Variant, validate};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use wol_core::theme::{ThemeCatalog, ThemeSpec, generate, terminal};

// ---- OKLab ----

fn linear(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn gamma(c: f64) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let v = if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    // In 0..=255 after the clamp; the cast cannot truncate.
    (v * 255.0).round().clamp(0.0, 255.0) as u8
}

/// sRGB to OKLab `[L, a, b]` (L 0 black .. 1 white). The alpha is not part of it.
pub fn to_oklab(c: Color) -> [f64; 3] {
    let (r, g, b) = (linear(c.r), linear(c.g), linear(c.b));
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    [
        0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766 * s,
    ]
}

/// OKLab back to sRGB with `alpha`; colours outside sRGB are clipped per channel.
pub fn from_oklab([ll, a, b]: [f64; 3], alpha: f64) -> Color {
    let l = (ll + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
    let m = (ll - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
    let s = (ll - 0.089_484_177_5 * a - 1.291_485_548 * b).powi(3);
    Color {
        r: gamma(4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s),
        g: gamma(-1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s),
        b: gamma(-0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701 * s),
        a: alpha,
    }
}

/// One group's slider positions: lightness moved by `light` (OKLab L, -0.3..0.3) and chroma
/// scaled by `chroma` (0 grey .. 1 as made .. 2 twice as vivid).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shift {
    pub light: f64,
    pub chroma: f64,
}

impl Default for Shift {
    fn default() -> Self {
        Shift {
            light: 0.0,
            chroma: 1.0,
        }
    }
}

/// Slider ranges: lightness in percent of L, chroma in percent.
pub const LIGHT_RANGE: (f64, f64, f64) = (-30.0, 30.0, 1.0);
pub const CHROMA_RANGE: (f64, f64, f64) = (0.0, 200.0, 5.0);

/// `c` moved by `s` in OKLab (hue kept, alpha kept).
pub fn shift(c: Color, s: Shift) -> Color {
    let [l, a, b] = to_oklab(c);
    let k = s.chroma.max(0.0);
    from_oklab([(l + s.light).clamp(0.0, 1.0), a * k, b * k], c.a)
}

/// A token value as `theme.json` writes it: `#rrggbb`, or `#rrggbbaa` when translucent (CSS
/// colour 4 hex, which `Color::parse` and every toolkit file reads).
pub fn css(c: Color) -> String {
    if c.a >= 0.999 {
        return c.to_hex();
    }
    // In 0..=255 after the clamp; the cast cannot truncate.
    let a = (c.a.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("{}{a:02x}", c.to_hex())
}

/// The token groups the sliders move, with their titles.
pub const GROUPS: [(&str, &[&str]); 4] = [
    (
        "Surfaces",
        &["surface", "sidebar", "card", "line", "glass", "control"],
    ),
    ("Text", &["text", "dim"]),
    ("Accent", &["accent", "accentText"]),
    ("Status Colors", &["success", "warning", "danger"]),
];

/// `base` with each group's tokens moved by its shift. Tokens that are not colours stay as they
/// are (validate names them).
pub fn apply(base: &BTreeMap<String, String>, shifts: &[Shift; 4]) -> BTreeMap<String, String> {
    let mut out = base.clone();
    for ((_, tokens), s) in GROUPS.iter().zip(shifts) {
        if *s == Shift::default() {
            continue;
        }
        for t in *tokens {
            if let Some(c) = base.get(*t).and_then(|v| Color::parse(v)) {
                out.insert((*t).to_owned(), css(shift(c, *s)));
            }
        }
    }
    out
}

/// One contrast rule measured: text `fg` on `bg`, its ratio and the least it needs.
#[derive(Clone, Debug, PartialEq)]
pub struct Badge {
    pub fg: &'static str,
    pub bg: &'static str,
    pub ratio: f64,
    pub min: f64,
}

impl Badge {
    pub fn passes(&self) -> bool {
        self.ratio >= self.min
    }
}

/// Every rule of the contract, measured as `validate` does (a translucent background over the
/// surface, the surface over white). A rule whose colours are missing is left out (validate
/// names those).
pub fn badges(tokens: &BTreeMap<String, String>) -> Vec<Badge> {
    let get = |k: &str| tokens.get(k).and_then(|v| Color::parse(v));
    let surface = get("surface").map_or(Color::WHITE, |s| s.over(Color::WHITE));
    CONTRAST_RULES
        .iter()
        .filter_map(|(fg, bg, min)| {
            let (f, b) = (get(fg)?, get(bg)?.over(surface));
            Some(Badge {
                fg,
                bg,
                ratio: Color::contrast(f.over(b), b),
                min: *min,
            })
        })
        .collect()
}

/// A token id as words (QA-PLUG-50): `accentText` is "accent text", `glass` is "glass".
pub fn token_words(id: &str) -> String {
    let mut out = String::new();
    for c in id.chars() {
        if c.is_uppercase() && !out.is_empty() {
            out.push(' ');
        }
        out.extend(c.to_lowercase());
    }
    out
}

/// What a badge says: "Accent text on accent 4.5:1" (the words, then the ratio).
pub fn badge_text(b: &Badge) -> String {
    let said = format!(
        "{} on {} {:.1}:1",
        token_words(b.fg),
        token_words(b.bg),
        b.ratio
    );
    let mut c = said.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// The longest theme name Save Theme takes.
pub const NAME_MAX: i32 = 40;

/// The theme contract's own verdict (empty = readable everywhere).
pub fn problems(tokens: &BTreeMap<String, String>) -> Vec<String> {
    validate(|t| tokens.get(t).cloned())
}

/// A theme id from its name: lowercase words joined by dashes; never a WolOS look's `wol-`.
pub fn slug(name: &str) -> String {
    let s: String = name
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let s: String = s.chars().take(MAX_ID).collect();
    if s.is_empty() {
        "my-theme".into()
    } else if crate::ui::theme_picker::is_wolos_look(&s) {
        format!("my-{s}")
    } else {
        s
    }
}

/// Longest theme id made from a name.
const MAX_ID: usize = 48;

/// A resolved theme from a complete spec (`from_samples` makes complete ones).
pub fn resolved(spec: &ThemeSpec) -> Theme {
    Theme {
        id: spec.id.clone(),
        name: spec.name.clone(),
        variant: spec.variant,
        tokens: spec.tokens.clone(),
        wallpaper: None,
        icon_theme: spec.icon_theme.clone(),
        terminal: spec.terminal.clone(),
    }
}

/// The spec the studio saves: `base`'s variant, terminal list and icon theme, `tokens`, `name`.
pub fn spec_for(base: &Theme, name: &str, tokens: BTreeMap<String, String>) -> ThemeSpec {
    ThemeSpec {
        id: slug(name),
        name: name.to_owned(),
        variant: base.variant,
        extends: None,
        tokens,
        wallpaper: None,
        icon_theme: base.icon_theme.clone(),
        terminal: base.terminal.clone(),
        dir: PathBuf::new(),
    }
}

/// `theme.json` of user theme `id` under `config_home`.
pub fn user_theme_path(config_home: &Path, id: &str) -> PathBuf {
    config_home
        .join(shared::theme::USER_THEMES_DIR)
        .join(id)
        .join("theme.json")
}

/// The id `spec` is saved under: its own, unless another user theme holds it already (one the
/// studio did not open, `own`); then the first free `<id>-2`, `<id>-3`, ... Blocks.
pub fn free_id(config_home: &Path, id: &str, own: Option<&str>) -> String {
    let taken = |i: &str| {
        own != Some(i)
            && user_theme_path(config_home, i)
                .parent()
                .is_some_and(|d| d.symlink_metadata().is_ok())
    };
    if !taken(id) {
        return id.to_owned();
    }
    (2..)
        .map(|n| format!("{id}-{n}"))
        .find(|i| !taken(i))
        .unwrap_or_default()
}

/// Saves `spec` as a user theme (atomically); refuses one that does not read everywhere. Never
/// replaces another user theme of the same id: only `own` (the theme the studio opened) is
/// written over, any other gets a free id ([`free_id`]). Returns the file written.
/// Blocks: run it off the GTK thread. The error is the sentence.
pub fn save_user_theme(
    config_home: &Path,
    spec: &ThemeSpec,
    own: Option<&str>,
) -> Result<PathBuf, &'static str> {
    if !problems(&spec.tokens).is_empty() {
        return Err(copy::THEME_FIX_FIRST);
    }
    let mut spec = spec.clone();
    spec.id = free_id(config_home, &spec.id, own);
    let spec = &spec;
    let path = user_theme_path(config_home, &spec.id);
    let dir = path.parent().ok_or(copy::THEME_NOT_SAVED)?;
    std::fs::create_dir_all(dir).map_err(|_| copy::THEME_NOT_SAVED)?;
    super::editor::save_atomic(&path, &generate::to_json(spec))
        .map_err(|_| copy::THEME_NOT_SAVED)?;
    Ok(path)
}

/// Largest `theme.json` opened.
const MAX_THEME_FILE: u64 = 256 * 1024;

/// Reads a `theme.json` and resolves it against the installed catalogue (a theme that extends
/// another gets its tokens). Blocks: run it off the GTK thread. The error is the sentence.
pub fn open_theme(path: &Path) -> Result<Theme, &'static str> {
    let small = std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() <= MAX_THEME_FILE);
    if !small {
        return Err(copy::THEME_NOT_READ);
    }
    let text = std::fs::read_to_string(path).map_err(|_| copy::THEME_NOT_READ)?;
    let dir = path.parent().unwrap_or(Path::new(""));
    let spec = ThemeSpec::parse(&text, dir).map_err(|_| copy::THEME_NOT_READ)?;
    let home = wol_core::theme::config_home();
    let cat = ThemeCatalog::load(&wol_core::theme::search_paths(&home)).with(spec.clone());
    cat.resolve(&spec.id).map_err(|_| copy::THEME_NOT_READ)
}

/// Colour samples of a picture (a small copy is decoded: a palette needs no detail). Empty when
/// it cannot be read. Blocks: run it off the GTK thread.
pub fn samples_of(picture: &Path) -> Vec<Color> {
    let Ok(pb) = gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(picture, 128, 128, true) else {
        return Vec::new();
    };
    let bytes = pb.read_pixel_bytes();
    let dim = |v: i32| usize::try_from(v).unwrap_or(0);
    generate::samples(
        &bytes,
        dim(pb.width()),
        dim(pb.height()),
        dim(pb.rowstride()),
        dim(pb.n_channels()),
    )
}

// ---- The studio view ----

/// The Theme Studio page.
pub struct Studio {
    pub root: gtk::Box,
    base: RefCell<Theme>,
    /// The id of the theme opened (or last saved): the one Save may write over.
    own: RefCell<String>,
    shifts: RefCell<[Shift; 4]>,
    name: gtk::Entry,
    sliders: gtk::Box,
    preview: gtk::Box,
    checks: gtk::Box,
    status: gtk::Label,
}

impl Studio {
    /// The studio on `base` (the theme in use, or one opened). Nothing blocks here.
    pub fn new(base: Theme) -> Rc<Studio> {
        let root = w::page("Theme Studio");
        root.append(&w::footnote(copy::THEME_ABOUT));
        let actions = w::hbox(w::S2);
        root.append(&actions);
        let name = w::text_field("Theme name");
        name.set_max_length(NAME_MAX);
        name.set_text(&base.name);
        name.set_width_chars(w::VALUE_CHARS);
        let preview = w::hbox(w::S4);
        let checks = w::vbox(w::S2);
        let sliders = w::vbox(w::S4);
        let status = w::label("", "secondary");
        status.set_wrap(true);
        root.append(&w::section("", &[form::row("Name", "", &name)]));
        for (title, part) in [
            ("Preview", preview.clone()),
            ("Readability", checks.clone()),
        ] {
            let b = w::vbox(w::S2);
            b.append(&w::label(title, "title"));
            b.append(&part);
            root.append(&b);
        }
        root.append(&sliders);
        root.append(&status);
        let st = Rc::new(Studio {
            root,
            own: RefCell::new(base.id.clone()),
            base: RefCell::new(base),
            shifts: RefCell::new([Shift::default(); 4]),
            name,
            sliders,
            preview,
            checks,
            status,
        });
        let me = Rc::downgrade(&st);
        actions.append(&w::button("Open Theme…", w::Kind::Default, move || {
            if let Some(s) = me.upgrade() {
                s.choose();
            }
        }));
        let me = Rc::downgrade(&st);
        actions.append(&w::button(
            "Use Wallpaper Colors",
            w::Kind::Default,
            move || {
                if let Some(s) = me.upgrade() {
                    s.from_wallpaper();
                }
            },
        ));
        actions.append(&w::spacer());
        let me = Rc::downgrade(&st);
        actions.append(&w::button("Save Theme", w::Kind::Primary, move || {
            if let Some(s) = me.upgrade() {
                s.save();
            }
        }));
        st.rebuild();
        st
    }

    /// The tokens with the sliders applied.
    pub fn tokens(&self) -> BTreeMap<String, String> {
        apply(&self.base.borrow().tokens, &self.shifts.borrow())
    }

    /// A new base: the sliders go back to the middle.
    pub fn set_base(self: &Rc<Self>, t: Theme) {
        self.name.set_text(&t.name);
        self.base.replace(t);
        self.shifts.replace([Shift::default(); 4]);
        self.rebuild();
    }

    fn rebuild(self: &Rc<Self>) {
        while let Some(c) = self.sliders.first_child() {
            self.sliders.remove(&c);
        }
        let shifts = *self.shifts.borrow();
        for (i, ((title, _), s)) in GROUPS.iter().zip(shifts).enumerate() {
            let (me, me2) = (Rc::downgrade(self), Rc::downgrade(self));
            let light = form::slider_row(
                "Lightness",
                LIGHT_RANGE,
                s.light * 100.0,
                ("Darker", "Lighter"),
                move |v| {
                    if let Some(st) = me.upgrade() {
                        if let Some(x) = st.shifts.borrow_mut().get_mut(i) {
                            x.light = v / 100.0;
                        }
                        st.refresh();
                    }
                },
            );
            let chroma = form::slider_row(
                "Color",
                CHROMA_RANGE,
                s.chroma * 100.0,
                ("Grey", "Vivid"),
                move |v| {
                    if let Some(st) = me2.upgrade() {
                        if let Some(x) = st.shifts.borrow_mut().get_mut(i) {
                            x.chroma = v / 100.0;
                        }
                        st.refresh();
                    }
                },
            );
            self.sliders.append(&w::section(title, &[light, chroma]));
        }
        self.refresh();
    }

    /// Redraws the previews and the checks from the current tokens.
    fn refresh(&self) {
        let tokens = self.tokens();
        let mut t = self.base.borrow().clone();
        t.tokens = tokens.clone();
        while let Some(c) = self.preview.first_child() {
            self.preview.remove(&c);
        }
        let card = w::theme_preview(vec![w::Palette::of(&t)], &t.name, false);
        card.set_can_target(false);
        self.preview.append(&card);
        self.preview.append(&terminal_strip(&t));
        while let Some(c) = self.checks.first_child() {
            self.checks.remove(&c);
        }
        for line in badges(&tokens).chunks(BADGES_PER_LINE) {
            let row = w::hbox(w::S2);
            for b in line {
                let tone = if b.passes() {
                    d::Tone::Success
                } else {
                    d::Tone::Danger
                };
                row.append(&d::badge(&badge_text(b), tone));
            }
            self.checks.append(&row);
        }
        let ok = problems(&tokens).is_empty();
        self.checks.append(&w::label(
            if ok {
                copy::THEME_READABLE
            } else {
                copy::THEME_UNREADABLE
            },
            "secondary",
        ));
    }

    /// Open Theme…: a `theme.json` from anywhere.
    fn choose(self: &Rc<Self>) {
        let filter = gtk::FileFilter::new();
        filter.add_suffix("json");
        filter.set_name(Some(copy::THEME_FILTER));
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dlg = gtk::FileDialog::builder()
            .title(copy::OPEN_THEME_TITLE)
            .modal(true)
            .filters(&filters)
            .build();
        let win = self.root.root().and_downcast::<gtk::Window>();
        let me = Rc::downgrade(self);
        dlg.open(win.as_ref(), gio::Cancellable::NONE, move |r| {
            let (Some(s), Some(p)) = (me.upgrade(), r.ok().and_then(|f| f.path())) else {
                return;
            };
            s.open(p);
        });
    }

    /// Opens `path` off the GTK thread.
    pub fn open(self: &Rc<Self>, path: PathBuf) {
        let me = Rc::downgrade(self);
        off_thread(
            move || open_theme(&path),
            move |r| {
                let Some(s) = me.upgrade() else { return };
                match r {
                    Ok(t) => {
                        s.status.set_label("");
                        s.own.replace(t.id.clone());
                        s.set_base(t);
                    }
                    Err(m) => s.status.set_label(m),
                }
            },
        );
    }

    /// Use Wallpaper Colors: a theme from the desktop picture, of the same variant.
    fn from_wallpaper(self: &Rc<Self>) {
        let Some(pic) = crate::kit::pickers::desktop_wallpaper() else {
            self.status.set_label(copy::THEME_NO_PICTURE);
            return;
        };
        let variant: Variant = self.base.borrow().variant;
        let name = self.name.text().to_string();
        let me = Rc::downgrade(self);
        off_thread(
            move || samples_of(&pic),
            move |samples| {
                let Some(s) = me.upgrade() else { return };
                if samples.is_empty() {
                    s.status.set_label(copy::THEME_NO_PICTURE);
                    return;
                }
                let spec = generate::from_samples(&slug(&name), &name, &samples, Some(variant));
                s.status.set_label("");
                s.set_base(resolved(&spec));
            },
        );
    }

    /// Save Theme: the tokens as a user theme, when every check passes.
    fn save(self: &Rc<Self>) {
        let name = self.name.text().trim().to_string();
        let name = if name.is_empty() {
            self.base.borrow().name.clone()
        } else {
            name
        };
        let spec = spec_for(&self.base.borrow(), &name, self.tokens());
        let home = wol_core::theme::config_home();
        let own = self.own.borrow().clone();
        let me = Rc::downgrade(self);
        off_thread(
            move || save_user_theme(&home, &spec, Some(&own)),
            move |r| {
                if let Some(s) = me.upgrade() {
                    s.status.set_label(match r {
                        Ok(p) => {
                            // The next Save writes over this one, under the id it got.
                            if let Some(id) = p.parent().and_then(|d| d.file_name()) {
                                s.own.replace(id.to_string_lossy().into_owned());
                            }
                            copy::THEME_SAVED
                        }
                        Err(m) => m,
                    });
                }
            },
        );
    }
}

/// The terminal's 16 colours over its background, as every terminal shows this theme.
fn terminal_strip(t: &Theme) -> gtk::DrawingArea {
    let p = terminal::palette(t);
    let area = gtk::DrawingArea::builder()
        .content_width(TERMINAL.0)
        .content_height(TERMINAL.1)
        .build();
    area.add_css_class("preview-frame");
    area.set_overflow(gtk::Overflow::Hidden);
    area.set_valign(gtk::Align::Center);
    area.update_property(&[gtk::accessible::Property::Label("Terminal colors")]);
    area.set_draw_func(move |_, cr, wd, ht| {
        let (wd, ht) = (f64::from(wd), f64::from(ht));
        paint(cr, p.background);
        cr.paint().ok();
        let pad = f64::from(w::S2);
        let side = (wd - pad * 2.0) / 8.0;
        for (i, c) in p.ansi.iter().enumerate() {
            let (col, row) = (i % 8, i / 8);
            let x = pad + side * f64::from(u8::try_from(col).unwrap_or(0));
            let y = pad + (side + pad / 2.0) * f64::from(u8::try_from(row).unwrap_or(0));
            paint(cr, *c);
            rounded(cr, x + 1.0, y, side - 2.0, side, 3.0);
            cr.fill().ok();
        }
        // The prompt line: foreground and cursor under the swatches.
        let y = ht - pad - 4.0;
        paint(cr, p.foreground);
        rounded(cr, pad, y, wd * 0.4, 4.0, 2.0);
        cr.fill().ok();
        paint(cr, p.cursor);
        cr.rectangle(pad + wd * 0.4 + 4.0, y - 4.0, 6.0, 8.0);
        cr.fill().ok();
    });
    area
}

/// Contrast badges per line under Readability.
const BADGES_PER_LINE: usize = 3;

/// Terminal preview size in px: the gallery card's height, a wider strip.
const TERMINAL: (i32, i32) = (200, 72);

/// Every token group lists real tokens, and every token is in exactly one group.
#[cfg(test)]
mod tests {
    /// QA-PLUG-50: badges say "Accent text on accent 4.5:1", not the token ids; the name has a cap.
    #[test]
    fn badges_use_words_not_token_ids() {
        use super::*;
        assert_eq!(token_words("accentText"), "accent text");
        assert_eq!(token_words("glass"), "glass");
        let b = Badge {
            fg: "accentText",
            bg: "accent",
            ratio: 4.52,
            min: 4.5,
        };
        assert_eq!(badge_text(&b), "Accent text on accent 4.5:1");
        assert!(!badge_text(&b).contains("accentText"));
        assert_eq!(NAME_MAX, 40);
        let src = include_str!("theme_studio.rs");
        assert!(src.contains("name.set_max_length(NAME_MAX)"));
    }

    use super::*;

    fn wol(id: &str) -> Theme {
        ThemeCatalog::builtin().resolve(id).unwrap()
    }

    #[test]
    fn groups_cover_every_token_once() {
        let mut all: Vec<&str> = GROUPS.iter().flat_map(|(_, t)| t.iter().copied()).collect();
        all.sort_unstable();
        let mut want = shared::theme::TOKENS.to_vec();
        want.sort_unstable();
        assert_eq!(all, want);
    }

    #[test]
    fn oklab_round_trips_and_knows_its_greys() {
        for id in ["wol-light", "wol-dark"] {
            for (k, v) in wol(id).tokens {
                let c = Color::parse(&v).unwrap();
                let back = from_oklab(to_oklab(c), c.a);
                assert_eq!(back.to_hex(), c.to_hex(), "{id} {k}");
            }
        }
        let [l, a, b] = to_oklab(Color::WHITE);
        assert!((l - 1.0).abs() < 1e-3 && a.abs() < 1e-3 && b.abs() < 1e-3);
        let black = Color {
            r: 0,
            g: 0,
            b: 0,
            a: 1.0,
        };
        assert!(to_oklab(black)[0].abs() < 1e-6);
        // Known value: sRGB red is L 0.628, a 0.225, b 0.126 in OKLab.
        let [l, a, b] = to_oklab(Color {
            r: 255,
            g: 0,
            b: 0,
            a: 1.0,
        });
        assert!((l - 0.628).abs() < 1e-3 && (a - 0.225).abs() < 1e-3 && (b - 0.126).abs() < 1e-3);
    }

    #[test]
    fn shifts_move_lightness_and_chroma_and_keep_alpha() {
        let t = wol("wol-dark");
        let c = Color::parse(&t.tokens["accent"]).unwrap();
        assert_eq!(shift(c, Shift::default()).to_hex(), c.to_hex());
        assert!(
            to_oklab(shift(
                c,
                Shift {
                    light: 0.2,
                    chroma: 1.0
                }
            ))[0]
                > to_oklab(c)[0]
        );
        let grey = shift(
            c,
            Shift {
                light: 0.0,
                chroma: 0.0,
            },
        );
        assert!(
            grey.r.abs_diff(grey.g) <= 1 && grey.g.abs_diff(grey.b) <= 1,
            "{grey:?}"
        );
        let glass = Color::parse(&t.tokens["glass"]).unwrap();
        assert!(glass.a < 1.0, "the glass token is translucent");
        let moved = shift(
            glass,
            Shift {
                light: 0.1,
                chroma: 1.0,
            },
        );
        assert!((moved.a - glass.a).abs() < 1e-9);
        let back = Color::parse(&css(moved)).unwrap();
        assert_eq!(back.to_hex(), moved.to_hex(), "what is written reads back");
        assert!((back.a - moved.a).abs() <= 1.0 / 255.0);
    }

    #[test]
    fn sliders_move_only_their_group() {
        let t = wol("wol-dark");
        let mut s = [Shift::default(); 4];
        assert_eq!(apply(&t.tokens, &s), t.tokens, "the middle changes nothing");
        s[2] = Shift {
            light: -0.1,
            chroma: 1.5,
        };
        let out = apply(&t.tokens, &s);
        for (k, v) in &t.tokens {
            let moved = out[k] != *v;
            let in_accent = ["accent", "accentText"].contains(&k.as_str());
            assert!(!moved || in_accent, "{k} moved");
        }
        assert_ne!(out["accent"], t.tokens["accent"]);
    }

    #[test]
    fn badges_agree_with_the_theme_contract() {
        let t = wol("wol-light");
        let b = badges(&t.tokens);
        assert_eq!(b.len(), CONTRAST_RULES.len());
        assert!(b.iter().all(Badge::passes), "{b:?}");
        assert!(problems(&t.tokens).is_empty());
        // Text pushed down to the surface: the same rules fail in both.
        let mut s = [Shift::default(); 4];
        s[1] = Shift {
            light: 0.6,
            chroma: 1.0,
        };
        let worse = apply(&t.tokens, &s);
        let failing: Vec<String> = badges(&worse)
            .iter()
            .filter(|b| !b.passes())
            .map(|b| format!("{}/{}", b.fg, b.bg))
            .collect();
        let contract: Vec<String> = problems(&worse)
            .iter()
            .filter_map(|p| p.split(':').next().map(str::to_owned))
            .collect();
        assert!(!failing.is_empty());
        assert_eq!(failing, contract);
    }

    #[test]
    fn a_saved_theme_is_a_user_theme_the_catalogue_reads() {
        let home = std::env::temp_dir().join(format!("wol-maker-theme-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let base = wol("wol-dark");
        let mut s = [Shift::default(); 4];
        s[0] = Shift {
            light: 0.03,
            chroma: 1.2,
        };
        let spec = spec_for(&base, "Night Owl", apply(&base.tokens, &s));
        assert_eq!(spec.id, "night-owl");
        let p = save_user_theme(&home, &spec, None).unwrap();
        assert_eq!(p, home.join("wolos/themes/night-owl/theme.json"));
        let back = ThemeCatalog::load(&wol_core::theme::search_paths(&home))
            .resolve("night-owl")
            .unwrap();
        assert_eq!(back.tokens, spec.tokens);
        assert!(back.check().is_empty());
        // An unreadable one is refused and nothing is written.
        let mut bad = spec.clone();
        bad.id = "bad".into();
        bad.tokens
            .insert("text".into(), bad.tokens["surface"].clone());
        assert_eq!(
            save_user_theme(&home, &bad, None),
            Err(copy::THEME_FIX_FIRST)
        );
        assert!(!user_theme_path(&home, "bad").exists());
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn a_second_theme_of_the_same_id_never_replaces_the_first() {
        let home = std::env::temp_dir().join(format!("wol-maker-slug-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let base = wol("wol-dark");
        let first = spec_for(&base, "Night Owl", base.tokens.clone());
        let a = save_user_theme(&home, &first, None).unwrap();
        let second = spec_for(&base, "night owl!", base.tokens.clone());
        assert_eq!(second.id, first.id, "the same slug");
        let b = save_user_theme(&home, &second, None).unwrap();
        assert_eq!(b, user_theme_path(&home, "night-owl-2"));
        assert_ne!(
            std::fs::read_to_string(&a).unwrap(),
            std::fs::read_to_string(&b).unwrap(),
            "the first theme is untouched"
        );
        // The theme the studio opened is written over in place.
        assert_eq!(
            save_user_theme(&home, &second, Some("night-owl")).unwrap(),
            a
        );
        assert_eq!(free_id(&home, "night-owl", None), "night-owl-3");
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn a_theme_from_colors_reads_and_round_trips() {
        let samples = vec![
            Color {
                r: 30,
                g: 60,
                b: 120,
                a: 1.0
            };
            64
        ];
        let spec = generate::from_samples("dusk", "Dusk", &samples, Some(Variant::Dark));
        let t = resolved(&spec);
        assert!(t.check().is_empty());
        assert_eq!(t.variant, Variant::Dark);
    }

    #[test]
    fn ids_come_from_names_and_never_take_a_wolos_look() {
        assert_eq!(slug("Night Owl 2"), "night-owl-2");
        assert_eq!(slug("  "), "my-theme");
        assert_eq!(slug("Wol Dark"), "my-wol-dark");
    }

    #[test]
    fn a_file_that_is_not_a_theme_says_so() {
        let d = std::env::temp_dir().join(format!("wol-maker-open-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("theme.json");
        std::fs::write(&f, "{\"id\": 1}").unwrap();
        assert_eq!(open_theme(&f).unwrap_err(), copy::THEME_NOT_READ);
        assert_eq!(
            open_theme(&d.join("none.json")).unwrap_err(),
            copy::THEME_NOT_READ
        );
        let _ = std::fs::remove_dir_all(d);
    }
}
