// SPDX-License-Identifier: GPL-2.0-or-later
//! The sentences of the surfaces that replaced a Terminal line (0.13, "everything a person can only
//! do in the Terminal has a visual way"): Store > Web Apps (`wol webapp add|list|remove`), Store >
//! Plugins > Add From File (`wol plugin add`), Settings > Hardware Check Again and Undo Fix
//! (`wol hw`, `pacman -R`). STANDARDS §1: the pages hold none of their own. Not in the text unit
//! yet (`locale/shell/sources.tsv`), like the other topic modules; every sentence passes
//! `copy::lint` ([`ALL`], tested below).

/// Settings > Hardware row subtitles in Settings words, never `wol hw` text (QA-SET-13). `{what}`
/// is the package or driver names Fix works with, so the row says what will change.
pub const HW_DRIVER_OFF_NAMED: &str =
    "Its driver is installed but not turned on. Fix turns on {what}.";
pub const HW_NEEDS_DRIVER_NAMED: &str = "Needs a driver. Fix installs {what}.";
pub const HW_NEEDS_FIRMWARE_NAMED: &str = "Needs firmware. Fix installs {what}.";
/// A working device whose driver packages came from Fix here; Undo Fix takes them off again.
pub const HW_ADDED_BY_FIX: &str = "added by Fix";
/// The summary row's button, and what it says after the devices were read again.
pub const HW_CHECK_AGAIN: &str = "Check Again";
pub const HW_CHECKED_NOW: &str = "Checked just now.";
pub const HW_FROM_START: &str = "Read at the last start.";
/// The row menu that takes back what Fix installed for one device.
pub const HW_UNDO_FIX: &str = "Undo Fix";
/// Undo Fix did not complete.
pub const HW_UNDO_NOT_DONE: &str = "Fix was not undone. Nothing else changed.";
// ---- Store > Web Apps (what `wol webapp add|list|remove` did only in the Terminal) ----

pub const STORE_WEBAPPS_TITLE: &str = "Web Apps";
pub const STORE_WEBAPPS_INTRO: &str = "A web site in a window of its own, with its own icon in Search and the Dock. \
    It opens like any app and switches with Super+Tab.";
pub const STORE_WEBAPPS_NEW: &str = "New Web App";
pub const STORE_WEBAPPS_NAME: &str = "Name";
pub const STORE_WEBAPPS_NAME_HINT: &str = "Mail";
pub const STORE_WEBAPPS_ADDRESS: &str = "Web Address";
pub const STORE_WEBAPPS_ADDRESS_HINT: &str = "mail.example.com";
pub const STORE_WEBAPPS_ICON: &str = "Icon";
pub const STORE_WEBAPPS_ICON_SITE: &str = "The site’s own icon, when it has one";
pub const STORE_WEBAPPS_ICON_CHOOSE: &str = "Choose…";
pub const STORE_WEBAPPS_ICON_RESET: &str = "Use the Site’s";
pub const STORE_WEBAPPS_OWN: &str = "Keep Sign-Ins Separate";
pub const STORE_WEBAPPS_OWN_HINT: &str = "Its cookies and sign-ins stay apart from the Browser, so two accounts of one site can be open together.";
pub const STORE_WEBAPPS_MAKE: &str = "Make Web App";
pub const STORE_WEBAPPS_MAKING: &str = "Making…";
pub const STORE_WEBAPPS_MINE: &str = "Your Web Apps";
pub const STORE_WEBAPPS_LOADING: &str = "Looking for your web apps…";
pub const STORE_WEBAPPS_NONE: &str =
    "No web apps yet. Fill in the name and address above, then choose Make Web App.";
pub const STORE_WEBAPPS_REMOVE: &str = "Remove Web App";
/// Row subtitle for a web app with its own sign-ins; `{host}` is the address.
pub const STORE_WEBAPPS_ROW_OWN: &str = "{host}, own sign-ins";
/// No name typed.
pub const STORE_WEBAPPS_NEED_NAME: &str = "Give the web app a name.";
/// The name has no letter or digit, or is too long.
pub const STORE_WEBAPPS_BAD_NAME: &str = "Give the web app a short name with letters or digits.";
/// No address typed.
pub const STORE_WEBAPPS_NEED_ADDRESS: &str = "Type the web address, for example mail.example.com.";
/// The address does not check.
pub const STORE_WEBAPPS_BAD_ADDRESS: &str =
    "That is not a web address WolOS can open. Type it like mail.example.com, with no spaces.";
/// Same name as one that exists; `{n}` is the name.
pub const STORE_WEBAPPS_EXISTS: &str =
    "You already have a web app named “{n}”. Choose another name.";
