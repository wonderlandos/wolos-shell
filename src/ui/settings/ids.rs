// SPDX-License-Identifier: GPL-2.0-or-later
//! The names `wol-settings <page>[#<row>]` is given: a page id (`privacy`, `wifi`) and a row id
//! (`screen-recording`), never a title. A title is what the window shows and what a language
//! changes ("Privacy & Security", "Privacidade e Segurança"), so a link built from it breaks the
//! day the title is edited or translated (QA-SEARCH-1: a result opened the first pane); an id is
//! written once, here, and tests pin it.
//!
//! Page ids are the table [`PAGE_IDS`]. A row id is the slug of its English title (the English
//! text is the source of every translation, ADR 0045, so it does not move when a language is added):
//! `Screen Recording` is `screen-recording`.
//! [`resolve`] also takes what older links and scripts pass (the English title, an alias, the
//! title in any language Settings ships) and answers the same [`Target`].

use super::search::{ROWS, data_rows};
use shared::i18n::Lang;

/// (page id, English title) of every sidebar pane and subpage, in [`super::PAGES`] then
/// [`super::SUBPAGES`] order. An id never changes once shipped; a title may.
pub const PAGE_IDS: [(&str, &str); 38] = [
    ("wifi", "Wi-Fi"),
    ("bluetooth", "Bluetooth"),
    ("network", "Network"),
    ("notifications", "Notifications"),
    ("sound", "Sound"),
    ("general", "General"),
    ("appearance", "Appearance"),
    ("accessibility", "Accessibility"),
    ("control-center", "Control Center"),
    ("desktop-dock", "Desktop & Dock"),
    ("displays", "Displays"),
    ("wallpaper", "Wallpaper"),
    ("effects", "Effects & Looks"),
    ("widgets", "Widgets"),
    ("battery", "Battery"),
    ("lock-screen", "Lock Screen"),
    ("privacy", "Privacy & Security"),
    ("users", "Users & Groups"),
    ("keyboard", "Keyboard"),
    ("trackpad-mouse", "Trackpad & Mouse"),
    ("printers", "Printers"),
    ("hardware", "Hardware"),
    ("assistants", "Assistants & Automation"),
    ("modules", "Modules"),
    ("restore", "Restore"),
    ("keyboard-shortcuts", "Keyboard Shortcuts"),
    ("startup-security", "Startup Security"),
    ("about", "About"),
    ("software-update", "Software Update"),
    ("storage", "Storage"),
    ("date-time", "Date & Time"),
    ("language-region", "Language & Region"),
    ("login-items", "Login Items"),
    ("default-apps", "Default Apps"),
    ("sharing", "Sharing"),
    ("ip-settings", "IP Settings"),
    ("proxy", "Proxy"),
    ("dns-over-tls", "DNS over TLS"),
];

/// Ids the shell's own links use (Control Center, the menu bar, alerts): one spelling each.
pub const WIFI: &str = "wifi";
pub const SOUND: &str = "sound";
pub const DISPLAYS: &str = "displays";
pub const GENERAL: &str = "general";
pub const ABOUT: &str = "about";
pub const SOFTWARE_UPDATE: &str = "software-update";
pub const PRIVACY: &str = "privacy";
pub const MODULES: &str = "modules";
pub const WALLPAPER: &str = "wallpaper";

/// Separates the page id from the row id: `privacy#screen-recording`.
pub const ROW_SEP: char = '#';

/// Where a link lands: a page (its English title, the name the window keys its panes by) and,
/// when the link names one, a row on it (its English title).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub page: &'static str,
    pub row: Option<&'static str>,
}

/// `s` as an id: lowercase letters and digits, runs of anything else one `-`.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// The id of the page whose English title is `title`.
pub fn page_id(title: &str) -> Option<&'static str> {
    PAGE_IDS.iter().find(|p| p.1 == title).map(|p| p.0)
}

/// The English title of page id `id`.
pub fn title_of(id: &str) -> Option<&'static str> {
    PAGE_IDS.iter().find(|p| p.0 == id).map(|p| p.1)
}

/// The argument for `wol-settings`: `privacy` for a page, `privacy#screen-recording` for a row.
/// `page` and `row` are English titles. An unknown page gives its slug (resolve then fails and
/// Settings opens as it was; a test names every page, so none ships unknown).
pub fn arg(page: &str, row: Option<&str>) -> String {
    let id = page_id(page).map_or_else(|| slug(page), str::to_string);
    match row {
        Some(r) => format!("{id}{ROW_SEP}{}", slug(r)),
        None => id,
    }
}

