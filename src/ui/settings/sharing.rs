// SPDX-License-Identifier: GPL-2.0-or-later
//! General > Sharing: the computer's name and Remote Login (wol-admind `hostname`, `remote-login`;
//! the Authenticate sheet asks first). Controls whose verb this build lacks are not shown.
use super::*;
use wol_core::sharing::is_valid_hostname;

/// The account the Remote Login line tells people to use: the one signed in (QA-NET-14: bob read
/// `ssh alice@host.local`, the first line of /etc/passwd). Only when that account is not a login
/// account here (an odd session) does the first one stand in.
pub(crate) fn ssh_user(users: &[String], current: &str) -> String {
    if users.iter().any(|u| u == current) {
        return current.to_owned();
    }
    users.first().cloned().unwrap_or_else(|| "you".into())
}

fn render(e: &Rc<Env>, into: &gtk::Box, s: Sharing, status: &gtk::Label) {
    let host = e.hostname();
    let mut rows = Vec::new();
    if admin("hostname", &[]).is_some() {
        let (ctx, st, env) = (e.ctx.clone(), status.clone(), Rc::downgrade(e));
        // Waits for wol-admind: after Cancel or a failure Return sends the same name again
        // (QA-SET-12: it did nothing), and a saved name shows as saved (lowercase) everywhere.
        rows.push(form::pending_entry_row(
            "Computer name",
            &host,
            "wonderland",
            move |t, done| {
                let name = t.trim().to_lowercase();
                if !is_valid_hostname(&name) {
                    say(&st, crate::copy::HOSTNAME_RULE);
                    return done(false);
                }
                let Some(a) = admin("hostname", &[&name]) else {
                    return done(false);
                };
                let env = env.clone();
                ctx.act_switch(
                    a,
                    &st,
                    Box::new(move |ok| {
                        done(ok);
                        if ok && let Some(e) = env.upgrade() {
                            e.set_hostname(&name);
                        }
                    }),
                )
            },
        ));
    } else {
        rows.push(info("Computer name", &host));
    }
    into.append(&w::section("", &rows));
    into.append(&w::footnote(
        "Other computers on your network see this name, and Bluetooth devices show it when pairing.",
    ));
    if let (Some(on), Some(_)) = (s.remote_login, admin("remote-login", &[])) {
        let (ctx, st) = (e.ctx.clone(), status.clone());
        let user = ssh_user(
            &e.host.users,
            &crate::ui::lock::user_name(&e.machine().passwd()),
        );
        into.append(&w::section(
            "",
            &[form::pending_toggle_row(
                "Remote Login",
                &format!("Log in from another computer: ssh {user}@{host}.local"),
                on,
                move |v, done| match admin("remote-login", &[if v { "on" } else { "off" }]) {
                    Some(a) => ctx.act_switch(a, &st, done),
                    None => done(false),
                },
            )],
        ));
    }
    if let (Some(on), Some(_)) = (s.airplay_receiver, admin("airplay-receiver", &[])) {
        let (ctx, st) = (e.ctx.clone(), status.clone());
        into.append(&w::section(
            "",
            &[form::pending_toggle_row(
                "AirPlay Receiver",
                "Play audio from iPhones, iPads and Macs on this computer.",
                on,
                move |v, done| match admin("airplay-receiver", &[if v { "on" } else { "off" }]) {
                    Some(a) => ctx.act_switch(a, &st, done),
                    None => done(false),
                },
            )],
        ));
    }
}

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let p = general::subpage(e, "Sharing");
    let body = w::vbox(w::S6);
    p.append(&body);
    let status = status_label();
    p.append(&status);
    let (m, e2) = (e.machine(), e.clone());
    later(
        &body,
        move || m.sharing(),
        move |into, s| render(&e2, into, s, &status),
    );
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    /// QA-NET-14: two accounts, the line names the one signed in.
    #[test]
    fn ssh_line_names_the_current_user() {
        let two = vec!["alice".to_string(), "bob".to_string()];
        assert_eq!(ssh_user(&two, "bob"), "bob");
        assert_eq!(ssh_user(&two, "alice"), "alice");
        assert_eq!(ssh_user(&two, "nobody-here"), "alice");
        assert_eq!(ssh_user(&[], "bob"), "you");
    }
}
