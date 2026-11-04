// SPDX-License-Identifier: GPL-2.0-or-later
//! What the panes read from the machine, behind one trait: [`Live`] asks the system through
//! `wol_core` readers (always off the UI thread, see `super::later`); [`Demo`] is a fixed laptop so
//! golden renders and screenshots never depend on the build machine.
use std::sync::Arc;
use wol_core::bluetooth::{BtDevice, BtKind, BtState};
use wol_core::netinfo::{IfKind, Interface, NetInfo};
use wol_core::power::{BatteryInfo, Charge, Lid, Profile};
use wol_core::printers::Printer;
use wol_core::storage::{Category, Storage};
use wol_core::system::Host as Os;

pub use shared::hardware::{Device, HardwareSnapshot, Kind, State};

/// Sharing facts (General > Sharing, Privacy & Security). None = cannot tell here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sharing {
    pub remote_login: Option<bool>,
    pub airplay_receiver: Option<bool>,
    pub firewall: Option<bool>,
}

/// Kernel facts for About.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Kernel {
    /// `uname -r` (/proc/sys/kernel/osrelease): "6.17.1-wolos".
    pub release: String,
}

impl Kernel {
    /// "WolKernel 6.17.1" on the WolOS kernel (release ends in `-wolos`), else "Linux 6.17.1-arch1-1".
    pub fn label(&self) -> String {
        match self.release.strip_suffix("-wolos") {
            Some(v) => format!("WolKernel {v}"),
            None if self.release.is_empty() => String::new(),
            None => format!("Linux {}", self.release),
        }
    }
}

pub trait Machine: Send + Sync {
    fn storage(&self) -> Option<Storage>;
    fn battery(&self) -> Option<BatteryInfo>;
    /// Energy profile (hwpstate EPP); None without one.
    fn profile(&self) -> Option<Profile>;
    fn lid(&self) -> Option<Lid>;
    fn zones(&self) -> Vec<String>;
    fn zone(&self) -> String;
    fn ntp(&self) -> Option<bool>;
    fn net(&self) -> NetInfo;
    fn known_networks(&self) -> Vec<String>;
    fn bluetooth(&self) -> BtState;
    fn printers(&self) -> Option<Vec<Printer>>;
    /// Device URIs CUPS can see (slow: probes the network).
    fn discover_printers(&self) -> Vec<String>;
    fn hardware(&self) -> HardwareSnapshot;
    /// The installed WolOS version (`/etc/os-release` VERSION_ID). None: not known here (demo
    /// machines), About shows the build's own.
    fn os_version(&self) -> Option<String> {
        None
    }
    /// The devices as they are now (detects; slow), for the page after a Fix: the boot snapshot
    /// [`Self::hardware`] reads still lists what Fix just mended (QA-SET-7).
    fn hardware_live(&self) -> HardwareSnapshot {
        self.hardware()
    }
    fn sharing(&self) -> Sharing;
    /// Compositor consent grants: (app path, capability) (`wol_core::consent`).
    fn consent(&self) -> Vec<(String, String)>;
    /// This account's apps wold refused a grant (`wol_core::consent::asked`): shown with their
    /// switch off. None by default.
    fn asked(&self) -> Vec<(String, String)> {
        Vec::new()
    }
    /// Flatpak apps with the permissions they were granted (`wol_core::flatpak`).
    fn flatpak_apps(&self) -> Vec<wol_core::flatpak::App>;
    fn kernel(&self) -> Kernel;
    /// Energy mode, CPU steering and what the power plan does now (`wol_core::power::policy`).
    fn power(&self) -> Option<wol_core::power::policy::Status>;
    /// Firewall, remote login and kernel protections (`wol_core::security`).
    fn security(&self) -> Option<wol_core::security::Status>;
    /// The malware check and what it stopped (`wol_core::malware`). None: nothing to show here.
    fn malware(&self) -> Option<wol_core::malware::Status> {
        None
    }
    /// The daily update check's findings; None before the first check.
    fn updates(&self) -> Option<wol_core::update::check::Updates>;
    /// Installed with one copy of WolOS (ADR 0023, `wol_core::update::SINGLE_COPY`): Update Now
    /// changes the running system and keeps no restore point.
    fn single_copy(&self) -> bool {
        false
    }
    /// /etc/passwd text (accounts and their login shells).
    fn passwd(&self) -> String;
    /// Automatic updates: (download system updates, install app updates) (`wol_core::update::auto`).
    fn auto_updates(&self) -> (bool, bool);
    /// `wol boot status` (Startup Security). None where `wol` cannot tell.
    fn boot(&self) -> Option<wol_core::boot::Status> {
        None
    }
    /// This computer's key waits for the firmware's key screen (boot-enroll's mark).
    fn boot_key_pending(&self) -> bool {
        false
    }
    /// `wol modules list --json` (ADR 0032). None where `wol` cannot tell.
    fn modules(&self) -> Option<Vec<wol_core::modules::Status>> {
        None
    }
    /// What modules-install gave up on and is still not installed (`super::modules::GAVE_UP_FILE`).
    fn modules_gave_up(&self) -> Vec<super::modules::GaveUp> {
        Vec::new()
    }
    /// A fixed sound state for previews; None on the running system, where the sound service
    /// streams it (`super::sound`).
    fn sound(&self) -> Option<super::sound::SoundState> {
        None
    }
    /// Sound settings the sound service does not report (balance, autoswitch, user fragments).
    fn sound_extras(&self) -> super::sound::Extras {
        super::sound::Extras::default()
    }
}

