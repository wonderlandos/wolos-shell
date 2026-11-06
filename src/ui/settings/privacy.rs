// SPDX-License-Identifier: GPL-2.0-or-later
//! Privacy & Security: which apps may post notifications (libwol, ADR 0020), control input, read
//! the clipboard history or see the window list (consent through wol-admind `consent`) and how
//! each choice is enforced; that apps ask each time they share the screen (no grant, the portal
//! asks; `wol_core::consent::SCREEN_CAPTURE`), the kernel protections WolOS ships, disk encryption,
//! and the ways to Startup Security (`super::startup_security`) and Assistants & Automation
//! (`wol_core::security`). The Firewall switch is in Network (`super::network`).
use super::*;
use wol_core::security::{self, Check, Enforcement};

/// (capability, title, what it allows): `wol_core::consent::CAPABILITIES` in words.
pub(crate) const KINDS: [(&str, &str, &str); 25] = [
    (
        "notifications",
        "Notifications",
        "Can show you notifications.",
    ),
    (
        "input-injection",
        "Remote Control",
        "Can type and click as if it were you.",
    ),
    (
        "clipboard-history",
        "Clipboard History",
        "Can read everything you copy.",
    ),
    (
        "window-list",
        "Window List",
        "Can see which windows are open.",
    ),
    (
        "packages-install",
        "Install Apps",
        "Can install apps and packages for everyone on this computer.",
    ),
    (
        "packages-remove",
        "Remove Apps",
        "Can remove apps and packages for everyone on this computer.",
    ),
    (
        "packages-update",
        "System Updates",
        "Can update WolOS and its apps to releases WolOS signed.",
    ),
    (
        "packages-unreviewed",
        "Unreviewed Apps",
        "Can install apps built on this computer that nobody at WolOS reviewed.",
    ),
    (
        "clipboard-read",
        "Read Clipboard",
        "Can read the text you copied, when it asks.",
    ),
    (
        "clipboard-write",
        "Change Clipboard",
        "Can replace what you copied.",
    ),
    (
        "app-preferences",
        "Preferences",
        "Can keep its own preferences. No app can read another’s.",
    ),
    (
        "file-panels",
        "File Panels",
        "Can ask you to choose a file. It sees only the file you pick.",
    ),
    (
        "power-info",
        "Battery Status",
        "Can see the battery level and whether Low Power Mode is on.",
    ),
    (
        "keep-awake",
        "Keep Awake",
        "Can keep this computer from sleeping while it works.",
    ),
    (
        "network-info",
        "Network Status",
        "Can see whether this computer is online, but not where.",
    ),
    (
        "audio-info",
        "Sound Status",
        "Can see the volume and what is playing.",
    ),
    (
        "audio-control",
        "Sound Control",
        "Can change the volume and mute the sound.",
    ),
    (
        "search",
        "Search",
        "Can search your apps and settings the way Search does.",
    ),
    (
        "search-provider",
        "Search Results",
        "Sees what you type in Search, and can add results to it.",
    ),
    (
        "account-info",
        "Account Name",
        "Can read your account name and full name.",
    ),
    (
        "ipc",
        "Messages from Apps",
        "Can receive messages from apps you allow, and announce news to them.",
    ),
    (
        "devices",
        "Connected Devices",
        "Can see the names of connected USB and Bluetooth devices.",
    ),
    (
        "share",
        "Sharing",
        "Can offer text and files for you to copy, save or open in another app. Nothing leaves until you choose.",
    ),
    (
        "background",
        "Background Activity",
        "Can keep running parts of itself after you close it.",
    ),
    (
        "trash",
        "Move to Trash",
        "Can move your files to the Trash. You can put them back from there.",
    ),
];

