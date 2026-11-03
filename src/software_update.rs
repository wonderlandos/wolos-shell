// SPDX-License-Identifier: GPL-2.0-or-later
//! Software update badge in the menu bar (ADR 0031 W5): a small count when the daily check
//! (`wol_core::update::check`, the same files `wol update status` reads) found updates, a
//! security mark when one of them fixes a known vulnerability, and a click that opens Settings >
//! Software Update. Pure model plus one blocking reader; the menu bar (ui/menubar.rs) shows it.
//! Invariant: no badge without a finished check, and the count is the check's own `count()`
//! (what `wol update status` prints), never a number the shell works out by itself.
use wol_core::update::check::Updates;

/// Settings pane the badge opens (`wol-settings <pane title>`, as Control Center does).
pub const OPEN: &[&str] = &["wol-settings", "Software Update"];
/// Highest number drawn on the badge; more shows as "99+".
pub const MAX_SHOWN: usize = 99;
/// Icon for the badge button (Adwaita).
pub const ICON: &str = "software-update-available-symbolic";
/// Icon when a security fix is among the updates.
pub const ICON_SECURITY: &str = "software-update-urgent-symbolic";

/// What the menu bar draws; None = no badge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Badge {
    pub count: usize,
    pub security: bool,
    /// Text on the badge ("3", "99+").
    pub label: String,
    /// Tooltip (the notification's own sentence, `copy::updates_ready`).
    pub tooltip: String,
}

impl Badge {
    pub fn icon(&self) -> &'static str {
        if self.security { ICON_SECURITY } else { ICON }
    }
}

/// The badge for the last check; None when it never ran or found nothing.
pub fn badge(u: Option<&Updates>) -> Option<Badge> {
    let u = u?;
    let count = u.count();
    if count == 0 {
        return None;
    }
    let security = !u.security.is_empty();
    let label = if count > MAX_SHOWN {
        format!("{MAX_SHOWN}+")
    } else {
        count.to_string()
    };
    Some(Badge {
        count,
        security,
        label,
        tooltip: crate::copy::updates_ready(count, security).1,
    })
}

/// Reads the check's files and builds the badge. Blocking (a few small files): call it off the
/// GTK thread, as the menu bar poll does.
pub fn read() -> Option<Badge> {
    let u = Updates::read(
        std::path::Path::new(wol_core::update::check::DIR),
        crate::focus::unix_now(),
    )?;
    let running = std::fs::read_to_string(wol_core::update::check::RELEASE_FILE).ok();
    badge(Some(&u.installed(running.as_deref())))
}

// ---- Plugin updates (audit6 ST6-4) ----

/// A plugin of this account the signed plugin index has a newer, runnable version of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginNews {
    pub id: String,
    pub name: String,
    /// The version that is ready.
    pub to: String,
}

/// The plugin updates for the plugins in `p`: the verified plugin list (a download only when
/// wopkg's own rule asks for one: nothing cached, expired or stale), against what this account
/// has. Nothing is asked of the network when no plugin is installed. A version that needs a newer
/// WolOS is not news. Blocking: off the GTK thread.
pub(crate) fn plugin_news_in(
    p: &crate::ui::store::plugins::Paths,
    keys: &[[u8; 32]],
    net: &wopkg::fetch::Net,
    url: &str,
    os: &str,
) -> Vec<PluginNews> {
    let (have, _, _) = crate::ui::store::plugins::on_this_account(p);
    if have.is_empty() {
        return Vec::new();
    }
    let Ok(index) = wopkg::index::plugins::load(net, url, &p.cache, keys, false) else {
        return Vec::new();
    };
    wopkg::release::plugin_updates(os, &have, &index)
        .into_iter()
        .filter(|u| u.reason == wopkg::release::PluginReason::Available)
        .filter_map(|u| {
            let e = index.get(&u.id)?;
            Some(PluginNews {
                id: u.id,
                name: e.name.clone(),
                to: u.to,
            })
        })
        .collect()
}

