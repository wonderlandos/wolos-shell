// SPDX-License-Identifier: GPL-2.0-or-later
//! The Install app's sentences (ADR 0023; audit3 R2B-20): every sentence a person reads on its
//! pages lives here, in `shell::copy` (`copy::installer`), so `copy::lint` reads them and the scan in
//! `tests/copy_lint.rs` keeps `src/install*.rs` and `src/ui/installer.rs` free of their own.
//! Invariant: the pages pick a sentence by a fact (step, stage, tier, what stays), never build one.
use crate::install::Stage;
use crate::install::flow::Step;

/// The erase warning names what will be lost: "This disk has Windows on it. Erasing removes it."
pub fn erase_warning(others: &[String]) -> String {
    match others {
        [] => "Everything on this disk will be erased.".to_string(),
        [one] => format!("This disk has {one} on it. Erasing removes it."),
        [rest @ .., last] => format!(
            "This disk has {} and {last} on it. Erasing removes them.",
            rest.join(", ")
        ),
    }
}

/// Why the app won't erase `title` (`refuse` reason from `install-check`); None: it may.
/// `in-use` is not a refusal here: install-run lets go of the disk's mounts first.
pub fn refusal_text(why: &str, title: &str) -> Option<String> {
    match why {
        "in-use" => None,
        "too-small" => Some(format!(
            "“{title}” is too small. WolOS needs at least 16 GB."
        )),
        _ => Some(format!(
            "“{title}” can't be erased: it's the disk WolOS started from, or it's gone. Choose another disk."
        )),
    }
}

/// Sheet shown when this computer trusts only Windows at startup: honest, no jargon beyond the
/// setting's own name. The image carries the signed first stage, so the one step left after the
/// install is the firmware setting, then the key screen; never a second install (R2B-3).
pub const SECURE_BOOT_TITLE: &str = "Change One Startup Setting First";
pub const SECURE_BOOT_TEXT: &str = "This computer's Secure Boot only trusts Windows, so WolOS \
can’t start yet. After installing, open your computer's startup settings (often F2, F10, F12 or \
Delete) and allow third-party certificates, sometimes called Microsoft UEFI CA or Other OS. Then a \
blue screen asks once to accept this computer's WolOS key.";

/// Each page's line under its title. Done and Secure Boot say more than a line: see
/// `copy::INSTALL_RESTART` and [`SECURE_BOOT_TEXT`].
pub fn subtitle(step: Step) -> &'static str {
    match step {
        Step::Begin => "Install it on this computer, or try it first. Trying changes nothing.",
        Step::Language => "Choose your language and the keyboard you type on.",
        Step::Zone => "Your clock follows this place.",
        Step::Disk => "Choose where WolOS goes.",
        Step::Method => "Nothing changes until the last step.",
        Step::Replace => {
            "Choose the partition WolOS takes over. It is erased; the rest of the disk stays."
        }
        Step::Alongside => "Drag to choose how much room each system gets.",
        Step::Manual => "Choose a partition to change it, or free space to add one.",
        Step::Edit => "Choose what this partition is for.",
        Step::Protect => "Encryption keeps your files private if this computer is lost.",
        Step::Account => "You use this to sign in.",
        Step::Summary => "Nothing has changed yet. This is exactly what will happen.",
        Step::SecureBoot => SECURE_BOOT_TEXT,
        Step::Installing => "This takes a few minutes. Keep your computer plugged in.",
        Step::Done => super::INSTALL_RESTART,
        Step::Failed => "",
        Step::KeyUnread => KEY_UNREAD_TEXT,
    }
}

/// Under the key-unread page's title: the install worked, the key is what is missing.
pub const KEY_UNREAD_TEXT: &str = "WolOS is installed, but its recovery key could not be read. Click Try Again: Restart waits until the key is shown.";

/// Continue on the disk page (and the edit page) with no disk chosen.
pub const CHOOSE_DISK: &str = "Choose a disk to install on.";
/// Continue on the Replace page with no partition chosen.
pub const CHOOSE_REPLACE: &str = "Choose the partition WolOS takes over.";
/// The disk password is shorter than 8 characters.
pub const PASSWORD_SHORT: &str = "Use at least 8 characters.";
/// The disk password and its second typing differ.
pub const PASSWORDS_DIFFER: &str = "Passwords don't match.";
/// The computer's name is not one RFC 1123 allows.
pub const HOSTNAME_RULE: &str = "Use letters, digits and hyphens for the computer's name.";
/// The time zone search found nothing (the empty state's title, then its hint).
pub const NO_PLACE: (&str, &str) = ("No matching place", "Try the name of a big city near you.");
/// The disk page while the disks are looked at.
pub const LOOKING_AT_DISKS: &str = "Looking at your disks…";
/// No disk the installer may use (the empty state's title, then its hint; the button is "Look Again").
pub const NO_DISK: (&str, &str) = (
    "No disk found",
    "Connect the disk you want to install on, then click Look Again.",
);

