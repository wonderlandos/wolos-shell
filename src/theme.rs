// SPDX-License-Identifier: GPL-2.0-or-later
//! Theme -> GTK CSS. Catalog, contract and active-theme file come from `wol_core::theme` / `shared::theme`;
//! this module only adds the kit stylesheet.
pub use shared::theme::Variant;
use wol_core::theme::{self as core_theme, ThemeCatalog, active};
pub type Theme = core_theme::ResolvedTheme;

thread_local! {
    /// Built-ins and the accent variants only: no disk work on the GTK thread. The installed
    /// themes (folders, plugin packs) are read off-thread the first time an id is unknown.
    static CATALOG: std::cell::RefCell<ThemeCatalog> =
        std::cell::RefCell::new(with_accents(ThemeCatalog::builtin()));
    /// Ids already looked for on disk and not found: one re-read per id (audit6 S6-2).
    static MISSED: std::cell::RefCell<std::collections::HashSet<String>> = Default::default();
    /// Runs on the GTK thread after the installed themes were adopted (the stylesheet loader).
    static REFRESHED: std::cell::RefCell<Option<std::rc::Rc<dyn Fn()>>> = Default::default();
}

/// Built-ins, theme folders and enabled plugins' theme packs (ADR 0031 themes v2). Walks the
/// disk: only ever called from a worker thread ([`refresh_off_thread`]).
fn installed() -> ThemeCatalog {
    let data = core_theme::apps::data_home(|k| std::env::var(k).ok());
    ThemeCatalog::installed(&core_theme::config_home(), data.as_deref())
}

/// `f` runs on the GTK thread each time a background re-read of the installed themes was
/// adopted, so the loader can draw an id that was unknown until then.
pub fn on_refreshed(f: std::rc::Rc<dyn Fn()>) {
    REFRESHED.with(|r| *r.borrow_mut() = Some(f));
}

/// The installed themes may have changed (a plugin turned on or off, a theme folder or the
/// active-theme file changed): every id gets one fresh look on disk at its next resolve
/// (audit7 R7-4). Without this, an id missed once stayed on the default until a restart.
pub fn forget_missed() {
    MISSED.with(forget);
}

fn forget(missed: &std::cell::RefCell<std::collections::HashSet<String>>) {
    missed.borrow_mut().clear();
}

/// What the shell watches for [`forget_missed`]: the enabled-plugins file (theme packs) and the
/// account's theme folder. The active-theme file is watched by the stylesheet loader already.
pub fn installed_sources() -> [std::path::PathBuf; 2] {
    let home = core_theme::config_home();
    [
        wol_core::plugins::enabled_path(&home),
        home.join(shared::theme::USER_THEMES_DIR),
    ]
}

/// Reads the installed themes on a worker thread and adopts them on the GTK thread.
fn refresh_off_thread() {
    let _ = std::thread::Builder::new()
        .name("theme-refresh".into())
        .spawn(|| {
            let cat = installed();
            gtk::glib::MainContext::default().invoke(move || {
                CATALOG.with(|c| MISSED.with(|m| adopt_in(c, m, cat)));
                redraw();
            });
        });
}

/// Draws with `installed` (built-ins, theme folders and plugin packs, read off the GTK thread by
/// the theme picker) from now on, plus the accent variants: a pick the picker offers is one the
/// shell can draw (audit5 S9-2). No disk work. An id missed earlier that `installed` knows is
/// drawn now: it leaves `MISSED` and the loader redraws (audit7 R7-4; before, the desktop stayed
/// on the default until the next theme-file event).
pub fn adopt(installed: ThemeCatalog) {
    if CATALOG.with(|c| MISSED.with(|m| adopt_in(c, m, installed))) {
        redraw();
    }
}

/// Swaps `installed` (plus accents) in; true when it knows an id in `missed`, which then leaves
/// it. Ids it still lacks stay missed: `installed` is a fresh look on disk, and clearing them
/// would start a re-read after every adopt, whose own adopt would start the next one.
fn adopt_in(
    cat: &std::cell::RefCell<ThemeCatalog>,
    missed: &std::cell::RefCell<std::collections::HashSet<String>>,
    installed: ThemeCatalog,
) -> bool {
    let fresh = with_accents(installed);
    let mut m = missed.borrow_mut();
    let before = m.len();
    m.retain(|id| fresh.resolve(id).is_err());
    *cat.borrow_mut() = fresh;
    m.len() != before
}