/// [`plugin_news_in`] on this computer's account, the real index and the running version.
pub fn plugin_news() -> Vec<PluginNews> {
    plugin_news_in(
        &crate::ui::store::plugins::paths(),
        &crate::ui::store::plugins::keys(),
        &wopkg::fetch::Net::new(false),
        wopkg::index::plugins::DEFAULT_URL,
        env!("CARGO_PKG_VERSION"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked(pkg: &str, audit: &str) -> Option<Updates> {
        Updates::parse("1700000000", pkg, audit, "")
    }

    #[test]
    fn no_check_or_nothing_found_means_no_badge() {
        assert_eq!(badge(None), None);
        assert_eq!(badge(checked("", "").as_ref()), None);
    }

    #[test]
    fn the_badge_counts_what_the_check_found() {
        let u = checked("curl 8.1-1 -> 8.2-1\nfirefox 1-1 -> 2-1\n", "");
        let b = badge(u.as_ref()).unwrap();
        assert_eq!((b.count, b.label.as_str(), b.security), (2, "2", false));
        assert_eq!(b.icon(), ICON);
        assert!(b.tooltip.contains("2 updates"), "{}", b.tooltip);
        assert!(crate::copy::lint(&b.tooltip).is_empty());
        let u = checked("curl 8.1-1 -> 8.2-1\n", "curl\n");
        let b = badge(u.as_ref()).unwrap();
        assert!(b.security && b.icon() == ICON_SECURITY);
        assert!(b.tooltip.contains("security fixes"));
    }

    #[test]
    fn big_counts_are_capped_on_the_badge() {
        let pkg: String = (0..150).map(|i| format!("p{i} 1-1 -> 2-1\n")).collect();
        let b = badge(checked(&pkg, "").as_ref()).unwrap();
        assert_eq!((b.count, b.label.as_str()), (150, "99+"));
    }

    #[test]
    fn the_click_opens_software_update() {
        assert_eq!(OPEN, ["wol-settings", "Software Update"]);
    }

    /// ST6-4: a plugin update is found by the background check, not only by opening the Store:
    /// from the verified list, for the plugins this account has, and only a version this WolOS can
    /// run. No plugin installed, no list asked for.
    #[test]
    fn plugin_updates_are_found_from_the_verified_list() {
        use crate::ui::store::plugins::Paths;
        let root = std::env::temp_dir().join(format!("wol-plugin-news-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let p = Paths {
            data: root.join("data"),
            config: root.join("config"),
            cache: root.join("cache"),
        };
        let seed = [9u8; 32];
        let keys = [wol_core::package::public_of(&seed)];
        let net = wopkg::fetch::Net::new(false);
        // Nothing listens here: whatever is found comes from the cache.
        let url = "http://127.0.0.1:9/index.json";
        let list = |ver: &str, min: &str| {
            let e = serde_json::json!({
                "id": "alice.demo", "name": "Demo", "version": ver, "summary": "Says hello",
                "author": "Alice", "license": "MIT", "kinds": ["menubar"], "permissions": [],
                "min_wolos": min, "url": "https://plugins.wolos.org/alice.demo.wolplug",
                "sha256": "ab".repeat(32), "size": 4096
            });
            let text =
                serde_json::to_vec(&serde_json::json!({"format": 1, "plugins": [e]})).unwrap();
            std::fs::create_dir_all(&p.cache).unwrap();
            std::fs::write(p.cache.join("index.json"), &text).unwrap();
            std::fs::write(
                p.cache.join("index.json.sig"),
                wol_core::package::sign(&seed, &text),
            )
            .unwrap();
        };
        list("0.2", "0.12");
        assert!(
            plugin_news_in(&p, &keys, &net, url, "0.12.0").is_empty(),
            "no plugin installed: nothing to update"
        );
        let user = p.data.join(shared::plugins::USER_DIR);
        crate::ui::maker::editor::create(&user, "alice.demo", &[shared::plugins::Kind::Menubar])
            .unwrap();
        // The scaffold is version 0.1.
        assert_eq!(
            plugin_news_in(&p, &keys, &net, url, "0.12.0"),
            [PluginNews {
                id: "alice.demo".into(),
                name: "Demo".into(),
                to: "0.2".into()
            }]
        );
        list("0.1", "0.12");
        assert!(
            plugin_news_in(&p, &keys, &net, url, "0.12.0").is_empty(),
            "same version"
        );
        list("0.3", "0.13");
        assert!(
            plugin_news_in(&p, &keys, &net, url, "0.12.0").is_empty(),
            "needs a newer WolOS: not news"
        );
        assert_eq!(plugin_news_in(&p, &keys, &net, url, "0.13.0").len(), 1);
        // A list no known key signed is not believed.
        assert!(plugin_news_in(&p, &[[0; 32]], &net, url, "0.13.0").is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}
