// SPDX-License-Identifier: GPL-2.0-or-later
//! What the user reads when something did not work: the one place errors become sentences.
//! HIG §3: inline, one or two short sentences, what happened + what to do. Surfaces show these,
//! never an error's own text (`Display`, io errors, exit codes). [`lint`] is the rule;
//! `tests/copy_lint.rs` runs it on every sentence here and on the copy of other modules.
use crate::system::ActionError;
use wol_core::install::disk::{Blocked, Disk as InstallDisk};
use wol_core::install::layout::Refusal;
use wol_core::install::plan::Change;
use wol_core::journal::UndoError;

/// The Install app's sentences (audit3 R2B-20): its pages hold none of their own.
#[path = "copy_installer.rs"]
pub mod installer;

/// System Settings' sentences (v0.12 panes): the panes hold none of their own.
#[path = "copy_settings.rs"]
pub mod settings;

/// Plugin Maker's sentences (ADR 0031): its pages hold none of their own.
#[path = "copy_maker.rs"]
pub mod maker;

/// Fragments a user must never read (case-insensitive, at a word start; the [`WHOLE_WORD`] ones
/// as whole words only), and what to do instead. Upstream projects are never named on screen
/// (docs/THIRD-PARTY.md:4-5): the reasons starting "upstream:" mark them for RU-006.
pub const BANNED: [(&str, &str); 41] = [
    ("failed", "say what happened and what to do"),
    ("error", "say what happened and what to do"),
    ("errno", "raw OS error"),
    ("os error", "raw io::Error text"),
    ("exit status", "process exit status"),
    ("exit code", "process exit code"),
    ("(exit", "process exit code"),
    ("panicked", "stack text"),
    ("backtrace", "stack text"),
    ("unwrap", "stack text"),
    ("permission denied", "raw EACCES text"),
    ("no such file", "raw ENOENT text"),
    ("invalid", "say what is wrong with it"),
    ("authentication", "say password"),
    ("unavailable", "say what is missing and what to do"),
    ("stderr", "plumbing"),
    ("stdout", "plumbing"),
    ("stdin", "plumbing"),
    ("daemon", "plumbing"),
    ("socket", "plumbing"),
    ("json", "file format"),
    ("wol-", "program name"),
    ("wpa_", "program name"),
    ("wlan", "interface name"),
    ("ssid", "say network name"),
    ("ifconfig", "program name"),
    ("sysctl", "program name"),
    ("hyprctl", "program name"),
    ("hyprland", "upstream: say the desktop"),
    ("aur", "upstream: say community packages"),
    ("aether", "upstream: credited project"),
    ("omapager", "upstream: credited project"),
    ("omarchy", "upstream: credited project"),
    ("nmcli", "program name"),
    ("bectl", "program name"),
    ("pkg", "say app or update"),
    ("sudo", "terminal"),
    ("root", "say administrator"),
    ("boot environment", "say restore point"),
    ("some(", "debug text"),
    ("err(", "debug text"),
];

/// Longest sentence pair a sheet or status line shows whole.
pub const MAX_LEN: usize = 140;

/// BANNED entries that are short enough to sit inside ordinary words ("aura", "Aurora").
pub const WHOLE_WORD: [&str; 1] = ["aur"];

/// Everything wrong with `s` as an error sentence (empty = fine).
pub fn lint(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let lower = s.to_lowercase();
    for (pat, why) in BANNED {
        let whole = WHOLE_WORD.contains(&pat);
        let at_word = lower.match_indices(pat).any(|(i, _)| {
            !lower[..i]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric)
                && !(whole
                    && lower[i + pat.len()..]
                        .chars()
                        .next()
                        .is_some_and(char::is_alphanumeric))
        });
        if at_word {
            out.push(format!("“{pat}”: {why}"));
        }
    }
    for t in s.split_whitespace() {
        let t = t.trim_start_matches(['“', '(', '"']);
        if t.starts_with(['/', '~', '$', '`', '-']) || t.contains("::") {
            out.push(format!("“{t}”: path, flag or code"));
        }
        if [".conf", ".json", ".log", ".sock", ".rs", ".so"]
            .iter()
            .any(|x| t.trim_end_matches(['.', ',', '”']).ends_with(x))
        {
            out.push(format!("“{t}”: file name"));
        }
    }
    if !s.starts_with(|c: char| c.is_uppercase() || c == '“') {
        out.push("starts with a capital letter".into());
    }
    if !s.ends_with('.') {
        out.push("ends with a full stop".into());
    }
    if s.contains('!') {
        out.push("no exclamation marks".into());
    }
    if s.chars().count() > MAX_LEN {
        out.push(format!("at most {MAX_LEN} characters"));
    }
    if s.matches(". ").count() > 1 {
        out.push("at most two sentences".into());
    }
    out
}

/// A blank disk has nothing to sit alongside.
pub const INSTALL_BLANK: &str =
    "This disk is empty, so there’s nothing to install alongside. Use Erase Disk.";
pub const INSTALL_RECOVERY_NOTE: &str = "Write this key down and keep it safe. It opens your disk if your password or security chip stops working.";
pub const INSTALL_POINT_OF_NO_RETURN: &str =
    "Back up anything you need first. These changes can’t be undone.";