/// "A", "A and B", "A, B and C".
fn listed(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// What WolOS can promise for these permissions (`security::PERMISSIONS`): the compositor or wold
/// refuses some outright; for the rest it passes the choice on and cannot stop an app that ignores it.
pub(crate) fn enforcement_note(caps: &[&str]) -> String {
    let titles = |want: &[Enforcement]| -> Vec<&str> {
        KINDS
            .iter()
            .filter(|k| {
                caps.contains(&k.0) && security::enforcement(k.0).is_some_and(|e| want.contains(&e))
            })
            .map(|k| k.1)
            .collect()
    };
    let (hard, soft) = (
        titles(&[Enforcement::Compositor, Enforcement::System]),
        titles(&[Enforcement::Advisory]),
    );
    let mut note = Vec::new();
    if !hard.is_empty() {
        note.push(format!("WolOS enforces {}.", listed(&hard)));
    }
    if !soft.is_empty() {
        note.push(format!(
            "For {}, WolOS passes on your choice but cannot stop an app that ignores it.",
            listed(&soft)
        ));
    }
    note.join(" ")
}

/// The Privacy footnote: how the screen is guarded, then [`enforcement_note`] for the grants shown.
pub(crate) fn footnote(caps: &[&str]) -> String {
    let note = enforcement_note(caps);
    if note.is_empty() {
        crate::copy::settings::PRIVACY_SCREEN_NOTE.to_string()
    } else {
        format!("{} {note}", crate::copy::settings::PRIVACY_SCREEN_NOTE)
    }
}

/// "On", "4 of 6 on"; None when the kernel could not be asked.
pub(crate) fn protections_line(checks: &[Check]) -> Option<String> {
    let known: Vec<&Check> = checks.iter().filter(|c| c.now.is_some()).collect();
    if known.is_empty() {
        return None;
    }
    let on = known.iter().filter(|c| c.ok()).count();
    Some(if on == checks.len() {
        "On".to_string()
    } else {
        format!("{on} of {} on", checks.len())
    })
}

/// App name and icon for an executable path: its desktop entry, else the file name.
fn app_of(entries: &[crate::desktop::Entry], path: &str) -> (String, String) {
    let base = path.rsplit('/').next().unwrap_or(path);
    entries
        .iter()
        .find(|e| {
            e.exec
                .first()
                .is_some_and(|x| x == path || x.rsplit('/').next() == Some(base))
        })
        .map(|e| (e.name.clone(), e.icon.clone()))
        .unwrap_or_else(|| (base.to_string(), w::PACKAGE_ICON.to_string()))
}

/// The rows of one capability: each granted app (switch on), then each app that asked and was
/// refused without a grant (switch off, so the place a refusal names has the switch).
fn switches<'a>(
    grants: &'a [(String, String)],
    asked: &'a [(String, String)],
    cap: &str,
) -> Vec<(&'a String, bool)> {
    let on: Vec<&String> = grants.iter().filter(|g| g.1 == cap).map(|g| &g.0).collect();
    let off = asked
        .iter()
        .filter(|g| g.1 == cap && !on.contains(&&g.0))
        .map(|g| (&g.0, false));
    on.iter().map(|p| (*p, true)).chain(off).collect()
}

struct Facts {
    grants: Vec<(String, String)>,
    /// Refused and not granted: listed with the switch off (`Machine::asked`).
    asked: Vec<(String, String)>,
    flatpaks: Vec<wol_core::flatpak::App>,
    security: Option<security::Status>,
    malware: Option<wol_core::malware::Status>,
    encrypted: Option<bool>,
    /// `wol boot status`: Secure Boot and this computer's key. None where `wol` cannot tell.
    boot: Option<wol_core::boot::Status>,
    /// Installed apps (names and icons of the grants), read with the rest off the UI thread
    /// (QA-SET-45: `load_all` ran on the GTK thread).
    entries: Vec<crate::desktop::Entry>,
    /// Error reports are sent (`Machine::reports_on`); None unread.
    reports: Option<bool>,
}