/// Runs the loader registered with [`on_refreshed`], outside any borrow of the catalog.
fn redraw() {
    let f = REFRESHED.with(|r| r.borrow().clone());
    if let Some(f) = f {
        f();
    }
}

fn with_accents(installed: ThemeCatalog) -> ThemeCatalog {
    core_theme::accent::variants()
        .into_iter()
        .fold(installed, ThemeCatalog::with)
}

/// Saved id (None = follow appearance) -> resolved theme; unknown ids fall back to the variant default.
/// `wol-auto-<accent>` follows `prefer_dark` (`wol_core::theme::accent`).
/// Never touches the disk: an id the catalog does not know (a pack picked after the shell
/// started, or one that was removed) draws the default now and starts one background re-read
/// per id, after which `on_refreshed` redraws (audit5 S9-2, audit6 S6-2).
pub fn resolve(saved: Option<&str>, prefer_dark: bool) -> Theme {
    CATALOG.with(|c| MISSED.with(|m| resolve_in(c, m, refresh_off_thread, saved, prefer_dark)))
}

fn resolve_in(
    cat: &std::cell::RefCell<ThemeCatalog>,
    missed: &std::cell::RefCell<std::collections::HashSet<String>>,
    refresh: impl FnOnce(),
    saved: Option<&str>,
    prefer_dark: bool,
) -> Theme {
    let saved = core_theme::accent::effective(saved, prefer_dark);
    let id = active::effective(saved.as_deref(), prefer_dark);
    if cat.borrow().resolve(&id).is_ok() {
        missed.borrow_mut().remove(&id);
    } else if missed.borrow_mut().insert(id.clone()) {
        refresh();
    }
    let c = cat.borrow();
    c.resolve(&id)
        .or_else(|_| {
            c.resolve(
                if prefer_dark {
                    Variant::Dark
                } else {
                    Variant::Light
                }
                .default_theme(),
            )
        })
        .expect("built-in themes resolve")
}

/// `$XDG_CONFIG_HOME/wolos/theme`.
pub fn active_path() -> std::path::PathBuf {
    active::default_file(&core_theme::config_home())
}

pub fn saved() -> Option<String> {
    active::get(&active_path())
}

/// Saves the active theme, journaled (`wol undo`, Settings > Restore).
pub fn save(id: &str) -> std::io::Result<()> {
    journal_theme(Some(&format!("{id}\n")))
}

/// Back to following the appearance (no theme file), journaled.
pub fn clear() -> std::io::Result<()> {
    journal_theme(None)
}

fn journal_theme(text: Option<&str>) -> std::io::Result<()> {
    let home = core_theme::config_home();
    wol_core::journal::Journal::for_config(&home)
        .save(
            "theme",
            "settings",
            &[(shared::theme::ACTIVE_FILE, text)],
            wol_core::journal::now(),
        )
        .map(drop)
}

/// Kit stylesheet: the only visual style (HIG). Colours via `var(--token)`.
pub const KIT_CSS: &str = include_str!("kit/kit.css");

/// The dark scheme's elevation shadows and glass rim, over the light values in [`KIT_CSS`].
pub const DARK_CSS: &str = include_str!("kit/dark.css");