/// Don't Encrypt was chosen: said once more over the Install button.
pub const INSTALL_UNENCRYPTED: &str =
    "Your files won’t be encrypted. Anyone who takes the disk can read them.";
/// The disk password is typed before any desktop exists, on a plain keyboard map.
pub const INSTALL_PASSPHRASE_ASCII: &str = "Use letters, digits and common symbols only. Accented letters can’t be typed before WolOS starts.";
/// An account name a system user already has on this computer.
pub const INSTALL_NAME_TAKEN: &str = "That name is already used on this computer. Choose another.";
/// Own keys: the firmware now trusts only this computer's keys and Microsoft's.
pub const INSTALL_OWN_KEYS_MEDIA: &str = "The install media is not signed with those keys, so it will no longer start on this computer. Remove it when the screen goes dark.";
/// The Done page's key screen card.
pub const INSTALL_KEY_SCREEN_TITLE: &str = "One blue screen after restart";
/// The live system runs from the stick until the screen goes dark: never pulled before Restart.
pub const INSTALL_KEY_SCREEN_MEDIA: &str =
    "When the screen goes dark after you click Restart, remove the USB stick or detach the ISO.";
/// The Done page's subtitle: the medium comes out only once the live system has stopped.
pub const INSTALL_RESTART: &str = "Click Restart to start using WolOS. When the screen goes dark, remove the USB stick or detach the ISO.";
/// A new startup partition next to Windows': its menu lists WolOS only (systemd-boot cannot
/// reach another partition), so Windows starts from the firmware's startup menu.
pub const INSTALL_WINDOWS_START: &str = "To start Windows, open your computer’s startup menu while it starts, often with F12, F9 or Esc.";
/// Security Chip: the account password opens the disk when the chip can't, on a plain keymap.
pub const INSTALL_TPM_FALLBACK: &str = "If the chip can’t open the disk, your account password does. Use letters, digits and common symbols only.";
/// The recovery key must be acknowledged before Restart.
pub const INSTALL_KEY_ACK: &str = "I wrote the key down or saved it";
/// Save to USB: nothing to save on.
pub const INSTALL_NO_USB: &str = "No USB stick was found. Plug one in and try again.";
/// Save to USB: written.
pub fn install_key_saved(where_: &str) -> String {
    format!("The key was saved on “{where_}”. Keep that disk safe.")
}
/// Save to USB: not written.
pub const INSTALL_KEY_NOT_SAVED: &str = "The key couldn’t be saved there. Try another USB stick.";
/// Replace: no partition on the disk can hold WolOS.
pub const INSTALL_NOTHING_TO_REPLACE: &str =
    "No partition on this disk is big enough to take over. Use Erase Disk or Manual.";
/// Summary line: the person asked to sign in without a password.
pub const INSTALL_AUTOLOGIN: &str = "Sign in automatically when the computer starts.";

/// Summary line for the swap choice; None: no swap, nothing to say.
pub fn install_swap(s: crate::install::flow::Swap) -> Option<String> {
    use crate::install::flow::Swap;
    match s {
        Swap::Zram => Some("Use compressed memory as swap.".into()),
        Swap::Swapfile(gib) => Some(format!("Add a swap file of {gib} GB.")),
        Swap::None => None,
    }
}

/// Summary line for the computer's name.
pub fn install_hostname(name: &str) -> String {
    format!("Name this computer “{name}”.")
}

/// What a mount is for, in plain words.
pub fn install_use(mount: &str) -> String {
    match mount {
        "/" => "WolOS system".into(),
        "/efi" => "Startup files".into(),
        "/home" => "Your files".into(),
        "swap" => "Swap".into(),
        m => format!("The {} folder", m.trim_start_matches('/')),
    }
}

/// The same inside a sentence: "for your files".
fn install_for(mount: &str) -> String {
    match mount {
        "/" => "WolOS".into(),
        "/efi" => "startup files".into(),
        "/home" => "your files".into(),
        "swap" => "swap".into(),
        m => format!("the {} folder", m.trim_start_matches('/')),
    }
}

/// Why Install Alongside can't shrink a partition.
pub fn install_blocked(b: Blocked) -> &'static str {
    match b {
        Blocked::BitLocker => {
            "BitLocker protects Windows here. Turn it off in Windows, then start the installer again."
        }
        Blocked::NotShutDown => {
            "Windows didn’t shut down fully. Turn off Fast Startup in Windows, shut down, then try again."
        }
        Blocked::MacOs => "Make room with Disk Utility in macOS, then start the installer again.",
        Blocked::Unreadable => {
            "This partition couldn’t be read. Check it in its own system, then try again."
        }
    }
}

