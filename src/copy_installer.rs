// SPDX-License-Identifier: GPL-2.0-or-later
//! The Install app's sentences (ADR 0023; audit3 R2B-20): every sentence a person reads on its
//! pages lives here, in `shell::copy` (`copy::installer`), so `copy::lint` reads them and the scan in
//! `tests/copy_lint.rs` keeps `src/install*.rs` and `src/ui/installer.rs` free of their own.
//! Invariant: the pages pick a sentence by a fact (step, stage, tier, what stays), never build one.
use crate::i18n::{tr, tr_fill, trn};
use crate::install::Stage;
use crate::install::flow::Step;

/// The erase warning names what will be lost: "This disk has Windows on it. Erasing removes it."
pub fn erase_warning(others: &[String]) -> String {
    match others {
        [] => tr("Everything on this disk will be erased.").to_string(),
        [one] => tr_fill(
            "This disk has {one} on it. Erasing removes it.",
            &[("one", one)],
        ),
        [rest @ .., last] => tr_fill(
            "This disk has {some} and {last} on it. Erasing removes them.",
            &[("some", &rest.join(", ")), ("last", last)],
        ),
    }
}

/// Why the app won't erase `title` (`refuse` reason from `install-check`); None: it may.
/// `in-use` is not a refusal here: install-run lets go of the disk's mounts first.
pub fn refusal_text(why: &str, title: &str) -> Option<String> {
    match why {
        "in-use" => None,
        "too-small" => Some(tr_fill(
            "“{title}” is too small. WolOS needs at least 16 GB.",
            &[("title", title)],
        )),
        _ => Some(tr_fill(
            "“{title}” can’t be erased: it’s the disk WolOS started from, or it’s gone. Choose another disk.",
            &[("title", title)],
        )),
    }
}

/// Sheet shown when this computer trusts only Windows at startup: honest, no jargon beyond the
/// setting's own name. The image carries the signed first stage, so the one step left after the
/// install is the firmware setting, then the key screen; never a second install (R2B-3).
pub const SECURE_BOOT_TITLE: &str = "Change One Startup Setting First";
pub const SECURE_BOOT_TEXT: &str =
    "This computer’s Secure Boot only trusts Windows, so WolOS can’t start yet.";
/// Under [`SECURE_BOOT_TEXT`] on the same sheet, one line each (RU-001: a sentence stays short):
/// the firmware setting to change, then the key screen that follows.
pub fn secure_boot_steps() -> [&'static str; 2] {
    [
        tr(
            "After installing, open your computer’s startup settings (often F2, F10, F12 or Delete) and \
     allow third-party certificates.",
        ),
        tr(
            "They are sometimes called Microsoft UEFI CA or Other OS. Then a blue screen asks once to \
     accept this computer’s WolOS key.",
        ),
    ]
}

/// Each page's line under its title. Done and Secure Boot say more than a line: see
/// `copy::INSTALL_RESTART` and [`SECURE_BOOT_STEPS`].
pub fn subtitle(step: Step) -> &'static str {
    match step {
        Step::Begin => tr("Install it on this computer, or try it first. Trying changes nothing."),
        Step::Language => tr("Choose your language and the keyboard you type on."),
        Step::Zone => tr("Your clock follows this place."),
        Step::Disk => tr("Choose where WolOS goes."),
        Step::Method => tr("Nothing changes until the last step."),
        Step::Replace => {
            tr("Choose the partition WolOS takes over. It is erased; the rest of the disk stays.")
        }
        Step::Alongside => tr("Drag to choose how much room each system gets."),
        Step::Manual => tr("Choose a partition to change it, or free space to add one."),
        Step::Edit => tr("Choose what this partition is for."),
        Step::Protect => tr("Encryption keeps your files private if this computer is lost."),
        Step::SecureBootChoice => tr(
            "Secure Boot checks at every start that nobody changed WolOS while the computer was off.",
        ),
        Step::Account => tr("You use this to sign in."),
        Step::Modules => {
            tr("Add sets of apps for what you do, or none. You can change this later.")
        }
        Step::Extras => tr("Play the music and video files most people have, in one click."),
        Step::Summary => tr("Nothing has changed yet. This is exactly what will happen."),
        Step::SecureBoot => tr(SECURE_BOOT_TEXT),
        Step::Installing => tr("This takes a few minutes. Keep your computer plugged in."),
        Step::Done => tr(super::INSTALL_RESTART),
        Step::Failed => "",
        Step::KeyUnread => tr(KEY_UNREAD_TEXT),
    }
}

