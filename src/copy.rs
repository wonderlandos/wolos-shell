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

/// Fragments a user must never read (case-insensitive, at a word start), and what to do instead.
pub const BANNED: [(&str, &str); 37] = [
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
    ("hyprland", "say the desktop"),
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

/// Everything wrong with `s` as an error sentence (empty = fine).
pub fn lint(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let lower = s.to_lowercase();
    for (pat, why) in BANNED {
        let at_word = lower.match_indices(pat).any(|(i, _)| {
            !lower[..i]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric)
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
    "This disk is empty, so there's nothing to install alongside. Use Erase Disk.";
pub const INSTALL_RECOVERY_NOTE: &str = "Write this key down and keep it safe. It opens your disk if your password or security chip stops working.";
pub const INSTALL_POINT_OF_NO_RETURN: &str =
    "Back up anything you need first. These changes can't be undone.";

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
            "Windows didn't shut down fully. Turn off Fast Startup in Windows, shut down, then try again."
        }
        Blocked::MacOs => "Make room with Disk Utility in macOS, then start the installer again.",
        Blocked::Unreadable => {
            "This partition couldn't be read. Check it in its own system, then try again."
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
            format!("{} can't get that small. Choose a larger size.", named(id))
        }
        Refusal::NoRoom => {
            "There isn't enough room for that. Delete or shrink a partition first.".into()
        }
        Refusal::NoRoot => "Choose a partition for the WolOS system.".into(),
        Refusal::NoEsp => "Choose the startup partition, or add one of 1 GB.".into(),
        Refusal::RootMustFormat => {
            "The WolOS system needs an erased partition. Turn on Erase for it.".into()
        }
        Refusal::EspInUse => {
            "Another system starts from that partition, so it can't be erased.".into()
        }
        Refusal::NotAnEsp(id) => format!(
            "{} can't hold the startup files. Choose another or add a new one.",
            named(id)
        ),
        Refusal::BadMount(_) => {
            "That folder name can't be used. Use lowercase letters, and not a system folder.".into()
        }
        Refusal::DuplicateMount(_) => "Two partitions have the same job. Give each its own.".into(),
        Refusal::BadFilesystem(id) => format!(
            "{} can't do that job as it is. Turn on Erase or choose another job.",
            named(id)
        ),
    }
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
        Change::Format { name, mount } => {
            format!("Erase {name} and use it for {}.", install_for(mount))
        }
        Change::New {
            bytes,
            mount,
            encrypted,
        } => {
            let lock = if *encrypted { ", encrypted" } else { "" };
            format!("Add {} for {}{lock}.", size(*bytes), install_for(mount))
        }
        Change::ReuseEsp => "Add WolOS to the startup partition that's already there.".into(),
        Change::Keep { name, mount } => {
            format!("Keep {name} as it is, for {}.", install_for(mount))
        }
        Change::Untouched { name } => format!("{name} stays as it is."),
        Change::OneCopy => {
            "This layout keeps one copy of WolOS, so updates can't be undone.".into()
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
        ActionError::Unavailable => "This isn't available on this computer.",
        ActionError::NeedsAuth => "Enter an administrator's password to make this change.",
        ActionError::Denied => "Incorrect password. Try again.",
        ActionError::NotAdmin => "Only an administrator can make this change.",
        ActionError::Invalid | ActionError::Failed => "The change did not work. Try again.",
        ActionError::WrongPassword => {
            "The network didn't accept that password. Check it and try again."
        }
    }
}

/// Joining the Wi-Fi network `name`. Denied = the administrator sheet was cancelled.
pub fn wifi_join(e: ActionError, name: &str) -> String {
    match e {
        ActionError::WrongPassword => {
            format!("“{name}” didn't accept that password. Check it and try again.")
        }
        ActionError::Invalid => {
            "Wi-Fi passwords have 8 to 63 characters. Check it and try again.".into()
        }
        ActionError::Denied | ActionError::NeedsAuth => {
            format!("Not joined. Joining “{name}” needs an administrator's password.")
        }
        ActionError::NotAdmin => format!("Only an administrator can join “{name}”."),
        ActionError::Unavailable => "Wi-Fi isn't available on this computer.".into(),
        ActionError::Failed => format!("Couldn't join “{name}”. Move closer to it and try again."),
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
            format!("{name} wasn't removed. Removing apps needs an administrator's password.")
        }
        ActionError::NotAdmin => format!("Only an administrator can remove {name}."),
        ActionError::Unavailable => {
            format!("{name} can't be removed right now. Restart the computer and try again.")
        }
        ActionError::Invalid | ActionError::Failed | ActionError::WrongPassword => {
            format!("{name} couldn't be removed. Quit it, then try again.")
        }
    }
}