/// Why a layout can't run, in one or two sentences; `d` names the partitions.
pub fn install_refusal(r: &Refusal, d: &InstallDisk) -> String {
    let named = |uuid: &str| {
        d.part(uuid).map_or_else(
            || "That partition".to_string(),
            |p| format!("“{}”", p.title()),
        )
    };
    match r {
        Refusal::DiskChanged | Refusal::UnknownPartition => {
            "The disk changed since you chose it. Go back and choose it again.".into()
        }
        Refusal::NotGpt => "This disk uses an older layout. Only Erase Disk can use it.".into(),
        Refusal::TooSmall => {
            "WolOS needs at least 16 GB. Make more room or choose another disk.".into()
        }
        Refusal::Conflict(id) => format!("{} has two jobs. Give it just one.", named(id)),
        Refusal::Mounted(id) => format!("{} is in use. Restart and try again.", named(id)),
        Refusal::CantResize(id) => {
            format!("{} can’t get that small. Choose a larger size.", named(id))
        }
        Refusal::NoRoom => {
            "There isn’t enough room for that. Delete or shrink a partition first.".into()
        }
        Refusal::NoRoot => "Choose a partition for the WolOS system.".into(),
        Refusal::NoEsp => "Choose the startup partition, or add one of 1 GB.".into(),
        Refusal::RootMustFormat => {
            "The WolOS system needs an erased partition. Turn on Erase for it.".into()
        }
        Refusal::EspInUse => {
            "Another system starts from that partition, so it can’t be erased.".into()
        }
        Refusal::EspHasWolos(id) => format!(
            "{} already starts another WolOS. Add a new startup partition instead.",
            named(id)
        ),
        // Windows is hibernated (Fast Startup): its sentence says how to shut it down fully.
        Refusal::WindowsNotShutDown(_) => install_blocked(Blocked::NotShutDown).into(),
        Refusal::NotAnEsp(id) => format!(
            "{} can’t hold the startup files. Choose another or add a new one.",
            named(id)
        ),
        Refusal::BadMount(_) => {
            "That folder name can’t be used. Use lowercase letters, and not a system folder.".into()
        }
        Refusal::DuplicateMount(_) => "Two partitions have the same job. Give each its own.".into(),
        Refusal::BadFilesystem(id) => format!(
            "{} can’t do that job as it is. Turn on Erase or choose another job.",
            named(id)
        ),
    }
}

/// What `wol install check` refuses a plan with, besides a layout [`Refusal`] (controller
/// `plan_problem`, core `plan::unlock_problem`), as the Failed page says it.
pub fn install_plan_refusal(word: &str) -> Option<&'static str> {
    Some(match word {
        "BadSettings" => {
            "The language, keyboard or time zone can’t be used. Try again and choose them once more."
        }
        "BadName" => {
            "Use lowercase letters and digits for the account name, starting with a letter."
        }
        "ReservedName" => INSTALL_NAME_TAKEN,
        "WeakPassword" => "Choose an account password of at least 8 characters, then try again.",
        "UntypablePassword" => INSTALL_TPM_FALLBACK,
        _ => return None,
    })
}

/// A refusal word of `wol install check` (`Word` or `Word("part")`, see
/// [`crate::install::refused_word`]) as one sentence; None: a word this app does not know.
pub fn install_check_refusal(word: &str, part: Option<&str>, d: &InstallDisk) -> Option<String> {
    if let Some(t) = install_plan_refusal(word) {
        return Some(t.to_string());
    }
    let id = || part.unwrap_or_default().to_string();
    let r = match word {
        "DiskChanged" => Refusal::DiskChanged,
        "NotGpt" => Refusal::NotGpt,
        "TooSmall" => Refusal::TooSmall,
        "UnknownPartition" => Refusal::UnknownPartition,
        "Conflict" => Refusal::Conflict(id()),
        "Mounted" => Refusal::Mounted(id()),
        "CantResize" => Refusal::CantResize(id()),
        "NoRoom" => Refusal::NoRoom,
        "NoRoot" => Refusal::NoRoot,
        "NoEsp" => Refusal::NoEsp,
        "RootMustFormat" => Refusal::RootMustFormat,
        "EspInUse" => Refusal::EspInUse,
        "EspHasWolos" => Refusal::EspHasWolos(id()),
        "WindowsNotShutDown" => Refusal::WindowsNotShutDown(id()),
        "NotAnEsp" => Refusal::NotAnEsp(id()),
        "BadMount" => Refusal::BadMount(id()),
        "DuplicateMount" => Refusal::DuplicateMount(id()),
        "BadFilesystem" => Refusal::BadFilesystem(id()),
        _ => return None,
    };
    Some(install_refusal(&r, d))
}

/// One line of the summary.
pub fn install_change(c: &Change) -> String {
    use crate::install::size_text as size;
    match c {
        Change::EraseDisk { bytes, others } if others.is_empty() => {
            format!("Erase the whole disk ({}).", size(*bytes))
        }
        Change::EraseDisk { bytes, others } => format!(
            "Erase the whole disk ({}), including {}.",
            size(*bytes),
            others.join(" and ")
        ),
        Change::Delete { name, bytes } => format!("Delete {name} ({}).", size(*bytes)),
        Change::Resize { name, from, to } if to < from => {
            format!("Shrink {name} from {} to {}.", size(*from), size(*to))
        }
        Change::Resize { name, from, to } => {
            format!("Grow {name} from {} to {}.", size(*from), size(*to))
        }
        Change::Format {
            name,
            mount,
            encrypted,
        } => {
            let lock = if *encrypted { ", encrypted" } else { "" };
            format!("Erase {name} and use it for {}{lock}.", install_for(mount))
        }
        Change::New {
            bytes,
            mount,
            encrypted,
        } => {
            let lock = if *encrypted { ", encrypted" } else { "" };
            format!("Add {} for {}{lock}.", size(*bytes), install_for(mount))
        }
        Change::ReuseEsp => "Add WolOS to the startup partition that’s already there.".into(),
        Change::Keep { name, mount, plain } if *plain => format!(
            "Keep {name} as it is, for {}. It stays unencrypted.",
            install_for(mount)
        ),
        Change::Keep { name, mount, .. } => {
            format!("Keep {name} as it is, for {}.", install_for(mount))
        }
        Change::Untouched { name } => format!("{name} stays as it is."),
        Change::OneCopy => {
            "This layout keeps one copy of WolOS, so updates can’t be undone.".into()
        }
    }
}

