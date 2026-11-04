// SPDX-License-Identifier: GPL-2.0-or-later
//! System data seam. Topics match IaraOS system-api.md: power, audio, network, brightness.
//! [`CoreSystem`] is the real backend (`wol_core::controls`, Linux): a worker thread keeps cached
//! snapshots, reads never fork or block, actions run off the UI thread through [`System::spawn`].
//! `Sysfs` reads files only; `Fake` is deterministic for goldens.
use crate::status::{Audio, Battery, BatteryStatus, Network};
use std::fs;
pub use wol_core::controls::{BtDevice, Monitor, Output};

/// One change the user asked for. Root ones go through wol-admind and may answer `NeedsAuth`.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Wifi(bool),
    /// Join a Wi-Fi network; `hidden`: it does not announce its name, so NetworkManager asks for
    /// it by name (`802-11-wireless.hidden yes`, QA-NET-10).
    WifiJoin {
        ssid: String,
        password: Option<String>,
        hidden: bool,
    },
    Bluetooth(bool),
    Brightness(u8),
    Volume(u8),
    Muted(bool),
    Output(String),
    NightLight(bool),
    MediaToggle,
    Power(PowerMode),
    /// Compositor monitor rule (`wol_core::controls::monitor_rule`).
    Monitor(String),
    UserAdd {
        name: String,
        full_name: String,
        admin: bool,
        /// New account password: reaches wol-admind on the socket, then `pw` on stdin.
        password: String,
    },
    PkgUpgrade,
    /// App Store: catalog id or package name (`wol_core::store`).
    PkgInstall(String),
    /// App Store: remove a catalog id or package name.
    PkgRemove(String),
    /// App Store: a Flathub app for this account (Flatpak's consent sheet asks for permissions).
    FlatpakInstall(String),
    FlatpakRemove(String),
    MediaNext,
    MediaPrevious,
    /// Bluetooth device address, connect (true) or disconnect.
    BtConnect(String, bool),
    KeyboardBrightness(u8),
    ReduceMotion(bool),
    Zoom(bool),
    /// Kit stylesheet contrast overrides (`kit::style::CONTRAST_FILE`), every shell process.
    IncreaseContrast(bool),
    /// Settings: save desktop prefs and apply the changed compositor keywords live (`wol_core::hyprprefs`).
    Desktop(Box<wol_core::hyprprefs::DesktopPrefs>),
    /// Settings > Keyboard > Shortcuts: save overrides and rebind live.
    Shortcuts(Box<wol_core::shortcuts::Overrides>),
    /// Pause (true) or resume WolOS shortcuts while a shortcut is being recorded.
    ShortcutCapture(bool),
    /// Settings > Restore: start from this restore point (a root slot) next time.
    BootEnvironment(String),
    /// Settings: one allow-listed administrator request (wol-admind validates it again).
    Admin(shared::admin::AdminRequest),
    /// Settings > Privacy: one Flatpak permission on or off, or `perm: None` = back to what the
    /// app asked for. System installs go through wol-admind.
    FlatpakOverride {
        scope: wol_core::flatpak::Scope,
        app: String,
        perm: Option<String>,
        allow: bool,
    },
}

pub type Done = async_channel::Receiver<Result<(), ActionError>>;

pub trait System: Send + Sync {
    fn battery(&self) -> Option<Battery>;
    fn audio(&self) -> Audio;
    fn network(&self) -> Network;
    /// 0..=100, None without a backlight.
    fn brightness(&self) -> Option<u8>;

