// SPDX-License-Identifier: GPL-2.0-or-later
//! The Control Center's labels (ADR 0045, migration step 3): tile titles, state lines, page and
//! list headings, buttons and the empty states. Each constant is the English source of a key
//! `cc.<name>` in `locale/shell/en.tsv`; the panel reads it through `i18n::t`. `{name}` marks a
//! value filled in at run time. Wi-Fi, Bluetooth and network names are terms or data and stay in
//! the code. Strings built elsewhere (`system::wifi_subtitle`, `focus_label`) are not here yet.

/// Title of the sheet that asks for an administrator's password.
/// budget: 23
pub const CONTROL_CENTER: &str = "Control Center";
/// State line of a tile or switch that is off.
/// budget: 8
pub const OFF: &str = "Off";
/// State line of a tile or switch that is on.
/// budget: 8
pub const ON: &str = "On";
/// State line of a tile whose hardware is missing.
/// budget: 18
pub const UNAVAILABLE: &str = "Unavailable";
/// Tile and page title for Do Not Disturb.
/// budget: 8
pub const FOCUS: &str = "Focus";
/// Tile title: switches the whole look to dark.
/// budget: 15
pub const DARK_MODE: &str = "Dark Mode";
/// Slider title and page title for the screen's brightness.
/// budget: 12
pub const DISPLAY: &str = "Display";
/// Slider title and page title for the volume.
/// budget: 8
pub const SOUND: &str = "Sound";
/// Detail button of the volume slider and heading of the output list.
/// budget: 10
pub const OUTPUT: &str = "Output";
/// Heading of the microphone list.
/// budget: 8
pub const INPUT: &str = "Input";
/// Tile title: show this screen on another display.
/// budget: 26
pub const SCREEN_MIRRORING: &str = "Screen Mirroring";
/// Tile title: backlight of the keyboard.
/// budget: 31
pub const KEYBOARD_BRIGHTNESS: &str = "Keyboard Brightness";
/// Tile title: warmer colors in the evening.
/// budget: 18
pub const NIGHT_SHIFT: &str = "Night Shift";
/// Tile title: opens the capture bar.
/// budget: 16
pub const SCREENSHOT: &str = "Screenshot";
/// Tile title: opens the capture bar in recording mode.
/// budget: 10
pub const RECORD: &str = "Record";
/// Tile title for the accessibility switch.
/// budget: 21
pub const REDUCE_MOTION: &str = "Reduce Motion";
/// Tile title for the accessibility switch.
/// budget: 28
pub const INCREASE_CONTRAST: &str = "Increase Contrast";
/// Tile title for the accessibility switch.
/// budget: 8
pub const ZOOM: &str = "Zoom";
/// Row and state line when notifications are silenced.
/// budget: 23
pub const DO_NOT_DISTURB: &str = "Do Not Disturb";
/// Row: Do Not Disturb for an hour.
/// budget: 16
pub const FOR_ONE_HOUR: &str = "For 1 hour";
/// Row: Do Not Disturb until the morning.
/// budget: 36
pub const UNTIL_MORNING: &str = "Until tomorrow morning";
/// Link at the bottom of the Display page; opens System Settings.
/// budget: 29
pub const DISPLAYS_SETTINGS: &str = "Displays Settings…";
/// Slider title on the Display page.
/// budget: 16
pub const BRIGHTNESS: &str = "Brightness";
/// Heading of the list of displays that can mirror this one.
/// budget: 15
pub const MIRRORING: &str = "Mirroring";
/// Shown when no second display is connected.
/// budget: 26
pub const NO_OTHER_DISPLAY: &str = "No other display";
/// Title of the password sheet; {name} is the Wi-Fi network.
/// budget: 21
pub const JOIN_NETWORK: &str = "Join “{name}”";
/// Title of the sheet for a hidden network.
/// budget: 29
pub const JOIN_OTHER: &str = "Join Other Network";
/// Field in the Join Other Network sheet.
/// budget: 20
pub const NETWORK_NAME: &str = "Network name";
/// Field in the Join sheet.
/// budget: 13
pub const PASSWORD: &str = "Password";
/// Button of the Join sheet.
/// budget: 8
pub const JOIN: &str = "Join";
/// Button of the Join sheet.
/// budget: 10
pub const CANCEL: &str = "Cancel";
/// Heading of the list of Wi-Fi networks.
/// budget: 13
pub const NETWORKS: &str = "Networks";
/// Link at the bottom of the Wi-Fi page; opens System Settings.
/// budget: 24
pub const WIFI_SETTINGS: &str = "Wi-Fi Settings…";
/// Heading of the list of Bluetooth devices.
/// budget: 12
pub const DEVICES: &str = "Devices";
/// Row: join a network that is not in the list.
/// budget: 23
pub const OTHER_NETWORK: &str = "Other Network…";
/// Shown when Wi-Fi sees no network.
/// budget: 28
pub const NO_NETWORKS: &str = "No networks found";
/// State line of the network or device in use.
/// budget: 15
pub const CONNECTED: &str = "Connected";
/// Shown when the computer has no Bluetooth.
/// budget: 39
pub const BT_UNAVAILABLE: &str = "Bluetooth is unavailable";
/// Shown when no Bluetooth device is paired.
/// budget: 28
pub const NO_PAIRED: &str = "No paired devices";
/// Slider title on the Sound page.
/// budget: 16
pub const MICROPHONE: &str = "Microphone";
/// Link at the bottom of the Sound page; opens System Settings.
/// budget: 24
pub const SOUND_SETTINGS: &str = "Sound Settings…";
/// Slider title for the output on the Sound page.
/// budget: 8
pub const MUTE: &str = "Mute";
/// Slider title for the microphone on the Sound page.
/// budget: 24
pub const MUTE_MICROPHONE: &str = "Mute Microphone";
/// Shown when there is no speaker or headphone.
/// budget: 28
pub const NO_OUTPUTS: &str = "No output devices";
