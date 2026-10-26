//! System data seam. Topics match IaraOS system-api.md: power, audio, network, brightness.
//! [`CoreSystem`] is the real backend (`wol_core::controls`, Linux and FreeBSD): a worker thread keeps cached
//! snapshots, reads never fork or block, actions run off the UI thread through [`System::spawn`].
//! `Sysfs` reads files only; `Fake` is deterministic for goldens.
use crate::status::{Audio, Battery, BatteryStatus, Network};
use std::fs;
pub use wol_core::controls::{Monitor, Output};

/// One change the user asked for. Root ones go through wol-admind and may answer `NeedsAuth`.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Wifi(bool),
    WifiJoin(String, Option<String>),
    Bluetooth(bool),
    Brightness(u8),
    Volume(u8),
    Muted(bool),
    Output(String),
    NightLight(bool),
    MediaToggle,
    Power(PowerMode),
    /// Hyprland monitor rule (`wol_core::controls::monitor_rule`).
    Monitor(String),
    UserAdd {
        name: String,
        full_name: String,
        admin: bool,
    },
    PkgUpgrade,
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
    /// Admin action: may answer `NeedsAuth`; the UI then shows the Authenticate sheet and calls `authorize`.
    fn wifi_join(&self, _ssid: &str, _password: Option<&str>) -> Result<(), ActionError> {
        Err(ActionError::Unavailable)
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
    fn media_toggle(&self) -> Result<(), ActionError> {
        Err(ActionError::Unavailable)
    }
    fn night_light(&self) -> Option<bool> {
        None
    }
    fn set_night_light(&self, _on: bool) -> Result<(), ActionError> {
        Err(ActionError::Unavailable)
    }
    fn power_mode(&self) -> Option<PowerMode> {
        None
    }
    fn set_power_mode(&self, _m: PowerMode) -> Result<(), ActionError> {
        Err(ActionError::Unavailable)
    }
    /// Admin authorization (IaraOS ADR 0026): password for the pending admin action.
    fn authorize(&self, _password: &str) -> Result<(), ActionError> {
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
    /// Blocking dispatch of one action (backends call this off the UI thread).
    fn perform(&self, a: &Action) -> Result<(), ActionError> {
        match a {
            Action::Wifi(on) => self.set_wifi(*on),
            Action::WifiJoin(s, p) => self.wifi_join(s, p.as_deref()),
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub playing: bool,
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
    Failed,
}

impl From<wol_core::controls::ControlError> for ActionError {
    fn from(e: wol_core::controls::ControlError) -> Self {
        use wol_core::controls::ControlError as C;
        match e {
            C::Unavailable => Self::Unavailable,
            C::NeedsAuth => Self::NeedsAuth,
            C::Denied => Self::Denied,
            C::NotAdmin => Self::NotAdmin,
            C::Invalid | C::Failed => Self::Failed,
        }
    }
}

impl ActionError {
    pub fn message(self) -> &'static str {
        match self {
            ActionError::Unavailable => "Unavailable on this computer",
            ActionError::NeedsAuth => "Administrator password required",
            ActionError::Denied => "Incorrect password",
            ActionError::NotAdmin => "Only administrators can make this change",
            ActionError::Failed => "The change did not work. Try again.",
        }
    }
}

/// Control Center subtitles.
pub fn wifi_subtitle(n: crate::status::Network, nets: &[WifiNetwork]) -> String {
    match nets.iter().find(|w| w.connected) {
        Some(w) => w.ssid.clone(),
        None if n.online && !n.wifi => "Ethernet".into(),
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
        })
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
        }]
    }
}

// ---- Real backend ----

use std::sync::{Arc, Mutex, Weak};
use wol_core::controls::{Controls, Volume, WifiNet};

/// Everything the UI reads; refreshed by the worker, cached to `$XDG_RUNTIME_DIR/wol-controls.json`
/// so a freshly opened Control Center shows real values before the first refresh lands.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub wifi_on: Option<bool>,
    pub nets: Vec<WifiNet>,
    pub volume: Option<Volume>,
    pub outputs: Vec<Output>,
    pub brightness: Option<u8>,
    pub bluetooth: Option<bool>,
    pub monitors: Vec<Monitor>,
    pub night: Option<bool>,
}

/// Seconds between refreshes per topic (system-api.md TTLs).
const TTL_AUDIO: u64 = 2;
const TTL_WIFI: u64 = 5;
const TTL_SLOW: u64 = 10;

struct Inner {
    c: Controls,
    files: Sysfs,
    snap: Mutex<Snapshot>,
    /// Admin password for the next root action (cleared after use).
    password: Mutex<String>,
    watchers: Mutex<Vec<async_channel::Sender<()>>>,
    cache: Option<std::path::PathBuf>,
}