    // Control Center topics. Defaults = unavailable; `core` implements them through the System API / admin actions.
    fn wifi_networks(&self) -> Vec<WifiNetwork> {
        Vec::new()
    }
    /// Admin action: may answer `NeedsAuth`; the UI then shows the Authenticate sheet and retries through `spawn_with`.
    fn wifi_join(&self, _ssid: &str, _password: Option<&str>) -> Result<(), ActionError> {
        Err(ActionError::Unavailable)
    }
    /// NetworkManager's connectivity (portal, limited), from the snapshot: no process.
    fn connectivity(&self) -> wol_core::netinfo::Connectivity {
        wol_core::netinfo::Connectivity::Unknown
    }
    /// A network that needs 802.1X (Enterprise): no password sheet, a sentence instead (QA-NET-9).
    fn wifi_enterprise(&self, _ssid: &str) -> bool {
        false
    }
    fn set_wifi(&self, _on: bool) -> Result<(), ActionError> {
        Err(ActionError::Unavailable)
    }
    fn bluetooth(&self) -> Option<bool> {
        None
    }
    fn set_bluetooth(&self, _on: bool) -> Result<(), ActionError> {
        Err(ActionError::Unavailable)
    }
    fn set_brightness(&self, _percent: u8) -> Result<(), ActionError> {
        Err(ActionError::Unavailable)
    }
    fn set_volume(&self, _percent: u8) -> Result<(), ActionError> {
        Err(ActionError::Unavailable)
    }
    fn set_muted(&self, _muted: bool) -> Result<(), ActionError> {
        Err(ActionError::Unavailable)
    }
    fn outputs(&self) -> Vec<String> {
        Vec::new()
    }
    fn now_playing(&self) -> Option<NowPlaying> {
        None
    }
    /// App icon of the player behind [`System::now_playing`] (cover art placeholder).
    fn now_playing_icon(&self) -> Option<String> {
        None
    }
    fn media_toggle(&self) -> Result<(), ActionError> {
        Err(ActionError::Unavailable)
    }
    fn night_light(&self) -> Option<bool> {
        None
    }
    fn set_night_light(&self, _on: bool) -> Result<(), ActionError> {
        Err(ActionError::Unavailable)
    }
    /// Power source, mode and what this machine can steer (Settings > Battery, Control Center):
    /// `wol_core::power::policy::status`. None where nothing is read (previews).
    fn power_status(&self) -> Option<wol_core::power::policy::Status> {
        None
    }
    /// Firewall, remote login, system log, kernel hardening (Privacy & Security):
    /// `wol_core::security::status`. How each app permission is enforced is
    /// `wol_core::security::enforcement`.
    fn security_status(&self) -> Option<wol_core::security::Status> {
        None
    }
    fn power_mode(&self) -> Option<PowerMode> {
        None
    }
    fn set_power_mode(&self, _m: PowerMode) -> Result<(), ActionError> {
        Err(ActionError::Unavailable)
    }
    /// Wi-Fi radio state (None = no Wi-Fi hardware/tooling).
    fn wifi_enabled(&self) -> Option<bool> {
        self.network().wifi.then_some(true)
    }
    fn output_devices(&self) -> Vec<Output> {
        self.outputs()
            .into_iter()
            .enumerate()
            .map(|(i, n)| Output {
                id: n.clone(),
                name: n,
                default: i == 0,
            })
            .collect()
    }
    fn monitors(&self) -> Vec<Monitor> {
        Vec::new()
    }
    /// Minutes to empty (discharging) or full (charging); None when unknown.
    fn battery_time(&self) -> Option<u32> {
        None
    }
    /// Paired Bluetooth devices.
    fn bt_devices(&self) -> Vec<BtDevice> {
        Vec::new()
    }
    /// Why the last connect of device `addr` failed, as BlueZ said it (page-timeout, profile
    /// unavailable, not powered), as a sentence; None when it worked or there is no reason.
    fn bt_connect_reason(&self, _addr: &str) -> Option<&'static str> {
        None
    }
    /// 0..=100, None without a keyboard backlight.
    fn keyboard_brightness(&self) -> Option<u8> {
        None
    }
    fn accessibility(&self) -> Accessibility {
        Accessibility::default()
    }

    // Settings > Assistants & Automation, Restore (archicture/modules/automation.md).
    /// Helper policy and the helpers seen in the audit log.
    fn helpers(&self) -> wol_core::automation::Status {
        wol_core::automation::Status::default()
    }
    /// The off switch for every helper.
    fn set_helpers_enabled(&self, _on: bool) -> Result<(), ActionError> {
        Err(ActionError::Unavailable)
    }
    /// Allow or deny one capability (`wol_core::automation::CAPABILITIES` id).
    fn set_helper_capability(&self, _id: &str, _allowed: bool) -> Result<(), ActionError> {
        Err(ActionError::Unavailable)
    }
    /// Always Allow on the approval sheet: `helper` may change `cap` without asking (journaled).
    fn allow_helper_always(&self, _helper: &str, _cap: &str) -> Result<(), ActionError> {
        Err(ActionError::Unavailable)
    }
    /// Settings changes, newest first.
    fn history(&self) -> Vec<wol_core::journal::Entry> {
        Vec::new()
    }
    /// Undoes one change (None = the latest) and reloads what it touched; Err = why not.
    fn undo(&self, _id: Option<u64>) -> Result<wol_core::journal::Entry, String> {
        Err("Undo is not available here.".into())
    }
    /// Restore points (the root slots one can start from), empty without them.
    fn boot_environments(&self) -> Vec<wol_core::update::be::BootEnv> {
        Vec::new()
    }
    /// A helper's administrator request, run after the user typed `password` on the approval
    /// sheet (`ui::approval`). Always checks the password, never a cached authorization.
    fn approve(&self, _req: shared::shell::ApprovalRequest, _password: String) -> Done {
        let (tx, rx) = async_channel::bounded(1);
        let _ = tx.try_send(Err(ActionError::Unavailable));
        rx
    }
    /// Blocking dispatch of one action (backends call this off the UI thread).
    fn perform(&self, a: &Action) -> Result<(), ActionError> {
        match a {
            Action::Wifi(on) => self.set_wifi(*on),
            Action::WifiJoin { ssid, password, .. } => self.wifi_join(ssid, password.as_deref()),
            Action::Bluetooth(on) => self.set_bluetooth(*on),
            Action::Brightness(p) => self.set_brightness(*p),
            Action::Volume(p) => self.set_volume(*p),
            Action::Muted(m) => self.set_muted(*m),
            Action::NightLight(on) => self.set_night_light(*on),
            Action::MediaToggle => self.media_toggle(),
            Action::Power(m) => self.set_power_mode(*m),
            _ => Err(ActionError::Unavailable),
        }
    }
    /// Runs `a` without blocking the caller; the receiver yields the result once.
    fn spawn(&self, a: Action) -> Done {
        let (tx, rx) = async_channel::bounded(1);
        let _ = tx.try_send(self.perform(&a));
        rx
    }
    /// [`System::spawn`] with the password the Authenticate sheet took for this action (None =
    /// none typed yet). Each queued action carries its own copy, so two changes waiting on one
    /// sheet both get it (QA 0.12.0 bugs 4, 6); a one-shot slot went to whichever ran first.
    fn spawn_with(&self, a: Action, _password: Option<&str>) -> Done {
        self.spawn(a)
    }
    /// Re-reads audio now (volume keys -> OSD); resolves once `audio()` has the new value.
    fn refresh_audio(&self) -> Done {
        let (tx, rx) = async_channel::bounded(1);
        let _ = tx.try_send(Ok(()));
        rx
    }
    /// Fires after the snapshot changed (None = static data).
    fn watch(&self) -> Option<async_channel::Receiver<()>> {
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WifiNetwork {
    pub ssid: String,
    pub signal: u8,
    pub secure: bool,
    pub connected: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub playing: bool,
    /// Seconds.
    pub position: u32,
    pub length: Option<u32>,
    /// Cover art URL (`file://` loads; other schemes show the placeholder).
    pub art: Option<String>,
}

/// Accessibility shortcuts; None = not available here (no compositor).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Accessibility {
    pub reduce_motion: Option<bool>,
    pub zoom: Option<bool>,
    pub increase_contrast: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerMode {
    LowPower,
    Automatic,
    HighPerformance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionError {
    Unavailable,
    NeedsAuth,
    Denied,
    NotAdmin,
    /// The request was refused before it ran (e.g. a Wi-Fi password of the wrong length).
    Invalid,
    Failed,
    /// Wi-Fi join: the network refused the password.
    WrongPassword,
    /// The system service turned the change down before any password was checked (wopkg's exit 77:
    /// the consent grant is off or wold's own sheet said no). Never `Denied`: that reopens the
    /// Authenticate sheet, and no password can help here.
    Refused,
    /// The person closed the Authenticate sheet with Cancel: nothing was sent, no password was
    /// checked. Never `Denied`, whose sentence says the password was wrong (hunt 1 E1).
    Cancelled,
}

impl From<wol_core::controls::ControlError> for ActionError {
    fn from(e: wol_core::controls::ControlError) -> Self {
        use wol_core::controls::ControlError as C;
        match e {
            C::Unavailable => Self::Unavailable,
            C::NeedsAuth => Self::NeedsAuth,
            C::Denied => Self::Denied,
            C::NotAdmin => Self::NotAdmin,
            C::Invalid => Self::Invalid,
            C::Failed => Self::Failed,
            C::WrongPassword => Self::WrongPassword,
            // Not in range: the join sentence says to move closer.
            C::NotFound => Self::Failed,
            // The surfaces never send an 802.1X join ([`System::wifi_enterprise`]); `wol` names it.
            C::Unsupported => Self::Unavailable,
        }
    }
}

impl ActionError {
    /// The sentence for any surface ([`crate::copy::action`]; contexts have their own there).
    pub fn message(self) -> &'static str {
        crate::copy::action(self)
    }
}

/// Control Center subtitles.
pub fn wifi_subtitle(nets: &[WifiNetwork]) -> String {
    match nets.iter().find(|w| w.connected) {
        Some(w) => w.ssid.clone(),
        None => "Not Connected".into(),
    }
}

pub fn battery_label(b: Option<crate::status::Battery>) -> Option<String> {
    let b = b?;
    Some(match b.status {
        crate::status::BatteryStatus::Full => format!("{}% · Charged", b.percent),
        crate::status::BatteryStatus::Charging => format!("{}% · Charging", b.percent),
        crate::status::BatteryStatus::Discharging => format!("{}%", b.percent),
    })
}

pub fn signal_icon(signal: u8) -> &'static str {
    crate::status::network(crate::status::Network {
        online: true,
        wifi: true,
        signal: Some(signal),
        limited: false,
    })
}

/// Reads files directly (no process spawn). Audio is a fixed stub until `core` lands.
pub struct Sysfs {
    pub root: std::path::PathBuf,
}

impl Default for Sysfs {
    fn default() -> Self {
        Sysfs {
            root: "/sys/class".into(),
        }
    }
}

/// A virtual link that carries the default route over a physical one (VPN, tunnel).
fn is_tunnel(iface: &str) -> bool {
    ["tun", "tap", "wg", "ppp", "tailscale", "ipsec", "vpn", "zt"]
        .iter()
        .any(|p| iface.starts_with(p))
}

impl Sysfs {
    /// Does `iface` have a wireless extension (a Wi-Fi link, whatever its name)?
    fn is_wireless(&self, iface: &str) -> bool {
        self.root.join("net").join(iface).join("wireless").exists()
    }
}

fn read_trim(p: std::path::PathBuf) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

pub fn parse_battery(capacity: &str, status: &str) -> Option<Battery> {
    let percent = capacity.trim().parse::<u8>().ok()?.min(100);
    let status = match status.trim() {
        "Charging" => BatteryStatus::Charging,
        "Full" => BatteryStatus::Full,
        _ => BatteryStatus::Discharging,
    };
    Some(Battery { percent, status })
}