/// The running system.
pub struct Live(pub Arc<dyn Os>);

impl Live {
    pub fn new() -> Self {
        Live(Arc::new(wol_core::system::PosixHost))
    }
    fn os(&self) -> &dyn Os {
        &*self.0
    }
    fn wlan(&self) -> String {
        self.net()
            .interfaces
            .into_iter()
            .find(|i| i.kind == IfKind::WiFi)
            .map_or_else(|| "wlan0".into(), |i| i.name)
    }
}

impl Default for Live {
    fn default() -> Self {
        Self::new()
    }
}

impl Machine for Live {
    fn auto_updates(&self) -> (bool, bool) {
        use wol_core::update::auto;
        let os = self.os();
        let apps = os.run(&["systemctl", "--user", "is-enabled", auto::APPS_TIMER]);
        (
            auto::downloads(os.read_text(auto::FILE).as_deref()),
            auto::apps_enabled(apps.as_deref()),
        )
    }
    fn storage(&self) -> Option<Storage> {
        wol_core::storage::read(self.os())
    }
    fn battery(&self) -> Option<BatteryInfo> {
        wol_core::power::battery(self.os())
    }
    fn profile(&self) -> Option<Profile> {
        wol_core::power::profile(self.os())
    }
    fn lid(&self) -> Option<Lid> {
        wol_core::power::lid(self.os())
    }
    fn zones(&self) -> Vec<String> {
        wol_core::region::zones(self.os())
    }
    fn zone(&self) -> String {
        wol_core::region::current_zone(self.os())
    }
    fn ntp(&self) -> Option<bool> {
        wol_core::region::ntp_enabled(self.os())
    }
    fn net(&self) -> NetInfo {
        wol_core::netinfo::read(self.os())
    }
    fn known_networks(&self) -> Vec<String> {
        wol_core::netinfo::known_networks(self.os(), &self.wlan())
    }
    fn bluetooth(&self) -> BtState {
        wol_core::bluetooth::read(self.os())
    }
    fn printers(&self) -> Option<Vec<Printer>> {
        wol_core::printers::read(self.os())
    }
    fn discover_printers(&self) -> Vec<String> {
        wol_core::printers::discover(self.os())
    }
    fn hardware(&self) -> HardwareSnapshot {
        // The boot step's snapshot is fresh enough and instant; detect only without one.
        self.os()
            .read_text(shared::hardware::SNAPSHOT_PATH)
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_else(|| wol_core::hardware::read(self.os()))
    }
    fn hardware_live(&self) -> HardwareSnapshot {
        wol_core::hardware::read(self.os())
    }
    fn os_version(&self) -> Option<String> {
        wol_core::base::version(&self.os().read_text("/etc/os-release")?)
    }
    fn sharing(&self) -> Sharing {
        Sharing {
            remote_login: wol_core::sharing::remote_login(self.os()),
            airplay_receiver: wol_core::sharing::airplay_receiver(self.os()),
            firewall: wol_core::sharing::firewall(self.os()),
        }
    }
    fn consent(&self) -> Vec<(String, String)> {
        wol_core::consent::read(self.os())
    }
    fn asked(&self) -> Vec<(String, String)> {
        let path = wol_core::consent::asked_path(shared::sys::euid());
        self.os()
            .read_text(&path.to_string_lossy())
            .map(|t| wol_core::consent::asked(&t))
            .unwrap_or_default()
    }
    fn flatpak_apps(&self) -> Vec<wol_core::flatpak::App> {
        let home = std::env::var("HOME").unwrap_or_default();
        wol_core::flatpak::apps(self.os(), &home)
    }
    fn kernel(&self) -> Kernel {
        Kernel {
            release: self
                .os()
                .read_text("/proc/sys/kernel/osrelease")
                .map(|r| r.trim().to_string())
                .unwrap_or_default(),
        }
    }
    fn power(&self) -> Option<wol_core::power::policy::Status> {
        use wol_core::power::policy;
        let os = self.os();
        let facts = policy::facts(&|argv: &[&str]| os.run(argv));
        let state = policy::State::parse(os.read_text(policy::STATE_FILE).as_deref());
        Some(policy::status(&facts, &state))
    }
    fn security(&self) -> Option<wol_core::security::Status> {
        let os = self.os();
        Some(wol_core::security::status(&|argv: &[&str]| os.run(argv)))
    }
    fn malware(&self) -> Option<wol_core::malware::Status> {
        let os = self.os();
        let read = |p: &str| std::fs::read_to_string(p).ok();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let uid = wol_core::malware::my_uid();
        Some(wol_core::malware::status(
            &|argv: &[&str]| os.run(argv),
            &read,
            uid,
            now,
        ))
    }
    fn updates(&self) -> Option<wol_core::update::check::Updates> {
        use wol_core::update::check::{DIR, OS_RELEASE, USERLAND_ARGV, Updates};
        let os = self.os();
        let keys = wol_core::package::read_keys(std::path::Path::new(shared::package::KEYS_DIR));
        let f = |n: &str| os.read_text(&format!("{DIR}/{n}")).unwrap_or_default();
        let u = Updates::from_files_at(f, &keys, crate::focus::unix_now())?;
        // No manifest in the image (v0.12.0): os-release names the running release.
        let userland = u
            .base
            .as_ref()
            .and_then(|_| os.run(&USERLAND_ARGV).or_else(|| os.read_text(OS_RELEASE)));
        Some(u.installed(userland.as_deref()))
    }
    fn single_copy(&self) -> bool {
        self.os().read_text(wol_core::update::SINGLE_COPY).is_some()
    }
    fn passwd(&self) -> String {
        self.os().read_text("/etc/passwd").unwrap_or_default()
    }
    fn boot(&self) -> Option<wol_core::boot::Status> {
        let out = self.os().run(&["wol", "boot", "status"])?;
        serde_json::from_str(out.trim()).ok()
    }
    fn boot_key_pending(&self) -> bool {
        std::path::Path::new(crate::install::MOK_PENDING).exists()
    }
    fn modules(&self) -> Option<Vec<wol_core::modules::Status>> {
        let out = self.os().run(&["wol", "modules", "list", "--json"])?;
        serde_json::from_str(out.trim()).ok()
    }
    fn modules_gave_up(&self) -> Vec<super::modules::GaveUp> {
        let path = std::path::Path::new(super::modules::GAVE_UP_FILE);
        let text =
            shared::bounded::read_text(path, super::modules::GAVE_UP_MAX).unwrap_or_default();
        // A package the person installed since (Retry, the Store) is no longer given up on; a
        // module is checked against the module list (`super::modules::render`).
        super::modules::parse_gave_up(&text)
            .into_iter()
            .filter(|g| {
                g.module
                    || self
                        .os()
                        .run(&["pacman", "-Q", "--", g.name.as_str()])
                        .is_none()
            })
            .collect()
    }
    fn sound_extras(&self) -> super::sound::Extras {
        use super::sound::{AWAKE_FILE, Extras, SOFT_VOLUME_FILE, parse_autoswitch, parse_balance};
        let os = self.os();
        let conf = |rel: &str| {
            std::path::Path::new(&crate::kit::config_home())
                .join(rel)
                .is_file()
        };
        let setting = "bluetooth.autoswitch-to-headset-profile";
        Extras {
            balance: parse_balance(
                &os.run(&["pactl", "get-sink-volume", "@DEFAULT_SINK@"])
                    .unwrap_or_default(),
            ),
            autoswitch: parse_autoswitch(
                &os.run(&["wpctl", "settings", setting]).unwrap_or_default(),
            ),
            keep_awake: conf(AWAKE_FILE),
            soft_volume: conf(SOFT_VOLUME_FILE),
            bluetooth: self
                .bluetooth()
                .devices
                .into_iter()
                .filter(|d| d.connected)
                .map(|d| (d.name, d.addr))
                .collect(),
        }
    }
}

