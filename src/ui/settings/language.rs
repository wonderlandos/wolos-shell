// SPDX-License-Identifier: GPL-2.0-or-later
//! General > Language & Region: the language of menus and formats (desktop prefs `language`, the
//! session's `LANG` from the next login) and the keyboard's input sources live in Keyboard.
use super::*;
use crate::i18n::tr;
use wol_core::region::LANGUAGES;

/// Index in [`LANGUAGES`] for the language in use: the saved choice, else the computer's own
/// (`system`, the session's `LANG`: what Setup or the installer picked), else English (US). Showing
/// English (US) for a Portuguese computer that never saved a choice was a lie (QA-SET-27).
pub(crate) fn language_index(saved: Option<&str>, system: &str) -> usize {
    let at = |l: &str| LANGUAGES.iter().position(|x| x.0 == l);
    saved.and_then(at).or_else(|| at(system)).unwrap_or(0)
}

/// The session's `LANG` (empty when unset).
pub(super) fn system_locale() -> String {
    crate::envv::var("LANG").unwrap_or_default()
}

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let p = general::subpage(e, tr("Language & Region"));
    let status = status_label();
    let d = e.desk.get();
    let names: Vec<&str> = LANGUAGES.iter().map(|l| l.1).collect();
    let note = w::footnote(tr("Apps use the new language the next time you log in."));
    note.set_visible(false);
    // The menu, so a failed save can put it back to the saved language (Desk::set_or).
    let menu = Rc::new(glib::WeakRef::<gtk::DropDown>::new());
    let (dk, st, n, m) = (e.desk.clone(), status.clone(), note.clone(), menu.clone());
    let opened = d.language.clone();
    let picker = form::search_picker_row(
        tr("Preferred language"),
        tr("Menus and messages."),
        &names,
        language_index(d.language.as_deref(), &system_locale()),
        move |i| {
            let (dk2, n2, m, opened) = (dk.clone(), n.clone(), m.clone(), opened.clone());
            n.set_visible(true);
            dk.set_or(
                &st,
                |p| p.language = Some(LANGUAGES[i].0.to_string()),
                move || {
                    let saved = dk2.get().language;
                    // The note only for a language saved since the page opened.
                    n2.set_visible(saved != opened);
                    if let Some(m) = m.upgrade() {
                        m.set_selected(
                            u32::try_from(language_index(saved.as_deref(), &system_locale()))
                                .unwrap_or_default(),
                        );
                    }
                },
            );
        },
    );
    menu.set(control_in::<gtk::DropDown>(&picker).as_ref());
    p.append(&w::section("", &[picker]));
    p.append(&note);
    let keyboard = form::nav_row(
        "input-keyboard-symbolic",
        tr("Input Sources"),
        tr("Keyboard layouts you type with"),
    );
    let open = e.opener("Keyboard");
    keyboard.connect_clicked(move |_| open());
    p.append(&w::section("", &[keyboard.upcast()]));
    p.append(&status);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_language_selects_its_row() {
        assert_eq!(language_index(None, ""), 0);
        assert_eq!(language_index(Some("pt_BR.UTF-8"), ""), 1);
        assert_eq!(language_index(Some("xx_XX.UTF-8"), ""), 0);
        // A saved choice beats the computer's.
        assert_eq!(language_index(Some("de_DE.UTF-8"), "pt_BR.UTF-8"), 6);
        // A language no longer offered (es_ES from 0.12) shows the computer's, else English.
        assert_eq!(language_index(Some("es_ES.UTF-8"), "fr_FR.UTF-8"), 3);
    }

    /// QA-SET-27: nothing saved yet on a Portuguese computer: the pane shows Português (Brasil).
    #[test]
    fn language_index_none_picks_system() {
        assert_eq!(language_index(None, "pt_BR.UTF-8"), 1);
        assert_eq!(language_index(None, "ja_JP.UTF-8"), 2);
        assert_eq!(language_index(None, "C.UTF-8"), 0);
        assert_eq!(language_index(None, ""), 0);
        assert_eq!(language_index(Some("junk"), "fr_FR.UTF-8"), 3);
    }
}
