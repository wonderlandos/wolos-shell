// SPDX-License-Identifier: GPL-2.0-or-later
//! General > Default Apps: which app opens web links, mail, pictures, music, video, PDF files, text and
//! calendar files. Each row writes the types of its kind into `~/.config/mimeapps.list` (the file
//! `xdg-mime`, `xdg-open` and the Open With chooser read), as one journaled change that Restore can
//! undo; the line it does not own stay as they were (`open_with::MimeApps`).
use super::{Cell, Env, Rc, form, general, glib, save_journaled, say, status_label, w};
use crate::copy::settings as copy;
use crate::open_with::{self as ow, CATEGORIES, Category, Snapshot};
use gtk::prelude::*;

pub(super) const TITLE: &str = "Default Apps";

/// The menu of one row: the apps that can open the kind, by name, and which one is chosen.
/// A leading "Not chosen" stands for a current app that is not among them.
pub(crate) struct Menu {
    pub options: Vec<String>,
    pub selected: usize,
    /// Entries before the first app (0 or 1).
    pub lead: usize,
}

pub(crate) fn menu(snap: &Snapshot, cat: &Category) -> Menu {
    let apps = snap.candidates(cat);
    if apps.is_empty() {
        return Menu {
            options: vec![copy::DEFAULT_APPS_NO_APP.to_string()],
            selected: 0,
            lead: 0,
        };
    }
    let now = snap.current(cat);
    let at = apps.iter().position(|a| Some(&a.id) == now.as_ref());
    let mut options: Vec<String> = apps.iter().map(|a| a.name.clone()).collect();
    match at {
        Some(i) => Menu {
            options,
            selected: i,
            lead: 0,
        },
        None => {
            options.insert(0, copy::DEFAULT_APPS_NONE.to_string());
            Menu {
                options,
                selected: 0,
                lead: 1,
            }
        }
    }
}

fn render(into: &gtk::Box, snap: Snapshot, status: &gtk::Label) {
    let snap = Rc::new(snap);
    let rows: Vec<gtk::Widget> = CATEGORIES
        .iter()
        .map(|cat| {
            let m = menu(&snap, cat);
            let names: Vec<&str> = m.options.iter().map(String::as_str).collect();
            let (pick, st, lead) = (snap.clone(), status.clone(), m.lead);
            let menu = form::pending_picker(&names, m.selected, move |i, done| {
                let Some(app) = i
                    .checked_sub(lead)
                    .and_then(|n| pick.candidates(cat).get(n).copied().cloned())
                else {
                    return done(true);
                };
                let st = st.clone();
                glib::spawn_future_local(async move {
                    let now = ow::user_text().await;
                    let ok = save_journaled(
                        ow::JOURNAL_WHAT,
                        ow::FILE,
                        Some(&ow::choose(&now, cat, &app)),
                    )
                    .is_ok();
                    say(&st, if ok { "" } else { copy::DEFAULT_APPS_NOT_SAVED });
                    done(ok);
                });
            });
            // The menu says which kind it is for, to a screen reader (the row's title is a
            // separate label beside it).
            menu.update_property(&[gtk::accessible::Property::Label(cat.title)]);
            let row = form::row(cat.title, cat.subtitle, &menu);
            if snap.candidates(cat).is_empty() {
                row.set_sensitive(false);
            }
            row
        })
        .collect();
    into.append(&w::section("", &rows));
    into.append(&w::footnote(copy::DEFAULT_APPS_ABOUT));
}

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let p = general::subpage(e, TITLE);
    let body = w::vbox(w::S6);
    p.append(&body);
    let status = status_label();
    p.append(&status);
    // The apps are read off the UI thread; the spinner shows only after 150 ms (as `later`).
    let spinner = w::spinner();
    spinner.set_visible(false);
    body.append(&spinner);
    let done = Rc::new(Cell::new(false));
    {
        let (s, done) = (spinner.downgrade(), done.clone());
        glib::timeout_add_local_once(std::time::Duration::from_millis(150), move || {
            if let (false, Some(s)) = (done.get(), s.upgrade()) {
                s.set_visible(true);
            }
        });
    }
    glib::spawn_future_local(async move {
        let snap = ow::snapshot().await;
        done.set(true);
        body.remove(&spinner);
        render(&body, snap, &status);
    });
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open_with::{Handler, MimeApps};

    fn viewer(id: &str, name: &str, mime: &str) -> Handler {
        Handler {
            id: id.into(),
            name: name.into(),
            mimes: vec![mime.into()],
            exec: "v %F".into(),
            ..Handler::default()
        }
    }

    fn snap(user: &str) -> Snapshot {
        Snapshot {
            apps: vec![
                viewer("b.desktop", "Bravo", "image/png"),
                viewer("a.desktop", "alpha", "image/png"),
                viewer("t.desktop", "Text", "text/plain"),
            ],
            files: vec![MimeApps::parse(user)],
            user_text: user.into(),
        }
    }

    /// The row lists the apps for its kind by name, alphabetical ignoring case, and selects the
    /// current one.
    #[test]
    fn a_row_lists_the_apps_of_its_kind_and_selects_the_current() {
        let images = &CATEGORIES[3];
        let m = menu(
            &snap("[Default Applications]\nimage/png=b.desktop;\n"),
            images,
        );
        assert_eq!(m.options, ["alpha", "Bravo"]);
        assert_eq!((m.selected, m.lead), (1, 0));
    }

    /// No current app among the choices: "Not chosen" leads, so picking the first app is a pick.
    #[test]
    fn an_unset_row_says_not_chosen() {
        let m = menu(&snap(""), &CATEGORIES[3]);
        assert_eq!(m.options, [copy::DEFAULT_APPS_NONE, "alpha", "Bravo"]);
        assert_eq!((m.selected, m.lead), (0, 1));
    }

    #[test]
    fn a_kind_no_app_serves_has_one_entry() {
        let m = menu(&snap(""), &CATEGORIES[6]);
        assert_eq!(m.options, [copy::DEFAULT_APPS_NO_APP]);
    }
}