impl System for Sysfs {
    fn battery(&self) -> Option<Battery> {
        let d = fs::read_dir(self.root.join("power_supply"))
            .ok()?
            .flatten()
            .find(|e| e.file_name().to_string_lossy().starts_with("BAT"))?;
        parse_battery(
            &read_trim(d.path().join("capacity"))?,
            &read_trim(d.path().join("status")).unwrap_or_default(),
        )
    }
    fn audio(&self) -> Audio {
        Audio {
            volume: Some(60),
            muted: false,
            mic: None,
            mic_muted: false,
        }
    }
    fn network(&self) -> Network {
        let Ok(rd) = fs::read_dir(self.root.join("net")) else {
            return Network::default();
        };
        let mut n = Network::default();
        for e in rd.flatten() {
            if e.file_name() == "lo"
                || read_trim(e.path().join("operstate")).as_deref() != Some("up")
            {
                continue;
            }
            n.online = true;
            if e.path().join("wireless").exists() {
                n.wifi = true;
            }
        }
        n
    }
    fn brightness(&self) -> Option<u8> {
        let d = fs::read_dir(self.root.join("backlight"))
            .ok()?
            .flatten()
            .next()?;
        let cur: u32 = read_trim(d.path().join("brightness"))?.parse().ok()?;
        let max: u32 = read_trim(d.path().join("max_brightness"))?.parse().ok()?;
        (max > 0).then(|| (cur * 100 / max) as u8)
    }
}

/// Deterministic data for golden renders and UI work without hardware (`WOL_FAKE_SYSTEM=1`).
pub struct Fake;

impl System for Fake {
    fn battery(&self) -> Option<Battery> {
        Some(Battery {
            percent: 82,
            status: BatteryStatus::Discharging,
        })
    }
    fn audio(&self) -> Audio {
        Audio {
            volume: Some(64),
            muted: false,
            mic: Some(50),
            mic_muted: false,
        }
    }
    fn network(&self) -> Network {
        Network {
            online: true,
            wifi: true,
            signal: Some(80),
            limited: false,
        }
    }
    fn brightness(&self) -> Option<u8> {
        Some(72)
    }
    fn wifi_networks(&self) -> Vec<WifiNetwork> {
        vec![
            WifiNetwork {
                ssid: "Wonderland".into(),
                signal: 88,
                secure: true,
                connected: true,
            },
            WifiNetwork {
                ssid: "Rabbit Hole 5G".into(),
                signal: 60,
                secure: true,
                connected: false,
            },
            WifiNetwork {
                ssid: "Tea Party".into(),
                signal: 30,
                secure: false,
                connected: false,
            },
        ]
    }
    fn bluetooth(&self) -> Option<bool> {
        Some(true)
    }
    fn outputs(&self) -> Vec<String> {
        vec!["Speakers".into(), "Headphones".into()]
    }
    fn now_playing(&self) -> Option<NowPlaying> {
        Some(NowPlaying {
            title: "White Rabbit".into(),
            artist: "Jefferson Airplane".into(),
            playing: true,
            position: 61,
            length: Some(152),
            art: None,
        })
    }
    fn now_playing_icon(&self) -> Option<String> {
        Some("wolos-music".into())
    }
    fn night_light(&self) -> Option<bool> {
        Some(false)
    }
    fn wifi_enabled(&self) -> Option<bool> {
        Some(true)
    }
    fn output_devices(&self) -> Vec<Output> {
        vec![
            Output {
                id: "speakers".into(),
                name: "Built-in Speakers".into(),
                default: true,
            },
            Output {
                id: "headphones".into(),
                name: "Headphones".into(),
                default: false,
            },
        ]
    }
    fn battery_time(&self) -> Option<u32> {
        Some(191)
    }
    fn bt_devices(&self) -> Vec<BtDevice> {
        [
            ("AirPods Pro", true),
            ("MX Keys", true),
            ("Magic Trackpad", false),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (n, c))| BtDevice {
            addr: format!("AA:BB:CC:DD:EE:0{i}"),
            name: n.into(),
            connected: c,
        })
        .collect()
    }
    fn keyboard_brightness(&self) -> Option<u8> {
        Some(40)
    }
    fn power_mode(&self) -> Option<PowerMode> {
        Some(PowerMode::Automatic)
    }
    fn accessibility(&self) -> Accessibility {
        Accessibility {
            reduce_motion: Some(false),
            zoom: Some(false),
            increase_contrast: false,
        }
    }
    fn monitors(&self) -> Vec<Monitor> {
        vec![Monitor {
            name: "eDP-1".into(),
            description: "Built-in Display".into(),
            width: 2560,
            height: 1600,
            refresh: 60.0,
            scale: 2.0,
            modes: vec![
                "2560x1600@60.00Hz".into(),
                "1920x1200@60.00Hz".into(),
                "1440x900@60.00Hz".into(),
            ],
            mirror_of: None,
        }]
    }
    // Settings > Assistants & Automation and Restore: a helper seen once, a few changes, restore points.
    fn helpers(&self) -> wol_core::automation::Status {
        let mut s = wol_core::automation::status(
            &wol_core::automation::policy::Policy {
                enabled: false,
                ..Default::default()
            },
            &[],
        );
        s.helpers = vec![wol_core::automation::audit::HelperSeen {
            name: "Claude".into(),
            via: "MCP".into(),
            first: 0,
            last: 0,
            actions: 12,
            refused: 1,
        }];
        s
    }
    fn history(&self) -> Vec<wol_core::journal::Entry> {
        let now = wol_core::journal::now();
        [
            (4, 300, "settings", "theme"),
            (3, 2 * 3600, "mcp:Claude", "wallpaper"),
            (2, 26 * 3600, "settings", "desktop"),
            (1, 3 * 86_400, "cli", "shortcuts"),
        ]
        .into_iter()
        .map(|(id, ago, by, what)| wol_core::journal::Entry {
            id,
            t: now - ago,
            by: by.into(),
            what: what.into(),
            changes: vec![],
            undoes: None,
        })
        .collect()
    }
    fn boot_environments(&self) -> Vec<wol_core::update::be::BootEnv> {
        let be = |name: &str, active, created: &str, space: &str| wol_core::update::be::BootEnv {
            name: name.into(),
            active,
            next_boot: active,
            mountpoint: active.then(|| "/".into()),
            space: space.into(),
            created: created.into(),
        };
        // bootctl's shapes (A/B slots), as `wol_core::update::be` lists them (QA-SET-30).
        vec![
            be("wolos-a", true, "2026/11/02", ""),
            be("wolos-b", false, "2026/10/20", ""),
        ]
    }
}

/// [`Fake`] on a desktop: no battery, no backlights, no Low Power Mode.
pub struct FakeDesktop;

impl System for FakeDesktop {
    fn battery(&self) -> Option<Battery> {
        None
    }
    fn audio(&self) -> Audio {
        Fake.audio()
    }
    fn network(&self) -> Network {
        Network {
            online: true,
            wifi: false,
            signal: None,
            limited: false,
        }
    }
    fn brightness(&self) -> Option<u8> {
        None
    }
    fn wifi_networks(&self) -> Vec<WifiNetwork> {
        Fake.wifi_networks()
    }
    fn wifi_enabled(&self) -> Option<bool> {
        Some(true)
    }
    fn bluetooth(&self) -> Option<bool> {
        Some(false)
    }
    fn output_devices(&self) -> Vec<Output> {
        Fake.output_devices()
    }
    fn now_playing(&self) -> Option<NowPlaying> {
        None
    }
    fn night_light(&self) -> Option<bool> {
        Fake.night_light()
    }
    fn accessibility(&self) -> Accessibility {
        Fake.accessibility()
    }
}

// ---- Real backend ----

use std::sync::{Arc, Mutex, Weak};

use wol_core::controls::{A11y, BatteryState, Charge, Controls, Media, Volume, WifiNet};

