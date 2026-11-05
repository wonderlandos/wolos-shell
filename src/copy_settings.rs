// SPDX-License-Identifier: GPL-2.0-or-later
//! System Settings' sentences (STANDARDS §1, audit3 R2A-19): the panes rebuilt in v0.12 (Sound,
//! Modules, Startup Security, search) hold none of their own; they pick one from here by a fact.
//! Invariant: every sentence passes `copy::lint` ([`ALL`], tested below).

// ---- Sound ----

/// The sound service is (re)starting: the device lists are empty for a moment.
pub const SOUND_RECONNECTING: &str = "Sound devices are coming back. This takes a few seconds.";
/// "Allow louder" subtitle.
pub const SOUND_LOUDER: &str = "Lets volume sliders pass 100%. Sound above 100% can distort.";
/// "Automatic input level" subtitle.
pub const SOUND_AUTO_LEVEL: &str = "Keeps your voice at an even level on calls and recordings.";
/// "Reduce background noise" subtitle.
pub const SOUND_NOISE: &str = "Filters out fans, typing and traffic behind your voice.";
/// Bluetooth autoswitch subtitle.
pub const SOUND_AUTOSWITCH: &str = "Headphones use their microphone while an app records.";
/// "Keep device awake" subtitle.
pub const SOUND_AWAKE: &str = "Stops a click or a lost first second when sound starts.";
/// "Ignore hardware volume" subtitle.
pub const SOUND_SOFT_VOLUME: &str = "For volume that jumps or does nothing on this output.";
/// "Reset audio" subtitle.
pub const SOUND_RESET_ABOUT: &str = "Forgets stored volumes and devices, then restarts sound.";
/// After Reset audio.
pub const SOUND_RESET_DONE: &str = "Sound settings were reset. Devices start at their usual level.";
/// "Copy diagnostics" put the report on the clipboard.
pub const SOUND_DIAG_COPIED: &str =
    "Sound diagnostics copied. Paste them into a report or a message.";
/// "Copy diagnostics" had nothing to copy.
pub const SOUND_DIAG_NONE: &str = "Sound diagnostics couldn’t be collected. Try again in a moment.";
/// Pro Audio footnote.
pub const SOUND_PRO_NOTE: &str =
    "Smaller buffers lower the delay but can crackle. Dropouts count the moments sound ran out.";
/// A sound change did not take.
pub const SOUND_NOT_SAVED: &str =
    "This sound setting couldn’t be changed. Try again, or use Reset Audio below.";
/// No microphone is connected.
pub const SOUND_NO_INPUT: &str = "No microphone is connected.";
/// A Troubleshooting switch was saved, but sound did not restart to use it.
pub const SOUND_SAVED_LATER: &str = "Saved. It takes effect after you log out and back in.";

// ---- Modules ----

/// Modules pane footnote.
pub const MODULES_ABOUT: &str =
    "Modules add tools not everyone needs. Installing one asks for an administrator’s password.";
/// The registry was read and lists nothing.
pub const MODULES_NONE: &str = "No modules are available on this computer right now.";
/// The registry could not be read (`wol` missing or its answer not understood).
pub const MODULES_UNREADABLE: &str =
    "The list of modules couldn’t be read. Try again after the next update.";
/// `wol modules enable` did not finish.
pub const MODULES_ENABLE_NOT_DONE: &str =
    "That module couldn’t finish setting up. Check that you’re online, then try again.";
/// A module the installer queued.
pub const MODULES_PENDING: &str = "Installs when this computer next starts online.";
/// A module modules-install gave up on (modules-gave-up).
pub const MODULES_GAVE_UP: &str = "This module couldn’t be downloaded after several tries. Check that you’re online, then try again.";
/// A media format package it gave up on.
pub const MODULES_GAVE_UP_MEDIA: &str = "This media format couldn’t be downloaded after several tries. Check that you’re online, then try again.";

// ---- Startup Security ----

/// Startup Security footnote.
pub const STARTUP_ABOUT: &str =
    "Secure Boot lets this computer start only WolOS releases signed for it.";
/// The key screen still has to come.
pub const STARTUP_KEY_QUEUED: &str =
    "This computer’s key is queued: a blue screen asks once at the next restart.";
