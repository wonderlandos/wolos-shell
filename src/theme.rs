//! Theme -> GTK CSS. Catalog, contract and active-theme file come from `wol_core::theme` / `shared::theme`;
//! this module only adds the kit stylesheet.
pub use shared::theme::Variant;
use wol_core::theme::{self as core_theme, ThemeCatalog, active};
pub type Theme = core_theme::ResolvedTheme;

thread_local! {
    static CATALOG: ThemeCatalog = ThemeCatalog::load(&core_theme::search_paths(&core_theme::config_home()));
}

/// Saved id (None = follow appearance) -> resolved theme; unknown ids fall back to the variant default.
pub fn resolve(saved: Option<&str>, prefer_dark: bool) -> Theme {
    let id = active::effective(saved, prefer_dark);
    CATALOG.with(|c| {
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
    })
}

/// `$XDG_CONFIG_HOME/wolos/theme`.
pub fn active_path() -> std::path::PathBuf {
    active::default_file(&core_theme::config_home())
}

pub fn saved() -> Option<String> {
    active::get(&active_path())
}

pub fn save(id: &str) -> std::io::Result<()> {
    active::set(&active_path(), id)
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

    #[test]
    fn resolves_builtins_and_falls_back() {
        assert_eq!(resolve(None, true).variant, Variant::Dark);
        assert_eq!(resolve(Some("garbage"), false).variant, Variant::Light);
        assert_eq!(resolve(Some("wol-dark"), false).id, "wol-dark");
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