/// The Error Reports switch and where it starts: shown when the state was read and this build
/// has the `reports` verb (wol-admind writes reports.conf and the timer follows it).
fn reports_switch(on: Option<bool>, verb: bool) -> Option<bool> {
    on.filter(|_| verb)
}

/// What a grant for one peer app or one device allows.
fn scoped(entries: &[crate::desktop::Entry], cap: &str) -> Option<String> {
    use wol_core::consent::{device_of, peer_of};
    if let Some(peer) = peer_of(cap) {
        return Some(format!("Can send messages to {}.", app_of(entries, peer).0));
    }
    device_of(cap).map(|d| format!("Can use the USB device {d}."))
}

/// A consent switch: waits for wol-admind and flips back on Cancel, a refusal or a failure
/// (QA-SET-39: it stayed off after Cancel).
fn consent_row(
    e: &Rc<Env>,
    status: &gtk::Label,
    (icon, name): (&str, &str),
    what: &str,
    (path, granted): (&str, bool),
    cap: &str,
    can_change: bool,
) -> gtk::Widget {
    let (ctx, st, path, cap) = (
        e.ctx.clone(),
        status.clone(),
        path.to_string(),
        cap.to_string(),
    );
    let r = form::pending_permission_row(icon, name, what, granted, move |on, done| {
        let verdict = if on { "allow" } else { "deny" };
        match admin("consent", &[&path, &cap, verdict]) {
            Some(a) => ctx.act_switch(a, &st, done),
            None => done(false),
        }
    });
    r.set_sensitive(can_change);
    r
}

