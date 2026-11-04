// SPDX-License-Identifier: GPL-2.0-or-later
//! L10N-2: Setup in the seven official languages (the language card and the account card, as a
//! person sees them on first boot after the installer chose the language), the same cards in the
//! pseudo-locale `xx`, and the pseudo-locale check: in `xx` every word Setup shows is bracketed,
//! so a text still written in the code (never translated) is found by a test, not by a person.
use super::{Case, find_type};
use shared::i18n::Lang;
use shell::setup::{Hints, KEYBOARDS, LANGUAGES, Step};
use shell::ui;
use std::rc::Rc;

/// Setup on a computer whose installer chose `locale` (`LANG` of the new system).
fn setup_in(locale: &str) -> Rc<ui::setup::Setup> {
    let hints = Hints {
        lang: Some(locale.into()),
        ..Hints::default()
    };
    ui::setup::Setup::with(&shell::system::Fake, &hints)
}

/// Setup in the pseudo-locale (`WOL_LANG=xx`).
fn setup_xx() -> Rc<ui::setup::Setup> {
    let s = ui::setup::Setup::new(&shell::system::Fake);
    shell::i18n::set_lang(Lang::Xx);
    s.relabel();
    s
}

pub fn cases() -> Vec<Case> {
    let mut v = Vec::new();
    for l in &LANGUAGES[1..] {
        // Golden names are lower-case kebab-case (RU-071): `pt-br`.
        let tag = shell::i18n::lang_of(l.locale).tag().to_lowercase();
        for st in [Step::Language, Step::Account] {
            let (locale, tag) = (l.locale, tag.clone());
            v.push(Case {
                name: format!("setup-{}-{tag}-1280", st.id()),
                size: (1280, 800),
                class: "lock-window",
                wallpaper: false,
                build: Box::new(move || {
                    let s = setup_in(locale);
                    s.show(st);
                    s.root.clone()
                }),
            });
        }
    }
    for st in [Step::Language, Step::Account] {
        v.push(Case {
            name: format!("setup-{}-xx-1280", st.id()),
            size: (1280, 800),
            class: "lock-window",
            wallpaper: false,
            build: Box::new(move || {
                let s = setup_xx();
                s.show(st);
                s.root.clone()
            }),
        });
    }
    v
}

/// Words that are not text of the shell: the language names (each in itself), the keyboard
/// layouts' names (`wol_core::prefs::LAYOUTS`, the same in every language for now), the theme
/// names (`setup::THEMES`, names like a product's), the Fake system's networks, the key screen's
/// password, the product names, and GTK's own words in a drop-down (GTK translates those).
fn is_a_name(t: &str) -> bool {
    LANGUAGES.iter().any(|l| l.name == t || l.region == t)
        || KEYBOARDS.iter().any(|k| k.1 == t)
        || shell::setup::THEMES.iter().any(|th| th.1 == t)
        || ["Wonderland", "Rabbit Hole 5G", "Tea Party"].contains(&t)
        || t == wol_core::boot::MOK_PASSWORD
        || ["WolOS", "Wi-Fi"].contains(&t)
        || ["(None)", "Search…"].contains(&t)
        || t.chars().all(|c| !c.is_alphabetic())
}

/// QA-L10N-2 (pseudo-locale test): Setup's cards in `xx` show no plain English. A label that is
/// not bracketed is a text the code still holds; the test names it.
pub(super) fn pseudo_locale_leaves_no_english() {
    let s = setup_xx();
    let mut labels: Vec<gtk::Label> = Vec::new();
    find_type(&s.root, &mut labels);
    let plain: Vec<String> = labels
        .iter()
        .map(|l| l.text().to_string())
        // A numbered step ("1. …") is the number, then the text.
        .map(|t| match t.split_once(". ") {
            Some((n, rest)) if n.chars().all(|c| c.is_ascii_digit()) => rest.to_string(),
            _ => t,
        })
        .filter(|t| !t.trim().is_empty() && !t.starts_with('[') && !is_a_name(t))
        .collect();
    shell::i18n::set_lang(Lang::En);
    assert!(plain.is_empty(), "Setup text not in the catalog: {plain:?}");
}

/// L10N-2: a pick in the language list speaks the new language at once (QA-L10N-2: the cards
/// after the pick were built in the language Setup started in).
pub(super) fn picking_a_language_relabels_setup() {
    let s = ui::setup::Setup::new(&shell::system::Fake);
    let ja = LANGUAGES
        .iter()
        .position(|l| l.locale == "ja_JP.UTF-8")
        .unwrap();
    s.pick_language(ja);
    s.relabel();
    let mut labels: Vec<gtk::Label> = Vec::new();
    find_type(&s.root, &mut labels);
    let texts: Vec<String> = labels.iter().map(|l| l.text().to_string()).collect();
    shell::i18n::set_lang(Lang::En);
    assert!(texts.iter().any(|t| t == "アカウントを作成"), "{texts:?}");
    assert!(
        !texts.iter().any(|t| t == "Create Your Account"),
        "{texts:?}"
    );
}
