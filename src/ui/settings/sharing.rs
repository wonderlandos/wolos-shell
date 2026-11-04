// SPDX-License-Identifier: GPL-2.0-or-later
//! General > Sharing: the computer's name, Remote Login, AirPlay Receiver (wol-admind `hostname`,
//! `remote-login`, `airplay-receiver`; the Authenticate sheet asks first) and Internet Sharing
//! (the Wi-Fi hotspot, NetworkManager as the person, NET2-2). Controls whose verb this build lacks
//! are not shown. On the live medium the administrator rows are greyed with the reason: its
//! account has no password to allow them (QA-NET-6), and Remote Login is not offered (NET2-8).
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

/// Remote Login is offered: an installed system with the verb (the live account's login is the
/// test door and has no password anyone can type; NET2-8).
pub(crate) fn offers_remote_login(live: bool, verb: bool) -> bool {
    !live && verb
}

/// The administrator rows can be changed: not on the live medium (QA-NET-6).
pub(crate) fn admin_rows_sensitive(live: bool) -> bool {
    !live
}

/// Internet Sharing's hotspot password is one WPA takes.
pub(crate) fn hotspot_password_ok(p: &str) -> bool {
    !p.is_empty() && wol_core::wifi::validate("x", Some(p)).is_none()
}

fn render(e: &Rc<Env>, into: &gtk::Box, s: Sharing, hotspot: Option<bool>, status: &gtk::Label) {
    let host = e.hostname();
    let live = crate::apps::is_live();
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
    for r in &rows {
        r.set_sensitive(admin_rows_sensitive(live));
    }
    into.append(&w::section("", &rows));
    if live {
        into.append(&w::footnote(crate::copy::settings::SHARING_LIVE));
    }
    into.append(&w::footnote(
        "Other computers on your network see this name, and Bluetooth devices show it when pairing.",
    ));
    if let Some(on) = s
        .remote_login
        .filter(|_| offers_remote_login(live, admin("remote-login", &[]).is_some()))
    {
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
        let row = form::pending_toggle_row(
            "AirPlay Receiver",
            "Play audio from iPhones, iPads and Macs on this computer.",
            on,
            move |v, done| match admin("airplay-receiver", &[if v { "on" } else { "off" }]) {
                Some(a) => ctx.act_switch(a, &st, done),
                None => done(false),
            },
        );
        row.set_sensitive(admin_rows_sensitive(live));
        into.append(&w::section("", &[row]));
    }
    if let Some(on) = hotspot {
        into.append(&hotspot_section(e, &host, on, status));
    }
}

/// Internet Sharing (NET2-2): the Wi-Fi hotspot named after the computer, with the password typed
/// here (NetworkManager's shared mode; dnsmasq hands out addresses). Waits for its answer.
fn hotspot_section(e: &Rc<Env>, host: &str, on: bool, status: &gtk::Label) -> gtk::Box {
    let pw = w::password_field("Hotspot password");
    let hint = w::footnote(&if on {
        crate::copy::settings::hotspot_on(host)
    } else {
        crate::copy::settings::HOTSPOT_OFF.to_string()
    });
    let (m, st, pwc, name, hint2) = (
        e.machine(),
        status.clone(),
        pw.clone(),
        host.to_string(),
        hint.clone(),
    );
    let toggle = form::pending_toggle_row("Internet Sharing", "", on, move |v, done| {
        let want = if v {
            let p = pwc.text().to_string();
            if !hotspot_password_ok(&p) {
                say(&st, crate::copy::settings::HOTSPOT_PASSWORD);
                pwc.grab_focus();
                return done(false);
            }
            Some((name.clone(), p))
        } else {
            None
        };
        let (m, st, hint, name) = (m.clone(), st.clone(), hint2.clone(), name.clone());
        off_thread(
            move || m.set_hotspot(want),
            move |r| {
                let ok = r.is_ok();
                match r {
                    Ok(()) if v => hint.set_label(&crate::copy::settings::hotspot_on(&name)),
                    Ok(()) => hint.set_label(crate::copy::settings::HOTSPOT_OFF),
                    Err(wol_core::controls::ControlError::Unavailable) => {
                        say(&st, crate::copy::settings::HOTSPOT_NO_WIFI)
                    }
                    Err(_) => say(&st, crate::copy::settings::HOTSPOT_FAILED),
                }
                done(ok)
            },
        );
    });
    let pw_row = w::list_row("", "Password", "", Some(pw.upcast_ref()));
    let b = w::vbox(w::S2);
    b.append(&w::section("", &[toggle, pw_row.upcast()]));
    let caps = w::caps_lock_note();
    caps.set_halign(gtk::Align::Start);
    b.append(&caps);
    b.append(&hint);
    b
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
        move || (m.sharing(), m.hotspot()),
        move |into, (s, hot)| render(&e2, into, s, hot, &status),
    );
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    /// QA-NET-6 / NET2-8: on the live medium Remote Login is not offered and the administrator
    /// rows are greyed (the account has no password to allow them); installed, both are there.
    #[test]
    fn live_medium_rows() {
        assert!(!offers_remote_login(true, true));
        assert!(offers_remote_login(false, true));
        assert!(!offers_remote_login(false, false));
        assert!(!admin_rows_sensitive(true) && admin_rows_sensitive(false));
        assert!(hotspot_password_ok("hot12345") && !hotspot_password_ok("short"));
        assert!(!hotspot_password_ok(""));
    }

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