/// Attested gaming placeholder (docs/research/attested-gaming-mode.md).
pub const STARTUP_ATTESTED: &str = "Attested gaming lets games with kernel anti-cheat check this \
     computer runs WolOS as released. It isn’t available yet.";

// ---- Privacy & Security ----

/// Screen Recording row: not an on/off grant (`wol_core::consent::SCREEN_CAPTURE`).
pub const PRIVACY_SCREEN: &str =
    "Apps ask each time they share your screen. You choose which screen or window.";
/// Privacy footnote, before what WolOS does with the grants listed: how the screen is guarded.
pub const PRIVACY_SCREEN_NOTE: &str = "Only WolOS’s own screenshot and recording tools can copy \
     the screen without asking. Every other app has to ask, each time.";
/// No app has a grant yet, and none asked.
pub const PRIVACY_NONE: &str =
    "Apps that ask to send notifications, control input or install apps are listed here.";

// ---- Users & Groups ----

/// Add User: the name belongs to the system (`wol_core::settings::users::is_reserved`).
pub const ACCOUNT_RESERVED: &str = "That name is reserved by the system. Choose another.";

// ---- Appearance ----

/// "Theme from your wallpaper" subtitle.
pub const THEME_FROM_WALLPAPER: &str =
    "Colors taken from the desktop picture, readable everywhere.";
/// The desktop picture could not be decoded for a theme.
pub const THEME_PICTURE_UNREADABLE: &str =
    "The desktop picture couldn’t be read. Choose another in Wallpaper.";

// ---- Software Update ----

/// Software Update > Download on metered connections: what counts as metered.
pub const AUTO_METERED_HINT: &str = "Phone hotspots and plans with a data cap count as metered.";
/// Privacy > Send error reports: what goes out, and only that (files/usr/lib/wolos/report).
pub const REPORTS_HINT: &str = "When a program or service fails, WolOS sends its name and the system version. No files or names.";
/// Software Update > Install app updates: the timer change did not take.
pub const AUTO_APPS_FAILED: &str =
    "App updates couldn’t be turned on or off. Try again in a moment.";
/// Under "N updates available" on a computer installed with one copy of WolOS (ADR 0023):
/// Update Now changes the running system, with no restore point to go back to.
pub const UPDATE_ONE_COPY: &str = "This computer has one copy of WolOS, so there is no restore point; the update changes the running system.";
/// Restore: the only list there can be on a computer with one copy of WolOS (QA-STORE-5).
pub const RESTORE_ONE_COPY: &str =
    "This computer has one copy of WolOS, so there is no earlier system to go back to.";
/// Check Now could not write what it found: the disk is full (QA-FDAY F10).
pub const UPDATE_NO_ROOM: &str = "There is no room left on the system disk to check for updates. Free some space in Storage, then try again.";
/// Software Update before any check ever finished (QA-FDAY-4).
pub const UPDATE_NEVER_CHECKED: &str = "Check for new WolOS packages. It has not checked yet.";
/// Under an answer Check Now just got (QA-FDAY-4).
pub const UPDATE_CHECKED_NOW: &str = "Last checked just now.";

// ---- Displays ----

/// Empty state's title: the list of displays could not be read (the page is open, so the desktop
/// is running: the old "appear once the desktop is running" blamed the wrong thing, QA-DESIGN-28).
pub const DISPLAYS_NONE_TITLE: &str = "Displays couldn’t be read";
/// Empty state's hint.
pub const DISPLAYS_NONE: &str = "WolOS couldn’t read the list of displays. Try again; if it keeps happening, restart the computer.";
/// Empty state's button.
pub const DISPLAYS_NONE_RETRY: &str = "Try Again";
/// The countdown sheet's title after a changed resolution, refresh rate or scale (QA-LIVE-13).
pub const DISPLAYS_KEEP_TITLE: &str = "Keep these display settings?";
/// The countdown sheet's line.
pub fn displays_keep_line(secs: u32) -> String {
    let unit = if secs == 1 { "second" } else { "seconds" };
    format!("If you don’t answer, the display goes back in {secs} {unit}.")
}
/// After the display went back.
pub const DISPLAYS_REVERTED: &str = "The display went back to its previous settings.";
/// The display could not be put back.
pub const DISPLAYS_REVERT_FAILED: &str =
    "The display couldn’t be put back. Choose its earlier resolution again.";