/// `wol-settings <arg>` as the list of words to run.
pub fn argv(page: &str, row: Option<&str>) -> Vec<String> {
    vec!["wol-settings".into(), arg(page, row)]
}

/// The row titles a page has (its [`ROWS`] entry and the rows its pane builds from data).
fn rows_of(page: &str) -> Vec<&'static str> {
    ROWS.iter()
        .filter(|r| r.0 == page)
        .flat_map(|r| r.1.iter().copied())
        .chain(
            data_rows()
                .into_iter()
                .filter(|r| r.0 == page)
                .flat_map(|r| r.1),
        )
        .collect()
}

/// `english` in `lang`, as the catalog has it.
type Translate<'a> = &'a dyn Fn(&str, Lang) -> String;

/// The catalog of this binary.
fn catalog(english: &str, lang: Lang) -> String {
    crate::i18n::tr_in(english, lang).to_string()
}

/// `text` is `english` in some language Settings ships (or `english` itself), ignoring case.
fn is_title(english: &str, text: &str, tr: Translate) -> bool {
    let text = text.trim().to_lowercase();
    Lang::SHIPPED
        .iter()
        .any(|l| tr(english, *l).to_lowercase() == text)
}

/// The page a name stands for: an id, an alias of an older name, the English title or the title
/// in a shipped language.
fn page_of(name: &str, tr: Translate) -> Option<&'static str> {
    let name = name.trim();
    if let Some(t) = title_of(&slug(name)) {
        return Some(t);
    }
    let canon = super::canonical(name);
    PAGE_IDS
        .iter()
        .find(|p| p.1.eq_ignore_ascii_case(canon))
        .or_else(|| PAGE_IDS.iter().find(|p| is_title(p.1, name, tr)))
        .map(|p| p.1)
}

/// What `wol-settings <arg>` asks for; None when the page is not one Settings has. A row the page
/// does not have is dropped (the page still opens).
pub fn resolve(arg: &str) -> Option<Target> {
    resolve_with(arg, &catalog)
}

