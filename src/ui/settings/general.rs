// SPDX-License-Identifier: GPL-2.0-or-later
//! General: the way into About, Software Update, Storage, Login Items, Language & Region, Date &
//! Time and Sharing (each a subpage with a back button), and the shell new terminal windows open
//! in (`wol_core::terminal_shell`, journaled so the change can be undone).
use super::*;

/// (icon, title, subtitle) per row; rows of a group share a card.
const GROUPS: [&[(&str, &str, &str)]; 4] = [
    &[
        (
            "help-about-symbolic",
            "About",
            "Name, kernel, processor and memory",
        ),
        (
            "software-update-available-symbolic",
            "Software Update",
            "Updates with a restore point first",
        ),
        (
            "drive-harddisk-symbolic",
            "Storage",
            "What uses your disk, and restore points",
        ),
    ],
    &[(
        "view-app-grid-symbolic",
        "Login Items",
        "Apps that open when you log in",
    )],
    &[
        (
            "preferences-desktop-locale-symbolic",
            "Language & Region",
            "Language of menus and formats",
        ),
        (
            "preferences-system-time-symbolic",
            "Date & Time",
            "Time zone and network time",
        ),
    ],
    &[(
        "folder-publicshare-symbolic",
        "Sharing",
        "Computer name and remote login",
    )],
];

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let p = w::page("General");
    p.append(&w::footnote(
        "Manage your computer’s overall setup: updates, storage, language, time and sharing.",
    ));
    // The first sign-in Setup was put off (Later): it can be finished here, also after it stopped asking.
    if crate::first_login::Marks::here().unfinished(crate::apps::is_live()) {
        let b = form::nav_row(
            "emblem-default-symbolic",
            "Finish Setting Up",
            crate::copy::first_login::SETTINGS_ROW,
        );
        b.connect_clicked(|_| {
            kit::spawn(&[
                "wol-setup".to_string(),
                crate::first_login::ARG.to_string(),
                crate::ui::first_login::AGAIN.to_string(),
            ])
        });
        p.append(&w::section("", &[b.upcast()]));
    }
    for g in GROUPS {
        let rows: Vec<gtk::Widget> = g
            .iter()
            .map(|(icon, title, sub)| {
                let b = form::nav_row(icon, title, sub);
                let open = e.opener(title);
                b.connect_clicked(move |_| open());
                b.upcast()
            })
            .collect();
        p.append(&w::section("", &rows));
    }
    terminal_shell(e, &p);
    p
}

/// "Shell in the terminal": the installed shells by program name; the choice is saved to
/// `$XDG_CONFIG_HOME/wolos/terminal-shell`, which `wol-term-shell` reads for each new window.
fn terminal_shell(e: &Rc<Env>, p: &gtk::Box) {
    use wol_core::terminal_shell as ts;
    let shells: Vec<&'static str> = if e.host.terminal_shells.is_empty() {
        vec![ts::DEFAULT]
    } else {
        e.host.terminal_shells.clone()
    };
    let saved = Rc::new(std::cell::Cell::new(ts::effective(
        read(&config(ts::FILE)).as_deref(),
        &shells,
    )));
    let at = |id: &str, list: &[&str]| list.iter().position(|s| *s == id).unwrap_or(0);
    let note = w::footnote(crate::copy::settings::GENERAL_TERMINAL_SHELL_NOT_SAVED);
    note.set_visible(false);
    // Waits for the save: a failed one puts the menu back on the saved shell.
    let (list, sv, n) = (shells.clone(), saved.clone(), note.clone());
    let picker = form::pending_picker_row(
        "Shell in the terminal",
        crate::copy::settings::GENERAL_TERMINAL_SHELL,
        &shells,
        at(saved.get(), &shells),
        move |i, done| {
            let id = list[i];
            let ok = save_journaled("terminal shell", ts::FILE, Some(&ts::render(id))).is_ok();
            n.set_visible(!ok);
            if ok {
                sv.set(id);
            }
            done(ok)
        },
    );
    p.append(&w::section("", &[picker]));
    p.append(&note);
}

/// A General subpage: back to General, large title.
pub(super) fn subpage(e: &Rc<Env>, title: &str) -> gtk::Box {
    form::subpage(title, e.opener("General"))
}