/// Token block, then the kit CSS (and the dark scheme's shadows), then surface glue.
pub fn stylesheet(t: &Theme, extra: &str) -> String {
    let mut s = t.css();
    s.push_str(KIT_CSS);
    if t.variant == Variant::Dark {
        s.push_str(DARK_CSS);
    }
    s.push_str(extra);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Audit5 S9-2: a pack picked after the shell started is drawn, not the variant default.
    /// Audit6 S6-2: the re-read happens once per unknown id, not on every resolve.
    #[test]
    fn a_theme_installed_after_start_resolves_without_restart() {
        let cat = std::cell::RefCell::new(ThemeCatalog::builtin());
        let missed = Default::default();
        let base = ThemeCatalog::builtin()
            .list()
            .into_iter()
            .find(|s| s.variant == Variant::Dark)
            .cloned()
            .expect("a built-in dark theme");
        let mut pack = base.clone();
        pack.id = "pack-night".into();
        pack.name = "Night".into();
        let first = resolve_in(&cat, &missed, || panic!("builtin id"), None, true);
        assert_ne!(first.id, "pack-night");
        let reads = std::cell::Cell::new(0);
        let t = resolve_in(
            &cat,
            &missed,
            || reads.set(reads.get() + 1),
            Some("pack-night"),
            true,
        );
        assert_ne!(
            t.id, "pack-night",
            "unknown until the re-read lands: the default"
        );
        assert_eq!(reads.get(), 1);
        // The background read lands (`adopt` swaps the catalog in).
        *cat.borrow_mut() = ThemeCatalog::builtin().with(pack.clone());
        let t = resolve_in(
            &cat,
            &missed,
            || panic!("known now"),
            Some("pack-night"),
            true,
        );
        assert_eq!(t.id, "pack-night");
    }

    /// Audit6 S6-2: a saved id that stays unknown (its plugin was removed) walked the theme
    /// folders on every resolve. One re-read per id; a second unknown id gets its own.
    #[test]
    fn an_id_that_stays_unknown_is_looked_for_once() {
        let cat = std::cell::RefCell::new(ThemeCatalog::builtin());
        let missed = Default::default();
        let reads = std::cell::Cell::new(0);
        for _ in 0..5 {
            let t = resolve_in(
                &cat,
                &missed,
                || reads.set(reads.get() + 1),
                Some("gone"),
                false,
            );
            assert_eq!(t.id, Variant::Light.default_theme());
        }
        assert_eq!(reads.get(), 1, "five resolves, one re-read");
        resolve_in(
            &cat,
            &missed,
            || reads.set(reads.get() + 1),
            Some("other"),
            false,
        );
        assert_eq!(reads.get(), 2);
        resolve_in(&cat, &missed, || panic!("asked again"), Some("gone"), false);
    }

    /// Audit7 R7-4: a pack missed at login (its plugin off) and turned on later is drawn: the
    /// plugin-state or theme-file event forgets the miss, so the next resolve looks again.
    #[test]
    fn a_missed_pack_is_looked_for_again_after_a_change() {
        let cat = std::cell::RefCell::new(ThemeCatalog::builtin());
        let missed = Default::default();
        let reads = std::cell::Cell::new(0);
        let count = || reads.set(reads.get() + 1);
        resolve_in(&cat, &missed, count, Some("pack-late"), true);
        assert_eq!(reads.get(), 1, "the miss starts one re-read");
        // That re-read lands without the pack (still off): `adopt` of a catalog without the id.
        *cat.borrow_mut() = ThemeCatalog::builtin();
        resolve_in(&cat, &missed, count, Some("pack-late"), true);
        assert_eq!(reads.get(), 1, "no event: not looked for again");
        // The plugin is turned on: the enabled-plugins file changes.
        forget(&missed);
        let t = resolve_in(&cat, &missed, count, Some("pack-late"), true);
        assert_eq!(reads.get(), 2, "after the event: a second re-read");
        assert_ne!(t.id, "pack-late", "the default until it lands");
        let mut pack = ThemeCatalog::builtin()
            .list()
            .into_iter()
            .find(|s| s.variant == Variant::Dark)
            .cloned()
            .expect("a built-in dark theme");
        pack.id = "pack-late".into();
        *cat.borrow_mut() = ThemeCatalog::builtin().with(pack);
        let t = resolve_in(
            &cat,
            &missed,
            || panic!("known now"),
            Some("pack-late"),
            true,
        );
        assert_eq!(t.id, "pack-late");
    }

    /// Audit7 R7-4 / SP-5: the picker (or Settings) reads the themes and adopts them. A missed id
    /// they bring is drawn (it leaves `MISSED`, the loader redraws); one they still lack stays
    /// missed, so the background re-read's own adopt does not start another (no re-read loop).
    #[test]
    fn adopt_brings_back_a_missed_id_and_keeps_the_ones_still_gone() {
        let cat = std::cell::RefCell::new(ThemeCatalog::builtin());
        let missed = Default::default();
        let reads = std::cell::Cell::new(0);
        let count = || reads.set(reads.get() + 1);
        resolve_in(&cat, &missed, count, Some("pack-back"), true);
        resolve_in(&cat, &missed, count, Some("pack-gone"), true);
        assert_eq!(reads.get(), 2);
        // The background re-read lands without either: nothing to redraw, both stay missed.
        assert!(!adopt_in(&cat, &missed, ThemeCatalog::builtin()));
        resolve_in(
            &cat,
            &missed,
            || panic!("re-read loop"),
            Some("pack-back"),
            true,
        );
        resolve_in(
            &cat,
            &missed,
            || panic!("re-read loop"),
            Some("pack-gone"),
            true,
        );
        // The picker's read has "pack-back" (its plugin was turned on): redraw, it is drawn.
        let mut pack = ThemeCatalog::builtin()
            .list()
            .into_iter()
            .find(|s| s.variant == Variant::Dark)
            .cloned()
            .expect("a built-in dark theme");
        pack.id = "pack-back".into();
        assert!(adopt_in(&cat, &missed, ThemeCatalog::builtin().with(pack)));
        assert!(!missed.borrow().contains("pack-back"));
        assert!(
            missed.borrow().contains("pack-gone"),
            "still gone: still missed"
        );
        let t = resolve_in(&cat, &missed, || panic!("known"), Some("pack-back"), true);
        assert_eq!(t.id, "pack-back");
        assert!(
            cat.borrow().resolve("wol-light-blue").is_ok(),
            "accent variants kept"
        );
    }

    #[test]
    fn the_installed_sources_are_the_plugin_state_and_the_theme_folder() {
        let [plugins, themes] = installed_sources();
        let home = core_theme::config_home();
        assert_eq!(plugins, home.join(wol_core::plugins::ENABLED_FILE));
        assert_eq!(themes, home.join(shared::theme::USER_THEMES_DIR));
    }

    #[test]
    fn an_adopted_catalog_is_drawn_with_its_accents() {
        let base = ThemeCatalog::builtin()
            .list()
            .into_iter()
            .find(|s| s.variant == Variant::Light)
            .cloned()
            .expect("a built-in light theme");
        let mut pack = base;
        pack.id = "pack-dawn".into();
        adopt(ThemeCatalog::builtin().with(pack));
        CATALOG.with(|c| {
            let c = c.borrow();
            assert!(c.resolve("pack-dawn").is_ok());
            assert!(c.resolve("wol-light-blue").is_ok(), "accent variants kept");
        });
    }

    #[test]
    fn resolves_builtins_and_falls_back() {
        assert_eq!(resolve(None, true).variant, Variant::Dark);
        assert_eq!(resolve(Some("garbage"), false).variant, Variant::Light);
        assert_eq!(resolve(Some("wol-dark"), false).id, "wol-dark");
        assert_eq!(resolve(Some("wol-light-blue"), true).id, "wol-light-blue");
        assert_eq!(resolve(Some("wol-auto-green"), true).id, "wol-dark-green");
        assert_eq!(resolve(Some("wol-auto-green"), false).id, "wol-light-green");
    }

    #[test]
    fn stylesheet_has_tokens_then_kit() {
        let css = stylesheet(&resolve(Some("wol-light"), false), "");
        for t in shared::theme::TOKENS {
            assert!(
                css.contains(&format!("{}: ", shared::theme::css_var(t))),
                "{t}"
            );
        }
        assert!(css.find("--accent:").unwrap() < css.find(".glass-panel").unwrap());
    }

    /// MANUAL 0.12.0: in dark mode the Dock read light. Its dark shelf is the menu bar's material;
    /// light keeps the glass.
    #[test]
    fn the_dock_follows_dark_mode() {
        let shelf = |dark: bool, id: &str| {
            let css = stylesheet(&resolve(Some(id), dark), "");
            css.rmatch_indices(".dock-shelf {")
                .next()
                .map(|(i, _)| css[i..].split('}').next().unwrap_or_default().to_owned())
                .unwrap_or_default()
        };
        assert!(shelf(true, "wol-dark").contains("background: var(--bar-fill)"));
        assert!(shelf(true, "wol-dark-blue").contains("background: var(--bar-fill)"));
        let light = shelf(false, "wol-light");
        assert!(
            light.contains("var(--glass) var(--dock-mix, 58%)"),
            "{light}"
        );
        assert!(!light.contains("--bar-fill"), "{light}");
        assert!(!stylesheet(&resolve(Some("wol-light"), false), "").contains(DARK_CSS));
        assert!(KIT_CSS.contains("--bar-fill: alpha(var(--surface)"));
    }

    #[test]
    fn kit_css_uses_tokens_not_raw_accent() {
        assert!(KIT_CSS.contains("var(--accent)"));
        for raw in ["#0a84ff", "#7c5cff", "#7C5CFF"] {
            assert!(!KIT_CSS.contains(raw), "{raw}");
        }
    }
}
