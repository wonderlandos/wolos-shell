//! Privacy & Security: which apps may record the screen, control input, read the clipboard history
//! or see the window list (compositor consent through wol-admind `consent`), the firewall (pf,
//! `firewall`), disk encryption, and the way to Assistants & Automation.
use super::*;

/// (capability, title, what it allows): `wol_core::consent::CAPABILITIES` in words.
pub(crate) const KINDS: [(&str, &str, &str); 4] = [
    (
        "screen-capture",
        "Screen Recording",
        "Can see and record everything on your screen.",
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
];

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
    sharing: Sharing,
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
    if f.grants.is_empty() {
        into.append(&w::section(
            "Privacy",
            &[w::list_row(
                "security-high-symbolic",
                "No app has extra access",
                "Apps ask the first time they want to record the screen or control input.",
                None,
            )
            .upcast()],
        ));
    }
    let mut security = Vec::new();
    if let (Some(on), Some(_)) = (f.sharing.firewall, admin("firewall", &[])) {
        let (ctx, st) = (e.ctx.clone(), status.clone());
        security.push(form::toggle_row(
            "Firewall",
            "Blocks connections from other computers that you did not start.",
            on,
            move |v| {
                if let Some(a) = admin("firewall", &[if v { "on" } else { "off" }]) {
                    ctx.act_say(a, &st)
                }
            },
        ));
    }
    if let Some(enc) = f.encrypted {
        security.push(info("Disk encryption", if enc { "On" } else { "Off" }));
    }
    if !security.is_empty() {
        into.append(&w::section("Security", &security));
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
            sharing: m.sharing(),
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
    fn every_grantable_capability_is_described() {
        for c in wol_core::consent::CAPABILITIES {
            assert!(KINDS.iter().any(|k| k.0 == c), "{c}");
        }
        assert_eq!(
            app_of(&[], "/usr/local/bin/obs"),
            ("obs".into(), w::PACKAGE_ICON.into())
        );
    }
}
