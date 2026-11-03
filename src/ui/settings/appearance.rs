// SPDX-License-Identifier: GPL-2.0-or-later
//! Appearance: Light, Dark or Auto with an accent colour (the WolOS themes), any theme of the
//! catalogue in one click, the theme packs of enabled plugins, a theme made from the wallpaper and
//! a way to the wallpaper itself. Every pick is journaled (`wol undo`, Settings > Restore).
use super::*;
use crate::ui::theme_picker;
use wol_core::theme::generate;

/// Saved id of the theme made from the wallpaper (a user theme folder, so every catalogue sees it).
const FROM_WALLPAPER: &str = "from-wallpaper";

/// Saves `id` (None = follow light/dark) as one journaled change, then restyles every app.
/// Err when the save or the restyle failed: the caller says so and puts the old pick back.
fn choose(id: Option<&str>) -> std::io::Result<()> {
    save_pick(std::path::Path::new(&kit::config_home()), id)?;
    match id {
        // Writes the same text again (no second journal entry), then restyles the apps.
        Some(id) => kit::style::set_active(id),
        None => crate::theme::clear(),
    }
}

/// The journaled save of [`choose`], under the config home `home`: the theme file as
/// `crate::theme::save` writes it, so the restyle after it is not a second change to undo.
fn save_pick(home: &std::path::Path, id: Option<&str>) -> std::io::Result<()> {
    let text = id.map(|id| {
        format!(
            "{id}
"
        )
    });
    wol_core::journal::Journal::for_config(home).save(
        "theme",
        "settings",
        &[(shared::theme::ACTIVE_FILE, text.as_deref())],
        wol_core::journal::now(),
    )?;
    changed("theme");
    Ok(())
}

/// The variant whose accent colours the swatches show: Light/Dark as picked; Auto the one it
/// shows at `hour` (dark from evening to morning, as the desktop switches).
fn swatch_variant(mode: Option<Variant>, hour: u8) -> Variant {
    match mode {
        Some(v) => v,
        None if accent::auto_is_dark(hour) => Variant::Dark,
        None => Variant::Light,
    }
}

/// The swatches' colours in `variant`: (accent id, name, colour).
fn swatch_colors(variant: Variant) -> Vec<(&'static str, &'static str, &'static str)> {
    accent::ACCENTS
        .iter()
        .map(|(id, name, ..)| (*id, *name, accent::color(variant, id).unwrap_or_default()))
        .collect()
}

/// Page state shared by the tiles, the swatches and the theme cards.
struct Look {
    /// (mode, accent) of the Light / Dark / Auto tiles, as saved.
    pick: RefCell<(Option<Variant>, String)>,
    /// Set while a failed pick is being put back, so the toggles do not save again.
    reverting: Cell<bool>,
    status: gtk::Label,
    /// The tile or theme card that shows what is saved (None: a theme no card shows), and the
    /// one just clicked; a failed save checks `checked` again.
    checked: RefCell<Option<gtk::ToggleButton>>,
    clicked: RefCell<Option<gtk::ToggleButton>>,
    tiles: RefCell<Vec<gtk::ToggleButton>>,
    /// Shown while a tile is checked: accents apply to the WolOS looks only.
    accent_row: gtk::Box,
    swatches: gtk::FlowBox,
}

impl Look {
    /// Puts `t` in the one group of tiles and cards and follows which is clicked.
    fn join(self: &Rc<Self>, join: &dyn Fn(&gtk::ToggleButton), t: &gtk::ToggleButton) {
        join(t);
        if t.is_active() {
            *self.checked.borrow_mut() = Some(t.clone());
        }
        // Runs before the card's own handler (connected after this), which saves.
        let me = Rc::downgrade(self);
        t.connect_toggled(move |t| {
            if let (true, Some(me)) = (t.is_active(), me.upgrade())
                && !me.reverting.get()
            {
                *me.clicked.borrow_mut() = Some(t.clone());
            }
        });
    }

