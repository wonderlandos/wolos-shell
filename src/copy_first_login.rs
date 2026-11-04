// SPDX-License-Identifier: GPL-2.0-or-later
//! The first sign-in Setup's sentences (`copy::first_login`): every sentence a person reads on its
//! pages lives here, so `copy::lint` reads them and `tests/copy_scan.rs` keeps the surfaces free
//! of their own. Pages pick a sentence by a fact, never build one.
use crate::first_login::Page;

/// One sentence under each page title: what the page is for.
pub fn subtitle(p: Page) -> &'static str {
    match p {
        Page::Privacy => {
            "Choose what this computer may share. Nothing is sent unless you allow it."
        }
        Page::Look => "Choose how your desktop looks. It changes only your account.",
        Page::Accounts => {
            "Add the accounts you use on the web, if you want Mail and Calendar to use them."
        }
        Page::Keyboard => "Choose the keyboard you type on, and a way to type other scripts.",
        Page::Helper => "Choose whether assistants and scripts may help you with WolOS.",
        Page::Done => "Your desktop is ready. Every choice you made is in System Settings.",
    }
}

/// Privacy: the error reports row.
pub const REPORTS_HINT: &str = "When something breaks, send a short report without your files.";
/// Privacy: error reports belong to the whole computer; this account can't change them.
pub const REPORTS_NOT_YOURS: &str = "An administrator of this computer chooses this.";
/// Privacy: the location row.
pub const LOCATION_HINT: &str =
    "Apps that ask may learn roughly where you are. Off keeps it to yourself.";
/// Privacy: error reports need the account's password once.
pub const REPORTS_PASSWORD: &str = "Enter your password to change reports.";
/// Privacy: the password typed for error reports was not right.
pub const REPORTS_WRONG: &str = "That password isn’t right. Try again.";
/// A page's choice could not be saved.
pub const NOT_SAVED: &str = "That choice couldn’t be saved just now. Press Continue to try again.";

/// Look: under the wallpaper row.
pub const LOOK_MORE: &str = "More looks, colours and pictures are in System Settings.";

/// Accounts: no account on this computer yet.
pub const ACCOUNTS_NONE: &str = "No online account is on this computer yet.";
/// Accounts: how mail and calendar accounts are added.
pub const ACCOUNTS_MAIL: &str = "Mail adds your account, then Calendar uses it too.";
/// Accounts: the list could not be read.
pub const ACCOUNTS_UNREAD: &str =
    "The accounts on this computer couldn’t be read. You can add them later in Mail.";

/// Keyboard: the input method module is installed.
pub const IME_READY: &str = "Input methods are installed. Press ⌃Space to switch.";
/// Keyboard: the input method module is not installed.
pub const IME_MISSING: &str = "To type Chinese, Japanese or Korean, add input methods.";

/// Helper: what the switch allows.
pub const HELPER_HINT: &str = "They ask before every change. Off, nothing reaches the system.";
/// Helper: the manual's own helper on the website.
pub const HELPER_MANUAL: &str =
    "The manual’s helper on the WolOS website asks before it downloads.";

/// Settings > General row: Setup was put off and can be finished there.
pub const SETTINGS_ROW: &str = "Privacy, look, accounts, keyboard and helpers, a page each.";