/// Under the key-unread page's title: the install worked, the key is what is missing.
pub const KEY_UNREAD_TEXT: &str = "WolOS is installed, but its recovery key could not be read. Click Try Again: Restart waits until the key is shown.";

/// What each page means for someone coming from Windows, macOS or another Linux (lead,
/// 2026-11-02): what will happen, what is kept. Said after the subtitle, before the page's
/// "Next:" line ([`next_words`]), as one paragraph. Each entry is one or two sentences.
pub fn explain(step: Step) -> Vec<&'static str> {
    match step {
        Step::Begin => vec![tr(
            "The installer asks a few questions first. Nothing on this computer changes until you click Install on the last page.",
        )],
        Step::Language => vec![tr(
            "This is the language of menus and apps, and what your keys type. Both can be changed later in System Settings.",
        )],
        Step::Zone => vec![tr(
            "Type a big city near you: the clock and calendar follow its time.",
        )],
        Step::Disk => vec![tr(
            "A disk is the drive inside your computer that holds Windows, macOS or Linux and your files. Choosing one changes nothing yet.",
        )],
        Step::Method => vec![
            tr(
                "Sharing keeps your other system and its files, and you pick one when the computer starts.",
            ),
            tr(
                "Erase Disk removes everything on the disk. Manual is for people who already know partitions.",
            ),
        ],
        Step::Replace => vec![tr(
            "A partition is one section of a disk, like a drive letter on Windows. Every other partition and its files stay.",
        )],
        Step::Alongside => vec![tr(
            "Your other system and its files stay; it only gets smaller. As with any change to a disk, back up what matters first.",
        )],
        Step::Manual => vec![
            tr(
                "A partition is one section of a disk. WolOS needs one for its system and a startup partition, the small part your computer starts from.",
            ),
            tr("Nothing changes until you click Install."),
        ],
        Step::Edit => vec![tr(
            "Turn on Erase to clear this partition for its new job. Leave it off and the files on it stay.",
        )],
        Step::Protect => vec![tr(
            "It works like BitLocker on Windows or FileVault on a Mac: without your password or this computer, nobody can read the disk.",
        )],
        Step::Account => vec![tr(
            "It is like your Windows or Mac user account: it signs in, installs apps and changes settings.",
        )],
        Step::Modules => vec![tr(
            "None are chosen for you. They download while WolOS installs; without the internet, WolOS adds them at its first start.",
        )],
        Step::Extras => vec![tr(
            "Not added unless you turn this on. They download while WolOS installs; without the internet, find Media Codecs in the App Store.",
        )],
        Step::Summary => vec![tr(
            "Install starts the changes and takes a few minutes. Back lets you change anything first.",
        )],
        Step::Installing => vec![tr(
            "Nothing restarts until you click Restart. The tips below show a few things worth knowing.",
        )],
        Step::SecureBootChoice | Step::SecureBoot | Step::Done | Step::Failed | Step::KeyUnread => {
            Vec::new()
        }
    }
}

/// The page after this one, in words, for the end of the paragraph; None: it depends on a
/// choice still to be made, or nothing comes next.
pub fn next_words(next: Step) -> Option<&'static str> {
    Some(match next {
        Step::Language => tr("Next: your language and keyboard."),
        Step::Zone => tr("Next: your time zone."),
        Step::Disk => tr("Next: the disk WolOS goes on."),
        Step::Method => tr("Next: whether WolOS shares the disk or takes all of it."),
        Step::Protect => tr("Next: how your files are protected."),
        Step::SecureBootChoice => tr("Next: whether WolOS uses Secure Boot."),
        Step::Account => tr("Next: your account."),
        Step::Modules => tr("Next: optional apps for what you do."),
        Step::Extras => tr("Next: music and video formats."),
        Step::Summary => tr("Next: a summary of everything, before anything changes."),
        _ => return None,
    })
}

