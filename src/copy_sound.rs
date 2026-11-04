// SPDX-License-Identifier: GPL-2.0-or-later
//! Settings > Sound's labels (ADR 0045, migration step 3): titles, row names, picker choices and
//! the templates of the values it shows. The sentences (subtitles, notes) are in `copy_settings.rs`.
//! Each constant is the English source of a key `sound.<name>` in `locale/shell/en.tsv`; the pane
//! reads it through `i18n::t`. `{name}` marks a value filled in at run time. Codec names and the
//! word Bluetooth are product terms and stay as they are in the code.

/// The pane's title and the name of its sidebar row.
/// budget: 8
pub const TITLE: &str = "Sound";
/// Slider row: volume of the chosen output.
/// budget: 21
pub const OUTPUT_VOLUME: &str = "Output volume";
/// Switch row: silence the output.
/// budget: 8
pub const MUTE: &str = "Mute";
/// Switch row; its subtitle says what it does.
/// budget: 20
pub const ALLOW_LOUDER: &str = "Allow louder";
/// Section title above the list of speakers and headphones.
/// budget: 10
pub const OUTPUT: &str = "Output";
/// Slider row: left and right loudness.
/// budget: 12
pub const BALANCE: &str = "Balance";
/// Left end of the balance slider.
/// budget: 8
pub const LEFT: &str = "Left";
/// Right end of the balance slider.
/// budget: 8
pub const RIGHT: &str = "Right";
/// Section title above the list of microphones.
/// budget: 8
pub const INPUT: &str = "Input";
/// Slider row: volume of the chosen microphone.
/// budget: 20
pub const INPUT_VOLUME: &str = "Input volume";
/// Live meter of what the microphone hears.
/// budget: 18
pub const INPUT_LEVEL: &str = "Input level";
/// Switch row: silence the microphone.
/// budget: 16
pub const MUTE_INPUT: &str = "Mute input";
/// Switch row; its subtitle says what it does.
/// budget: 34
pub const AUTO_INPUT_LEVEL: &str = "Automatic input level";
/// Switch row; its subtitle says what it does.
/// budget: 37
pub const REDUCE_NOISE: &str = "Reduce background noise";
/// Switch row for Bluetooth headsets; its subtitle says what it does.
/// budget: 64
pub const USE_HEADSET_MIC: &str = "Use the headset microphone automatically";
/// Picker row: the audio codec of a Bluetooth device; {device} is its name.
/// budget: 23
pub const CODEC_OF: &str = "{device} codec";
/// Section title for sample rate and buffer size.
/// budget: 15
pub const PRO_AUDIO: &str = "Pro Audio";
/// Picker row: samples per second.
/// budget: 18
pub const SAMPLE_RATE: &str = "Sample rate";
/// Picker row: how much sound is held before it plays.
/// budget: 18
pub const BUFFER_SIZE: &str = "Buffer size";
/// First choice of the sample rate and buffer pickers: WolOS decides.
/// budget: 15
pub const AUTOMATIC: &str = "Automatic";
/// A sample rate in a picker; {rate} is a number such as 44.1.
/// budget: 16
pub const RATE_KHZ: &str = "{rate} kHz";
/// A buffer size in a picker; {samples} is a count and {ms} the delay it adds.
/// budget: 44
pub const BUFFER_SAMPLES: &str = "{samples} samples ({ms} ms)";
/// Info row: how many times sound ran out.
/// budget: 13
pub const DROPOUTS: &str = "Dropouts";
/// Dropouts row when sound never ran out.
/// budget: 8
pub const DROPOUTS_NONE: &str = "None";
/// Dropouts row; {n} is the number of dropouts.
/// budget: 37
pub const DROPOUTS_SINCE: &str = "{n} since sound started";
/// Section title for repair switches.
/// budget: 24
pub const TROUBLESHOOTING: &str = "Troubleshooting";
/// Switch row; its subtitle says what it does.
/// budget: 28
pub const KEEP_AWAKE: &str = "Keep device awake";
/// Switch row; its subtitle says what it does.
/// budget: 36
pub const IGNORE_HW_VOLUME: &str = "Ignore hardware volume";
/// Row: copies a report of the sound setup.
/// budget: 18
pub const DIAGNOSTICS: &str = "Diagnostics";
/// Button in the Diagnostics row.
/// budget: 8
pub const COPY: &str = "Copy";
/// Row: forget stored volumes and devices.
/// budget: 18
pub const RESET_AUDIO: &str = "Reset audio";
/// Button in the Reset audio row.
/// budget: 8
pub const RESET: &str = "Reset";