/// [`System::bt_connect_reason`] for the real host: why the last connect of each device failed,
/// by address (two devices connecting at once keep their own reasons).
static BT_REASON: Mutex<Vec<(String, &'static str)>> = Mutex::new(Vec::new());

/// Everything the UI reads; refreshed by the worker, cached to `$XDG_RUNTIME_DIR/wol-controls.json`
/// so a freshly opened Control Center shows real values before the first refresh lands.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub wifi_on: Option<bool>,
    pub nets: Vec<WifiNet>,
    pub volume: Option<Volume>,
    /// The default input (microphone): level and mute (audit5 S9-3).
    pub mic: Option<Volume>,
    pub outputs: Vec<Output>,
    pub brightness: Option<u8>,
    pub bluetooth: Option<bool>,
    pub monitors: Vec<Monitor>,
    pub night: Option<bool>,
    pub battery: Option<BatteryState>,
    /// Default-route interface from /proc/net/route (wired vs Wi-Fi without sysfs).
    pub route: Option<String>,
    /// NetworkManager's connectivity (portal, limited; QA-NET-12, NET2-3), read with the slow topic.
    #[serde(default)]
    pub connectivity: wol_core::netinfo::Connectivity,
    pub media: Option<Media>,
    pub bt_devices: Vec<BtDevice>,
    pub keyboard: Option<u8>,
    pub low_power: Option<bool>,
    pub a11y: Option<A11y>,
}

/// The default source, as `wpctl get-volume` prints it (`Volume: 0.50 [MUTED]`); None = no input.
fn mic_volume(host: &dyn wol_core::system::Host) -> Option<Volume> {
    host.run(&["wpctl", "get-volume", "@DEFAULT_AUDIO_SOURCE@"])
        .and_then(|t| wol_core::controls::parse_wpctl_volume(&t))
}

/// Seconds between refreshes per topic (system-api.md TTLs).
const TTL_AUDIO: u64 = 2;
const TTL_WIFI: u64 = 5;
const TTL_SLOW: u64 = 10;
/// Refresh cadence while a surface watches the snapshot (Control Center, Settings).
pub const WATCHED_PERIOD: std::time::Duration = std::time::Duration::from_secs(1);
/// Cadence with nobody watching: the menu bar polls every 5 s; nothing needs fresher data.
pub const IDLE_PERIOD: std::time::Duration = std::time::Duration::from_secs(10);
/// Control Center-only topics: never refreshed while nobody watches.
const NEVER: u64 = u64::MAX;
/// Topics: audio, wifi, slow, media (Now Playing progress), panel (Bluetooth devices, keyboard
/// backlight, Low Power Mode, accessibility, Night Shift).
const TOPICS: usize = 5;
/// TTLs (s) per topic while watched / unwatched. Unwatched, only the menu bar reads: volume keys
/// refresh audio on demand (`refresh_audio`), so every helper runs once a minute.
const TTLS_WATCHED: [u64; TOPICS] = [TTL_AUDIO, TTL_WIFI, TTL_SLOW, 1, TTL_WIFI];
const TTLS_IDLE: [u64; TOPICS] = [60, 60, 60, NEVER, NEVER];

struct Inner {
    c: Controls,
    files: Sysfs,
    snap: Mutex<Snapshot>,
    watchers: Mutex<Vec<async_channel::Sender<()>>>,
    cache: Option<std::path::PathBuf>,
    /// Last refresh per topic; None = never.
    last: Mutex<[Option<std::time::Instant>; TOPICS]>,
    /// One refresh at a time: each writes back a whole snapshot.
    refreshing: Mutex<()>,
}

impl Inner {
    fn lock(&self) -> std::sync::MutexGuard<'_, Snapshot> {
        self.snap.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Next refresh delay: fast only while a live watcher exists.
    fn period(&self) -> std::time::Duration {
        let mut w = self.watchers.lock().unwrap_or_else(|e| e.into_inner());
        w.retain(|t| !t.is_closed());
        if w.is_empty() {
            IDLE_PERIOD
        } else {
            WATCHED_PERIOD
        }
    }

    /// Refreshes every topic whose TTL elapsed (all when `force`).
    fn refresh(&self, force: bool) {
        let now = std::time::Instant::now();
        let due = {
            let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
            let mut due = [false; TOPICS];
            let ttls = if self.period() == WATCHED_PERIOD {
                TTLS_WATCHED
            } else {
                TTLS_IDLE
            };
            for (i, ttl) in ttls.into_iter().enumerate() {
                // 100 ms slack: a 1 s tick must not miss a 2 s TTL by jitter.
                let ready = |t: std::time::Instant| {
                    now.duration_since(t).as_millis() + 100 >= u128::from(ttl) * 1000
                };
                if ttl == NEVER && !force {
                    continue;
                }
                if force || last[i].is_none_or(ready) {
                    last[i] = Some(now);
                    due[i] = true;
                }
            }
            due
        };
        self.refresh_topics(due);
    }

    /// Audio only, now (volume keys); the other topics keep their schedule.
    fn refresh_audio(&self) {
        self.refresh_now([true, false, false, false, false]);
    }

    /// The `due` topics now (after an action that changed them); the others keep their schedule.
    fn refresh_now(&self, due: [bool; TOPICS]) {
        let now = std::time::Instant::now();
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        for (l, d) in last.iter_mut().zip(due) {
            if d {
                *l = Some(now);
            }
        }
        drop(last);
        self.refresh_topics(due);
    }