    /// A tile or a swatch: saves the WolOS look `next`.
    fn apply(self: &Rc<Self>, next: (Option<Variant>, String), from_tile: bool) {
        if self.reverting.get() {
            return;
        }
        let was = self.pick.replace(next.clone());
        let r = choose(accent::theme_id(next.0, &next.1).as_deref());
        if r.is_err() {
            *self.pick.borrow_mut() = was.clone();
        }
        self.settle(r, from_tile, was.0 != next.0);
    }

    /// A theme card: saves catalogue theme `id`.
    fn pick_theme(self: &Rc<Self>, id: &str) {
        if self.reverting.get() {
            return;
        }
        self.settle(choose(Some(id)), true, false);
    }

    /// After a save: the status line, the accent row and the swatches follow; a failure checks
    /// the old tile or card (and swatch) again.
    fn settle(self: &Rc<Self>, r: std::io::Result<()>, moved: bool, recolor: bool) {
        let ok = r.is_ok();
        say(&self.status, if ok { "" } else { SAVE_FAILED });
        if moved {
            let clicked = self.clicked.borrow_mut().take();
            if ok {
                *self.checked.borrow_mut() = clicked;
            } else {
                self.reverting.set(true);
                match (self.checked.borrow().clone(), clicked) {
                    (Some(old), _) => old.set_active(true),
                    (None, Some(new)) => new.set_active(false),
                    (None, None) => {}
                }
                self.reverting.set(false);
            }
        }
        let on_tile = self
            .checked
            .borrow()
            .as_ref()
            .is_some_and(|c| self.tiles.borrow().contains(c));
        self.accent_row.set_visible(on_tile);
        if recolor || !ok {
            self.fill_swatches();
        }
    }

    /// (Re)builds the swatches in the colours of the current variant (S7-17).
    fn fill_swatches(self: &Rc<Self>) {
        while let Some(c) = self.swatches.first_child() {
            self.swatches.remove(&c);
        }
        let (mode, acc) = self.pick.borrow().clone();
        let variant = swatch_variant(mode, kit::now().hour);
        let mut first: Option<gtk::ToggleButton> = None;
        for (id, name, hex) in swatch_colors(variant) {
            let c = Color::parse(hex).unwrap_or(Color::WHITE);
            let dot = Color::parse(accent::text_on(hex)).unwrap_or(Color::WHITE);
            let s = w::accent_swatch(c, dot, name, id == acc);
            if let Some(f) = &first {
                s.set_group(Some(f));
            } else {
                first = Some(s.clone());
            }
            let me = Rc::downgrade(self);
            s.connect_toggled(move |s| {
                if let (true, Some(me)) = (s.is_active(), me.upgrade()) {
                    let mode = me.pick.borrow().0;
                    me.apply((mode, id.to_string()), false);
                }
            });
            self.swatches.insert(&s, -1);
        }
    }
}

/// `wolos/themes/<id>/theme.json`, relative to `$XDG_CONFIG_HOME`.
fn user_theme_file(id: &str) -> String {
    format!("{}/{id}/theme.json", shared::theme::USER_THEMES_DIR)
}

/// The theme the wallpaper's colours make (`generate::from_samples`); None when the picture
/// cannot be read. Reads a small copy: a palette needs no detail.
fn theme_from(picture: &std::path::Path) -> Option<String> {
    let pb = gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(picture, 128, 128, true).ok()?;
    let bytes = pb.read_pixel_bytes();
    let samples = generate::samples(
        &bytes,
        usize::try_from(pb.width()).ok()?,
        usize::try_from(pb.height()).ok()?,
        usize::try_from(pb.rowstride()).ok()?,
        usize::try_from(pb.n_channels()).ok()?,
    );
    if samples.is_empty() {
        return None;
    }
    let mut spec = generate::from_samples(FROM_WALLPAPER, "From Your Wallpaper", &samples, None);
    spec.wallpaper = Some(picture.display().to_string());
    Some(generate::to_json(&spec))
}

