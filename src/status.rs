// SPDX-License-Identifier: GPL-2.0-or-later
//! Status icon selection (port of Iara.Core.Shell.StatusIcons). Static names: no allocation per poll.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatteryStatus {
    Charging,
    Discharging,
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    pub percent: u8,
    pub status: BatteryStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Audio {
    pub volume: Option<u8>,
    pub muted: bool,
    pub mic: Option<u8>,
    pub mic_muted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Network {
    pub online: bool,
    pub wifi: bool,
    pub signal: Option<u8>,
    /// A link, but no internet through it: NetworkManager says limited, or a sign-in page holds
    /// it (portal; QA-NET-12, NET2-3). The glyph says so instead of a plain "connected".
    pub limited: bool,
}

pub fn battery(b: Option<Battery>) -> Option<&'static str> {
    let b = b?;
    if b.status == BatteryStatus::Full {
        return Some("battery-full-charged-symbolic");
    }
    let charging = b.status == BatteryStatus::Charging;
    Some(match (b.percent, charging) {
        (90.., false) => "battery-full-symbolic",
        (90.., true) => "battery-full-charging-symbolic",
        (40.., false) => "battery-good-symbolic",
        (40.., true) => "battery-good-charging-symbolic",
        (15.., false) => "battery-low-symbolic",
        (15.., true) => "battery-low-charging-symbolic",
        (_, false) => "battery-caution-symbolic",
        (_, true) => "battery-caution-charging-symbolic",
    })
}

pub fn volume(a: Audio) -> &'static str {
    match a.volume {
        _ if a.muted => "audio-volume-muted-symbolic",
        None | Some(0) => "audio-volume-muted-symbolic",
        Some(..34) => "audio-volume-low-symbolic",
        Some(..67) => "audio-volume-medium-symbolic",
        _ => "audio-volume-high-symbolic",
    }
}

pub fn mic(a: Audio) -> Option<&'static str> {
    match a.mic? {
        _ if a.mic_muted => Some("microphone-sensitivity-muted-symbolic"),
        0 => Some("microphone-sensitivity-muted-symbolic"),
        _ => Some("microphone-sensitivity-high-symbolic"),
    }
}

pub fn network(n: Network) -> &'static str {
    match (n.online, n.wifi, n.signal) {
        (true, false, _) if n.limited => "network-wired-disconnected-symbolic",
        (true, true, _) if n.limited => "network-wireless-signal-none-symbolic",
        (false, _, _) => "network-offline-symbolic",
        (true, false, _) => "network-wired-symbolic",
        (true, true, None) => "network-wireless-symbolic",
        (true, true, Some(75..)) => "network-wireless-signal-excellent-symbolic",
        (true, true, Some(50..)) => "network-wireless-signal-good-symbolic",
        (true, true, Some(25..)) => "network-wireless-signal-ok-symbolic",
        (true, true, Some(_)) => "network-wireless-signal-weak-symbolic",
    }
}

/// The menu bar's glyph while airplane mode is on (NET-3): every radio is off.
pub const AIRPLANE_GLYPH: &str = "airplane-mode-symbolic";

/// Menu bar network glyph with airplane mode: the plane while it is on and no wired link carries
/// the traffic (a cable still works in airplane mode), else [`menubar_network`].
pub fn menubar_glyph(n: Network, wifi_on: Option<bool>, airplane: Option<bool>) -> &'static str {
    if airplane == Some(true) && (!n.online || n.wifi) {
        AIRPLANE_GLYPH
    } else {
        menubar_network(n, wifi_on)
    }
}