// ---- Search ----

/// Search found nothing: the empty state's title (a title, not a sentence: not in [`ALL`]) and its hint.
pub const SEARCH_NONE_TITLE: &str = "No Results";
pub const SEARCH_NONE: &str = "No settings match. Try another word.";

// ---- Bluetooth ----

/// Under the switch before the first read (a progress line ending in …, not a sentence: not in [`ALL`]).
pub const BLUETOOTH_LOOKING: &str = "Looking for Bluetooth…";
/// An adapter, but bluetoothd is not running: the switch offers Turn On.
pub const BLUETOOTH_DOWN: &str = "Bluetooth isn’t running. Turn it on to use wireless devices.";
/// Add Device… is looking around (a progress line ending in …, not a sentence: not in [`ALL`]).
pub const BLUETOOTH_SEARCHING: &str = "Searching for devices…";
/// The search found nothing new.
pub const BLUETOOTH_NONE_FOUND: &str =
    "No new devices found. Put the device in pairing mode, bring it close, and search again.";
/// The agent asks for a PIN (older keyboards and headsets).
pub const BLUETOOTH_PIN_HINT: &str =
    "Type the PIN for this device. Keyboards and headsets often use 0000 or 1234.";
/// The agent asks for a passkey.
pub const BLUETOOTH_PASSKEY_HINT: &str = "Type the code the device shows.";
/// The agent shows a code to compare.
pub const BLUETOOTH_CONFIRM_HINT: &str = "Check that the device shows the same code.";
/// The agent asks to accept without a code.
pub const BLUETOOTH_CONFIRM_BARE: &str = "Confirm the pairing on the device if it asks.";
/// The device wants a code typed on it.
pub const BLUETOOTH_SHOW_HINT: &str = "Type this code on the device, then press Return there.";
/// Pairing started, nothing asked yet (a progress line ending in …, not a sentence: not in [`ALL`]).
pub const BLUETOOTH_WAITING: &str = "Waiting for the device…";
/// Forget asks first.
pub const BLUETOOTH_FORGET_BODY: &str = "You will need to pair it again to use it.";
/// After Forget.
pub const BLUETOOTH_FORGOT: &str = "The device was forgotten.";
/// Forget did not work.
pub const BLUETOOTH_FORGET_FAILED: &str = "Couldn’t forget the device. Try again.";
/// No bluetoothctl in this build.
pub const BLUETOOTH_NO_CLIENT: &str = "Bluetooth isn’t available on this computer.";
/// The person closed the pairing sheet.
pub const BLUETOOTH_CANCELLED: &str = "Pairing was canceled.";
/// My Devices with nothing paired yet.
pub const BLUETOOTH_EMPTY_HINT: &str = "Put a device in pairing mode to connect it the first time.";
/// The empty state of a computer with no adapter.
pub const BLUETOOTH_NO_ADAPTER: &str =
    "This computer has no Bluetooth adapter. Plug in a USB adapter to use wireless devices.";
/// Another pairing or Forget is still open.
pub const BLUETOOTH_BUSY: &str = "Finish or cancel the open Bluetooth sheet first.";

/// The pairing sheet's question.
pub fn bluetooth_pair_title(name: &str) -> String {
    format!("Pair with “{name}”?")
}
/// The sheet asking for a PIN.
pub fn bluetooth_pin_title(name: &str) -> String {
    format!("PIN for “{name}”")
}
/// The sheet showing a code to type on the device.
pub fn bluetooth_show_title(name: &str) -> String {
    format!("Pairing with “{name}”")
}
/// The Forget question.
pub fn bluetooth_forget_title(name: &str) -> String {
    format!("Forget “{name}”?")
}
/// After a pairing.
pub fn bluetooth_paired(name: &str, connected: bool) -> String {
    if connected {
        format!("“{name}” is paired and connected.")
    } else {
        format!("“{name}” is paired.")
    }
}
/// Under the switch: the name other devices see.
pub fn bluetooth_appears_as(host: &str) -> String {
    format!("This computer appears as “{host}”.")
}
/// The Bluetooth sentences built from a name, for the lint (the sheet's titles are questions
/// and labels, not sentences).
pub fn bluetooth_lines(name: &str) -> Vec<String> {
    vec![
        bluetooth_paired(name, true),
        bluetooth_paired(name, false),
        bluetooth_appears_as(name),
    ]
}

