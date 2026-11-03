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
    let u = Updates::read(std::path::Path::new(wol_core::update::check::DIR))?;
    let running = std::fs::read_to_string(wol_core::update::check::RELEASE_FILE).ok();
    badge(Some(&u.installed(running.as_deref())))
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
}