/// Short name of the desktop picture for the Wallpaper row.
fn picture_name(p: Option<&std::path::Path>) -> String {
    let Some(stem) = p.and_then(|p| p.file_stem()) else {
        return "Default".into();
    };
    stem.to_string_lossy()
        .split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let p = w::page("Appearance");
    let status = status_label();
    let saved = kit::style::active_id();
    let (mode, acc) = accent::split(saved.as_deref());
    let custom = saved
        .as_deref()
        .filter(|id| !theme_picker::is_wolos_look(id))
        .map(str::to_string);
    let light = crate::theme::resolve(Some("wol-light"), false);
    let dark = crate::theme::resolve(Some("wol-dark"), true);
    let tiles = w::wrap_row(w::S3, 3);
    let choices: [(Option<Variant>, &str, Vec<w::Palette>); 3] = [
        (Some(Variant::Light), "Light", vec![w::Palette::of(&light)]),
        (Some(Variant::Dark), "Dark", vec![w::Palette::of(&dark)]),
        (
            None,
            "Auto",
            vec![w::Palette::of(&light), w::Palette::of(&dark)],
        ),
    ];
    let accent_row = w::vbox(0);
    accent_row.set_visible(custom.is_none());
    let look = Rc::new(Look {
        pick: RefCell::new((mode, acc.to_string())),
        reverting: Cell::new(false),
        status: status.clone(),
        checked: RefCell::new(None),
        clicked: RefCell::new(None),
        tiles: RefCell::new(Vec::new()),
        accent_row: accent_row.clone(),
        swatches: w::wrap_row(w::S2, 8),
    });
    // One group for the tiles and every gallery: exactly one look is checked.
    let group = theme_picker::group();
    let join = {
        let look = look.clone();
        move |t: &gtk::ToggleButton| look.join(&group, t)
    };
    for (v, title, slices) in choices {
        let t = w::theme_preview(slices, title, custom.is_none() && v == mode);
        look.tiles.borrow_mut().push(t.clone());
        join(&t);
        let me = Rc::downgrade(&look);
        t.connect_toggled(move |t| {
            if let (true, Some(me)) = (t.is_active(), me.upgrade()) {
                let acc = me.pick.borrow().1.clone();
                me.apply((v, acc), true);
            }
        });
        tiles.append(&t);
    }
    p.append(&w::section("", &[tiles.clone().upcast()]));
    if let Some(card) = tiles.parent() {
        card.add_css_class("preview-card");
    }

    look.fill_swatches();
    let swatches = look.swatches.clone();
    accent_row.append(&w::section("", &[row("Accent color", &swatches)]));
    p.append(&accent_row);

    // The catalogue (theme folders, plugin packs) is read off the GTK thread; the galleries
    // join the tiles' group once it is in.
    let themes = w::vbox(w::S6);
    p.append(&themes);
    let pick: Rc<dyn Fn(&str)> = {
        let me = Rc::downgrade(&look);
        Rc::new(move |id: &str| {
            if let Some(me) = me.upgrade() {
                me.pick_theme(id);
            }
        })
    };
    later(&themes, theme_picker::load, move |into, (cat, packs)| {
        let (own, packed) = theme_picker::gallery(&cat, &packs);
        let gallery = |title: &str, list: &[theme_picker::Entry]| {
            let cards = theme_picker::cards(&cat, list, custom.as_deref(), &join, pick.clone());
            let s = w::section(title, &[cards.clone().upcast()]);
            if let Some(card) = cards.parent() {
                card.add_css_class("preview-card");
            }
            s
        };
        into.append(&gallery("Themes", &own));
        if !packed.is_empty() {
            into.append(&gallery("From Plugins", &packed));
        }
        // Settings draws with what it lists, as the theme picker does.
        crate::theme::adopt(cat);
    });
    p.append(&w::footnote(
        "A theme restyles the whole desktop and your apps at once. Accent colors apply to the WolOS Light and Dark looks.",
    ));

    let picture = kit::pickers::desktop_wallpaper();
    let (st, pic) = (status.clone(), picture.clone());
    p.append(&w::section(
        "",
        &[
            form::button_row(
                "Wallpaper",
                &picture_name(picture.as_deref()),
                "Choose…",
                e.opener("Wallpaper"),
            ),
            form::button_row(
                "Theme from your wallpaper",
                crate::copy::settings::THEME_FROM_WALLPAPER,
                "Make Theme",
                move || {
                    // The decode can take hundreds of ms on a large PNG: off the GTK thread.
                    let (st, path) = (st.clone(), pic.clone());
                    off_thread(
                        move || path.as_deref().and_then(theme_from),
                        move |made| {
                            let Some(json) = made else {
                                say(&st, crate::copy::settings::THEME_PICTURE_UNREADABLE);
                                return;
                            };
                            let rel = user_theme_file(FROM_WALLPAPER);
                            let saved = save_journaled("theme", &rel, Some(&json))
                                .and_then(|()| choose(Some(FROM_WALLPAPER)));
                            say(&st, if saved.is_ok() { "" } else { SAVE_FAILED });
                        },
                    );
                },
            ),
            form::button_row(
                "Make a theme",
                crate::copy::maker::APPEARANCE_ROW,
                "Open Studio",
                || crate::ui::maker::launch(&crate::ui::maker::THEME_ARGV),
            ),
        ],
    ));
    p.append(&status);
    // Every handler holds the state weakly: the page owns it, or no tile or swatch answers.
    let keep = look;
    p.connect_destroy(move |_| {
        let _ = &keep;
    });
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn the_wallpaper_theme_is_a_user_theme_the_catalogue_reads() {
        let rel = user_theme_file(FROM_WALLPAPER);
        assert_eq!(rel, "wolos/themes/from-wallpaper/theme.json");
        let home = Path::new("/home/u/.config");
        let dir = home.join(&rel);
        assert!(
            wol_core::theme::search_paths(home)
                .iter()
                .any(|p| dir.starts_with(p))
        );
        assert!(!theme_picker::is_wolos_look(FROM_WALLPAPER));
    }

    #[test]
    fn a_pick_that_cannot_be_saved_is_an_error() {
        let dir = std::env::temp_dir().join(format!("wol-appearance-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("tmp");
        // A file where the config folder should be: nothing under it can be written.
        let blocked = dir.join("not-a-dir");
        std::fs::write(&blocked, "").expect("file");
        assert!(save_pick(&blocked, Some("wol-dark")).is_err());
        let home = dir.join("home");
        assert!(save_pick(&home, Some("wol-dark")).is_ok());
        // As `crate::theme::save` writes it: the restyle after it changes nothing more to undo.
        assert_eq!(
            std::fs::read_to_string(home.join(shared::theme::ACTIVE_FILE)).ok(),
            Some(
                "wol-dark
"
                .to_string()
            )
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_swatches_take_the_colours_of_the_look_on_screen() {
        assert_eq!(swatch_variant(Some(Variant::Dark), 12), Variant::Dark);
        assert_eq!(swatch_variant(Some(Variant::Light), 23), Variant::Light);
        assert_eq!(swatch_variant(None, 12), Variant::Light, "Auto by day");
        assert_eq!(swatch_variant(None, 22), Variant::Dark, "Auto at night");
        let (light, dark) = (swatch_colors(Variant::Light), swatch_colors(Variant::Dark));
        assert_eq!(light.len(), accent::ACCENTS.len());
        assert_ne!(light, dark, "Dark has its own accent colours");
        for (id, _, hex) in &dark {
            assert_eq!(accent::color(Variant::Dark, id), Some(*hex));
        }
    }

    #[test]
    fn picture_names_read_like_words() {
        assert_eq!(
            picture_name(Some(Path::new(
                "/usr/share/wolos/wallpapers/wonderland-dusk.svg"
            ))),
            "Wonderland Dusk"
        );
        assert_eq!(picture_name(None), "Default");
    }
}