pub fn install(e: ActionError, name: &str) -> String {
    match e {
        ActionError::Denied | ActionError::NeedsAuth => {
            format!("{name} wasn't installed. Installing apps needs an administrator's password.")
        }
        ActionError::NotAdmin => {
            format!("Only an administrator can install {name}. Ask one to sign in.")
        }
        ActionError::Unavailable => {
            format!("{name} can't be installed right now. Restart the computer and try again.")
        }
        ActionError::Invalid | ActionError::Failed | ActionError::WrongPassword => {
            format!("{name} couldn't be installed. Check that you're online, then try again.")
        }
    }
}

/// Checking for updates found no answer.
pub const UPDATE_CHECK: &str =
    "Couldn't check for updates. Check that you're online, then try again.";

/// Installing updates.
pub fn update(e: ActionError) -> &'static str {
    match e {
        ActionError::Invalid | ActionError::Failed | ActionError::WrongPassword => {
            "The update didn't finish. Check that you're online, then try again."
        }
        e => action(e),
    }
}

/// Undoing a settings change (Settings > Restore, the Undo button).
pub fn undo(e: &UndoError) -> &'static str {
    match e {
        UndoError::Nothing => "There's nothing to undo.",
        UndoError::NotFound(_) => "That change is no longer in the history.",
        UndoError::Changed(_) => "This was changed again later. Undo the newer change first.",
        UndoError::Refused(_) => "This change can't be undone.",
        UndoError::Io(_) => "The change couldn't be undone. Try again.",
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
        E::Failed { .. } | E::TooLarge | E::BadJson(_) => {
            format!("{name} couldn't get its data. It tries again soon.")
        }
        E::NeedsConsent(host) => format!("Allow {name} to read data from {host}."),
        E::Offline(_) => format!("{name} can't reach the internet. It updates when you're online."),
    }
}

/// A widget package whose description has mistakes (each listed under Show Details).
pub fn widget_problems(name: &str) -> String {
    format!("{name} can't be shown. Its developer can fix it with the details.")
}

/// A widget on the desktop whose package is gone.
pub const WIDGET_MISSING: &str = "This widget isn't installed. Reinstall it, or remove it.";
/// A Lua widget file that does not load or whose script stopped (the reason is in the details).
pub const LUA_WIDGET_FAILED: &str = "This widget's script stopped. Fix the file and it reloads.";
/// A widget kind from a newer WolOS.
pub const WIDGET_NEWER: &str = "This widget needs a newer version of WolOS.";
/// The Weather city search got no answer.
pub const CITY_SEARCH: &str =
    "Couldn't search for cities. Check that you're online, then try again.";
/// Weather with nothing cached and no network.
pub const WEATHER_OFFLINE: &str = "No forecast right now. It updates when you're back online.";

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
        format!("Only {free} left. Empty the Trash or remove files you don't need."),
    )
}

/// Files stopped by the malware check: summary, body. Says what was found and what to do.
pub fn malware_found(file: &str, name: &str) -> (String, String) {
    let shown: String = file.chars().take(48).collect();
    (
        "Malware Found".into(),
        format!("“{shown}” matches known malware ({name}). Move it to the Trash."),
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
pub const MALWARE_FULL_NOTE: &str = "Downloads about 110 MB from the ClamAV project, then a small \
     update each day. While it checks a download it can use about 1 GB of memory.";

/// What the check does, and what it cannot do.
pub const MALWARE_SCOPE: &str = "Checks downloads, new programs and removable disks against \
     known malware, on this computer. New malware can get past it, so stay careful with files you \
     did not expect.";

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

/// Restart, Shut Down and Log Out ask first (macOS): question, countdown line, button.
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
