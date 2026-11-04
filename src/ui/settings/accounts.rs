// SPDX-License-Identifier: GPL-2.0-or-later
//! Users & Groups: the people who log in (name, full name, login shell), Change Password (the
//! account's and, for the account that opens the disk, the disk's: wol-admind `user-password`)
//! and the Add User sheet.
use super::*;

/// Longest account name `useradd` takes.
pub const ACCOUNT_MAX: usize = wol_core::settings::users::NAME_MAX;

/// `full` in account-name letters, before the length cut (core's one rule).
fn account_letters(full: &str) -> String {
    wol_core::settings::users::derive_letters(full)
}

/// "Alice Liddell" -> "aliceliddell", "Ñandú Álvarez" -> "nandualvarez"; at most [`ACCOUNT_MAX`]
/// letters. The Setup Assistant suggests the same (`wol_core::settings::users::derive_name`,
/// QA-LIVE3-24: Add User made `tomas`, Setup `maracosta`).
pub fn account_name(full: &str) -> String {
    wol_core::settings::users::derive_name(full)
}

/// The note under the account name when [`account_name`] could not use the full name as it
/// is: cut to 32 letters (QA-DESIGN-31: silently), or no Latin letters at all.
pub fn account_hint(full: &str) -> Option<&'static str> {
    let letters = account_letters(full);
    if letters.chars().count() > ACCOUNT_MAX {
        Some(crate::copy::ACCOUNT_NAME_CUT)
    } else if letters.is_empty() && !full.trim().is_empty() {
        Some(crate::copy::ACCOUNT_NAME_TYPE)
    } else {
        None
    }
}

/// One login user from passwd: (login, full name, shell name).
pub(crate) fn people(passwd: &str, logins: &[String]) -> Vec<(String, String, String)> {
    logins
        .iter()
        .map(|u| {
            let f: Vec<&str> = passwd
                .lines()
                .find(|l| l.split(':').next() == Some(u.as_str()))
                .map(|l| l.split(':').collect())
                .unwrap_or_default();
            let full = f.get(4).map_or("", |g| g.split(',').next().unwrap_or(""));
            let shell = f.last().and_then(|s| s.rsplit('/').next()).unwrap_or("");
            (u.clone(), full.replace('&', u), shell.to_string())
        })
        .collect()
}

fn person_row(
    login: &str,
    full: &str,
    shell: &str,
    change: Option<&Rc<dyn Fn(String)>>,
) -> gtk::Widget {
    let sub = match (full.is_empty(), shell.is_empty()) {
        (false, false) => format!("{login} · {shell}"),
        (false, true) => login.to_string(),
        (true, false) => shell.to_string(),
        (true, true) => String::new(),
    };
    let button = change.map(|c| {
        let (c, who) = (c.clone(), login.to_string());
        w::button(
            crate::copy::CHANGE_PASSWORD_MENU,
            w::Kind::Default,
            move || c(who.clone()),
        )
    });
    let r = w::list_row(
        "",
        if full.is_empty() { login } else { full },
        &sub,
        button.as_ref().map(|b| b.upcast_ref()),
    );
    // The colour picked in Setup, as on the lock screen; else the one the name gives.
    let avatar = w::avatar(login, w::ICON_M);
    if let Some(tone) = crate::login::read_avatar(login) {
        kit::assistant::set_tone(&avatar, tone);
    }
    r.prepend(&avatar);
    r.upcast()
}