impl Inner {
    fn lock(&self) -> std::sync::MutexGuard<'_, Snapshot> {
        self.snap.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn refresh(&self, tick: u64) {
        let mut s = self.lock().clone();
        let before = s.clone();
        if tick.is_multiple_of(TTL_AUDIO) {
            s.volume = self.c.volume().or(s.volume);
            s.brightness = self.c.brightness();
        }
        if tick.is_multiple_of(TTL_WIFI) {
            s.wifi_on = self.c.wifi_enabled();
            s.nets = self.c.wifi_networks();
        }
        if tick.is_multiple_of(TTL_SLOW) {
            s.outputs = self.c.outputs();
            s.bluetooth = self.c.bluetooth();
            s.monitors = self.c.monitors();
        }
        if s != before {
            *self.lock() = s.clone();
            if let (Some(p), Ok(j)) = (&self.cache, serde_json::to_string(&s)) {
                let tmp = p.with_extension("tmp");
                if fs::write(&tmp, j).is_ok() {
                    let _ = fs::rename(tmp, p);
                }
            }
            self.watchers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|t| !t.is_closed() && (t.try_send(()).is_ok() || t.is_full()));
        }
    }

    fn take_password(&self) -> String {
        std::mem::take(&mut *self.password.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Blocking; runs on a worker thread.
    fn perform(&self, a: &Action) -> Result<(), ActionError> {
        let c = &self.c;
        let pw = self.take_password();
        let r = match a {
            Action::Wifi(on) => c.set_wifi(*on, &pw),
            Action::WifiJoin(ssid, p) => c.wifi_join(ssid, p.as_deref(), &pw),
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
            } => c.add_user(name, full_name, *admin, &pw),
            Action::PkgUpgrade => c.pkg_upgrade(&pw),
            Action::MediaToggle | Action::Power(_) => {
                Err(wol_core::controls::ControlError::Unavailable)
            }
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
                Action::Brightness(p) => s.brightness = Some(*p),
                Action::Wifi(on) => s.wifi_on = Some(*on),
                Action::Bluetooth(on) => s.bluetooth = Some(*on),
                Action::NightLight(on) => s.night = Some(*on),
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

    /// Injected controls (tests use a `FakeHost`). Starts the refresh worker.
    pub fn with(c: Controls, files: Sysfs, cache: Option<std::path::PathBuf>) -> Self {
        let snap = cache
            .as_ref()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let inner = Arc::new(Inner {
            c,
            files,
            snap: Mutex::new(snap),
            password: Default::default(),
            watchers: Default::default(),
            cache,
        });
        let weak: Weak<Inner> = Arc::downgrade(&inner);
        let _ = std::thread::Builder::new()
            .name("wol-controls".into())
            .spawn(move || {
                let mut tick = 0u64;
                while let Some(i) = weak.upgrade() {
                    i.refresh(tick);
                    drop(i);
                    tick += 1;
                    std::thread::sleep(std::time::Duration::from_secs(1));
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
        self.inner.files.battery()
    }
    fn audio(&self) -> Audio {
        let v = self.inner.lock().volume;
        Audio {
            volume: v.map(|v| v.percent),
            muted: v.is_some_and(|v| v.muted),
            mic: None,
            mic_muted: false,
        }
    }
    fn network(&self) -> Network {
        let mut n = self.inner.files.network();
        let s = self.inner.lock();
        if let Some(w) = s.nets.iter().find(|w| w.connected) {
            n.wifi = true;
            n.online = true;
            n.signal = Some(w.signal);
        }
        n
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
    fn night_light(&self) -> Option<bool> {
        let s = self.inner.lock();
        (!s.monitors.is_empty()).then(|| s.night.unwrap_or(false))
    }
    fn authorize(&self, password: &str) -> Result<(), ActionError> {
        *self
            .inner
            .password
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = password.into();
        Ok(())
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
        let (tx, rx) = async_channel::bounded(1);
        let i = self.inner.clone();
        let _ = std::thread::Builder::new()
            .name("wol-action".into())
            .spawn(move || {
                let r = i.perform(&a);
                let _ = tx.try_send(r);
                i.refresh(0);
            });
        rx
    }
    fn watch(&self) -> Option<async_channel::Receiver<()>> {
        let (tx, rx) = async_channel::bounded(1);
        self.inner
            .watchers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(tx);
        Some(rx)
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
mod tests {
    use super::*;

    #[test]
    fn control_center_labels() {
        use crate::status::*;
        let nets = [WifiNetwork {
            ssid: "Home".into(),
            signal: 80,
            secure: true,
            connected: true,
        }];
        assert_eq!(
            wifi_subtitle(
                Network {
                    online: true,
                    wifi: true,
                    signal: None
                },
                &nets
            ),
            "Home"
        );
        assert_eq!(
            wifi_subtitle(
                Network {
                    online: true,
                    wifi: false,
                    signal: None
                },
                &[]
            ),
            "Ethernet"
        );
        assert_eq!(wifi_subtitle(Network::default(), &[]), "Not Connected");
        assert_eq!(
            battery_label(Some(Battery {
                percent: 82,
                status: BatteryStatus::Charging
            }))
            .unwrap(),
            "82% · Charging"
        );
        assert_eq!(battery_label(None), None);
        assert_eq!(signal_icon(30), "network-wireless-signal-ok-symbolic");
    }

    #[test]
    fn battery_files() {
        assert_eq!(
            parse_battery("55\n", "Charging\n"),
            Some(Battery {
                percent: 55,
                status: BatteryStatus::Charging
            })
        );
        assert_eq!(parse_battery("x", "Full"), None);
    }

    #[test]
    fn reads_fake_sysfs_tree() {
        let root = std::env::temp_dir().join(format!("wol-sysfs-{}", std::process::id()));
        let bat = root.join("power_supply/BAT0");
        fs::create_dir_all(&bat).unwrap();
        fs::write(bat.join("capacity"), "20\n").unwrap();
        fs::write(bat.join("status"), "Discharging\n").unwrap();
        let wl = root.join("net/wlan0/wireless");
        fs::create_dir_all(&wl).unwrap();
        fs::write(root.join("net/wlan0/operstate"), "up\n").unwrap();
        let s = Sysfs { root: root.clone() };
        assert_eq!(s.battery().unwrap().percent, 20);
        assert_eq!(
            s.network(),
            Network {
                online: true,
                wifi: true,
                signal: None
            }
        );
        assert_eq!(s.brightness(), None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn core_system_reads_snapshot_and_acts_off_thread() {
        use wol_core::system::fake::FakeHost;
        let h = Arc::new(FakeHost::new(false));
        h.command("wpctl get-volume @DEFAULT_AUDIO_SINK@", "Volume: 0.64\n")
            .command("nmcli radio wifi", "enabled\n")
            .command(
                "nmcli -t -f IN-USE,SIGNAL,SECURITY,SSID dev wifi list --rescan no",
                "*:70:WPA2:Home\n :40::Cafe\n",
            )
            .command("wpctl set-volume @DEFAULT_AUDIO_SINK@ 30%", "");
        let root = std::env::temp_dir().join(format!("wol-core-sys-{}", std::process::id()));
        let cache = root.join("snap.json");
        fs::create_dir_all(&root).unwrap();
        let sys = CoreSystem::with(
            Controls::new(h.clone()),
            Sysfs { root: root.clone() },
            Some(cache.clone()),
        );
        let w = sys.watch().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while sys.audio().volume.is_none() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(sys.audio().volume, Some(64));
        assert!(w.try_recv().is_ok(), "watchers hear about changes");
        assert_eq!(sys.wifi_enabled(), Some(true));
        assert_eq!(wifi_subtitle(sys.network(), &sys.wifi_networks()), "Home");
        assert_eq!(sys.network().signal, Some(70));
        let done = sys.spawn(Action::Volume(30));
        assert_eq!(done.recv_blocking(), Ok(Ok(())));
        assert_eq!(sys.audio().volume, Some(30), "optimistic snapshot");
        assert_eq!(
            sys.spawn(Action::UserAdd {
                name: "dinah".into(),
                full_name: "Dinah".into(),
                admin: false
            })
            .recv_blocking(),
            Ok(Err(ActionError::Unavailable)),
            "no wol-admind on the test host"
        );
        let cached: Snapshot = serde_json::from_str(&fs::read_to_string(&cache).unwrap()).unwrap();
        assert_eq!(cached.nets.len(), 2);
        drop(sys);
        fs::remove_dir_all(root).unwrap();
    }

    /// Manual: `cargo test -p shell --lib live_host -- --ignored --nocapture` prints this machine's snapshot.
    #[test]
    #[ignore]
    fn live_host_snapshot() {
        let sys = CoreSystem::new();
        std::thread::sleep(std::time::Duration::from_secs(3));
        let s = sys.snapshot();
        println!("{s:#?}\nnetwork {:?}", sys.network());
        assert!(s.volume.is_some() || s.wifi_on.is_some());
    }
}