/// A 14-inch laptop on WolOS: the same facts every render.
pub struct Demo;

/// What the demo Machine says about the automatic checks and installs (both on).
const DEMO_AUTO_UPDATES: (bool, bool) = (true, true);

const GB: u64 = 1_000_000_000;

impl Machine for Demo {
    fn auto_updates(&self) -> (bool, bool) {
        DEMO_AUTO_UPDATES
    }
    fn storage(&self) -> Option<Storage> {
        let c = |n: &str, gb: u64| Category {
            name: n.into(),
            bytes: gb * GB,
        };
        Some(Storage {
            name: "zroot".into(),
            total: 512 * GB,
            used: 143 * GB,
            categories: vec![
                c("Documents", 61),
                c("Apps", 38),
                c("System", 24),
                c("Restore points", 12),
                c("Other", 8),
            ],
            encrypted: Some(true),
            snapshots: 12 * GB,
            boot_environments: 3,
            zfs: true,
        })
    }
    fn battery(&self) -> Option<BatteryInfo> {
        Some(BatteryInfo {
            percent: 82,
            charge: Charge::Discharging,
            health: Some(94),
            cycles: Some(212),
            minutes_left: Some(205),
            design_mwh: Some(57_000),
            full_mwh: Some(53_580),
        })
    }
    fn profile(&self) -> Option<Profile> {
        Some(Profile::Automatic)
    }
    fn lid(&self) -> Option<Lid> {
        Some(Lid::Sleep)
    }
    fn zones(&self) -> Vec<String> {
        [
            "America/New_York",
            "America/Sao_Paulo",
            "Europe/Lisbon",
            "Europe/London",
            "Asia/Tokyo",
        ]
        .map(String::from)
        .to_vec()
    }
    fn zone(&self) -> String {
        "Europe/Lisbon".into()
    }
    fn ntp(&self) -> Option<bool> {
        Some(true)
    }
    fn net(&self) -> NetInfo {
        NetInfo {
            interfaces: vec![
                Interface {
                    name: "wlan0".into(),
                    kind: IfKind::WiFi,
                    up: true,
                    mac: "a4:c3:f0:12:7b:9e".into(),
                    ipv4: vec!["192.168.1.24/24".into()],
                    ipv6: vec!["2001:db8::24/64".into()],
                },
                Interface {
                    name: "em0".into(),
                    kind: IfKind::Ethernet,
                    up: false,
                    mac: "8c:16:45:aa:01:3c".into(),
                    ..Default::default()
                },
            ],
            dns: vec!["192.168.1.1".into(), "9.9.9.9".into()],
            search: vec!["home.arpa".into()],
            gateway: Some("192.168.1.1".into()),
        }
    }
    fn known_networks(&self) -> Vec<String> {
        vec!["Wonderland".into(), "Mad Hatter Café".into()]
    }
    fn bluetooth(&self) -> BtState {
        let d = |a: &str, n: &str, kind, connected| BtDevice {
            addr: a.into(),
            name: n.into(),
            kind,
            paired: true,
            connected,
        };
        BtState {
            present: true,
            powered: true,
            devices: vec![
                // Same devices as the Fake system's paired list (Control Center goldens).
                d("aa:bb:cc:dd:ee:00", "AirPods Pro", BtKind::Audio, true),
                d("aa:bb:cc:dd:ee:01", "MX Keys", BtKind::Keyboard, true),
                d("aa:bb:cc:dd:ee:02", "Magic Trackpad", BtKind::Mouse, false),
            ],
        }
    }
    fn printers(&self) -> Option<Vec<Printer>> {
        Some(vec![
            Printer {
                name: "Brother_HL_L2350DW".into(),
                uri: "ipp://192.168.1.40/ipp/print".into(),
                default: true,
                state: "Idle".into(),
            },
            Printer {
                name: "Office_Laser".into(),
                uri: "ipp://192.168.1.41/ipp/print".into(),
                default: false,
                state: "Stopped".into(),
            },
        ])
    }
    fn discover_printers(&self) -> Vec<String> {
        vec![
            "dnssd://HP%20Color%20LaserJet%20MFP._ipp._tcp.local/".into(),
            "ipp://192.168.1.40/ipp/print".into(),
        ]
    }
    fn hardware(&self) -> HardwareSnapshot {
        let dev = |kind, name: &str, driver: Option<&str>, state, hint: &str| Device {
            kind,
            name: name.into(),
            driver: driver.map(String::from),
            state,
            hint: hint.into(),
            ..Default::default()
        };
        HardwareSnapshot {
            devices: vec![
                dev(
                    Kind::Cpu,
                    "Intel Core i5-1240P",
                    Some("intel_pstate"),
                    State::Ready,
                    "",
                ),
                dev(
                    Kind::Gpu,
                    "Intel Iris Xe Graphics",
                    Some("i915"),
                    State::Ready,
                    "",
                ),
                dev(
                    Kind::Wifi,
                    "Intel Wi-Fi 6E AX211",
                    Some("iwlwifi"),
                    State::Ready,
                    "",
                ),
                dev(
                    Kind::Bluetooth,
                    "Intel Bluetooth",
                    Some("btusb"),
                    State::Ready,
                    "",
                ),
                dev(
                    Kind::Audio,
                    "Realtek ALC287 audio",
                    Some("snd_hda_intel"),
                    State::Ready,
                    "",
                ),
                Device {
                    missing_klds: vec!["uvcvideo".into()],
                    ..dev(
                        Kind::Webcam,
                        "Integrated Camera",
                        None,
                        State::NeedsDriver,
                        "load uvcvideo: wol hw fix --yes",
                    )
                },
                Device {
                    missing_packages: vec!["linux-firmware-realtek".into()],
                    ..dev(
                        Kind::CardReader,
                        "Realtek card reader",
                        Some("rtsx_pci"),
                        State::NeedsFirmware,
                        "install linux-firmware-realtek: wol hw fix --yes",
                    )
                },
                dev(
                    Kind::Input,
                    "Synaptics touchpad",
                    Some("i2c_hid_acpi"),
                    State::Ready,
                    "",
                ),
            ],
        }
    }
    fn sharing(&self) -> Sharing {
        Sharing {
            remote_login: Some(false),
            airplay_receiver: Some(false),
            firewall: Some(true),
        }
    }
    fn consent(&self) -> Vec<(String, String)> {
        vec![
            ("/usr/bin/recorder".into(), "keep-awake".into()),
            ("/usr/bin/ydotoold".into(), "input-injection".into()),
            ("/usr/bin/hello".into(), "notifications".into()),
        ]
    }
    /// An app built in the account's home that asked and was refused: its switch is off.
    fn asked(&self) -> Vec<(String, String)> {
        vec![("/opt/weather/weather".into(), "notifications".into())]
    }
    fn flatpak_apps(&self) -> Vec<wol_core::flatpak::App> {
        let perm = |s, on| (wol_core::flatpak::Perm::parse(s).expect("valid"), on);
        vec![wol_core::flatpak::App {
            id: "org.gimp.GIMP".into(),
            scope: wol_core::flatpak::Scope::User,
            name: "GIMP".into(),
            icon: "org.gimp.GIMP".into(),
            perms: vec![
                perm("sockets=x11", true),
                perm("filesystems=home", true),
                perm("shared=network", false),
                perm("devices=dri", true),
            ],
        }]
    }
    fn kernel(&self) -> Kernel {
        Kernel {
            release: "6.17.1-wolos".into(),
        }
    }
    fn power(&self) -> Option<wol_core::power::policy::Status> {
        use wol_core::power::policy::{Cpu, Mode, Source, Status};
        Some(Status {
            source: Source::Battery,
            mode: Mode::Auto,
            lid: Some(Lid::Sleep),
            battery: Some(82),
            cpu: Cpu::Epp,
            epp: Some(128),
            cx_lowest: Some("Cmax".into()),
            sleep: true,
            wifi_power_save: vec!["wlan0".into()],
            charge_limit: None,
        })
    }
    fn security(&self) -> Option<wol_core::security::Status> {
        Some(wol_core::security::Status {
            firewall: Some(true),
            firewall_running: Some(true),
            remote_login: Some(false),
            system_log_private: true,
            hardening: wol_core::security::HARDENING
                .iter()
                .map(|(k, v, _)| wol_core::security::Check {
                    key: (*k).into(),
                    want: (*v).into(),
                    now: Some((*v).into()),
                })
                .collect(),
        })
    }
    fn updates(&self) -> Option<wol_core::update::check::Updates> {
        Some(wol_core::update::check::Updates {
            checked: wol_core::journal::now().saturating_sub(3 * 3600),
            packages: vec!["firefox".into(), "curl".into(), "mesa".into()],
            security: vec!["curl".into()],
            base: Some("2026.11".into()),
            ..Default::default()
        })
    }
    fn passwd(&self) -> String {
        "root:*:0:0:Charlie &:/root:/bin/sh\n\
         alice:*:1001:1001:Alice Liddell:/home/alice:/usr/bin/zsh\n\
         dinah:*:1002:1002:Dinah:/home/dinah:/usr/bin/zsh\n"
            .into()
    }
    fn boot(&self) -> Option<wol_core::boot::Status> {
        Some(wol_core::boot::Status {
            secure_boot: true,
            setup_mode: false,
            tier: "mok".into(),
            lockdown: "integrity".into(),
            tpm2: true,
            measured: true,
            sealed: true,
            epoch: Some(12),
            floor: 12,
        })
    }
    fn modules(&self) -> Option<Vec<wol_core::modules::Status>> {
        let m = |id: &str, name: &str, summary: &str, size_mb, installed, enabled| {
            wol_core::modules::Status {
                id: id.into(),
                name: name.into(),
                summary: summary.into(),
                category: String::new(),
                size_mb,
                package: format!("{}{id}", wol_core::modules::PACKAGE_PREFIX),
                installed,
                enabled,
                pending: false,
                needs_enable: true,
                offline: false,
                needs: Vec::new(),
                backend: None,
            }
        };
        Some(vec![
            m(
                "dev",
                "Developer Tools",
                "Compilers, Git and version managers",
                900,
                true,
                true,
            ),
            m(
                "ai-claude",
                "AI Assistants",
                "The AI coding assistant in the terminal",
                120,
                true,
                false,
            ),
            m(
                "creative",
                "Craft Suite",
                "Photo, video and 3D apps",
                2400,
                false,
                false,
            ),
            m(
                "console",
                "Console",
                "A themed prompt and terminal tools",
                60,
                false,
                false,
            ),
        ])
    }
    fn sound(&self) -> Option<super::sound::SoundState> {
        Some(super::sound::SoundState::demo())
    }
    fn sound_extras(&self) -> super::sound::Extras {
        super::sound::Extras::demo()
    }
}