    fn refresh_topics(&self, due: [bool; TOPICS]) {
        let _one = self.refreshing.lock().unwrap_or_else(|e| e.into_inner());
        let mut s = self.lock().clone();
        let before = s.clone();
        if due[0] {
            s.volume = self.c.volume().or(s.volume);
            s.mic = mic_volume(&*self.c.host).or(s.mic);
            s.brightness = self.c.brightness();
        }
        if due[1] {
            s.wifi_on = self.c.wifi_enabled();
            s.nets = self.c.wifi_networks();
        }
        if due[2] {
            s.outputs = self.c.outputs();
            s.bluetooth = self.c.bluetooth();
            s.monitors = self.c.monitors();
            s.battery = self.c.battery();
            s.route = self.c.default_iface();
            s.connectivity = self
                .c
                .host
                .run(&wol_core::netinfo::CONNECTIVITY_ARGV)
                .map(|t| wol_core::netinfo::parse_connectivity(&t))
                .unwrap_or_default();
        }
        if due[3] {
            s.media = self.c.now_playing();
        }
        if due[4] {
            s.bt_devices = self.c.bt_devices();
            s.keyboard = self.c.keyboard_brightness();
            s.low_power = self.c.low_power();
            s.a11y = self.c.accessibility();
            s.night = self.c.night_shift();
        }
        if s != before {
            *self.lock() = s.clone();
            if let (Some(p), Ok(j)) = (&self.cache, serde_json::to_string(&s)) {
                let _ = shared::atomic::atomic_write(p, 0o644, j.as_bytes());
            }
            self.watchers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|t| !t.is_closed() && (t.try_send(()).is_ok() || t.is_full()));
        }
    }

    /// `a` on a worker thread; the receiver yields its result once.
    fn spawn(self: &Arc<Self>, a: Action, own: Option<Secret>) -> Done {
        let (tx, rx) = async_channel::bounded(1);
        let i = self.clone();
        let _ = std::thread::Builder::new()
            .name("wol-action".into())
            .spawn(move || {
                let r = i.perform_with(&a, own);
                // An answer like `NeedsAuth` changed nothing: reply now, nothing to read again.
                // The Authenticate sheet waited on a refresh of every topic, behind a 1 s poll
                // (QA 0.12.0 bug 3).
                let due = topics_of(&a);
                if r.is_ok() && due.contains(&true) {
                    // Done: reply with the host's state already read, so callers never see it
                    // go stale after; only the topics this action changed (audit4 S3-5).
                    i.refresh_now(due);
                }
                let _ = tx.try_send(r);
            });
        rx
    }

    /// Blocking; runs on a worker thread.
    fn perform(&self, a: &Action) -> Result<(), ActionError> {
        self.perform_with(a, None)
    }

    /// [`Self::perform`] with this action's own password (None = none typed yet). Nothing is
    /// stored between calls: a shared slot went to whichever action ran first (hunt 1 D2).
    fn perform_with(&self, a: &Action, own: Option<Secret>) -> Result<(), ActionError> {
        let c = &self.c;
        // Only an action that can ask for it takes a password.
        let pw = match own {
            Some(p) if may_need_password(a) => p,
            _ => Secret(String::new()),
        };
        // wopkg runs as the person and asks wold, which checks the consent grant (ADR 0021). Only
        // with wopkg's grant: without it wold refuses before any sheet (a fresh install has none),
        // so the Authenticate-sheet path through wol-admind stays (0.12.1, QA 0.12.0 bug 4).
        if let Some(r) = via_wopkg(a, &crate::packages::run_wopkg) {
            return r;
        }
        let r = match a {
            Action::Wifi(on) => c.set_wifi(*on, &pw),
            Action::WifiJoin {
                ssid,
                password,
                hidden,
            } => c.wifi_join_hidden(ssid, password.as_deref(), *hidden, &pw),
            Action::Bluetooth(on) => c.set_bluetooth(*on, &pw),
            Action::Brightness(p) => c.set_brightness(*p),
            Action::Volume(p) => c.set_volume(*p),
            Action::Muted(m) => c.set_muted(*m),
            Action::Output(id) => c.set_output(id, &pw),
            Action::NightLight(on) => c.set_night_shift(*on),
            Action::Monitor(rule) => c.set_monitor(rule),
            Action::UserAdd {
                name,
                full_name,
                admin,
                password,
            } => c.add_user(name, full_name, *admin, password, &pw),
            // Not through wopkg ([`via_wopkg`]): wol-admind with the Authenticate sheet's password.
            Action::PkgUpgrade => c.pkg_upgrade(&pw),
            Action::PkgInstall(t) => c.pkg_install(t, &pw),
            Action::PkgRemove(t) => c.pkg_remove(t, &pw),
            Action::FlatpakInstall(id) => {
                flatpak_install_tracked(id).unwrap_or_else(|| c.flatpak_install(id))
            }
            Action::FlatpakRemove(id) => c.flatpak_remove(id),
            Action::FlatpakOverride {
                scope,
                app,
                perm,
                allow,
            } => c.flatpak_override(*scope, app, perm.as_deref(), *allow, &pw),
            Action::MediaToggle => c.media("play-pause"),
            Action::MediaNext => c.media("next"),
            Action::MediaPrevious => c.media("previous"),
            Action::Power(m) => c.set_low_power(*m == PowerMode::LowPower, &pw),
            Action::BtConnect(addr, on) => {
                let r = c.bt_connect_reason(addr, *on);
                let mut why = BT_REASON.lock().unwrap_or_else(|e| e.into_inner());
                why.retain(|(a, _)| a != addr);
                if let Err((_, sentence)) = &r {
                    why.push((addr.clone(), *sentence));
                }
                drop(why);
                r.map_err(|e| e.0)
            }
            Action::KeyboardBrightness(p) => c.set_keyboard_brightness(*p),
            Action::ReduceMotion(on) => c.set_reduce_motion_saved(&config_home(), *on),
            Action::Zoom(on) => c.set_zoom(*on),
            Action::IncreaseContrast(on) => crate::kit::style::set_contrast(*on)
                .map_err(|_| wol_core::controls::ControlError::Failed),
            Action::Desktop(p) => wol_core::hyprprefs::apply_desktop(&*c.host, &config_home(), p)
                .map(drop)
                .map_err(|_| wol_core::controls::ControlError::Failed),
            Action::Shortcuts(o) => {
                wol_core::hyprprefs::apply_shortcuts(&*c.host, &config_home(), o)
                    .map(drop)
                    .map_err(|_| wol_core::controls::ControlError::Failed)
            }
            Action::ShortcutCapture(on) => {
                if wol_core::hyprprefs::capture(&*c.host, *on) {
                    Ok(())
                } else {
                    Err(wol_core::controls::ControlError::Unavailable)
                }
            }
            Action::BootEnvironment(name) => c.boot_environment(name, &pw),
            // Validated first, then wol-admind (`Controls::admin`: the socket, a fake in tests).
            Action::Admin(req) => c.admin_request(req, &pw),
        };
        // Optimistic snapshot so the next read (and other surfaces) agree right away.
        if r.is_ok() {
            let mut s = self.lock();
            match a {
                Action::Volume(p) => {
                    s.volume = Some(Volume {
                        percent: *p,
                        muted: s.volume.is_some_and(|v| v.muted),
                    })
                }
                Action::Muted(m) => s.volume = s.volume.map(|v| Volume { muted: *m, ..v }),
                Action::Brightness(p) => {
                    s.brightness = Some((*p).clamp(wol_core::controls::BRIGHTNESS_MIN, 100))
                }
                Action::Wifi(on) => s.wifi_on = Some(*on),
                Action::Bluetooth(on) => s.bluetooth = Some(*on),
                Action::NightLight(on) => s.night = Some(*on),
                Action::KeyboardBrightness(p) => s.keyboard = Some(*p),
                Action::Power(m) => s.low_power = Some(*m == PowerMode::LowPower),
                Action::ReduceMotion(on) => {
                    if let Some(a) = &mut s.a11y {
                        a.reduce_motion = *on
                    }
                }
                Action::Zoom(on) => {
                    if let Some(a) = &mut s.a11y {
                        a.zoom = *on
                    }
                }
                Action::MediaToggle => {
                    if let Some(m) = &mut s.media {
                        m.playing = !m.playing
                    }
                }
                Action::BtConnect(addr, on) => {
                    for d in s.bt_devices.iter_mut().filter(|d| &d.addr == addr) {
                        d.connected = *on;
                    }
                }
                Action::Output(id) => {
                    for o in &mut s.outputs {
                        o.default = &o.id == id;
                    }
                }
                _ => {}
            }
        }
        r.map_err(ActionError::from)
    }
}

/// A package change through wopkg when wopkg holds its consent grant
/// ([`crate::packages::usable`]); None = not this way (wol-admind and the Authenticate sheet).
fn via_wopkg(a: &Action, run: crate::packages::Run) -> Option<Result<(), ActionError>> {
    use crate::packages::{install, remove, upgrade, usable};
    match a {
        Action::PkgUpgrade => upgrade(usable("packages-update"), run),
        Action::PkgInstall(t) => install(t, usable("packages-install"), run),
        Action::PkgRemove(t) => remove(t, usable("packages-remove"), run),
        _ => None,
    }
}

/// This account's Flatpak install with its percentage published for the App Store; None when
/// there is no runtime dir to publish in (then it runs untracked).
fn flatpak_install_tracked(id: &str) -> Option<Result<(), wol_core::controls::ControlError>> {
    use wol_core::{controls::ControlError, flatpak, progress};
    if !flatpak::is_app_id(id) {
        return Some(Err(ControlError::Invalid));
    }
    let add = flatpak::remote_add_argv();
    let _ = std::process::Command::new(&add[0]).args(&add[1..]).output();
    let path = progress::file_for(&progress::user_dir()?, id)?;
    let argv = flatpak::install_argv(id);
    let a: Vec<&str> = argv.iter().map(String::as_str).collect();
    let ok = progress::run_tracked(
        &a,
        progress::Kind::Flatpak,
        &path,
        std::time::Duration::from_secs(1800),
    );
    Some(if ok {
        Ok(())
    } else {
        Err(ControlError::Failed)
    })
}

/// The admin password between the Authenticate sheet and the action: its bytes are overwritten
/// when it is dropped (audit4 S3-5).
struct Secret(String);