/// Every sentence above, for the lint.
pub const ALL: [&str; 62] = [
    MEMSAVER_ABOUT,
    MEMSAVER_NAP,
    AUTO_METERED_HINT,
    REPORTS_HINT,
    BLUETOOTH_EMPTY_HINT,
    BLUETOOTH_NO_ADAPTER,
    BLUETOOTH_DOWN,
    BLUETOOTH_NONE_FOUND,
    BLUETOOTH_PIN_HINT,
    BLUETOOTH_PASSKEY_HINT,
    BLUETOOTH_CONFIRM_HINT,
    BLUETOOTH_CONFIRM_BARE,
    BLUETOOTH_SHOW_HINT,
    BLUETOOTH_FORGET_BODY,
    BLUETOOTH_FORGOT,
    BLUETOOTH_FORGET_FAILED,
    BLUETOOTH_NO_CLIENT,
    BLUETOOTH_CANCELLED,
    BLUETOOTH_BUSY,
    SOUND_RECONNECTING,
    SOUND_LOUDER,
    SOUND_AUTO_LEVEL,
    SOUND_NOISE,
    SOUND_AUTOSWITCH,
    SOUND_AWAKE,
    SOUND_SOFT_VOLUME,
    SOUND_RESET_ABOUT,
    SOUND_RESET_DONE,
    SOUND_DIAG_COPIED,
    SOUND_DIAG_NONE,
    SOUND_PRO_NOTE,
    SOUND_NOT_SAVED,
    SOUND_NO_INPUT,
    SOUND_SAVED_LATER,
    MODULES_ABOUT,
    MODULES_NONE,
    MODULES_UNREADABLE,
    MODULES_ENABLE_NOT_DONE,
    MODULES_PENDING,
    MODULES_GAVE_UP,
    MODULES_GAVE_UP_MEDIA,
    STARTUP_ABOUT,
    STARTUP_KEY_QUEUED,
    STARTUP_ATTESTED,
    PRIVACY_SCREEN,
    PRIVACY_SCREEN_NOTE,
    PRIVACY_NONE,
    ACCOUNT_RESERVED,
    THEME_FROM_WALLPAPER,
    THEME_PICTURE_UNREADABLE,
    AUTO_APPS_FAILED,
    DISPLAYS_NONE,
    DISPLAYS_REVERTED,
    DISPLAYS_REVERT_FAILED,
    SEARCH_NONE,
    UPDATE_ONE_COPY,
    RESTORE_ONE_COPY,
    UPDATE_NO_ROOM,
    UPDATE_NEVER_CHECKED,
    UPDATE_CHECKED_NOW,
    GENERAL_TERMINAL_SHELL,
    GENERAL_TERMINAL_SHELL_NOT_SAVED,
];

// ---- Memory Saver ----

/// The Memory Saver switch, in one plain sentence for the three things it turns on together: a
/// background app gives its memory back (and an app with a budget, every 30 s while behind,
/// `api::memsaver::engine::PAGEOUT_EVERY_MS`), an app's processes share identical memory (KSM,
/// `wolos-mm exec-merge`), and Discord starts held to its budget (`memsaver::budget`).
pub const MEMSAVER_ABOUT: &str = "Apps you aren’t using give their memory back, apps share identical memory, and Discord keeps to a limit.";
/// "App Nap" subtitle.
pub const MEMSAVER_NAP: &str = "Pauses apps hidden for 10 minutes that aren’t playing sound. They may show notifications late.";

// ---- General ----

/// "Shell in the terminal" subtitle (`wol_core::terminal_shell`).
pub const GENERAL_TERMINAL_SHELL: &str =
    "New terminal windows open in it. You still log in with zsh.";
/// The choice could not be saved: the menu shows the saved shell again.
pub const GENERAL_TERMINAL_SHELL_NOT_SAVED: &str =
    "The shell couldn’t be changed. Try again in a moment.";

// ---- Network, Wi-Fi and sharing (B7) ----

