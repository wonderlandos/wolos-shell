// SPDX-License-Identifier: GPL-2.0-or-later
//! General > Language & Region: the language of menus and formats (desktop prefs `language`, the
//! session's `LANG` from the next login) and the keyboard's input sources live in Keyboard.
use super::*;
use crate::i18n;
use wol_core::region::LANGUAGES;

/// Index in [`LANGUAGES`] for a saved locale (None or unknown = English (US)).
pub(crate) fn language_index(saved: Option<&str>) -> usize {
    saved
        .and_then(|l| LANGUAGES.iter().position(|x| x.0 == l))
        .unwrap_or(0)
}

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let p = general::subpage(e, "Language & Region");
    let status = status_label();
    let d = e.desk.get();
    let names: Vec<&str> = LANGUAGES.iter().map(|l| l.1).collect();
    let note = w::footnote("Apps use the new language the next time you log in.");
    note.set_visible(false);
    // The menu, so a failed save can put it back to the saved language (Desk::set_or).
    let menu = Rc::new(glib::WeakRef::<gtk::DropDown>::new());
    let (dk, st, n, m) = (e.desk.clone(), status.clone(), note.clone(), menu.clone());
    let opened = d.language.clone();
    let picker = form::search_picker_row(
        "Preferred language",
        "Menus, dates and numbers.",
        &names,
        language_index(d.language.as_deref()),
        move |i| {
            let (dk2, n2, m, opened) = (dk.clone(), n.clone(), m.clone(), opened.clone());
            n.set_visible(true);
            // The shell itself speaks the new language at once (ADR 0045), after this handler
            // returns: the pane is built again in it, and this menu is part of the pane.
            let locale = LANGUAGES[i].0;
            glib::idle_add_local_once(move || i18n::set_lang(i18n::lang_of(locale)));
            dk.set_or(
                &st,
                |p| p.language = Some(LANGUAGES[i].0.to_string()),
                move || {
                    let saved = dk2.get().language;
                    // The save failed: the menu goes back, and so does the shell's language.
                    let back = saved.as_deref().map_or(i18n::lang_of(""), i18n::lang_of);
                    glib::idle_add_local_once(move || i18n::set_lang(back));
                    // The note only for a language saved since the page opened.
                    n2.set_visible(saved != opened);
                    if let Some(m) = m.upgrade() {
                        m.set_selected(
                            u32::try_from(language_index(saved.as_deref())).unwrap_or_default(),
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
        "Input Sources",
        "Keyboard layouts you type with",
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
        assert_eq!(language_index(None), 0);
        assert_eq!(language_index(Some("pt_BR.UTF-8")), 2);
        assert_eq!(language_index(Some("xx_XX.UTF-8")), 0);
    }
}