/// Small but enough.
pub fn install_low_space(bytes: u64) -> String {
    format!(
        "WolOS would have {}. It fits, but 40 GB or more leaves room for apps and updates.",
        crate::install::size_text(bytes)
    )
}

/// Every [`ActionError`], for the lint. [`action`] matches exhaustively, so a new variant needs a
/// sentence before it compiles; list it here too.
pub const ACTION_ERRORS: [ActionError; 7] = [
    ActionError::Unavailable,
    ActionError::NeedsAuth,
    ActionError::Denied,
    ActionError::NotAdmin,
    ActionError::Invalid,
    ActionError::Failed,
    ActionError::WrongPassword,
];

/// Any change that did not happen.
pub fn action(e: ActionError) -> &'static str {
    match e {
        ActionError::Unavailable => "This isn’t available on this computer.",
        ActionError::NeedsAuth => "Enter an administrator’s password to make this change.",
        ActionError::Denied => "Incorrect password. Try again.",
        ActionError::NotAdmin => "Only an administrator can make this change.",
        ActionError::Invalid | ActionError::Failed => "The change did not work. Try again.",
        ActionError::WrongPassword => {
            "The network didn’t accept that password. Check it and try again."
        }
    }
}

/// The detail line of the administrator sheet for a package built on this computer (ADR 0034):
/// the whole SHA-256 of wold's staged copy (the title shows its first digits).
pub fn approval_package_file(sha256: &str) -> String {
    format!("SHA-256 of the copy: {sha256}. Enter your password to allow this.")
}

/// The title of an administrator or consent sheet raised by any program that runs `/usr/bin/wol`
/// (any process of the account can; P6-4): the program is not named, the change is, so the sheet
/// cannot pass for a WolOS tool. `what` ends "wants to …" with the package names quoted.
pub fn approval_via_wol(what: &str) -> String {
    format!("A program using wol wants to {what}.")
}

/// Joining the Wi-Fi network `name`. Denied = the administrator sheet was cancelled.
pub fn wifi_join(e: ActionError, name: &str) -> String {
    match e {
        ActionError::WrongPassword => {
            format!("“{name}” didn’t accept that password. Check it and try again.")
        }
        ActionError::Invalid => {
            "Wi-Fi passwords have 8 to 63 characters. Check it and try again.".into()
        }
        ActionError::Denied | ActionError::NeedsAuth => {
            format!("Not joined. Joining “{name}” needs an administrator’s password.")
        }
        ActionError::NotAdmin => format!("Only an administrator can join “{name}”."),
        ActionError::Unavailable => "Wi-Fi isn’t available on this computer.".into(),
        ActionError::Failed => format!("Couldn’t join “{name}”. Move closer to it and try again."),
    }
}

/// Progress while joining the Wi-Fi network `name` (Control Center's Wi-Fi page).
pub fn wifi_joining(name: &str) -> String {
    format!("Joining “{name}”…")
}

/// Installing the app `name` from the App Store.
/// Why `name` is still installed after Remove.
pub fn remove(e: ActionError, name: &str) -> String {
    match e {
        ActionError::Denied | ActionError::NeedsAuth => {
            format!("{name} wasn’t removed. Removing apps needs an administrator’s password.")
        }
        ActionError::NotAdmin => format!("Only an administrator can remove {name}."),
        ActionError::Unavailable => {
            format!("{name} can’t be removed right now. Restart the computer and try again.")
        }
        ActionError::Invalid | ActionError::Failed | ActionError::WrongPassword => {
            format!("{name} couldn’t be removed. Quit it, then try again.")
        }
    }
}

pub fn install(e: ActionError, name: &str) -> String {
    match e {
        ActionError::Denied | ActionError::NeedsAuth => {
            format!("{name} wasn’t installed. Installing apps needs an administrator’s password.")
        }
        ActionError::NotAdmin => {
            format!("Only an administrator can install {name}. Ask one to sign in.")
        }
        ActionError::Unavailable => {
            format!("{name} can’t be installed right now. Restart the computer and try again.")
        }
        ActionError::Invalid | ActionError::Failed | ActionError::WrongPassword => {
            format!("{name} couldn’t be installed. Check that you’re online, then try again.")
        }
    }
}

/// Checking for updates found no answer.
pub const UPDATE_CHECK: &str =
    "Couldn’t check for updates. Check that you’re online, then try again.";

/// Under "N updates available" (Settings > Software Update and the App Store's Updates).
pub const UPDATE_RESTORE_POINT: &str = "WolOS saves a restore point before updating.";

/// "1 update available", "3 updates available".
pub fn updates_available(n: usize) -> String {
    format!("{n} update{} available", if n == 1 { "" } else { "s" })
}

/// Installing updates.
pub fn update(e: ActionError) -> &'static str {
    match e {
        ActionError::Invalid | ActionError::Failed | ActionError::WrongPassword => {
            "The update didn’t finish. Check that you’re online, then try again."
        }
        e => action(e),
    }
}