impl std::ops::Deref for Secret {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

// Wipes the password bytes before the memory is freed.
impl Drop for Secret {
    fn drop(&mut self) {
        let mut b = std::mem::take(&mut self.0).into_bytes();
        b.fill(0);
        std::hint::black_box(&b);
    }
}

/// Actions that go through wol-admind and may answer `NeedsAuth`: the only ones that take the
/// password the Authenticate sheet typed.
fn may_need_password(a: &Action) -> bool {
    matches!(
        a,
        Action::Wifi(_)
            | Action::WifiJoin { .. }
            | Action::Bluetooth(_)
            | Action::Output(_)
            | Action::UserAdd { .. }
            | Action::PkgUpgrade
            | Action::PkgInstall(_)
            | Action::PkgRemove(_)
            | Action::FlatpakOverride { .. }
            | Action::Power(_)
            | Action::BootEnvironment(_)
            | Action::Admin(_)
    )
}

/// The snapshot topics a helper's approved request changed ([`topics_of`] for the same change;
/// packages, boot environments and Doctor fixes change none).
fn approval_topics(req: &shared::shell::ApprovalRequest) -> [bool; TOPICS] {
    use shared::shell::ApprovalRequest as R;
    match req {
        R::Wifi { on } => topics_of(&Action::Wifi(*on)),
        R::WifiJoin { ssid, .. } => topics_of(&Action::WifiJoin {
            ssid: ssid.clone(),
            password: None,
            hidden: false,
        }),
        R::Bluetooth { on } => topics_of(&Action::Bluetooth(*on)),
        _ => [false; TOPICS],
    }
}

/// The snapshot topics (audio, wifi, slow, media, panel) an action changes: only those are read
/// again before its reply. Every topic after every action forked nmcli, bluetoothctl, hyprctl and
/// playerctl per slider step (audit4 S3-5).
fn topics_of(a: &Action) -> [bool; TOPICS] {
    let (audio, wifi, slow, media, panel) = match a {
        Action::Volume(_) | Action::Muted(_) | Action::Brightness(_) => (1, 0, 0, 0, 0),
        Action::Wifi(_) => (0, 1, 0, 0, 0),
        // A new network brings a new default route.
        Action::WifiJoin { .. } => (0, 1, 1, 0, 0),
        Action::Bluetooth(_) => (0, 0, 1, 0, 1),
        Action::BtConnect(..) => (0, 0, 0, 0, 1),
        Action::Output(_) | Action::Monitor(_) => (0, 0, 1, 0, 0),
        Action::MediaToggle | Action::MediaNext | Action::MediaPrevious => (0, 0, 0, 1, 0),
        Action::NightLight(_)
        | Action::Power(_)
        | Action::KeyboardBrightness(_)
        | Action::ReduceMotion(_)
        | Action::Zoom(_) => (0, 0, 0, 0, 1),
        // Only what the request changes: a Sharing or computer-name change used to read every
        // topic (nmcli, bluetoothctl, hyprctl, playerctl) before its reply (QA 0.12.0 bug 3).
        Action::Admin(req) => return admin_topics(req.action),
        Action::UserAdd { .. }
        | Action::PkgUpgrade
        | Action::PkgInstall(_)
        | Action::PkgRemove(_)
        | Action::FlatpakInstall(_)
        | Action::FlatpakRemove(_)
        | Action::FlatpakOverride { .. }
        | Action::IncreaseContrast(_)
        | Action::Desktop(_)
        | Action::Shortcuts(_)
        | Action::ShortcutCapture(_)
        | Action::BootEnvironment(_) => (0, 0, 0, 0, 0),
    };
    [audio, wifi, slow, media, panel].map(|t| t == 1)
}

/// The snapshot topics one allow-listed wol-admind request changes (see [`topics_of`]).
fn admin_topics(a: shared::admin::AdminAction) -> [bool; TOPICS] {
    use shared::admin::AdminAction as A;
    let (audio, wifi, slow, media, panel) = match a {
        A::WifiPower | A::WifiForget => (0, 1, 0, 0, 0),
        A::WifiJoin => (0, 1, 1, 0, 0),
        A::Bluetooth => (0, 0, 1, 0, 1),
        A::BtPair | A::BtForget | A::PowerMode => (0, 0, 0, 0, 1),
        A::AudioDefault => (1, 0, 1, 0, 0),
        // A kernel module can bring any device: read everything again.
        A::HwLoad => (1, 1, 1, 1, 1),
        A::Appearance
        | A::Keybinds
        | A::Idle
        | A::UserAdd
        | A::UserRemove
        | A::UserPassword
        | A::TimeZone
        | A::PkgUpgrade
        | A::PkgRefresh
        | A::Revoke
        | A::PkgInstall
        | A::PkgRemove
        | A::AurOpen
        | A::BootEnvironment
        | A::WolInstall
        | A::WolRemove
        | A::Ntp
        | A::Hostname
        | A::RemoteLogin
        | A::AirPlayReceiver
        | A::Firewall
        | A::MalwareSignatures
        | A::Lid
        | A::PrinterAdd
        | A::PrinterRemove
        | A::PrinterDefault
        | A::Consent
        | A::SessionPower
        | A::FlatpakOverride
        | A::FlatpakInstall
        | A::FlatpakRemove
        | A::AutoUpdate
        | A::BootEnroll
        | A::PkgInstallFile
        | A::SystemRollback
        | A::SystemChannel
        | A::Reports => (0, 0, 0, 0, 0),
    };
    [audio, wifi, slow, media, panel].map(|t| t == 1)
}

/// Real system: `wol_core::controls` on a background thread; the UI only reads the snapshot.
pub struct CoreSystem {
    inner: Arc<Inner>,
}

impl CoreSystem {
    pub fn new() -> Self {
        let cache = std::env::var_os("XDG_RUNTIME_DIR")
            .map(|d| std::path::PathBuf::from(d).join("wol-controls.json"));
        Self::with(
            Controls::new(Arc::new(wol_core::system::PosixHost)),
            Sysfs::default(),
            cache,
        )
    }

    /// Injected controls (tests use a `FakeHost`). Refreshes now, then every `Inner::period`.
    pub fn with(c: Controls, files: Sysfs, cache: Option<std::path::PathBuf>) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let sys = Self::with_ticks(c, files, cache, rx);
        let weak = Arc::downgrade(&sys.inner);
        let _ = std::thread::Builder::new()
            .name("wol-controls-tick".into())
            .spawn(move || {
                while tx.send(()).is_ok() {
                    let Some(period) = weak.upgrade().map(|i| i.period()) else {
                        break;
                    };
                    std::thread::sleep(period);
                }
            });
        sys
    }

    /// Refresh worker driven by `ticks` (one refresh per message); stops when either side drops.
    pub fn with_ticks(
        c: Controls,
        files: Sysfs,
        cache: Option<std::path::PathBuf>,
        ticks: std::sync::mpsc::Receiver<()>,
    ) -> Self {
        let snap = cache
            .as_ref()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let inner = Arc::new(Inner {
            c,
            files,
            snap: Mutex::new(snap),
            watchers: Default::default(),
            cache,
            last: Default::default(),
            refreshing: Default::default(),
        });
        let weak: Weak<Inner> = Arc::downgrade(&inner);
        let _ = std::thread::Builder::new()
            .name("wol-controls".into())
            .spawn(move || {
                while ticks.recv().is_ok() {
                    let Some(i) = weak.upgrade() else { break };
                    i.refresh(false);
                }
            });
        CoreSystem { inner }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.inner.lock().clone()
    }
}