/// [`Demo`] at the edges that goldens and checks render: the module list cannot be read (or, with
/// `gave_up`, it can and modules-install gave up on a module and a media format), and this
/// computer's key still has to reach the firmware (`queued` turns true once the key is queued, as
/// boot-enroll's mark does).
#[derive(Default)]
pub struct DemoEdge {
    pub queued: std::sync::atomic::AtomicBool,
    pub gave_up: bool,
    /// Installed with one copy of WolOS (Software Update says there is no restore point).
    pub one_copy: bool,
}

impl Machine for DemoEdge {
    fn storage(&self) -> Option<Storage> {
        Demo.storage()
    }
    fn battery(&self) -> Option<BatteryInfo> {
        Demo.battery()
    }
    fn profile(&self) -> Option<Profile> {
        Demo.profile()
    }
    fn lid(&self) -> Option<Lid> {
        Demo.lid()
    }
    fn zones(&self) -> Vec<String> {
        Demo.zones()
    }
    fn zone(&self) -> String {
        Demo.zone()
    }
    fn ntp(&self) -> Option<bool> {
        Demo.ntp()
    }
    fn net(&self) -> NetInfo {
        Demo.net()
    }
    fn known_networks(&self) -> Vec<String> {
        Demo.known_networks()
    }
    fn bluetooth(&self) -> BtState {
        Demo.bluetooth()
    }
    fn printers(&self) -> Option<Vec<Printer>> {
        Demo.printers()
    }
    fn discover_printers(&self) -> Vec<String> {
        Demo.discover_printers()
    }
    fn hardware(&self) -> HardwareSnapshot {
        Demo.hardware()
    }
    fn sharing(&self) -> Sharing {
        Demo.sharing()
    }
    fn consent(&self) -> Vec<(String, String)> {
        Demo.consent()
    }
    fn asked(&self) -> Vec<(String, String)> {
        Demo.asked()
    }
    fn flatpak_apps(&self) -> Vec<wol_core::flatpak::App> {
        Demo.flatpak_apps()
    }
    fn kernel(&self) -> Kernel {
        Demo.kernel()
    }
    fn power(&self) -> Option<wol_core::power::policy::Status> {
        Demo.power()
    }
    fn security(&self) -> Option<wol_core::security::Status> {
        Demo.security()
    }
    fn updates(&self) -> Option<wol_core::update::check::Updates> {
        Demo.updates()
    }
    fn single_copy(&self) -> bool {
        self.one_copy
    }
    fn passwd(&self) -> String {
        Demo.passwd()
    }
    fn auto_updates(&self) -> (bool, bool) {
        DEMO_AUTO_UPDATES
    }
    fn boot(&self) -> Option<wol_core::boot::Status> {
        Demo.boot().map(|b| wol_core::boot::Status {
            tier: "firmware".into(),
            ..b
        })
    }
    fn boot_key_pending(&self) -> bool {
        self.queued.load(std::sync::atomic::Ordering::Relaxed)
    }
    fn modules(&self) -> Option<Vec<wol_core::modules::Status>> {
        if self.gave_up { Demo.modules() } else { None }
    }
    fn modules_gave_up(&self) -> Vec<super::modules::GaveUp> {
        if self.gave_up {
            super::modules::parse_gave_up("m creative\np gst-libav\n")
        } else {
            Vec::new()
        }
    }
    fn sound(&self) -> Option<super::sound::SoundState> {
        Demo.sound()
    }
    fn sound_extras(&self) -> super::sound::Extras {
        Demo.sound_extras()
    }
}