/// Undoing a settings change (Settings > Restore, the Undo button).
pub fn undo(e: &UndoError) -> &'static str {
    match e {
        UndoError::Nothing => "There’s nothing to undo.",
        UndoError::NotFound(_) => "That change is no longer in the history.",
        UndoError::Changed(_) => "This was changed again later. Undo the newer change first.",
        UndoError::Refused(_) => "This change can’t be undone.",
        UndoError::Io(_) => "The change couldn’t be undone. Try again.",
    }
}

/// Every `UndoError` shape, for the lint.
pub fn undo_errors() -> [UndoError; 5] {
    [
        UndoError::Nothing,
        UndoError::NotFound(12),
        UndoError::Changed("wolos/theme".into()),
        UndoError::Refused("helpers cannot change their own permissions"),
        UndoError::Io("Permission denied (os error 13)".into()),
    ]
}

// ---- Desktop widgets (the card's sentence; the developer's detail sits behind Show Details) ----

/// A widget's data source did not give it data.
pub fn widget_source(e: &wol_core::widgets::SourceError, name: &str) -> String {
    use wol_core::widgets::SourceError as E;
    match e {
        E::NotFound(_) => format!("{name} is missing a file it needs. Reinstall it, or remove it."),
        E::Timeout(_) => format!("{name} is taking too long to update. It tries again soon."),
        E::Failed { .. } | E::TooLarge | E::BadJson(_) | E::Moved(_) => {
            format!("{name} couldn’t get its data. It tries again soon.")
        }
        E::NeedsConsent(host) => format!("Allow {name} to read data from {host}."),
        E::Offline(_) => format!("{name} can’t reach the internet. It updates when you’re online."),
    }
}

/// A widget package whose description has mistakes (each listed under Show Details).
pub fn widget_problems(name: &str) -> String {
    format!("{name} can’t be shown. Its developer can fix it with the details.")
}

/// A widget on the desktop whose package is gone.
pub const WIDGET_MISSING: &str = "This widget isn’t installed. Reinstall it, or remove it.";
/// A Lua widget file that does not load or whose script stopped (the reason is in the details).
pub const LUA_WIDGET_FAILED: &str = "This widget’s script stopped. Fix the file and it reloads.";
/// A widget kind from a newer WolOS.
pub const WIDGET_NEWER: &str = "This widget needs a newer version of WolOS.";
/// The Weather city search got no answer.
pub const CITY_SEARCH: &str =
    "Couldn’t search for cities. Check that you’re online, then try again.";
/// Weather with nothing cached and no network.
pub const WEATHER_OFFLINE: &str = "No forecast right now. It updates when you’re back online.";

// ---- Battery, disk and session power (notifications and the confirm sheet) ----

/// Battery notification at 20, 10 and 5 %: summary, body.
pub fn battery_low(percent: u8) -> (String, String) {
    (
        "Low Battery".into(),
        format!("Your battery has {percent}% left. Connect to power, or turn on Low Power Mode."),
    )
}

/// The battery is about to run out: the computer locks and sleeps.
pub const BATTERY_SLEEP: (&str, &str) = (
    "Battery Empty",
    "Your computer is going to sleep now. Connect to power to keep working.",
);

/// Under 5 % free on the startup disk: summary, body (`free` is "4.2 GB").
pub fn disk_full(free: &str) -> (String, String) {
    (
        "Your Disk Is Almost Full".into(),
        format!("Only {free} left. Empty the Trash or remove files you don’t need."),
    )
}

/// Files' "Check for Malware" (the full signature set).
pub const ANALYZE_TITLE: &str = "Check for Malware";

pub fn analyze_checking(shown: &str) -> (String, String) {
    (
        format!("Checking {shown}…"),
        "The full check can take a few seconds to start.".into(),
    )
}

/// The signatures are off or not here: nothing was checked, and Settings is where to change it.
pub fn analyze_offer(f: &wol_core::malware::Full) -> (String, String) {
    if f.on {
        (
            "Signatures Aren’t Ready".into(),
            format!("{} Nothing was checked.", malware_full(f)),
        )
    } else {
        (
            "Signatures Are Off".into(),
            "The full check needs its signature set. You can turn it on in Settings > Privacy & Security, which downloads it to this computer. Nothing was checked.".into(),
        )
    }
}

/// Title and sentence of a finished check.
pub fn analyze_result(
    a: &wol_core::malware::Analysis,
    shown: &str,
    f: &wol_core::malware::Full,
) -> (String, String) {
    use wol_core::malware::Analysis;
    match a {
        Analysis::Clean => (
            "Nothing Found".into(),
            format!(
                "The full check compared {shown} with {:.1} million signatures, version {}.",
                f.signatures as f64 / 1e6,
                f.version
            ),
        ),
        Analysis::Found(_) => (
            "Malware Found".into(),
            format!("The full check matched {shown} to known malware. Move it to the Trash."),
        ),
        Analysis::Incomplete(_) => (
            "Check Not Finished".into(),
            format!("Nothing was found, but part of {shown} could not be checked."),
        ),
    }
}

/// Files stopped by the malware check: summary, body. Says what was found and what to do.
pub fn malware_found(file: &str, name: &str) -> (String, String) {
    let shown: String = file.chars().take(48).collect();
    (
        "Malware Found".into(),
        format!("“{shown}” matches known malware ({name}). Move it to the Trash."),
    )
}