/// The Change Password sheet (QA-SET-31, manual recovery/security/settings): the account's
/// new password twice; wol-admind changes it and, for the account that opens the disk, the
/// disk's password too, so the two stay in step. The Authenticate sheet asks first.
fn password_sheet(e: &Rc<Env>, status: &gtk::Label) -> Rc<dyn Fn(String)> {
    let sheet = e.sheets.password.clone();
    let title = w::label("", "title");
    title.set_wrap(true);
    let pass = w::password_field("New password");
    let verify = w::password_field("Verify");
    let body = w::vbox(w::S3);
    body.append(&title);
    body.append(&w::label(crate::copy::PASSWORD_DISK_TOO, "secondary"));
    body.append(&pass);
    body.append(&verify);
    let caps = w::caps_lock_note();
    caps.set_halign(gtk::Align::Start);
    body.append(&caps);
    let problem = w::label("", "secondary");
    body.append(&problem);
    let who: Rc<RefCell<String>> = Rc::default();
    let buttons = w::hbox(w::S2);
    buttons.append(&w::spacer());
    {
        let (r, pass, verify) = (sheet.clone(), pass.clone(), verify.clone());
        buttons.append(&w::button("Cancel", w::Kind::Default, move || {
            pass.set_text("");
            verify.set_text("");
            r.set_reveal_child(false)
        }));
    }
    let go: Rc<dyn Fn()> = {
        let (ctx, r, pass, verify, who, problem, st) = (
            e.ctx.clone(),
            sheet.clone(),
            pass.clone(),
            verify.clone(),
            who.clone(),
            problem.clone(),
            status.clone(),
        );
        Rc::new(move || {
            let password = pass.text().to_string();
            if let Some(p) = password_problem(&password, &verify.text()) {
                return say(&problem, p);
            }
            let name = who.borrow().clone();
            let Some(a) = admin("user-password", &[&name, &password]) else {
                return;
            };
            pass.set_text("");
            verify.set_text("");
            say(&problem, "");
            r.set_reveal_child(false);
            let (st, n2) = (st.clone(), name.clone());
            say(&st, &crate::copy::password_changing(&name));
            ctx.act(
                a,
                Rc::new(move |res| match res {
                    Ok(()) => say(&st, &crate::copy::password_changed(&n2)),
                    Err(x) => say(&st, error_line(Err(x))),
                }),
            );
        })
    };
    {
        let g = go.clone();
        verify.connect_activate(move |_| g());
        buttons.append(&w::button(
            crate::copy::CHANGE_PASSWORD,
            w::Kind::Primary,
            move || go(),
        ));
    }
    body.append(&buttons);
    sheet_body(&sheet, &body);
    Rc::new(move |login: String| {
        title.set_label(&format!("New password for “{login}”"));
        *who.borrow_mut() = login;
        say(&problem, "");
        sheet.set_reveal_child(true);
        pass.grab_focus();
    })
}

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let (ctx, host, sheet) = (&e.ctx, &e.host, &e.sheets.user);
    let p = w::page("Users & Groups");
    let status = status_label();
    // The Users card: one row per person, rebuilt when someone is added.
    let list = w::vbox(0);
    type People = Rc<RefCell<Vec<(String, String, String)>>>;
    let known: People = Default::default();
    let change = admin("user-password", &[]).map(|_| password_sheet(e, &status));
    let users_known = host.users_known;
    let refill: Rc<dyn Fn()> = {
        let (list, known) = (list.clone(), known.clone());
        Rc::new(move || {
            clear(&list);
            let rows: Vec<gtk::Widget> = known
                .borrow()
                .iter()
                .map(|(u, f, sh)| person_row(u, f, sh, change.as_ref()))
                .collect();
            if rows.is_empty() {
                list.append(&w::section(
                    "Users",
                    &[w::list_row(
                        "system-users-symbolic",
                        no_users_line(users_known),
                        "",
                        None,
                    )
                    .upcast()],
                ));
            } else {
                list.append(&w::section("Users", &rows));
            }
        })
    };
    refill();
    p.append(&list);
    {
        let (m, users, known, refill) = (
            e.machine(),
            host.users.clone(),
            known.clone(),
            refill.clone(),
        );
        off_thread(
            move || people(&m.passwd(), &users),
            move |v| {
                *known.borrow_mut() = v;
                refill();
            },
        );
    }

    let full = w::text_field("Full name");
    let short = w::text_field("Account name");
    let pass = w::password_field("Password");
    let verify = w::password_field("Verify");
    let admin = w::toggle(false, |_| {});
    // Says why the account name is not simply the full name (cut at 32, other script).
    let hint = w::label("", "secondary");
    hint.set_wrap(true);
    hint.set_xalign(0.0);
    hint.set_visible(false);
    {
        let (s, h) = (short.clone(), hint.clone());
        full.connect_changed(move |f| {
            s.set_text(&account_name(&f.text()));
            let note = account_hint(&f.text());
            h.set_label(note.unwrap_or(""));
            h.set_visible(note.is_some());
        });
    }
    let body = w::vbox(w::S3);
    body.append(&w::label("New account", "title"));
    body.append(&full);
    body.append(&short);
    body.append(&hint);
    body.append(&pass);
    body.append(&verify);
    let caps = w::caps_lock_note();
    caps.set_halign(gtk::Align::Start);
    body.append(&caps);
    body.append(&w::list_row(
        "",
        "Allow this user to administer this computer",
        "",
        Some(admin.upcast_ref()),
    ));
    let buttons = w::hbox(w::S2);
    buttons.append(&w::spacer());
    {
        let r = sheet.clone();
        buttons.append(&w::button("Cancel", w::Kind::Default, move || {
            r.set_reveal_child(false)
        }));
    }
    {
        let (pass, verify2) = (pass.clone(), verify.clone());
        let (ctx, r, full, short, admin, st, known, refill) = (
            ctx.clone(),
            sheet.clone(),
            full.clone(),
            short.clone(),
            admin.clone(),
            status.clone(),
            known.clone(),
            refill.clone(),
        );
        let verify = verify2.clone();
        let create: Rc<dyn Fn()> = Rc::new(move || {
            let (name, full_name) = (short.text().to_string(), full.text().to_string());
            // wol-admind refuses longer ones; saying so here keeps the typing (QA-SET-34).
            if full_name.chars().count() > wol_core::admin::MAX_FULL_NAME {
                return say(
                    &st,
                    &crate::copy::full_name_too_long(wol_core::admin::MAX_FULL_NAME),
                );
            }
            if !wol_core::settings::users::is_valid_name(&name) {
                return say(&st, "Account names use lowercase letters, digits, - and _.");
            }
            // wol-admind refuses these too (S4-6); saying so here keeps the sheet's typing.
            if wol_core::settings::users::is_reserved(&name) {
                return say(&st, crate::copy::settings::ACCOUNT_RESERVED);
            }
            let password = pass.text().to_string();
            if let Some(e) = password_problem(&password, &verify.text()) {
                return say(&st, e);
            }
            pass.set_text("");
            verify.set_text("");
            r.set_reveal_child(false);
            // The Authenticate sheet may come up next: the line says so (QA-FILES-17).
            say(&st, &crate::copy::user_creating(&name));
            let (st, known, refill, n2, f2) = (
                st.clone(),
                known.clone(),
                refill.clone(),
                name.clone(),
                full_name.clone(),
            );
            let (full, short) = (full.clone(), short.clone());
            ctx.act(
                Action::UserAdd {
                    name,
                    full_name,
                    admin: admin.is_active(),
                    password,
                },
                Rc::new(move |res| match res {
                    Ok(()) => {
                        say(&st, "");
                        // The next Add User starts empty (QA-SET-34).
                        full.set_text("");
                        short.set_text("");
                        known
                            .borrow_mut()
                            .push((n2.clone(), f2.clone(), "zsh".into()));
                        refill();
                    }
                    Err(e) => say(&st, error_line(Err(e))),
                }),
            );
        });
        {
            // Return in Verify creates the user, like the button (QA-SET-34).
            let c = create.clone();
            verify2.connect_activate(move |_| c());
        }
        buttons.append(&w::button("Create User", w::Kind::Primary, move || {
            create()
        }));
    }
    body.append(&buttons);
    sheet_body(sheet, &body);
    let bar = w::hbox(w::S2);
    bar.append(&w::spacer());
    {
        let (r, full) = (sheet.clone(), full.clone());
        bar.append(&w::button("Add User…", w::Kind::Default, move || {
            r.set_reveal_child(true);
            full.grab_focus();
        }));
    }
    p.append(&bar);
    p.append(&w::footnote(
        "New accounts get a private home folder and use zsh in Terminal.",
    ));
    p.append(&status);
    p
}