/// [`resolve`] with the translations given (tests give their own language).
pub fn resolve_with(arg: &str, tr: Translate) -> Option<Target> {
    let (page, row) = match arg.split_once(ROW_SEP) {
        Some((p, r)) => (p, Some(r)),
        None => (arg, None),
    };
    let page = page_of(page, tr)?;
    let row = row.and_then(|r| {
        let want = slug(r);
        let rows = rows_of(page);
        rows.iter()
            .copied()
            .find(|t| slug(t) == want)
            .or_else(|| rows.iter().copied().find(|t| is_title(t, r, tr)))
    });
    Some(Target { page, row })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every page has one id, written in the table, and no id repeats.
    #[test]
    fn every_page_has_one_stable_id() {
        let titles: Vec<&str> = super::super::PAGES
            .iter()
            .chain(super::super::SUBPAGES.iter())
            .copied()
            .collect();
        assert_eq!(titles.len(), PAGE_IDS.len());
        for (i, t) in titles.iter().enumerate() {
            assert_eq!(
                PAGE_IDS[i].1, *t,
                "PAGE_IDS keeps PAGES then SUBPAGES order"
            );
        }
        for (id, _) in PAGE_IDS {
            assert_eq!(slug(id), id, "{id} is its own slug");
            assert_eq!(PAGE_IDS.iter().filter(|p| p.0 == id).count(), 1, "{id}");
        }
    }

    /// The ids the shell's links use are pages.
    #[test]
    fn the_ids_links_use_are_pages() {
        for id in [
            WIFI,
            SOUND,
            DISPLAYS,
            GENERAL,
            ABOUT,
            SOFTWARE_UPDATE,
            PRIVACY,
            MODULES,
            WALLPAPER,
        ] {
            assert!(title_of(id).is_some(), "{id}");
        }
    }

    /// Every row of every page has an id of its own on that page: a link to it lands on it.
    #[test]
    fn every_row_resolves_to_itself() {
        for (id, title) in PAGE_IDS {
            let mut rows = rows_of(title);
            rows.sort();
            rows.dedup(); // a row listed twice is one row
            for r in &rows {
                let t = resolve(&format!("{id}{ROW_SEP}{}", slug(r)))
                    .unwrap_or_else(|| panic!("{id}#{r}"));
                assert_eq!((t.page, t.row), (title, Some(*r)), "{id}#{r}");
            }
            // Two rows with one slug would make the second unreachable.
            let mut slugs: Vec<String> = rows.iter().map(|r| slug(r)).collect();
            slugs.sort();
            let n = slugs.len();
            slugs.dedup();
            assert_eq!(slugs.len(), n, "{id}: two rows share an id");
        }
    }

    #[test]
    fn a_page_resolves_by_id_title_alias_and_slug() {
        let want = Some(Target {
            page: "Privacy & Security",
            row: None,
        });
        for a in [
            "privacy",
            "Privacy",
            "Privacy & Security",
            "privacy & security",
        ] {
            assert_eq!(resolve(a), want, "{a}");
        }
        assert_eq!(resolve("lock-screen").map(|t| t.page), Some("Lock Screen"));
        assert_eq!(resolve("Lock & Idle").map(|t| t.page), Some("Lock Screen"));
        assert_eq!(resolve("no-such-page"), None);
        assert_eq!(resolve(""), None);
    }

    /// The id is in the Item the Search bar builds, a row's id follows `#`, and an unknown row
    /// still opens its page.
    #[test]
    fn a_row_follows_the_page() {
        assert_eq!(arg("Privacy & Security", None), "privacy");
        assert_eq!(
            arg("Privacy & Security", Some("Screen Recording")),
            "privacy#screen-recording"
        );
        assert_eq!(
            resolve("privacy#screen-recording"),
            Some(Target {
                page: "Privacy & Security",
                row: Some("Screen Recording")
            })
        );
        assert_eq!(
            resolve("privacy#nothing-here"),
            Some(Target {
                page: "Privacy & Security",
                row: None
            })
        );
        assert_eq!(argv("Sound", None), ["wol-settings", "sound"]);
    }

    /// A title in another language resolves to the same page (the argument may be a translated
    /// title from a script or an older link), and so does the id whatever the language in use.
    #[test]
    fn a_translated_title_still_resolves() {
        for lang in Lang::SHIPPED {
            for (id, english) in PAGE_IDS {
                let shown = crate::i18n::tr_in(english, lang);
                assert_eq!(
                    resolve(shown).map(|t| t.page),
                    Some(english),
                    "{id} as {shown:?} in {}",
                    lang.tag()
                );
                assert_eq!(resolve(id).map(|t| t.page), Some(english));
            }
        }
    }

    /// QA-SEARCH-1, PT-BR: Settings titles are translated, so a link may carry a translated title
    /// (a script, a bookmark from before ids); it must reach the same page and row as the id does.
    #[test]
    fn a_title_in_portuguese_resolves_like_its_id() {
        let pt = |english: &str, lang: Lang| -> String {
            match (english, lang) {
                ("Privacy & Security", Lang::PtBr) => "Privacidade e Segurança".into(),
                ("Screen Recording", Lang::PtBr) => "Gravação de Tela".into(),
                ("Sound", Lang::PtBr) => "Som".into(),
                _ => english.into(),
            }
        };
        let by_id = resolve_with("privacy#screen-recording", &pt);
        assert_eq!(
            resolve_with("Privacidade e Segurança#Gravação de Tela", &pt),
            by_id
        );
        assert_eq!(
            resolve_with("privacidade e segurança", &pt).map(|t| t.page),
            Some("Privacy & Security")
        );
        assert_eq!(resolve_with("Som", &pt).map(|t| t.page), Some("Sound"));
        assert_eq!(
            by_id,
            Some(Target {
                page: "Privacy & Security",
                row: Some("Screen Recording")
            })
        );
        // Without the translation the same words are no page.
        assert_eq!(
            resolve_with("Privacidade e Segurança", &|e, _| e.into()),
            None
        );
    }

    #[test]
    fn slugs_are_plain() {
        assert_eq!(slug("Wi-Fi"), "wi-fi");
        assert_eq!(slug("Change Password…"), "change-password");
        assert_eq!(
            slug("  Don’t use the proxy for "),
            "don-t-use-the-proxy-for"
        );
        assert_eq!(slug("日本語 設定"), "日本語-設定");
    }
}