impl Default for CoreSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl System for CoreSystem {
    fn battery(&self) -> Option<Battery> {
        let Some(b) = self.inner.lock().battery else {
            return self.inner.files.battery();
        };
        Some(Battery {
            percent: b.percent,
            status: match b.charge {
                Charge::Charging => BatteryStatus::Charging,
                Charge::Discharging => BatteryStatus::Discharging,
                Charge::Full => BatteryStatus::Full,
            },
        })
    }
    fn battery_time(&self) -> Option<u32> {
        self.inner.lock().battery.and_then(|b| b.minutes)
    }
    fn now_playing(&self) -> Option<NowPlaying> {
        self.inner.lock().media.as_ref().map(|m| NowPlaying {
            title: m.title.clone(),
            artist: m.artist.clone(),
            playing: m.playing,
            position: m.position,
            length: m.length,
            art: m.art.clone(),
        })
    }
    fn now_playing_icon(&self) -> Option<String> {
        let player = self.inner.lock().media.as_ref()?.player.clone()?;
        Some(player_icon(&player))
    }
    fn bt_devices(&self) -> Vec<BtDevice> {
        self.inner.lock().bt_devices.clone()
    }
    fn bt_connect_reason(&self, addr: &str) -> Option<&'static str> {
        let why = BT_REASON.lock().unwrap_or_else(|e| e.into_inner());
        why.iter().find(|(a, _)| a == addr).map(|(_, s)| *s)
    }
    fn keyboard_brightness(&self) -> Option<u8> {
        self.inner.lock().keyboard
    }
    fn power_status(&self) -> Option<wol_core::power::policy::Status> {
        use wol_core::power::policy;
        let host = &*self.inner.c.host;
        let saved = std::fs::read_to_string(policy::STATE_FILE).ok();
        let facts = policy::facts(&|argv: &[&str]| host.run(argv));
        let state = policy::State::parse(saved.as_deref());
        Some(policy::status(&facts, &state))
    }
    fn security_status(&self) -> Option<wol_core::security::Status> {
        let host = &*self.inner.c.host;
        Some(wol_core::security::status(&|argv: &[&str]| host.run(argv)))
    }
    fn power_mode(&self) -> Option<PowerMode> {
        self.inner.lock().low_power.map(|l| {
            if l {
                PowerMode::LowPower
            } else {
                PowerMode::Automatic
            }
        })
    }
    fn accessibility(&self) -> Accessibility {
        let a = self.inner.lock().a11y;
        Accessibility {
            reduce_motion: a.map(|a| a.reduce_motion),
            zoom: a.map(|a| a.zoom),
            increase_contrast: crate::kit::style::contrast_on(),
        }
    }
    fn audio(&self) -> Audio {
        let s = self.inner.lock();
        let (v, m) = (s.volume, s.mic);
        Audio {
            volume: v.map(|v| v.percent),
            muted: v.is_some_and(|v| v.muted),
            mic: m.map(|m| m.percent),
            mic_muted: m.is_some_and(|m| m.muted),
        }
    }
    fn network(&self) -> Network {
        // What decides the glyph is read now, on every call (the menu bar asks every 5 s): the
        // links' operstate and the default route cost no process. The snapshot's Wi-Fi list is
        // up to a minute old while nobody watches; it only adds the signal strength of a
        // network the live reads agree is in use (QA NET2-5: a pulled cable showed for 44 s).
        let mut n = self.inner.files.network();
        let linked = self.inner.files.root.join("net").is_dir();
        if let Some(iface) = self.inner.c.default_iface() {
            n.online = true;
            // The default route's medium names the glyph, not any wireless link that is up
            // (QA NET2-7: Ethernet carried the traffic, the tray showed Wi-Fi).
            // A tunnel (OpenVPN, WireGuard) carries the default route over some physical link
            // it hides: keep what the links say then, never call that wired.
            if !is_tunnel(&iface) {
                n.wifi = iface.starts_with("wlan") || self.inner.files.is_wireless(&iface);
            }
        }
        let s = self.inner.lock();
        if let Some(w) = s.nets.iter().find(|w| w.connected)
            && (n.wifi || !linked)
        {
            n.wifi = true;
            n.online = true;
            n.signal = Some(w.signal);
        }
        // A link that NetworkManager says reaches no internet (limited) or only a sign-in page
        // (portal) is not plainly online (NET2-3: the tray showed the wired glyph behind a portal).
        use wol_core::netinfo::Connectivity as K;
        n.limited = n.online && matches!(s.connectivity, K::Limited | K::Portal);
        n
    }
    fn connectivity(&self) -> wol_core::netinfo::Connectivity {
        self.inner.lock().connectivity
    }
    fn brightness(&self) -> Option<u8> {
        self.inner.lock().brightness
    }
    fn wifi_enabled(&self) -> Option<bool> {
        self.inner.lock().wifi_on
    }
    fn wifi_networks(&self) -> Vec<WifiNetwork> {
        self.inner
            .lock()
            .nets
            .iter()
            .map(|n| WifiNetwork {
                ssid: n.ssid.clone(),
                signal: n.signal,
                secure: n.secure,
                connected: n.connected,
            })
            .collect()
    }
    fn wifi_enterprise(&self, ssid: &str) -> bool {
        self.inner
            .lock()
            .nets
            .iter()
            .any(|n| n.ssid == ssid && n.security == wol_core::wifi::Security::Enterprise)
    }
    fn bluetooth(&self) -> Option<bool> {
        self.inner.lock().bluetooth
    }
    fn outputs(&self) -> Vec<String> {
        self.output_devices().into_iter().map(|o| o.name).collect()
    }
    fn output_devices(&self) -> Vec<Output> {
        self.inner.lock().outputs.clone()
    }
    fn monitors(&self) -> Vec<Monitor> {
        self.inner.lock().monitors.clone()
    }
    /// Probed from hyprsunset: None (tile hidden) when it is not running.
    fn night_light(&self) -> Option<bool> {
        self.inner.lock().night
    }
    // Sync setters: same path as `spawn`, for callers that are already off the UI thread.
    fn set_volume(&self, p: u8) -> Result<(), ActionError> {
        self.inner.perform(&Action::Volume(p))
    }
    fn set_brightness(&self, p: u8) -> Result<(), ActionError> {
        self.inner.perform(&Action::Brightness(p))
    }
    fn perform(&self, a: &Action) -> Result<(), ActionError> {
        self.inner.perform(a)
    }
    fn spawn(&self, a: Action) -> Done {
        self.inner.spawn(a, None)
    }
    fn spawn_with(&self, a: Action, password: Option<&str>) -> Done {
        self.inner.spawn(a, password.map(|p| Secret(p.into())))
    }
    fn helpers(&self) -> wol_core::automation::Status {
        wol_core::automation::user_status()
    }
    fn set_helpers_enabled(&self, on: bool) -> Result<(), ActionError> {
        wol_core::automation::set_enabled(&journal(), BY, on, wol_core::journal::now())
            .map(drop)
            .map_err(|_| ActionError::Failed)
    }
    fn set_helper_capability(&self, id: &str, allowed: bool) -> Result<(), ActionError> {
        wol_core::automation::set_capability(&journal(), BY, id, allowed, wol_core::journal::now())
            .map(drop)
            .map_err(|_| ActionError::Failed)
    }
    fn allow_helper_always(&self, helper: &str, cap: &str) -> Result<(), ActionError> {
        wol_core::automation::allow_always(&journal(), BY, helper, cap, wol_core::journal::now())
            .map(drop)
            .map_err(|_| ActionError::Failed)
    }
    fn history(&self) -> Vec<wol_core::journal::Entry> {
        let mut e = journal().entries();
        e.reverse();
        e
    }
    fn undo(&self, id: Option<u64>) -> Result<wol_core::journal::Entry, String> {
        let e = journal()
            .undo(id, BY, wol_core::journal::now())
            .map_err(|e| crate::copy::undo(&e).to_string())?;
        after_undo(&self.inner.c, &e);
        Ok(e)
    }
    fn boot_environments(&self) -> Vec<wol_core::update::be::BootEnv> {
        wol_core::update::be::read(&*self.inner.c.host).unwrap_or_default()
    }
    fn approve(&self, req: shared::shell::ApprovalRequest, password: String) -> Done {
        let (tx, rx) = async_channel::bounded(1);
        let i = self.inner.clone();
        let _ = std::thread::Builder::new()
            .name("wol-approve".into())
            .spawn(move || {
                let r = approve(&i.c, &req, &password);
                // Reply first: a wrong password's sentence and a right one's closing sheet waited
                // on a refresh of every topic (hunt 1 C1). Then only what the request changed.
                let ok = r.is_ok();
                let _ = tx.try_send(r);
                let due = approval_topics(&req);
                if ok && due.contains(&true) {
                    i.refresh_now(due);
                }
            });
        rx
    }
    fn refresh_audio(&self) -> Done {
        let (tx, rx) = async_channel::bounded(1);
        let i = self.inner.clone();
        let _ = std::thread::Builder::new()
            .name("wol-audio".into())
            .spawn(move || {
                i.refresh_audio();
                let _ = tx.try_send(Ok(()));
            });
        rx
    }
    /// Starts fast refresh and refreshes the due topics now (the tick thread may be asleep for
    /// the idle period; Control Center-only topics were never read while hidden).
    fn watch(&self) -> Option<async_channel::Receiver<()>> {
        let (tx, rx) = async_channel::bounded(1);
        self.inner
            .watchers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(tx);
        let i = self.inner.clone();
        let _ = std::thread::Builder::new()
            .name("wol-controls-open".into())
            .spawn(move || i.refresh(false));
        Some(rx)
    }
}