/// The Secure Boot page when the person may choose (default off) and the key screen is how it
/// turns on (`mok`): what On and Off mean.
pub fn secure_choice_screen() -> [&'static str; 3] {
    [
        tr("It is optional, and off unless you turn it on."),
        tr(
            "On: one blue screen appears after the restart and asks you to accept this computer’s key, once. Off: WolOS starts straight away.",
        ),
        tr("You can turn it on later in System Settings."),
    ]
}
/// The same choice in Setup Mode (`own-keys`): WolOS writes its keys, nothing to click.
pub fn secure_choice_own_keys() -> [&'static str; 3] {
    [
        tr("It is optional, and off unless you turn it on."),
        tr(
            "On: WolOS puts its own keys into your computer’s startup settings, with nothing to click. Off: nothing there changes.",
        ),
        tr("You can turn it on later in System Settings."),
    ]
}
/// The computer's own Secure Boot is on already, so leaving it off is not a real choice (WolOS
/// would only start after a trip to the startup settings): said, with no switch.
pub fn secure_already_on() -> [&'static str; 2] {
    [
        tr("Your computer already has Secure Boot turned on, so WolOS uses it too."),
        tr(
            "One blue screen appears after the restart and asks you to accept this computer’s key, once.",
        ),
    ]
}
/// Over the steps: what the key screen looks like, before it appears.
pub const KEY_SCREEN_LOOKS: &str = "It looks like an old computer screen: blue, white letters, a short menu. The mouse does not work there: use the arrow keys and Enter.";
/// The switch on the Secure Boot page.
pub const SECURE_SWITCH: &str = "Use Secure Boot";
/// Under the switch, by tier: what turning it on costs.
pub const SECURE_SWITCH_SCREEN: &str = "One blue screen after the restart";
pub const SECURE_SWITCH_OWN_KEYS: &str = "Nothing to click";

/// Summary lines about Secure Boot, by what was chosen.
pub const SUMMARY_SECURE_SCREEN: &str =
    "Secure Boot is on: after the restart, one blue screen asks you to accept this computer’s key.";
pub const SUMMARY_SECURE_OWN_KEYS: &str =
    "Secure Boot is on: WolOS puts its own keys into this computer’s startup settings.";
pub const SUMMARY_SECURE_OFF: &str =
    "Secure Boot stays off. You can turn it on later in System Settings.";

/// The Modules page's rows, by registry id, in the order the page lists them (lead: developer,
/// AI, creative, gaming): (title, what it brings). A record with no row here is not offered.
pub fn module_rows() -> [(&'static str, &'static str, &'static str); 5] {
    [
        (
            "dev",
            tr("Developer Tools"),
            tr("Code editor, Git, containers and programming languages"),
        ),
        (
            "ai-claude",
            tr("AI Assistants"),
            tr("Claude Code and other agents, inside WolOS’s rules"),
        ),
        (
            "ai-local",
            tr("Local AI"),
            tr("AI models that run on this computer, also offline"),
        ),
        (
            "creative",
            tr("Creative Apps"),
            tr("Blender, GIMP, Krita, Inkscape, OBS, Kdenlive and more"),
        ),
        (
            "gaming",
            tr("Gaming"),
            tr("Steam, controllers and game tools"),
        ),
    ]
}

/// Under the Modules list: how much the ticked ones download (`mb`, from their records).
pub fn modules_download(mb: u64) -> String {
    if mb == 0 {
        tr("Nothing extra to download.").to_string()
    } else {
        tr_fill(
            "About {size} to download.",
            &[("size", &crate::install::size_text(mb * 1_000_000))],
        )
    }
}

/// `a`, `a and b`, `a, b and c`.
fn and_list(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => (*one).to_string(),
        [rest @ .., last] => tr_fill(
            "{some} and {last}",
            &[("some", &rest.join(", ")), ("last", last)],
        ),
    }
}

/// The summary line for the ticked modules (`names`, the page's titles), None when none.
pub fn summary_modules(names: &[&str], mb: u64) -> Option<String> {
    (!names.is_empty()).then(|| {
        tr_fill(
            "Adds {names}, about {size} to download.",
            &[
                ("names", &and_list(names)),
                ("size", &crate::install::size_text(mb * 1_000_000)),
            ],
        )
    })
}

/// The Extras page's switch and the summary line when it is on.
pub const CODECS_SWITCH: &str = "Add Media Formats";
pub const SUMMARY_CODECS: &str =
    "Adds the media formats: MP3, AAC, H.264, HEVC, AC3 and common fonts.";