/// The make or the remove did not complete.
pub const STORE_WEBAPPS_NOT_MADE: &str = "The web app was not made, so nothing changed. Try again.";
pub const STORE_WEBAPPS_NOT_REMOVED: &str = "The web app was not removed. Try again.";
/// The make stopped because the disk is full (QA-A10-SD-06).
pub const STORE_WEBAPPS_DISK_FULL: &str =
    "The web app was not made because the disk is full. Free some space, then try again.";
/// The make could not write its files in this account's folders (QA-A10-SD-06).
pub const STORE_WEBAPPS_NOT_WRITABLE: &str =
    "The web app was not made because its folder cannot be changed. Nothing else changed.";
/// The remove found no such web app: it was already removed (QA-A10-SD-06).
pub const STORE_WEBAPPS_GONE: &str = "That web app was already removed.";
/// The list could not be read.
pub const STORE_WEBAPPS_NO_LIST: &str = "Your web apps could not be listed. Try again.";
/// After a make; `{n}` is the name.
pub const STORE_WEBAPPS_MADE: &str =
    "“{n}” is ready. Find it in Search, and drag it to the Dock to keep it.";
/// Plugins page: add a `.wolplug` file made or sent by someone (what `wol plugin add` did).
pub const STORE_PLUGINS_ADD_FILE: &str = "Add From File…";
pub const STORE_PLUGINS_ADD_FILE_KIND: &str = "WolOS plugins";
/// A file that is not signed by the plugin index: the sheet before it is added anyway.
pub const STORE_PLUGINS_UNSIGNED_TITLE: &str = "Add a plugin nobody has signed?";
pub const STORE_PLUGINS_UNSIGNED_DETAIL: &str =
    "WolOS cannot say who made this plugin. Add it only if you trust where it came from.";
pub const STORE_PLUGINS_ADD_ANYWAY: &str = "Add Anyway";
pub const STORE_PLUGINS_ADD_CANCEL: &str = "Cancel";
/// The file was added; `{id}` is the plugin.
pub const STORE_PLUGINS_ADDED: &str = "Added {id}. Turn it on from Your Plugins.";
/// The file is not a plugin WolOS can add (damaged, too large, wrong shape).
pub const STORE_PLUGINS_ADD_REFUSED: &str =
    "That file is not a plugin WolOS can add. Nothing was changed.";

/// Every sentence above that is a sentence (not a title or a progress line).
pub const ALL: [&str; 24] = [
    HW_DRIVER_OFF_NAMED,
    HW_NEEDS_DRIVER_NAMED,
    HW_NEEDS_FIRMWARE_NAMED,
    HW_CHECKED_NOW,
    HW_FROM_START,
    HW_UNDO_NOT_DONE,
    STORE_WEBAPPS_INTRO,
    STORE_WEBAPPS_OWN_HINT,
    STORE_WEBAPPS_NONE,
    STORE_WEBAPPS_NEED_NAME,
    STORE_WEBAPPS_BAD_NAME,
    STORE_WEBAPPS_NEED_ADDRESS,
    STORE_WEBAPPS_BAD_ADDRESS,
    STORE_WEBAPPS_EXISTS,
    STORE_WEBAPPS_NOT_MADE,
    STORE_WEBAPPS_NOT_REMOVED,
    STORE_WEBAPPS_DISK_FULL,
    STORE_WEBAPPS_NOT_WRITABLE,
    STORE_WEBAPPS_GONE,
    STORE_WEBAPPS_NO_LIST,
    STORE_WEBAPPS_MADE,
    STORE_PLUGINS_UNSIGNED_DETAIL,
    STORE_PLUGINS_ADDED,
    STORE_PLUGINS_ADD_REFUSED,
];

#[cfg(test)]
mod tests {
    #[test]
    fn every_sentence_passes_the_lint() {
        for s in super::ALL {
            let p = crate::copy::lint(s);
            assert!(p.is_empty(), "“{s}”: {p:?}");
        }
    }

    /// A template names the one thing it fills.
    #[test]
    fn templates_keep_their_slots() {
        for (s, slot) in [
            (super::HW_DRIVER_OFF_NAMED, "{what}"),
            (super::HW_NEEDS_DRIVER_NAMED, "{what}"),
            (super::HW_NEEDS_FIRMWARE_NAMED, "{what}"),
            (super::STORE_WEBAPPS_EXISTS, "{n}"),
            (super::STORE_WEBAPPS_MADE, "{n}"),
            (super::STORE_WEBAPPS_ROW_OWN, "{host}"),
            (super::STORE_PLUGINS_ADDED, "{id}"),
        ] {
            assert_eq!(s.matches(slot).count(), 1, "{s}");
        }
    }
}
