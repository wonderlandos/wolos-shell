// SPDX-License-Identifier: GPL-2.0-or-later
//! Theme -> GTK CSS. Catalog, contract and active-theme file come from `wol_core::theme` / `shared::theme`;
//! this module only adds the kit stylesheet.
pub use shared::theme::Variant;
use wol_core::theme::{self as core_theme, ThemeCatalog, active};
pub type Theme = core_theme::ResolvedTheme;

thread_local! {
    static CATALOG: std::cell::RefCell<ThemeCatalog> = std::cell::RefCell::new(catalog());
}

/// The installed catalog plus the accent variants: the one the shell draws with.
fn catalog() -> ThemeCatalog {
    with_accents(installed())
}

/// Built-ins, theme folders and enabled plugins' theme packs (ADR 0031 themes v2).
fn installed() -> ThemeCatalog {
    let data = core_theme::apps::data_home(|k| std::env::var(k).ok());
    ThemeCatalog::installed(&core_theme::config_home(), data.as_deref())
}

/// Draws with `installed` (built-ins, theme folders and plugin packs, read off the GTK thread by
/// the theme picker) from now on, plus the accent variants: a pick the picker offers is one the
/// shell can draw (audit5 S9-2). No disk work.
pub fn adopt(installed: ThemeCatalog) {
    let cat = with_accents(installed);
    CATALOG.with(|c| *c.borrow_mut() = cat);
}

fn with_accents(installed: ThemeCatalog) -> ThemeCatalog {
    core_theme::accent::variants()
        .into_iter()
        .fold(installed, ThemeCatalog::with)
}

/// Saved id (None = follow appearance) -> resolved theme; unknown ids fall back to the variant default.
/// `wol-auto-<accent>` follows `prefer_dark` (`wol_core::theme::accent`).
/// An id the catalog does not know re-reads the installed themes once before falling back
/// (audit5 S9-2: a pack picked after the shell started).
pub fn resolve(saved: Option<&str>, prefer_dark: bool) -> Theme {
    CATALOG.with(|c| resolve_in(c, catalog, saved, prefer_dark))
}

fn resolve_in(
    cat: &std::cell::RefCell<ThemeCatalog>,
    fresh: impl FnOnce() -> ThemeCatalog,
    saved: Option<&str>,
    prefer_dark: bool,
) -> Theme {
    let saved = core_theme::accent::effective(saved, prefer_dark);
    let id = active::effective(saved.as_deref(), prefer_dark);
    if cat.borrow().resolve(&id).is_err() {
        *cat.borrow_mut() = fresh();
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

/// Token block, then the kit CSS, then surface glue.
pub fn stylesheet(t: &Theme, extra: &str) -> String {
    let mut s = t.css();
    s.push_str(KIT_CSS);
    s.push_str(extra);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Audit5 S9-2: a pack picked after the shell started is drawn, not the variant default.
    #[test]
    fn a_theme_installed_after_start_resolves_without_restart() {
        let cat = std::cell::RefCell::new(ThemeCatalog::builtin());
        let base = ThemeCatalog::builtin()
            .list()
            .into_iter()
            .find(|s| s.variant == Variant::Dark)
            .cloned()
            .expect("a built-in dark theme");
        let mut pack = base.clone();
        pack.id = "pack-night".into();
        pack.name = "Night".into();
        let first = resolve_in(&cat, ThemeCatalog::builtin, None, true);
        assert_ne!(first.id, "pack-night");
        let added = ThemeCatalog::builtin().with(pack.clone());
        let t = resolve_in(&cat, move || added, Some("pack-night"), true);
        assert_eq!(
            t.id, "pack-night",
            "the unknown id re-read the installed themes"
        );
        // Still unknown after the re-read: the variant default.
        let t = resolve_in(&cat, ThemeCatalog::builtin, Some("gone"), false);
        assert_eq!(t.id, Variant::Light.default_theme());
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

    #[test]
    fn kit_css_uses_tokens_not_raw_accent() {
        assert!(KIT_CSS.contains("var(--accent)"));
        for raw in ["#0a84ff", "#7c5cff", "#7C5CFF"] {
            assert!(!KIT_CSS.contains(raw), "{raw}");
        }
    }
}