/// Test machines that differ from [`Demo`] in their hardware: every other required [`Machine`]
/// fact from Demo.
#[cfg(test)]
macro_rules! demo_but_hardware {
    () => {
        $crate::ui::settings::machine::demo_but_hardware!(@
            storage -> Option<wol_core::storage::Storage>,
            battery -> Option<wol_core::power::BatteryInfo>,
            profile -> Option<wol_core::power::Profile>,
            lid -> Option<wol_core::power::Lid>,
            zones -> Vec<String>,
            zone -> String,
            ntp -> Option<bool>,
            net -> wol_core::netinfo::NetInfo,
            known_networks -> Vec<String>,
            bluetooth -> wol_core::bluetooth::BtState,
            printers -> Option<Vec<wol_core::printers::Printer>>,
            discover_printers -> Vec<String>,
            sharing -> $crate::ui::settings::Sharing,
            consent -> Vec<(String, String)>,
            flatpak_apps -> Vec<wol_core::flatpak::App>,
            kernel -> $crate::ui::settings::Kernel,
            power -> Option<wol_core::power::policy::Status>,
            security -> Option<wol_core::security::Status>,
            updates -> Option<wol_core::update::check::Updates>,
            passwd -> String,
            auto_updates -> (bool, bool),
        );
    };
    (@ $($f:ident -> $t:ty),* $(,)?) => {
        $(
            fn $f(&self) -> $t {
                $crate::ui::settings::Machine::$f(&$crate::ui::settings::Demo)
            }
        )*
    };
}
#[cfg(test)]
pub(crate) use demo_but_hardware;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_label() {
        let k = |r: &str| Kernel { release: r.into() }.label();
        assert_eq!(k("6.17.1-wolos"), "WolKernel 6.17.1");
        assert_eq!(k("6.17.1-arch1-1"), "Linux 6.17.1-arch1-1");
        assert_eq!(k(""), "");
    }

    #[test]
    fn sharing_reads_systemd_units() {
        let h = wol_core::system::fake::FakeHost::linux();
        h.command(
            "systemctl show -p UnitFileState --value sshd.service",
            "enabled\n",
        )
        .command(
            "systemctl show -p UnitFileState --value nftables.service",
            "disabled\n",
        );
        let live = Live(Arc::new(h));
        assert_eq!(
            live.sharing(),
            Sharing {
                remote_login: Some(true),
                airplay_receiver: None,
                firewall: Some(false)
            }
        );
        let linux = Live(Arc::new(wol_core::system::fake::FakeHost::new(false)));
        assert_eq!(linux.sharing(), Sharing::default());
    }

    #[test]
    fn the_demo_grants_only_what_the_consent_file_may_hold() {
        let grants = Demo.consent();
        assert!(!grants.is_empty());
        for (app, cap) in &grants {
            assert!(wol_core::consent::is_valid_grant(app, cap), "{app} {cap}");
            assert_eq!(wol_core::consent::refusal(cap), None, "{app} {cap}");
        }
    }

    /// The Hardware page the manual, the previews and the goldens show is a Linux laptop (QA-DEV-16):
    /// kernel module names (`iwlwifi`, `btusb`, `snd_hda_intel`), the camera's `uvcvideo` to turn
    /// on and a real firmware package, not FreeBSD's `hdac0`, `webcamd` or `rtsx-firmware`.
    #[test]
    fn the_demo_hardware_uses_linux_names() {
        let hw = Demo.hardware();
        let drivers: Vec<&str> = hw
            .devices
            .iter()
            .filter_map(|d| d.driver.as_deref())
            .collect();
        for want in [
            "intel_pstate",
            "i915",
            "iwlwifi",
            "btusb",
            "snd_hda_intel",
            "rtsx_pci",
        ] {
            assert!(drivers.contains(&want), "{want} in {drivers:?}");
        }
        let text = format!("{hw:?}");
        for freebsd in [
            "webcamd",
            "hdac0",
            "rtsx-firmware",
            "rtsx0",
            "iichid0",
            "hwpstate",
            "drmn0",
            "ubt0",
            "iwlwifi0",
        ] {
            assert!(
                !text.contains(freebsd),
                "FreeBSD name {freebsd} in the demo hardware"
            );
        }
        let cam = hw.devices.iter().find(|d| d.kind == Kind::Webcam).unwrap();
        assert_eq!(cam.missing_klds, ["uvcvideo"]);
        assert!(cam.missing_packages.is_empty());
        let reader = hw
            .devices
            .iter()
            .find(|d| d.kind == Kind::CardReader)
            .unwrap();
        assert_eq!(reader.missing_packages, ["linux-firmware-realtek"]);
    }
}
