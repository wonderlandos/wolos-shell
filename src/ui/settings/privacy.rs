//! Privacy & Security: which apps may record the screen, post notifications (libwol, ADR 0020),
//! control input, read the clipboard history or see the window list (consent through wol-admind
//! `consent`) and how each choice is
//! enforced, the firewall (pf, `firewall`), the kernel protections WolOS ships, disk encryption,
//! and the way to Assistants & Automation (`wol_core::security`).
use super::*;
use wol_core::security::{self, Check, Enforcement};

/// (capability, title, what it allows): `wol_core::consent::CAPABILITIES` in words.
pub(crate) const KINDS: [(&str, &str, &str); 16] = [
    (
        "screen-capture",
        "Screen Recording",
        "Can see and record everything on your screen.",
    ),
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
        "Can keep its own preferences. No app can read another's.",
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
        "Can search your apps and settings the way Spotlight does.",
    ),
    (
        "search-provider",
        "Search Results",
        "Sees what you type in Spotlight, and can add results to it.",
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

struct Facts {
    grants: Vec<(String, String)>,
    flatpaks: Vec<wol_core::flatpak::App>,
    security: Option<security::Status>,
    encrypted: Option<bool>,
}

fn render(e: &Rc<Env>, into: &gtk::Box, f: Facts, status: &gtk::Label) {
    let entries = crate::desktop::load_all();
    let can_change = admin("consent", &[]).is_some();
    for (cap, title, what) in KINDS {
        let apps: Vec<&String> = f
            .grants
            .iter()
            .filter(|g| g.1 == cap)
            .map(|g| &g.0)
            .collect();
        if apps.is_empty() {
            continue;
        }
        let rows: Vec<gtk::Widget> = apps
            .into_iter()
            .map(|path| {
                let (name, icon) = app_of(&entries, path);
                let (ctx, st, path) = (e.ctx.clone(), status.clone(), path.clone());
                let r = form::permission_row(&icon, &name, what, true, move |on| {
                    let verdict = if on { "allow" } else { "deny" };
                    if let Some(a) = admin("consent", &[&path, cap, verdict]) {
                        ctx.act_say(a, &st)
                    }
                });
                r.set_sensitive(can_change);
                r
            })
            .collect();
        into.append(&w::section(title, &rows));
    }
    for app in &f.flatpaks {
        into.append(&flatpak_section(e, app, status));
    }
    if f.grants.is_empty() && f.flatpaks.is_empty() {
        into.append(&w::section(
            "Privacy",
            &[w::list_row(
                "security-high-symbolic",
                "No app has extra access",
                "Apps you allow to record the screen, send notifications or control input are listed here.",
                None,
            )
            .upcast()],
        ));
    }
    let shown: Vec<&str> = KINDS
        .iter()
        .map(|k| k.0)
        .filter(|c| f.grants.is_empty() || f.grants.iter().any(|g| g.1 == *c))
        .collect();
    into.append(&w::footnote(&enforcement_note(&shown)));
    let mut rows = Vec::new();
    let sec = f.security.as_ref();
    if let (Some(on), Some(_)) = (sec.and_then(|s| s.firewall), admin("firewall", &[])) {
        let (ctx, st) = (e.ctx.clone(), status.clone());
        let stopped = on && sec.and_then(|s| s.firewall_running) == Some(false);
        rows.push(form::pending_toggle_row(
            "Firewall",
            if stopped {
                "On at startup, but not running now."
            } else {
                "Blocks connections from other computers that you did not start."
            },
            on,
            move |v, done| match admin("firewall", &[if v { "on" } else { "off" }]) {
                Some(a) => ctx.act_switch(a, &st, done),
                None => done(false),
            },
        ));
    }
    if let Some(line) = sec.and_then(|s| protections_line(&s.hardening)) {
        let value = w::label(&line, "secondary");
        rows.push(
            form::status_row(
                "",
                "System protections",
                "Accounts cannot see each other's programs or link to each other's files.",
                &value,
            )
            .0,
        );
    }
    if let Some(enc) = f.encrypted {
        rows.push(info("Disk encryption", if enc { "On" } else { "Off" }));
    }
    if !rows.is_empty() {
        into.append(&w::section("Security", &rows));
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
fn flatpak_section(e: &Rc<Env>, app: &wol_core::flatpak::App, status: &gtk::Label) -> gtk::Box {
    let act = |perm: Option<String>, allow: bool| {
        let (ctx, st, scope, id) = (e.ctx.clone(), status.clone(), app.scope, app.id.clone());
        move || {
            ctx.act_say(
                Action::FlatpakOverride {
                    scope,
                    app: id.clone(),
                    perm: perm.clone(),
                    allow,
                },
                &st,
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
            let t = w::toggle(*on, move |v| {
                ctx.act_say(
                    Action::FlatpakOverride {
                        scope,
                        app: id.clone(),
                        perm: Some(spelled.clone()),
                        allow: v,
                    },
                    &st,
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
    let reset = act(None, true);
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
    let (m, e2) = (e.machine(), e.clone());
    later(
        &body,
        move || Facts {
            grants: m.consent(),
            flatpaks: m.flatpak_apps(),
            security: m.security(),
            encrypted: m.storage().and_then(|s| s.encrypted),
        },
        move |into, f| render(&e2, into, f, &status),
    );
    p
}

#[cfg(test)]
mod tests {
    use super::*;

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
            "WolOS enforces Screen Recording and Notifications. For Remote Control, \
             Clipboard History and Window List, WolOS passes on your choice but cannot stop an \
             app that ignores it."
        );
        assert_eq!(
            enforcement_note(&["screen-capture"]),
            "WolOS enforces Screen Recording."
        );
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