/// Summary and Done with no network at install time: what is added later, by what was chosen.
pub const DEFERRED_BOTH: &str =
    "Media formats and modules are added at first start with a network.";
pub const DEFERRED_CODECS: &str = "Media formats are added at first start with a network.";
pub const DEFERRED_MODULES: &str = "Modules are added at first start with a network.";

/// The deferral line offline: None when neither media formats nor modules were chosen.
pub fn deferred(codecs: bool, modules: bool) -> Option<&'static str> {
    match (codecs, modules) {
        (true, true) => Some(tr(DEFERRED_BOTH)),
        (true, false) => Some(tr(DEFERRED_CODECS)),
        (false, true) => Some(tr(DEFERRED_MODULES)),
        (false, false) => None,
    }
}

/// Continue on the disk page (and the edit page) with no disk chosen.
pub const CHOOSE_DISK: &str = "Choose a disk to install on.";
/// Continue on the Replace page with no partition chosen.
pub const CHOOSE_REPLACE: &str = "Choose the partition WolOS takes over.";
/// The disk password is shorter than 8 characters.
pub const PASSWORD_SHORT: &str = "Use at least 8 characters.";
/// The disk password and its second typing differ.
pub const PASSWORDS_DIFFER: &str = "Passwords don’t match.";
/// The computer's name is not one RFC 1123 allows.
pub const HOSTNAME_RULE: &str = "Use letters, digits and hyphens for the computer’s name.";
/// The time zone search found nothing (the empty state's title, then its hint).
pub fn no_place() -> (&'static str, &'static str) {
    (
        tr("No matching place"),
        tr("Try the name of a big city near you."),
    )
}
/// The disk page while the disks are looked at.
pub const LOOKING_AT_DISKS: &str = "Looking at your disks…";
/// No disk the installer may use (the empty state's title, then its hint; the button is "Look Again").
pub fn no_disk() -> (&'static str, &'static str) {
    (
        tr("No disk found"),
        tr("Connect the disk you want to install on, then click Look Again."),
    )
}

/// The line under the progress bar.
pub fn remaining_text(secs: Option<f64>) -> String {
    match secs {
        None => tr("Estimating time remaining…").into(),
        Some(s) if s < 60.0 => tr("Less than a minute remaining").into(),
        Some(s) => {
            let m = (s / 60.0).round().max(1.0) as u64;
            if m == 1 {
                tr("About a minute remaining").into()
            } else {
                trn(
                    "About {n} minute remaining",
                    "About {n} minutes remaining",
                    m,
                )
            }
        }
    }
}

/// What the progress page says the installer is doing.
pub fn doing(stage: Stage) -> &'static str {
    match stage {
        Stage::Prepare | Stage::Disk => tr("Preparing the disk…"),
        Stage::Files => tr("Copying system files…"),
        Stage::Keys => tr("Making this computer’s keys…"),
        Stage::Encrypt => tr("Encrypting the disk…"),
        Stage::Initramfs => tr("Preparing the first start…"),
        Stage::Boot => tr("Installing the startup files…"),
        Stage::Settings => tr("Saving settings…"),
        Stage::Seal => tr("Locking the disk key to this computer…"),
        Stage::Packages => tr("Installing apps…"),
        Stage::Finish => tr("Finishing up…"),
        Stage::Done => tr("Done"),
        Stage::Failed => "",
    }
}

/// Failed page subtitle, by the stage the install stopped in.
pub fn stopped_text(at: Stage) -> &'static str {
    match at {
        Stage::Prepare | Stage::Disk => tr(
            "The disk couldn’t be prepared. Check that it’s connected, then try again or choose another disk.",
        ),
        Stage::Files => {
            tr("The system files couldn’t be copied. Check the installer disk, then try again.")
        }
        Stage::Keys | Stage::Encrypt | Stage::Seal => {
            tr("The disk couldn’t be encrypted. Try again, or choose Don’t Encrypt.")
        }
        Stage::Initramfs | Stage::Boot => {
            tr("The startup files couldn’t be installed. Try again, or choose another disk.")
        }
        // Language and keyboard (68) or the account (90): `wol install settings` (R2B-8).
        Stage::Settings => tr(
            "Your settings couldn’t be saved. Try again, and check the language, keyboard and account.",
        ),
        _ => tr("The installation stopped before it finished. Try again, or choose another disk."),
    }
}