/// A download the malware check could not look at yet (the guard's `unchecked` answer, audit S4):
/// refused for now, not remembered, so opening it again asks again. Not a finding: says so.
pub fn malware_unchecked(file: &str) -> (String, String) {
    let shown: String = file.chars().take(48).collect();
    (
        "Not Checked Yet".into(),
        format!(
            "WolOS could not check “{shown}” yet, so it did not open. Try opening it again in a moment."
        ),
    )
}

/// The malware check's status line.
pub fn malware_state(s: &wol_core::malware::Status) -> String {
    match (s.on, s.signatures_age_days) {
        (Some(false), _) => "Off".into(),
        (_, None) => "No signatures yet".into(),
        (_, Some(0)) => "On, updated today".into(),
        (_, Some(1)) => "On, updated yesterday".into(),
        (_, Some(d)) if d >= wol_core::malware::STALE_DAYS => {
            format!("On, last updated {d} days ago")
        }
        (_, Some(d)) => format!("On, updated {d} days ago"),
    }
}

/// The full signature database's line in Settings.
pub fn malware_full(f: &wol_core::malware::Full) -> String {
    if !f.on {
        "Off. The standard check stays on.".into()
    } else if f.updating {
        "Downloading signatures…".into()
    } else if let Some(why) = &f.refused {
        format!("Signature update refused: {why}. The previous signatures stay in use.")
    } else if f.failed {
        "The last download did not finish. WolOS will try again tomorrow.".into()
    } else if f.version == 0 {
        "Waiting to download.".into()
    } else {
        format!(
            "{:.1} million signatures, version {}.",
            f.signatures as f64 / 1e6,
            f.version
        )
    }
}

/// What turning it on means.
pub const MALWARE_FULL_NOTE: &str = "Downloads about 115 MB once, then about 25 MB a day, and the \
     system checks every download before use. A check can use about 1 GB of memory.";

/// What the check does, and what it cannot do.
pub const MALWARE_SCOPE: &str = "Checks downloads, new programs and removable disks for known \
     malware. New malware can get past it, so be careful with unexpected files.";

/// The daily update check found something (wol_core::update::check).
pub fn updates_ready(count: usize, security: bool) -> (String, String) {
    let (n, it) = if count == 1 {
        ("1 update".to_string(), "it")
    } else {
        (format!("{count} updates"), "them")
    };
    let fixes = if security {
        ", including security fixes"
    } else {
        ""
    };
    (
        "Updates Available".into(),
        format!("WolOS has {n} ready{fixes}. Open Software Update to install {it}."),
    )
}

/// The plugin index has newer versions of plugins on this account (`shell::alerts`): the
/// notification's summary and body, for the plugins not yet told about.
pub fn plugin_updates_ready(names: &[&str]) -> (String, String) {
    match names {
        [one] => (
            "Plugin Update Available".into(),
            format!("{one} has an update. Open the App Store, then Plugins, to install it."),
        ),
        many => (
            "Plugin Updates Available".into(),
            format!(
                "Updates are ready for {} plugins. Open the App Store, then Plugins, to install them.",
                many.len()
            ),
        ),
    }
}

/// Restart, Shut Down and Log Out ask first: question, countdown line, button.
pub fn session_confirm(action: &str, secs: u32) -> Option<(&'static str, String, &'static str)> {
    let (q, what, button) = match action {
        "shutdown" => (
            "Are you sure you want to shut down your computer now?",
            "your computer will shut down",
            "Shut Down",
        ),
        "restart" => (
            "Are you sure you want to restart your computer now?",
            "your computer will restart",
            "Restart",
        ),
        "logout" => (
            "Are you sure you want to log out now?",
            "you will be logged out",
            "Log Out",
        ),
        _ => return None,
    };
    let unit = if secs == 1 { "second" } else { "seconds" };
    Some((
        q,
        format!("If you do nothing, {what} automatically in {secs} {unit}."),
        button,
    ))
}

// ---- Dock, Desktop & Dock settings, desktop widgets (audit3 R2A-19) ----

/// The Downloads stack with nothing in the folder.
pub const DOCK_DOWNLOADS_EMPTY: &str = "Nothing downloaded yet.";
/// Settings > Desktop & Dock, Tile windows.
pub const DOCK_TILE_HINT: &str = "New windows fill the screen side by side instead of floating.";
/// Settings > Desktop & Dock, Tiling shortcuts.
pub const DOCK_TILING_SHORTCUTS_HINT: &str =
    "Move, resize and focus tiled windows from the keyboard.";
/// Keep Floating > Add App with no window open to pick.
pub const DOCK_ADD_APP_EMPTY: &str = "Open the app first, then add it here.";
/// Edit Widgets: the desktop grid has no free spot of that size.
pub const WIDGET_NO_ROOM: &str = "No room. Remove a widget or pick a smaller size.";
/// An app's question (clipboard, file, sound) refused because too many are being answered at
/// once (`app_session::ANSWERS_IN_FLIGHT`).
pub const DESKTOP_BUSY: &str = "The desktop is busy. Try again.";
/// The password is right, but the account may not use the screen (not in group `seat`).
pub const LOGIN_NO_DISPLAY: &str =
    "This account can’t use the screen yet. Ask an administrator to add it to the seat group.";