/// An 802.1X network (Enterprise Wi-Fi): no password sheet (QA-NET-9).
pub const WIFI_ENTERPRISE: &str = "This network asks for a user name or a certificate (Enterprise Wi-Fi). WolOS can’t join those yet.";
/// The saved networks could not be read: no Wi-Fi device yet, NetworkManager silent (QA-SHADOW-55).
pub const WIFI_SAVED_UNREAD: &str = "The saved networks couldn’t be read. Check that Wi-Fi is on.";
/// Disconnect did not work (QA-NET-17).
pub const WIFI_DISCONNECT_FAILED: &str =
    "Couldn’t leave this network. Try again, or turn Wi-Fi off.";
/// A join that worked.
pub fn wifi_connected(name: &str) -> String {
    format!("Connected to “{name}”.")
}
/// The row under the Control Center list when more networks are in range (a label, QA-NET-15).
pub fn wifi_more_networks(n: usize) -> String {
    format!("{n} More Networks…")
}
/// Under Internet Sharing (the Wi-Fi hotspot, NET2-2), while it is on.
pub fn hotspot_on(name: &str) -> String {
    format!(
        "Other devices join “{name}” with the password you chose. They use this computer’s internet."
    )
}
/// Under Internet Sharing while it is off.
pub const HOTSPOT_OFF: &str =
    "Share this computer’s internet with phones and other computers over Wi-Fi.";
/// No Wi-Fi device: the hotspot cannot start.
pub const HOTSPOT_NO_WIFI: &str = "Internet Sharing needs a Wi-Fi device.";
/// The hotspot did not start.
pub const HOTSPOT_FAILED: &str =
    "The hotspot didn’t start. Check that Wi-Fi is on, then try again.";
/// The hotspot password is too short or too long.
pub const HOTSPOT_PASSWORD: &str = "Use 8 to 63 characters for the hotspot password.";
/// Sharing rows on the live medium: its account has no password to give (QA-NET-6, NET2-8).
pub const SHARING_LIVE: &str =
    "Install WolOS to change these. The live system’s account has no password to allow them.";
/// A sign-in page holds the network (NET2-3): the notification's title (a label).
pub const PORTAL_TITLE: &str = "Sign In to This Network";
/// The notification's body.
pub const PORTAL_BODY: &str =
    "This network shows a sign-in page before it lets you on the internet. Open it to sign in.";
/// Network pane: the link reaches no internet (limited).
pub const NETWORK_LIMITED: &str = "Connected, but the internet can’t be reached through it.";
/// Network pane: a sign-in page holds the link.
pub const NETWORK_PORTAL: &str = "Connected. Sign in on the network’s page to use the internet.";

/// Network > Firewall while the kernel has the table (or cannot tell).
pub const FIREWALL_ON: &str = "Blocks connections from other computers that you did not start.";
/// Network > Firewall: on at startup, but the kernel has no table (QA-NET-2).
pub const FIREWALL_STOPPED: &str = "On at startup, but not running now.";

/// Every network sentence, for the lint.
pub const NETWORK_ALL: [&str; 13] = [
    FIREWALL_ON,
    FIREWALL_STOPPED,
    WIFI_ENTERPRISE,
    WIFI_SAVED_UNREAD,
    WIFI_DISCONNECT_FAILED,
    HOTSPOT_OFF,
    HOTSPOT_NO_WIFI,
    HOTSPOT_FAILED,
    HOTSPOT_PASSWORD,
    SHARING_LIVE,
    PORTAL_BODY,
    NETWORK_LIMITED,
    NETWORK_PORTAL,
];

#[cfg(test)]
mod tests {
    #[test]
    fn network_sentences_pass_the_lint() {
        for s in super::NETWORK_ALL
            .iter()
            .copied()
            .map(String::from)
            .chain([super::wifi_connected("Home"), super::hotspot_on("WolOS")])
        {
            let p = crate::copy::lint(&s);
            assert!(p.is_empty(), "“{s}”: {p:?}");
        }
    }

    #[test]
    fn every_settings_sentence_passes_the_lint() {
        for s in super::ALL
            .iter()
            .copied()
            .map(String::from)
            .chain([super::displays_keep_line(1), super::displays_keep_line(15)])
            .chain(super::bluetooth_lines("Magic Keyboard"))
        {
            let s = s.as_str();
            let p = crate::copy::lint(s);
            assert!(p.is_empty(), "“{s}”: {p:?}");
        }
    }
}