/// Inline error for the Add User password fields, None when acceptable.
pub fn password_problem(password: &str, verify: &str) -> Option<&'static str> {
    if !wol_core::settings::users::is_valid_password(password) {
        Some("Use a password of at least 8 characters.")
    } else if password != verify {
        Some("The passwords do not match.")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// QA-LIVE3-24: the Setup Assistant and Add User suggest the same account name for the same
    /// full name (Setup made `maracosta`, Add User `tomas`).
    #[test]
    fn derive_name_same_in_installer_and_adduser() {
        for full in [
            "Mara Costa",
            "Tomás Reis",
            "Alice Liddell",
            "Ñandú Álvarez",
            "Weißmüller",
            "42 Wonder",
            "李",
        ] {
            assert_eq!(
                crate::setup::account_name(full),
                account_name(full),
                "{full}"
            );
            assert_eq!(
                account_name(full),
                wol_core::settings::users::derive_name(full),
                "{full}"
            );
        }
    }

    #[test]
    fn account_names_from_full_names() {
        for (full, want) in [
            ("Alice Liddell", "aliceliddell"),
            ("Mad Hatter", "madhatter"),
            ("  Dinah ", "dinah"),
            ("José", "jose"),
            ("42 Wonder", "wonder"),
        ] {
            assert_eq!(account_name(full), want, "{full}");
        }
    }

    /// QA-FILES-16: "Ñandú Álvarez" auto-filled the account name `and`.
    #[test]
    fn account_name_transliterates() {
        assert_eq!(account_name("Ñandú Álvarez"), "nandualvarez");
        assert_eq!(account_name("Søren Ærø"), "sorenaero");
        assert_eq!(account_name("Weißmüller"), "weissmuller");
        assert_eq!(account_name("Łukasz"), "lukasz");
        assert_eq!(
            account_name("李小龍"),
            "",
            "no Latin letters: typed by hand"
        );
        assert_eq!(account_hint("李小龍"), Some(crate::copy::ACCOUNT_NAME_TYPE));
        assert_eq!(account_hint("Ñandú Álvarez"), None);
    }

    /// QA-DESIGN-31: a 50-letter first name was cut to 32 with no word.
    #[test]
    fn account_name_cut_says_so() {
        let long = "alexandrinavictoriawilhelminalidellsmith Liddell";
        assert_eq!(account_name(long).chars().count(), ACCOUNT_MAX);
        assert_eq!(account_hint(long), Some(crate::copy::ACCOUNT_NAME_CUT));
        assert_eq!(account_hint("Alice Liddell"), None);
        for s in [
            crate::copy::ACCOUNT_NAME_CUT,
            crate::copy::ACCOUNT_NAME_TYPE,
        ] {
            assert!(crate::copy::lint(s).is_empty(), "{s}");
        }
        assert!(crate::copy::lint(&crate::copy::user_creating("and")).is_empty());
    }

    #[test]
    fn people_from_passwd() {
        let passwd = "alice:*:1001:1001:Alice Liddell,,,:/home/alice:/usr/bin/zsh\n\
                      bill:*:1002:1002:& the Lizard:/home/bill:/bin/sh\n";
        assert_eq!(
            people(passwd, &["alice".into(), "bill".into(), "ghost".into()]),
            [
                ("alice".into(), "Alice Liddell".into(), "zsh".into()),
                ("bill".into(), "bill the Lizard".into(), "sh".into()),
                ("ghost".into(), String::new(), String::new()),
            ]
        );
    }

    #[test]
    fn add_user_password_rules() {
        assert_eq!(
            password_problem("", ""),
            Some("Use a password of at least 8 characters.")
        );
        assert_eq!(
            password_problem("longenough", "longenouhg"),
            Some("The passwords do not match.")
        );
        assert_eq!(password_problem("longenough", "longenough"), None);
    }
}