/// The line under the progress bar.
pub fn remaining_text(secs: Option<f64>) -> String {
    match secs {
        None => "Estimating time remaining…".into(),
        Some(s) if s < 60.0 => "Less than a minute remaining".into(),
        Some(s) => {
            let m = (s / 60.0).round().max(1.0) as u64;
            if m == 1 {
                "About a minute remaining".into()
            } else {
                format!("About {m} minutes remaining")
            }
        }
    }
}

/// What the progress page says the installer is doing.
pub fn doing(stage: Stage) -> &'static str {
    match stage {
        Stage::Prepare | Stage::Disk => "Preparing the disk…",
        Stage::Files => "Copying system files…",
        Stage::Keys => "Making this computer's keys…",
        Stage::Encrypt => "Encrypting the disk…",
        Stage::Initramfs => "Preparing the first start…",
        Stage::Boot => "Installing the startup files…",
        Stage::Settings => "Saving settings…",
        Stage::Seal => "Locking the disk key to this computer…",
        Stage::Packages => "Installing apps…",
        Stage::Finish => "Finishing up…",
        Stage::Done => "Done",
        Stage::Failed => "",
    }
}

/// Failed page subtitle, by the stage the install stopped in.
pub fn stopped_text(at: Stage) -> &'static str {
    match at {
        Stage::Prepare | Stage::Disk => {
            "The disk couldn't be prepared. Check that it's connected, then try again or choose another disk."
        }
        Stage::Files => {
            "The system files couldn't be copied. Check the installer disk, then try again."
        }
        Stage::Keys | Stage::Encrypt | Stage::Seal => {
            "The disk couldn't be encrypted. Try again, or choose Don't Encrypt."
        }
        Stage::Initramfs | Stage::Boot => {
            "The startup files couldn't be installed. Try again, or choose another disk."
        }
        // Language and keyboard (68) or the account (90): `wol install settings` (R2B-8).
        Stage::Settings => {
            "Your settings couldn't be saved. Try again, and check the language, keyboard and account."
        }
        _ => "The installation stopped before it finished. Try again, or choose another disk.",
    }
}

/// Things to read while the install runs, one at a time.
pub const TIPS: [&str; 6] = [
    "Updates install into a second copy of WolOS, so you can always start the previous one.",
    "Press Command Space to search apps, files and settings from anywhere.",
    "The App Store installs apps for everyone on this computer, with a restore point first.",
    "Control Center, at the top right, has Wi-Fi, sound, brightness and Do Not Disturb.",
    "Your files stay private: with encryption on, the disk opens only on this computer.",
    "System Settings looks like the one you know, and every change there can be undone.",
];

/// A kept system keeps its own startup partition while WolOS gets a new one: WolOS's menu can't
/// list it, so it starts from the firmware's startup menu (R2B-11). `name`: "Windows", "Ubuntu".
pub fn other_start(name: &str) -> String {
    format!(
        "To start {name}, open your computer's startup menu while it starts, often with F12, F9 or Esc."
    )
}

/// Windows stays with BitLocker on, and this install changes the Secure Boot keys it measures
/// (own keys enrolled, or the firmware setting the Secure Boot sheet asks for) (R2B-9).
pub const BITLOCKER_RECOVERY: &str =
    "Windows uses BitLocker, so it asks once for its recovery key after this Secure Boot change.";
/// Where the person finds that key before they need it.
pub const BITLOCKER_KEY_WHERE: &str =
    "Have the key ready: it is saved at account.microsoft.com/devices/recoverykey.";
/// Own keys (Setup Mode): the factory keys and the one-time key screen leave Windows' keys alone.
pub const BITLOCKER_AVOID_OWN_KEYS: &str = "To avoid it, restore the factory Secure Boot keys in \
your startup settings first. WolOS then uses a one-time key screen.";
/// Firmware tier: the setting itself changes the keys; Windows can hold BitLocker for it.
pub const BITLOCKER_AVOID_SETTING: &str =
    "To avoid it, suspend BitLocker in Windows before you change the startup setting.";

/// What the Summary, the Secure Boot sheet and the Done page say about BitLocker for the tier
/// `install-check` named (`own-keys`, `firmware`); empty: this tier leaves Windows' keys alone.
/// `done`: the install ran, so own keys are already enrolled and only the key's place is left.
pub fn bitlocker(tier: &str, done: bool) -> Vec<&'static str> {
    match (tier, done) {
        ("own-keys", false) => vec![
            BITLOCKER_RECOVERY,
            BITLOCKER_KEY_WHERE,
            BITLOCKER_AVOID_OWN_KEYS,
        ],
        ("own-keys", true) => vec![BITLOCKER_RECOVERY, BITLOCKER_KEY_WHERE],
        ("firmware", _) => vec![
            BITLOCKER_RECOVERY,
            BITLOCKER_KEY_WHERE,
            BITLOCKER_AVOID_SETTING,
        ],
        _ => Vec::new(),
    }
}

/// Windows stays: WolOS keeps the clock in local time as Windows does (R2B-12).
pub const CLOCK_LOCAL: &str = "The clock keeps local time, as Windows does.";

/// Failed page subtitle when `wol install check` refused the plan: nothing was written, and the
/// refusal's own sentence follows it.
pub const REFUSED_STOPPED: &str =
    "Nothing on the disk was changed. Fix what is said below, then click Try Again.";
