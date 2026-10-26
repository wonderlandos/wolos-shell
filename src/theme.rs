//! Theme contract (IaraOS ADR 0016): 13 semantic color tokens + a fixed scale -> GTK CSS variables.
//! WolOS built-ins come from `design/tokens/colors.json`.

pub const TOKENS: [&str; 13] = [
    "surface",
    "sidebar",
    "card",
    "line",
    "text",
    "dim",
    "accent",
    "accentText",
    "glass",
    "control",
    "success",
    "warning",
    "danger",
];

/// Fixed scale: not themable.
pub const SCALE: [(&str, &str); 16] = [
    ("--radius-s", "6px"),
    ("--radius-m", "10px"),
    ("--radius-l", "14px"),
    ("--radius-xl", "20px"),
    ("--space-1", "4px"),
    ("--space-2", "8px"),
    ("--space-3", "12px"),
    ("--space-4", "16px"),
    ("--space-6", "24px"),
    ("--font-size", "13px"),
    ("--font-size-title", "17px"),
    ("--blur", "24px"),
    ("--motion-fast", "150ms"),
    ("--motion-normal", "250ms"),
    ("--motion-slow", "400ms"),
    ("--ease", "cubic-bezier(0.2, 0.8, 0.2, 1)"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    Light,
    Dark,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub id: String,
    pub variant: Variant,
    /// Same order as `TOKENS`.
    pub tokens: [String; 13],
}

/// `accentText` -> `--accent-text`.
pub fn css_var(token: &str) -> String {
    let mut s = String::with_capacity(token.len() + 4);
    s.push_str("--");
    for c in token.chars() {
        if c.is_ascii_uppercase() {
            s.push('-');
            s.push(c.to_ascii_lowercase());
        } else {
            s.push(c);
        }
    }
    s
}

const DESIGN_JSON: &str = include_str!("../../../design/tokens/colors.json");

/// Design token -> contract token mapping (WolOS colors.json names).
const FROM_DESIGN: [(&str, &str); 13] = [
    ("surface", "bg"),
    ("sidebar", "surface-2"),
    ("card", "surface"),
    ("line", "border"),
    ("text", "text"),
    ("dim", "text-2"),
    ("accent", "accent"),
    ("accentText", "accent-fg"),
    ("glass", "menubar"),
    ("control", "surface-2"),
    ("success", "green"),
    ("warning", "orange"),
    ("danger", "red"),
];

/// Built-in theme: `wol-light` / `wol-dark` (also accepts legacy `iara-*`). Unknown id -> None.
pub fn builtin(id: &str) -> Option<Theme> {
    let variant = match id.trim() {
        "wol-light" | "iara-light" => Variant::Light,
        "wol-dark" | "iara-dark" => Variant::Dark,
        _ => return None,
    };
    Some(from_design(DESIGN_JSON, variant).expect("embedded design tokens are valid"))
}

pub fn from_design(json: &str, variant: Variant) -> Option<Theme> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let key = if variant == Variant::Light {
        "light"
    } else {
        "dark"
    };
    let table = v.get(key)?;
    let tokens = FROM_DESIGN.map(|(_, d)| {
        table
            .get(d)
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string()
    });
    if tokens.iter().any(String::is_empty) {
        return None;
    }
    Some(Theme {
        id: format!("wol-{key}"),
        variant,
        tokens,
    })
}

/// Active theme id file content -> theme; absent/unknown falls back to the appearance default.
pub fn resolve(active: Option<&str>, prefer_dark: bool) -> Theme {
    active
        .and_then(builtin)
        .unwrap_or_else(|| builtin(if prefer_dark { "wol-dark" } else { "wol-light" }).unwrap())
}

/// `:root { --surface: ...; ... scale }`.
pub fn root_css(t: &Theme) -> String {
    let mut s = String::from(":root {");
    for (name, value) in TOKENS.iter().zip(&t.tokens) {
        s.push(' ');
        s.push_str(&css_var(name));
        s.push_str(": ");
        s.push_str(value);
        s.push(';');
    }
    for (k, v) in SCALE {
        s.push(' ');
        s.push_str(k);
        s.push_str(": ");
        s.push_str(v);
        s.push(';');
    }
    s.push_str(" }\n");
    s
}

/// Shell stylesheet (port of Iara.Shell/theme.css, var(--token) only).
pub const SHELL_CSS: &str = include_str!("../css/shell.css");

/// Full stylesheet: tokens first, then the component CSS.
pub fn stylesheet(t: &Theme, extra: &str) -> String {
    let mut s = root_css(t);
    s.push_str(SHELL_CSS);
    s.push_str(extra);
    s
}

/// `$XDG_CONFIG_HOME/wol/theme`.
pub fn active_path(config_home: &str) -> std::path::PathBuf {
    std::path::Path::new(config_home).join("wol/theme")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_var_kebabs_camel_case() {
        for (i, o) in [("accentText", "--accent-text"), ("surface", "--surface")] {
            assert_eq!(css_var(i), o);
        }
    }

    #[test]
    fn builtins_use_wolos_design_tokens() {
        let l = builtin("wol-light").unwrap();
        assert_eq!(l.tokens[6], "#7C5CFF");
        assert_eq!(builtin("wol-dark").unwrap().variant, Variant::Dark);
        assert!(builtin("nope").is_none());
    }

    #[test]
    fn root_css_has_all_13_tokens_and_scale() {
        let css = root_css(&builtin("wol-light").unwrap());
        assert!(css.starts_with(":root {"));
        for t in TOKENS {
            assert!(css.contains(&format!("{}: ", css_var(t))), "{t}");
        }
        assert!(css.contains("--accent: #7C5CFF;"));
        assert!(css.contains("--radius-m: 10px;"));
    }

    #[test]
    fn resolve_falls_back_to_appearance() {
        assert_eq!(resolve(None, true).id, "wol-dark");
        assert_eq!(resolve(Some("garbage"), false).id, "wol-light");
        assert_eq!(resolve(Some("wol-dark\n"), false).id, "wol-dark");
    }

    #[test]
    fn shell_css_uses_tokens_not_raw_accent() {
        assert!(!SHELL_CSS.contains("#0a84ff"));
        assert!(SHELL_CSS.contains("var(--accent)"));
    }
}