/// Login window with no account that can sign in (audit4 S3-7): (title, hint).
pub const LOGIN_NO_ACCOUNTS: (&str, &str) = (
    "No Accounts",
    "No account on this computer can sign in here. Ask the person who looks after it for help.",
);

/// A Sleep, Restart or Shut Down button whose request was not taken (audit4 S3-7, S3-9).
pub fn power_refused(p: shared::login::Power) -> &'static str {
    use shared::login::Power;
    match p {
        Power::Sleep => "This computer can’t sleep right now. Try again.",
        Power::Restart => "This computer can’t restart right now. Try again.",
        Power::ShutDown => "This computer can’t shut down right now. Try again.",
    }
}

/// The lock screen's Restart was refused: wol-admind does not end another account's session
/// (audit6 S6-5).
pub const LOCK_RESTART_REFUSED: &str = "This computer can’t restart right now. If someone else is signed in, they need to sign out first.";
/// Timer widget notifications: (summary, body).
pub const TIMER_BREAK: (&str, &str) = (
    "Time for a break",
    "Step away for a few minutes. Start the break when you are ready.",
);
pub const TIMER_BREAK_OVER: (&str, &str) = (
    "Break is over",
    "Start the next focus session when you are ready.",
);
pub const TIMER_DONE: (&str, &str) = ("Timer done", "Your countdown has finished.");

// ---- App Store: Kits and Plugins (ADR 0032 B, ADR 0031 W4) ----

/// A wopkg kit install or a plugin action did not complete.
pub const STORE_NOT_FINISHED: &str = "That did not finish. Nothing was changed.";
/// Kits page, under the title.
pub const STORE_KITS_INTRO: &str =
    "Sets of extras added in one step. Nothing here is installed until you choose it.";
/// Plugins page, under the title.
pub const STORE_PLUGINS_INTRO: &str = "Plugins add to the menu bar, the desktop and search. They install for you only and update on their own, apart from WolOS updates.";
/// Plugins page, last row.
pub const STORE_PLUGINS_SIGNED: &str =
    "Every plugin here was checked and signed by the WolOS plugin index.";
pub const STORE_PLUGINS_LOADING: &str = "Loading the plugin list…";
pub const STORE_PLUGINS_OFFLINE: &str =
    "The plugin list could not be loaded. Check that you are online, then try again.";
/// The plugin server answered, but has no list (404): nothing to fix on this computer.
pub const STORE_PLUGINS_NOT_PUBLISHED: &str =
    "The plugin list is not available from the WolOS plugin server yet. Try again later.";
/// A list came, but its signature does not check: it is never shown.
pub const STORE_PLUGINS_UNTRUSTED: &str =
    "The plugin list could not be checked, so it is not shown. Try again later.";
/// Previews and first start before the list was downloaded.
pub const STORE_PLUGINS_NOT_YET: &str = "The plugin list has not been downloaded yet.";
pub const STORE_PLUGINS_EMPTY: &str = "The plugin list is empty.";
pub const STORE_PLUGINS_TITLE: &str = "Plugins";
pub const STORE_PLUGINS_CREATE: &str = "Create Plugin…";
pub const STORE_PLUGINS_MINE: &str = "Your Plugins";
pub const STORE_PLUGINS_AVAILABLE: &str = "Available";
pub const STORE_PLUGINS_CHECKED: &str = "Checked Plugins";
/// Under "could not be loaded": loads the list again, fresh.
pub const STORE_PLUGINS_TRY_AGAIN: &str = "Try Again";
/// Plugin row buttons; `{v}` is a version.
pub const STORE_PLUGIN_GET: &str = "Get";
pub const STORE_PLUGIN_UPDATE_TO: &str = "Update to {v}";
pub const STORE_PLUGIN_NEEDS: &str = "Needs WolOS {v}";
pub const STORE_PLUGIN_TURN_ON: &str = "Turn On";
pub const STORE_PLUGIN_TURN_OFF: &str = "Turn Off";
/// Installed and on, but this version asks for more than was allowed: Allow grants it.
pub const STORE_PLUGIN_ALLOW: &str = "Allow";
/// The row menu on an installed plugin; `{n}` is its name.
pub const STORE_PLUGIN_REMOVE: &str = "Remove {n}";
/// A row's second line; `{s}` the summary, `{p}` the permissions, `{a}` the author.
pub const STORE_PLUGIN_ASKS_NOTHING: &str = "{s}. Asks for nothing.";
pub const STORE_PLUGIN_ASKS_TO: &str = "{s}. Asks to: {p}.";
pub const STORE_PLUGIN_BY: &str = "By {a}. {s}";
/// What an allowed plugin's new version asks for beyond its grant; `{s}` the summary, `{p}` what is new.
pub const STORE_PLUGIN_ASKS_MORE: &str = "{s}. Asks for more now: {p}.";
/// Added to an installed plugin's line when its newer version needs a newer WolOS: `{s}` the line,
/// `{v}` the version held back, `{m}` the WolOS version it needs.
pub const STORE_PLUGIN_HELD: &str = "{s} Version {v} needs WolOS {m}.";