/// Menu bar network glyph: [`network`], except the struck-through Wi-Fi glyph while the radio is
/// off (`wifi_on`, None = no Wi-Fi) and no wired link is up.
pub fn menubar_network(n: Network, wifi_on: Option<bool>) -> &'static str {
    if wifi_on == Some(false) && (!n.online || n.wifi) {
        "network-wireless-offline-symbolic"
    } else {
        network(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use BatteryStatus::*;

    #[test]
    fn battery_icons() {
        for (p, s, w) in [
            (100, Full, "battery-full-charged-symbolic"),
            (95, Discharging, "battery-full-symbolic"),
            (55, Discharging, "battery-good-symbolic"),
            (25, Discharging, "battery-low-symbolic"),
            (5, Discharging, "battery-caution-symbolic"),
            (55, Charging, "battery-good-charging-symbolic"),
        ] {
            assert_eq!(
                battery(Some(Battery {
                    percent: p,
                    status: s
                })),
                Some(w),
                "{p} {s:?}"
            );
        }
        assert_eq!(battery(None), None);
    }

    #[test]
    fn volume_icons() {
        for (v, m, w) in [
            (None, false, "audio-volume-muted-symbolic"),
            (Some(80), true, "audio-volume-muted-symbolic"),
            (Some(0), false, "audio-volume-muted-symbolic"),
            (Some(20), false, "audio-volume-low-symbolic"),
            (Some(50), false, "audio-volume-medium-symbolic"),
            (Some(90), false, "audio-volume-high-symbolic"),
        ] {
            assert_eq!(
                volume(Audio {
                    volume: v,
                    muted: m,
                    ..Default::default()
                }),
                w
            );
        }
    }

    #[test]
    fn mic_hidden_without_device() {
        assert_eq!(mic(Audio::default()), None);
        assert_eq!(
            mic(Audio {
                mic: Some(50),
                mic_muted: true,
                ..Default::default()
            }),
            Some("microphone-sensitivity-muted-symbolic")
        );
    }

    /// Wi-Fi switched off in Control Center: the menu bar shows the struck-through Wi-Fi glyph,
    /// not a generic "offline"; a wired link still wins.
    #[test]
    fn menubar_network_follows_the_wifi_radio() {
        let n = |online, wifi, signal| Network {
            online,
            wifi,
            signal,
            limited: false,
        };
        for (net, radio, want) in [
            (
                n(false, false, None),
                Some(false),
                "network-wireless-offline-symbolic",
            ),
            (n(true, false, None), Some(false), "network-wired-symbolic"),
            (
                n(false, false, None),
                Some(true),
                "network-offline-symbolic",
            ),
            (n(false, false, None), None, "network-offline-symbolic"),
            (
                n(true, true, Some(60)),
                Some(true),
                "network-wireless-signal-good-symbolic",
            ),
        ] {
            assert_eq!(menubar_network(net, radio), want, "{net:?} {radio:?}");
        }
    }

    /// NET-21 airplane: the plane in the menu bar while every radio is off; a cable still shows.
    #[test]
    fn menubar_shows_the_plane_in_airplane_mode() {
        let off = Network::default();
        let wired = Network {
            online: true,
            ..Default::default()
        };
        assert_eq!(menubar_glyph(off, Some(false), Some(true)), AIRPLANE_GLYPH);
        assert_eq!(
            menubar_glyph(wired, Some(false), Some(true)),
            "network-wired-symbolic"
        );
        assert_eq!(
            menubar_glyph(off, Some(false), Some(false)),
            "network-wireless-offline-symbolic"
        );
        assert_eq!(menubar_glyph(off, None, None), "network-offline-symbolic");
    }

    #[test]
    fn network_icons() {
        for (o, wf, s, w) in [
            (false, true, Some(90), "network-offline-symbolic"),
            (true, false, None, "network-wired-symbolic"),
            (true, true, None, "network-wireless-symbolic"),
            (
                true,
                true,
                Some(80),
                "network-wireless-signal-excellent-symbolic",
            ),
            (
                true,
                true,
                Some(10),
                "network-wireless-signal-weak-symbolic",
            ),
        ] {
            assert_eq!(
                network(Network {
                    online: o,
                    wifi: wf,
                    signal: s,
                    limited: false,
                }),
                w
            );
        }
    }

    /// NET2-3: a captive portal or a limited link never shows the plain "connected" glyph.
    #[test]
    fn limited_and_portal_glyphs() {
        let n = |wifi, signal| Network {
            online: true,
            wifi,
            signal,
            limited: true,
        };
        assert_eq!(
            network(n(false, None)),
            "network-wired-disconnected-symbolic"
        );
        assert_eq!(
            network(n(true, Some(90))),
            "network-wireless-signal-none-symbolic"
        );
        assert_eq!(
            menubar_network(n(true, Some(90)), Some(true)),
            "network-wireless-signal-none-symbolic"
        );
    }
}