fn render(e: &Rc<Env>, into: &gtk::Box, f: Facts, status: &gtk::Label, reload: &Rc<dyn Fn()>) {
    let entries = &f.entries;
    let can_change = admin("consent", &[]).is_some();
    for (cap, title, what) in KINDS {
        let rows: Vec<gtk::Widget> = switches(&f.grants, &f.asked, cap)
            .into_iter()
            .map(|(path, granted)| {
                let (name, icon) = app_of(entries, path);
                consent_row(
                    e,
                    status,
                    (&icon, &name),
                    what,
                    (path.as_str(), granted),
                    cap,
                    can_change,
                )
            })
            .collect();
        if !rows.is_empty() {
            into.append(&w::section(title, &rows));
        }
    }
    let peers: Vec<gtk::Widget> = f
        .grants
        .iter()
        .filter_map(|(path, cap)| Some((path, cap, scoped(entries, cap)?)))
        .map(|(path, cap, what)| {
            let (name, icon) = app_of(entries, path);
            consent_row(
                e,
                status,
                (&icon, &name),
                &what,
                (path.as_str(), true),
                cap,
                can_change,
            )
        })
        .collect();
    if !peers.is_empty() {
        into.append(&w::section("App Connections", &peers));
    }
    if !f.grants.is_empty() && !can_change {
        // Greyed switches say why (QA-SET-39).
        into.append(&w::footnote(crate::copy::PRIVACY_GRANTS_READ_ONLY));
    }
    for app in &f.flatpaks {
        into.append(&flatpak_section(e, app, status, reload));
    }
    if f.grants.is_empty() && f.asked.is_empty() && f.flatpaks.is_empty() {
        into.append(&w::section(
            "Privacy",
            &[w::list_row(
                "security-high-symbolic",
                "No app has extra access",
                crate::copy::settings::PRIVACY_NONE,
                None,
            )
            .upcast()],
        ));
    }
    // Not a grant either: the system TLS policy (ADR 0055), what each kind of connection uses.
    let conn: Vec<gtk::Widget> = crate::copy::settings::PRIVACY_CONNECTIONS
        .iter()
        .map(|(title, what)| w::list_row("", title, what, None).upcast())
        .collect();
    into.append(&w::section(
        crate::copy::settings::PRIVACY_CONNECTIONS_TITLE,
        &conn,
    ));
    into.append(&w::footnote(
        crate::copy::settings::PRIVACY_CONNECTIONS_NOTE,
    ));
    // Not a grant: the compositor's rules are static and the portal asks each time.
    into.append(&w::section(
        "Screen Recording",
        &[w::list_row(
            "wolos-capture-screen-symbolic",
            "Screen sharing",
            crate::copy::settings::PRIVACY_SCREEN,
            None,
        )
        .upcast()],
    ));
    let shown: Vec<&str> = KINDS
        .iter()
        .map(|k| k.0)
        .filter(|c| {
            (f.grants.is_empty() && f.asked.is_empty())
                || f.grants.iter().chain(&f.asked).any(|g| g.1 == *c)
        })
        .collect();
    into.append(&w::footnote(&footnote(&shown)));
    let mut rows = Vec::new();
    let sec = f.security.as_ref();
    if let Some(line) = sec.and_then(|s| protections_line(&s.hardening)) {
        let value = w::label(&line, "secondary");
        rows.push(
            form::status_row(
                "",
                "System protections",
                "Accounts cannot see each other’s programs or link to each other’s files.",
                &value,
            )
            .0,
        );
    }
    if let Some(enc) = f.encrypted {
        rows.push(info("Disk encryption", if enc { "On" } else { "Off" }));
    }
    if let Some(b) = &f.boot {
        // Secure Boot, the key and Finish Secure Boot Setup live on their own page.
        let startup = form::nav_row(
            "system-lock-screen-symbolic",
            super::startup_security::TITLE,
            &b.summary(),
        );
        let open = e.opener(super::startup_security::TITLE);
        startup.connect_clicked(move |_| open());
        rows.push(startup.upcast());
    }
    if !rows.is_empty() {
        into.append(&w::section("Security", &rows));
    }
    if let Some(m) = &f.malware {
        into.append(&super::malware::section(m));
        if admin("malware-signatures", &[]).is_some() {
            let (ctx, st) = (e.ctx.clone(), status.clone());
            into.append(&super::malware::full_section(
                &m.full,
                move |v, done| match admin("malware-signatures", &[if v { "on" } else { "off" }]) {
                    Some(a) => ctx.act_switch(a, &st, done),
                    None => done(false),
                },
            ));
        }
    }
    if let Some(on) = reports_switch(f.reports, admin("reports", &[]).is_some()) {
        let (ctx, st) = (e.ctx.clone(), status.clone());
        into.append(&w::section(
            "",
            &[form::pending_toggle_row(
                "Send error reports",
                crate::copy::settings::REPORTS_HINT,
                on,
                move |v, done| match admin("reports", &[if v { "on" } else { "off" }]) {
                    Some(a) => ctx.act_switch(a, &st, done),
                    None => done(false),
                },
            )],
        ));
    }
    let helpers = form::nav_row(
        "wolos-assistant-symbolic",
        "Assistants & Automation",
        "What helpers and scripts may change",
    );
    let open = e.opener("Assistants & Automation");
    helpers.connect_clicked(move |_| open());
    into.append(&w::section("", &[helpers.upcast()]));
}

/// Section title for a Flatpak app: its name, and who it is installed for.
pub(crate) fn flatpak_title(app: &wol_core::flatpak::App) -> String {
    match app.scope {
        wol_core::flatpak::Scope::User => app.name.clone(),
        wol_core::flatpak::Scope::System => format!("{} (all accounts)", app.name),
    }
}