/// Notification offers (`shell::offers`): the button labels.
pub const OFFER_COPY_CODE: &str = "Copy Code";
pub const OFFER_OPEN_LINK: &str = "Open Link";
pub const OFFER_COPY_NUMBER: &str = "Copy Number";
/// The button's label once its copy is on the clipboard.
pub const OFFER_COPIED: &str = "Copied";
/// The button's label when nothing reached the clipboard.
pub const OFFER_NOT_COPIED: &str = "Not Copied";
pub const OFFER_CODE_TIP: &str =
    "Copies the code. Clipboard history skips it, and it is cleared after a minute.";
pub const OFFER_NUMBER_TIP: &str = "Copies the number so you can paste it into a call.";

/// An offer button's label. A note with several codes names each one ("Copy 482913").
pub fn offer_label(o: &crate::offers::Offer, several_codes: bool) -> String {
    use crate::offers::Offer;
    match o {
        Offer::Code(c) if several_codes => format!("Copy {c}"),
        Offer::Code(_) => OFFER_COPY_CODE.into(),
        Offer::Link(_) => OFFER_OPEN_LINK.into(),
        Offer::Phone(_) => OFFER_COPY_NUMBER.into(),
    }
}

/// An offer button's tooltip: what the click does.
pub fn offer_tip(o: &crate::offers::Offer) -> String {
    use crate::offers::{Offer, host};
    match o {
        Offer::Code(_) => OFFER_CODE_TIP.into(),
        Offer::Link(u) => format!("Opens “{}” in your browser.", host(u)),
        Offer::Phone(_) => OFFER_NUMBER_TIP.into(),
    }
}

// Sessions (ADR 0032, crate::sessions): the one question at login.

/// Title of the login offer to reopen last time's windows.
pub fn sessions_offer(n: usize) -> String {
    if n == 1 {
        "Reopen 1 window from last time?".into()
    } else {
        format!("Reopen {n} windows from last time?")
    }
}

/// Body of the offer: what reopening does.
pub const SESSIONS_OFFER_BODY: &str =
    "Your terminals and coding agents open again on their desktops, in their folders.";
/// The offer's button.
pub const SESSIONS_REOPEN: &str = "Reopen";

// ---- Theme picker (`wol shell toggle themes`, ui/theme_picker.rs) ----

/// The picker's title.
pub const THEMES_TITLE: &str = "Themes";
/// The heading over the themes that enabled plugins bring.
pub const THEMES_FROM_PLUGINS: &str = "From plugins";

/// A plugin theme's row: its name and the plugin it comes from.
pub fn theme_from_plugin(theme: &str, plugin: &str) -> String {
    format!("{theme} (from {plugin})")
}

// ---- Shell plugins (ADR 0031 W2, ui/plugins.rs) ----

/// `wol.open` of a plugin found no app for the file, folder or link.
pub const PLUGIN_OPEN_FAILED: &str = "No app here opens this.";

#[cfg(test)]
mod tests {
    /// Audit F10: a download the system refused is said as such, not as "did not finish".
    #[test]
    fn a_refused_signature_update_is_said() {
        let f = wol_core::malware::Full {
            on: true,
            version: 28173,
            failed: false,
            refused: Some("the publisher's signature does not verify".into()),
            ..Default::default()
        };
        assert_eq!(
            super::malware_full(&f),
            "Signature update refused: the publisher's signature does not verify. The previous \
             signatures stay in use."
        );
        let downloading = wol_core::malware::Full {
            updating: true,
            ..f.clone()
        };
        assert_eq!(super::malware_full(&downloading), "Downloading signatures…");
        let fine = wol_core::malware::Full { refused: None, ..f };
        assert!(super::malware_full(&fine).starts_with("0.0 million signatures"));
    }

    use super::*;

    /// R2A-19: the Dock, Desktop & Dock and widget sentences pass the lint.
    #[test]
    fn dock_and_widget_sentences_pass_the_lint() {
        for s in [
            DOCK_DOWNLOADS_EMPTY,
            DOCK_TILE_HINT,
            DOCK_TILING_SHORTCUTS_HINT,
            DOCK_ADD_APP_EMPTY,
            WIDGET_NO_ROOM,
            TIMER_BREAK.1,
            TIMER_BREAK_OVER.1,
            TIMER_DONE.1,
        ] {
            assert!(lint(s).is_empty(), "{s}: {:?}", lint(s));
        }
    }

    #[test]
    fn login_and_power_sentences_pass_the_lint() {
        use shared::login::Power;
        let mut all = vec![LOGIN_NO_ACCOUNTS.1];
        all.extend([Power::Sleep, Power::Restart, Power::ShutDown].map(power_refused));
        for s in all {
            assert!(lint(s).is_empty(), "{s}: {:?}", lint(s));
        }
    }

    #[test]
    fn an_unchecked_file_reads_differently_from_a_finding() {
        let (title, body) = malware_unchecked("setup.AppImage");
        let (found_title, found_body) = malware_found("setup.AppImage", "Eicar-Test");
        assert_ne!(title, found_title);
        assert!(!body.contains("malware"), "{body}");
        assert!(body.contains("again in a moment"), "{body}");
        assert!(lint(&body).is_empty(), "{body}: {:?}", lint(&body));
        assert!(lint(&found_body).is_empty());
        let long = "a".repeat(80);
        assert!(malware_unchecked(&long).1.contains(&"a".repeat(48)));
        assert!(!malware_unchecked(&long).1.contains(&"a".repeat(49)));
    }
}
