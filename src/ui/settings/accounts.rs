//! Accounts: users and the Add User sheet.
use super::*;

// ---- Accounts ----

/// "Alice Liddell" -> "alice".
pub fn account_name(full: &str) -> String {
    let first = full.split_whitespace().next().unwrap_or("");
    let s: String = first
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .take(32)
        .collect();
    s.trim_start_matches(|c: char| c.is_ascii_digit())
        .to_string()
}

pub(super) fn page(ctx: &Rc<Ctx>, host: &Host, sheet: &gtk::Revealer) -> gtk::Box {
    let p = w::page("Accounts");
    let status = status_label();
    let list = w::vbox(0);
    let add_row = |list: &gtk::Box, u: &str, sub: &str| {
        let r = w::list_row("", u, sub, None);
        r.prepend(&w::avatar(u, w::ICON_M));
        list.append(&r);
    };
    if host.users.is_empty() {
        list.append(&row("No other accounts", &w::label("", "secondary")));
    }
    for u in &host.users {
        add_row(&list, u, "");
    }
    p.append(&w::section("Users", &[list.clone().upcast()]));

    let full = w::text_field("Full name");
    let short = w::text_field("Account name");
    let pass = w::password_field("Password");
    let verify = w::password_field("Verify");
    let admin = w::toggle(false, |_| {});
    {
        let s = short.clone();
        full.connect_changed(move |f| s.set_text(&account_name(&f.text())));
    }
    let body = w::vbox(w::S3);
    body.append(&w::label("New account", "title"));
    body.append(&full);
    body.append(&short);
    body.append(&pass);
    body.append(&verify);
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
        let (pass, verify) = (pass.clone(), verify.clone());
        let (ctx, r, full, short, admin, st, list) = (
            ctx.clone(),
            sheet.clone(),
            full.clone(),
            short.clone(),
            admin.clone(),
            status.clone(),
            list.clone(),
        );
        buttons.append(&w::button("Create User", w::Kind::Primary, move || {
            let (name, full_name) = (short.text().to_string(), full.text().to_string());
            if !wol_core::settings::users::is_valid_name(&name) {
                return say(&st, "Account names use lowercase letters, digits, - and _.");
            }
            let password = pass.text().to_string();
            if let Some(e) = password_problem(&password, &verify.text()) {
                return say(&st, e);
            }
            pass.set_text("");
            verify.set_text("");
            r.set_reveal_child(false);
            say(&st, &format!("Creating “{name}”…"));
            let (st, list, n2, f2) = (st.clone(), list.clone(), name.clone(), full_name.clone());
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
                        let r = w::list_row("", &n2, &f2, None);
                        r.prepend(&w::avatar(&n2, w::ICON_M));
                        list.append(&r);
                    }
                    Err(e) => say(&st, e.message()),
                }),
            );
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

    #[test]
    fn account_names_from_full_names() {
        for (full, want) in [
            ("Alice Liddell", "alice"),
            ("Mad Hatter", "mad"),
            ("  Dinah ", "dinah"),
            ("José", "jos"),
            ("42 Wonder", ""),
        ] {
            assert_eq!(account_name(full), want, "{full}");
        }
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