/// One Flatpak app: each permission it was granted with a switch, then Reset.
fn flatpak_section(
    e: &Rc<Env>,
    app: &wol_core::flatpak::App,
    status: &gtk::Label,
    reload: &Rc<dyn Fn()>,
) -> gtk::Box {
    // Reset: every switch of the app changes, so the page reads them again once it worked
    // (QA-SET-37: the switches kept the old values).
    let reset = {
        let (ctx, st, scope, id, rl) = (
            e.ctx.clone(),
            status.clone(),
            app.scope,
            app.id.clone(),
            reload.clone(),
        );
        move || {
            let rl = rl.clone();
            ctx.act_switch(
                Action::FlatpakOverride {
                    scope,
                    app: id.clone(),
                    perm: None,
                    allow: true,
                },
                &st,
                Box::new(move |ok| {
                    if ok {
                        rl()
                    }
                }),
            )
        }
    };
    let mut rows: Vec<gtk::Widget> = app
        .perms
        .iter()
        .map(|(p, on)| {
            // The app can't show windows without it: no switch.
            if p.is_display() {
                return w::list_row(
                    crate::ui::flatpak_consent::perm_icon(p),
                    "Display",
                    &p.plain(),
                    None,
                )
                .upcast();
            }
            let (ctx, st, scope, id, spelled) = (
                e.ctx.clone(),
                status.clone(),
                app.scope,
                app.id.clone(),
                p.spelling(),
            );
            // Waits for flatpak: Cancel or a failure flips it back (QA-SET-37).
            let t = w::pending_toggle(*on, move |v, done| {
                ctx.act_switch(
                    Action::FlatpakOverride {
                        scope,
                        app: id.clone(),
                        perm: Some(spelled.clone()),
                        allow: v,
                    },
                    &st,
                    done,
                )
            });
            w::list_row(
                crate::ui::flatpak_consent::perm_icon(p),
                &p.plain(),
                "",
                Some(t.upcast_ref()),
            )
            .upcast()
        })
        .collect();
    let b = w::button("Reset", w::Kind::Default, reset);
    let head = w::list_row(
        "",
        "Flatpak app",
        "Reset gives it back what it asked for when installed.",
        Some(b.upcast_ref()),
    );
    head.prepend(&w::app_icon(&app.icon, form::ICON_ROW));
    rows.insert(0, head.upcast());
    w::section(&flatpak_title(app), &rows)
}

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let p = w::page("Privacy & Security");
    let body = w::vbox(w::S6);
    p.append(&body);
    let status = status_label();
    p.append(&status);
    load(e, &body, &status);
    p
}

