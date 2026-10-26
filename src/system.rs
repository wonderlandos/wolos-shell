//! System data seam. Topics match IaraOS system-api.md: power, audio, network, brightness.
//! `core` will provide the FreeBSD implementation (sysctl, no forks). `Sysfs` is the Linux dev reader.
use crate::status::{Audio, Battery, BatteryStatus, Network};
use std::fs;

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
    Failed,
}

impl ActionError {
    pub fn message(self) -> &'static str {
        match self {
            ActionError::Unavailable => "Unavailable on this computer",
            ActionError::NeedsAuth => "Administrator password required",
            ActionError::Denied => "Incorrect password",
            ActionError::Failed => "Something went wrong",
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
}

/// `WOL_FAKE_SYSTEM=1` -> `Fake`, else the platform reader.
pub fn default_system() -> std::rc::Rc<dyn System> {
    if std::env::var_os("WOL_FAKE_SYSTEM").is_some() {
        std::rc::Rc::new(Fake)
    } else {
        std::rc::Rc::new(Sysfs::default())
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
}