/// Things to read while the install runs, one at a time.
pub fn tips() -> [&'static str; 6] {
    [
        tr(
            "Updates install into a second copy of WolOS, so you can always start the previous one.",
        ),
        tr("Press Command Space to search apps, files and settings from anywhere."),
        tr(
            "The App Store installs apps for everyone on this computer, with a restore point first.",
        ),
        tr("Control Center, at the top right, has Wi-Fi, sound, brightness and Do Not Disturb."),
        tr("Your files stay private: with encryption on, the disk opens only on this computer."),
        tr("System Settings looks like the one you know, and every change there can be undone."),
    ]
}

/// A kept system keeps its own startup partition while WolOS gets a new one: WolOS's menu can't
/// list it, so it starts from the firmware's startup menu (R2B-11). `name`: "Windows", "Ubuntu".
pub fn other_start(name: &str) -> String {
    tr_fill(
        "To start {name}, open your computer’s startup menu while it starts, often with F12, F9 or Esc.",
        &[("name", name)],
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
            tr(BITLOCKER_RECOVERY),
            tr(BITLOCKER_KEY_WHERE),
            tr(BITLOCKER_AVOID_OWN_KEYS),
        ],
        ("own-keys", true) => vec![tr(BITLOCKER_RECOVERY), tr(BITLOCKER_KEY_WHERE)],
        ("firmware", _) => vec![
            tr(BITLOCKER_RECOVERY),
            tr(BITLOCKER_KEY_WHERE),
            tr(BITLOCKER_AVOID_SETTING),
        ],
        _ => Vec::new(),
    }
}

/// Windows stays: WolOS keeps the clock in local time as Windows does (R2B-12).
pub const CLOCK_LOCAL: &str = "The clock keeps local time, as Windows does.";

/// Failed page foot when a step stopped: the computer's own words are behind the Details button.
pub const FAILED_SEE_DETAILS: &str = "Click Details to see what the computer said.";

/// Failed page subtitle when the disk filled up during the install (`No space left on device` in
/// the tool's words or the log, QA-SHADOW-24).
pub const DISK_FULL_STOPPED: &str =
    "The disk ran out of space. Choose a bigger disk or partition, then try again.";

/// Failed page subtitle when `wol install check` refused the plan: nothing was written, and the
/// refusal's own sentence follows it.
pub const REFUSED_STOPPED: &str =
    "Nothing on the disk was changed. Fix what is said below, then click Try Again.";

/// The steps on the firmware's blue key screen, in the English of
/// `wol_core::boot::key_screen_steps` (`key_screen_steps_are_the_core_ones` keeps them the same):
/// Setup and the installer show them in the person's language. `{password}` is the key screen's
/// password.
const KEY_SCREEN_STEPS: [&str; 5] = [
    crate::i18n::n_(
        "After the restart a blue screen appears. It comes from your computer’s firmware, not from WolOS, and it waits for you.",
    ),
    crate::i18n::n_("Press a key, choose Enroll MOK, then Continue, then Yes."),
    crate::i18n::n_("When it asks for a password, type {password}. Nothing shows while you type."),
    crate::i18n::n_("Choose Reboot. You will not see this screen again."),
    crate::i18n::n_(
        "Missed it? Start from the USB stick again: its blue screen shows Enroll MOK. Or choose Enroll key from disk and pick ENROL_THIS_KEY_FOR_WOLOS.cer.",
    ),
];

/// [`KEY_SCREEN_STEPS`] in the language in use, the password filled in.
pub fn key_screen_steps() -> Vec<String> {
    KEY_SCREEN_STEPS
        .iter()
        .map(|s| tr_fill(s, &[("password", wol_core::boot::MOK_PASSWORD)]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shell's copy of the key-screen steps is the core's English, word for word.
    #[test]
    fn key_screen_steps_are_the_core_ones() {
        let shell: Vec<String> = KEY_SCREEN_STEPS
            .iter()
            .map(|s| shared::i18n::fill(s, &[("password", wol_core::boot::MOK_PASSWORD)]))
            .collect();
        assert_eq!(shell, wol_core::boot::key_screen_steps());
    }
}
