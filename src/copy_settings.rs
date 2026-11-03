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
/// No app has a grant yet.
pub const PRIVACY_NONE: &str =
    "Apps you allow to send notifications, control input or install apps are listed here.";

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

/// Software Update > Install app updates: the timer change did not take.
pub const AUTO_APPS_FAILED: &str =
    "App updates couldn’t be turned on or off. Try again in a moment.";

// ---- Displays ----

/// No display was reported (the desktop session is not running).
pub const DISPLAYS_NONE: &str = "Displays appear here once the desktop is running.";

// ---- Search ----

/// Search found nothing: the empty state's title (a title, not a sentence: not in [`ALL`]) and its hint.
pub const SEARCH_NONE_TITLE: &str = "No Results";
pub const SEARCH_NONE: &str = "No settings match. Try another word.";

/// Every sentence above, for the lint.
pub const ALL: [&str; 34] = [
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
    SEARCH_NONE,
];

#[cfg(test)]
mod tests {
    #[test]
    fn every_settings_sentence_passes_the_lint() {
        for s in super::ALL {
            let p = crate::copy::lint(s);
            assert!(p.is_empty(), "“{s}”: {p:?}");
        }
    }
}