/// MPRIS player name -> app icon (dock pin, installed entry, else the name), read once per player.
fn player_icon(player: &str) -> String {
    static SEEN: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, icon)) = seen.iter().find(|(p, _)| p == player) {
        return icon.clone();
    }
    let icon = crate::apps::icon_for(player, crate::apps::PINS, &crate::desktop::load_all());
    seen.push((player.to_string(), icon.clone()));
    icon
}

fn config_home() -> std::path::PathBuf {
    crate::kit::config_home().into()
}

/// Journal actor for changes made in Settings, Control Center and the shell.
const BY: &str = "settings";

fn journal() -> wol_core::journal::Journal {
    wol_core::journal::Journal::for_config(&config_home())
}

/// A helper's admin request after the user typed the password on the approval sheet. The session's
/// cached authorization is forgotten first (the password is always checked) and again after (the
/// helper is left nothing to reuse).
pub fn approve(
    c: &Controls,
    req: &shared::shell::ApprovalRequest,
    password: &str,
) -> Result<(), ActionError> {
    use shared::admin::AdminAction;
    use shared::shell::ApprovalRequest as R;
    // User-level changes run in the helper after Allow; never here, and wol-admind never hears of them.
    if !req.is_admin() {
        return Err(ActionError::Unavailable);
    }
    // Audit S7: system packages from wold (ADR 0021) too. The typed password is always checked:
    // a warm session cache (FRESH_PASSWORD's Cached rows) would otherwise accept any text.
    c.revoke();
    let r = match req {
        R::Packages { verb, names } => packages(c, verb, names, password),
        R::Wifi { on } => c.set_wifi(*on, password),
        R::WifiJoin { ssid, password: p } => c.wifi_join(ssid, p.as_deref(), password),
        R::Bluetooth { on } => c.set_bluetooth(*on, password),
        R::BootEnvironment { name } => c.boot_environment(name, password),
        // Audit C7-1 (ADR 0034): wold's staged copy, Undo Update and the channel run in
        // wol-admind, which checks that this account is an administrator and the password.
        // The SHA-256 the sheet showed goes along: wol-admind installs only that file (ADR 0034).
        R::PackageFile { staged, sha256, .. } => {
            admin_one(c, AdminAction::PkgInstallFile, &[staged, sha256], password)
        }
        R::SystemRollback => admin_one(c, AdminAction::SystemRollback, &[], password),
        R::SystemChannel { channel } => {
            admin_one(c, AdminAction::SystemChannel, &[channel], password)
        }
        // ADR 0048: a Doctor fix, the check's wol-admind requests in order, as `wol doctor fix`
        // sends them; only Doctor's actions (wold relayed nothing else; checked again here).
        R::DoctorFix { .. } => match req.doctor_steps() {
            Some(steps) => steps.iter().try_for_each(|r| c.admin_request(r, password)),
            None => Err(wol_core::controls::ControlError::Unavailable),
        },
        // Refused above; answer Unavailable rather than panic.
        R::Change { .. } => Err(wol_core::controls::ControlError::Unavailable),
    };
    c.revoke();
    // No password means no approval: the sheet asks again instead of reporting "unavailable".
    r.map_err(|e| match ActionError::from(e) {
        ActionError::NeedsAuth => ActionError::Denied,
        e => e,
    })
}

/// One wol-admind request with the typed password.
fn admin_one(
    c: &Controls,
    action: shared::admin::AdminAction,
    args: &[&String],
    password: &str,
) -> Result<(), wol_core::controls::ControlError> {
    let args = args.iter().map(|a| (*a).clone()).collect();
    c.admin_request(&shared::admin::AdminRequest { action, args }, password)
}

/// One wol-admind request per package name. `update` with names installs or upgrades each one
/// (`-S --needed`, as an install: audit S2); without names, everything. `update-holding` is the
/// full update that leaves `names` (the person's pins) where they are.
fn packages(
    c: &Controls,
    verb: &str,
    names: &[String],
    password: &str,
) -> Result<(), wol_core::controls::ControlError> {
    use wol_core::controls::ControlError;
    match (verb, names.is_empty()) {
        ("install" | "update", false) => names.iter().try_for_each(|n| c.pkg_install(n, password)),
        ("remove", false) => names.iter().try_for_each(|n| c.pkg_remove(n, password)),
        ("update", true) => c.pkg_upgrade(password),
        ("update-holding", false) => c.admin_request(
            &shared::admin::AdminRequest {
                action: shared::admin::AdminAction::PkgUpgrade,
                args: names.to_vec(),
            },
            password,
        ),
        _ => Err(ControlError::Invalid),
    }
}

/// `systemctl --user restart wireplumber`: it reads the user fragments again (Settings > Sound).
const RESTART_WIREPLUMBER: [&str; 4] = ["systemctl", "--user", "restart", "wireplumber"];

/// What an undo touched follows it: other apps restyle, shell prefs and compositor reload, and
/// per topic what a reload does not reach (QA 0.12.0 bug 9): Sound's fragments are read by
/// WirePlumber at its start; desktop keywords set live and open windows re-tiled.
fn after_undo(c: &Controls, e: &wol_core::journal::Entry) {
    reload_after_undo(c, &wol_core::journal::reload_for(e));
    match e.what.as_str() {
        "sound" => {
            let _ = c.host.run(&RESTART_WIREPLUMBER);
        }
        "desktop" => {
            let cmds = desktop_undo_cmds(e, || {
                c.host
                    .run(&["hyprctl", "-j", "clients"])
                    .unwrap_or_default()
            });
            if !cmds.is_empty() {
                let _ = c.host.run(&["hyprctl", "--batch", &cmds.join(" ; ")]);
            }
        }
        _ => {}
    }
}

/// The compositor commands that move the session from the desktop prefs an undo replaced to the
/// ones it brought back: the changed keywords, and on a tiling switch the open windows (`clients`,
/// read only then).
fn desktop_undo_cmds(
    e: &wol_core::journal::Entry,
    clients: impl FnOnce() -> String,
) -> Vec<String> {
    use wol_core::hyprprefs::{DesktopPrefs, PREFS_FILE, retile};
    let Some((before, after)) = e.changes.iter().find_map(|c| match c {
        wol_core::journal::Change::File {
            path,
            before,
            after,
        } if path == PREFS_FILE => Some((before.as_deref(), after.as_deref())),
        _ => None,
    }) else {
        return Vec::new();
    };
    let (old, new) = (
        DesktopPrefs::from_files(before, None),
        DesktopPrefs::from_files(after, None),
    );
    let mut cmds = new.live(&old);
    if new.tiling != old.tiling {
        cmds.extend(retile(&clients(), new.tiling, &new.float_apps));
    }
    cmds
}

fn reload_after_undo(c: &Controls, r: &wol_core::journal::Reload) {
    use wol_core::theme::{accent, active};
    if let Some(saved) = &r.theme {
        let dark = accent::auto_is_dark(crate::kit::now().hour);
        let id = active::effective(accent::effective(saved.as_deref(), dark).as_deref(), dark);
        if let Err(e) = wol_core::theme::apps::apply(&id, &config_home()) {
            eprintln!("wol: app themes: {e}");
        }
    }
    if r.shell {
        crate::kit::spawn(&["wol".into(), "shell".into(), "reload".into()]);
    }
    if r.hyprland {
        let _ = c.host.run(&["hyprctl", "reload"]);
    }
}

/// `WOL_FAKE_SYSTEM=1` -> `Fake`, else [`CoreSystem`].
pub fn default_system() -> std::rc::Rc<dyn System> {
    if std::env::var_os("WOL_FAKE_SYSTEM").is_some() {
        std::rc::Rc::new(Fake)
    } else {
        std::rc::Rc::new(CoreSystem::new())
    }
}

#[cfg(test)]
mod tests;