/// Reads the facts off the UI thread and renders them into `body` (again after a Reset).
fn load(e: &Rc<Env>, body: &gtk::Box, status: &gtk::Label) {
    clear(body);
    let reload: Rc<dyn Fn()> = {
        let (e, body, st) = (e.clone(), body.downgrade(), status.clone());
        Rc::new(move || {
            if let Some(b) = body.upgrade() {
                load(&e, &b, &st)
            }
        })
    };
    let (m, e2, st) = (e.machine(), e.clone(), status.clone());
    later(
        body,
        move || Facts {
            grants: m.consent(),
            asked: m.asked(),
            flatpaks: m.flatpak_apps(),
            security: m.security(),
            malware: m.malware(),
            encrypted: m.storage().and_then(|s| s.encrypted),
            boot: m.boot(),
            entries: crate::desktop::load_all(),
            reports: m.reports_on(),
        },
        move |into, f| render(&e2, into, f, &st, &reload),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// B6 follow-up: the Error Reports switch is drawn from what reports.conf says, and only
    /// where the `reports` verb exists.
    #[test]
    fn reports_switch_reads_the_saved_choice() {
        assert_eq!(reports_switch(Some(true), true), Some(true));
        assert_eq!(reports_switch(Some(false), true), Some(false));
        assert_eq!(reports_switch(None, true), None, "unread: no switch");
        assert_eq!(
            reports_switch(Some(true), false),
            None,
            "no verb: no switch"
        );
        assert!(shared::admin::AdminAction::parse("reports").is_some());
    }

    /// MANUAL 0.12.0: "Notifications are off for hello. Turn them on in System Settings > Privacy
    /// & Security", and that pane had no switch for hello. An app that asked has its row, off;
    /// a granted one stays on and is not listed twice.
    #[test]
    fn an_app_that_asked_has_its_switch_off() {
        let s = |a: &str, c: &str| (a.to_string(), c.to_string());
        let grants = [
            s("/usr/bin/hello", "notifications"),
            s("/usr/bin/rec", "keep-awake"),
        ];
        let asked = [
            s("/home/a/Work/hi/hi", "notifications"),
            s("/usr/bin/hello", "notifications"),
            s("/home/a/Work/hi/hi", "share"),
        ];
        let rows = |cap| {
            switches(&grants, &asked, cap)
                .into_iter()
                .map(|(p, on)| (p.as_str(), on))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            rows("notifications"),
            [("/usr/bin/hello", true), ("/home/a/Work/hi/hi", false)]
        );
        assert_eq!(rows("share"), [("/home/a/Work/hi/hi", false)]);
        assert_eq!(rows("keep-awake"), [("/usr/bin/rec", true)]);
        assert!(rows("window-list").is_empty());
    }

    #[test]
    fn flatpak_sections_say_who_the_app_is_for() {
        let mut a = wol_core::flatpak::App {
            id: "org.gimp.GIMP".into(),
            scope: wol_core::flatpak::Scope::User,
            name: "GIMP".into(),
            icon: String::new(),
            perms: Vec::new(),
        };
        assert_eq!(flatpak_title(&a), "GIMP");
        a.scope = wol_core::flatpak::Scope::System;
        assert_eq!(flatpak_title(&a), "GIMP (all accounts)");
    }

    #[test]
    fn every_grantable_capability_is_described() {
        for c in wol_core::consent::CAPABILITIES {
            assert!(KINDS.iter().any(|k| k.0 == c), "{c}");
        }
        // Screen recording is the explanation row, never an on/off row per app.
        assert!(
            KINDS
                .iter()
                .all(|k| k.0 != wol_core::consent::SCREEN_CAPTURE)
        );
        assert_eq!(
            app_of(&[], "/usr/bin/obs"),
            ("obs".into(), w::PACKAGE_ICON.into())
        );
    }

    #[test]
    fn the_note_says_which_choices_are_enforced() {
        // The libwol services are all enforced (tested in api::portal): the sentence stays readable.
        let services = shared::portal::CAPABILITIES;
        let all: Vec<&str> = KINDS
            .iter()
            .map(|k| k.0)
            .filter(|c| *c == "notifications" || !services.contains(c))
            .collect();
        assert_eq!(
            enforcement_note(&all),
            "WolOS enforces Notifications, Install Apps, Remove Apps, System Updates and \
             Unreviewed Apps. For Remote Control, Clipboard History and Window List, \
             WolOS passes on your choice but cannot stop an app that ignores it."
        );
        // Screen recording is no grant: it never reads as one WolOS "enforces".
        assert_eq!(enforcement_note(&["screen-capture"]), "");
        assert_eq!(
            footnote(&["screen-capture"]),
            crate::copy::settings::PRIVACY_SCREEN_NOTE
        );
        let f = footnote(&["notifications"]);
        assert!(
            f.starts_with(crate::copy::settings::PRIVACY_SCREEN_NOTE)
                && f.ends_with("WolOS enforces Notifications."),
            "{f}"
        );
        assert!(!f.contains("enforces Screen"), "{f}");
        assert!(!enforcement_note(&["window-list"]).contains("enforces"));
        assert_eq!(
            enforcement_note(&["notifications"]),
            "WolOS enforces Notifications."
        );
    }

    #[test]
    fn protections_count_what_the_kernel_reports() {
        let c = |now: Option<&str>| Check {
            key: "kern.randompid".into(),
            want: "1".into(),
            now: now.map(Into::into),
        };
        assert_eq!(protections_line(&[c(None), c(None)]), None);
        assert_eq!(
            protections_line(&[c(Some("1")), c(Some("1"))]).unwrap(),
            "On"
        );
        assert_eq!(
            protections_line(&[c(Some("1")), c(Some("0")), c(None)]).unwrap(),
            "1 of 3 on"
        );
    }
}
