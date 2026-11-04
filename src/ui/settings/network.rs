// SPDX-License-Identifier: GPL-2.0-or-later
//! Network: every connection with its addresses, router and DNS (read off the UI thread), kept
//! current while the pane is open, and the Firewall switch (wol-admind `firewall`; the one place
//! it lives, QA-NET-22).
use super::*;
use wol_core::netinfo::{IfKind, Interface, NetInfo};
use wol_core::security;

/// "192.168.1.24/24" -> ("192.168.1.24", "255.255.255.0").
pub(crate) fn split_cidr(a: &str) -> (String, String) {
    let (ip, bits) = a.split_once('/').unwrap_or((a, ""));
    let mask = bits.parse::<u32>().ok().filter(|b| *b <= 32).map(|b| {
        let m: u32 = if b == 0 { 0 } else { u32::MAX << (32 - b) };
        let o = m.to_be_bytes();
        format!("{}.{}.{}.{}", o[0], o[1], o[2], o[3])
    });
    (ip.to_string(), mask.unwrap_or_default())
}

fn kind_label(i: &Interface) -> (&'static str, &'static str) {
    match i.kind {
        IfKind::WiFi => ("network-wireless-symbolic", "Wi-Fi"),
        IfKind::Ethernet => ("network-wired-symbolic", "Ethernet"),
        IfKind::Vpn => ("network-vpn-symbolic", "VPN"),
        IfKind::Other => ("network-wired-symbolic", "Connection"),
    }
}

/// The DNS servers shown under `i`: its own (NetworkManager per device), else the computer's
/// list only under the link that carries the default route. A VPN's resolver was shown under
/// Ethernet next to the LAN's (QA-NET-28, RC-03: one global fact in a per-link place).
pub(crate) fn dns_for<'a>(net: &'a NetInfo, i: &'a Interface) -> &'a [String] {
    if !i.dns.is_empty() {
        &i.dns
    } else if net.route_dev.as_deref() == Some(i.name.as_str()) || net.route_dev.is_none() {
        &net.dns
    } else {
        &[]
    }
}

/// The router is the default route's: shown under the link that carries it only.
pub(crate) fn carries_route(net: &NetInfo, i: &Interface) -> bool {
    net.route_dev.as_deref().is_none_or(|d| d == i.name)
}

/// The connection row's subtitle: limited and portal said so (QA-NET-12, NET2-3).
pub(crate) fn link_state(net: &NetInfo, i: &Interface, on: bool) -> &'static str {
    match (on, carries_route(net, i)) {
        (false, _) => "Not connected",
        (true, true) if net.portal() => crate::copy::settings::NETWORK_PORTAL,
        (true, true) if net.limited() => crate::copy::settings::NETWORK_LIMITED,
        (true, _) => "Connected",
    }
}

fn connected(i: &Interface) -> bool {
    i.up && !(i.ipv4.is_empty() && i.ipv6.is_empty())
}

/// What the pane shows: the connections and the firewall, read together off the UI thread.
#[derive(Clone, Debug, PartialEq)]
struct Snap {
    net: NetInfo,
    security: Option<security::Status>,
}

/// Firewall row text: "On at startup, but not running now" only when the unit is on and the
/// kernel has no table (`security::firewall_running`, QA-NET-2).
pub(crate) fn firewall_sentence(on: bool, running: Option<bool>) -> &'static str {
    if on && running == Some(false) {
        crate::copy::settings::FIREWALL_STOPPED
    } else {
        crate::copy::settings::FIREWALL_ON
    }
}

fn firewall_section(
    e: &Rc<Env>,
    sec: Option<&security::Status>,
    status: &gtk::Label,
) -> Option<gtk::Box> {
    let on = sec?.firewall?;
    admin("firewall", &[])?;
    let (ctx, st) = (e.ctx.clone(), status.clone());
    Some(w::section(
        "Firewall",
        &[form::pending_toggle_row(
            "Firewall",
            firewall_sentence(on, sec.and_then(|s| s.firewall_running)),
            on,
            move |v, done| match admin("firewall", &[if v { "on" } else { "off" }]) {
                Some(a) => ctx.act_switch(a, &st, done),
                None => done(false),
            },
        )],
    ))
}

fn render(e: &Rc<Env>, p: &gtk::Box, snap: Snap, status: &gtk::Label) {
    let net = snap.net;
    if net.interfaces.is_empty() {
        p.append(&w::empty_state(
            "network-offline-symbolic",
            "No Network Connections",
            "Plug in a network cable or turn on Wi-Fi.",
            None,
        ));
    }
    let mut ifs = net.interfaces.clone();
    ifs.sort_by_key(|i| !connected(i));
    for i in &ifs {
        let (icon, kind) = kind_label(i);
        let on = connected(i);
        let mut rows = vec![
            w::list_row(
                icon,
                &format!("{kind} ({})", i.name),
                link_state(&net, i, on),
                None,
            )
            .upcast(),
        ];
        if on {
            for a in &i.ipv4 {
                let (ip, mask) = split_cidr(a);
                rows.push(info("IP address", &ip));
                if !mask.is_empty() {
                    rows.push(info("Subnet mask", &mask));
                }
            }
            if let Some(g) = net.gateway.as_ref().filter(|_| carries_route(&net, i)) {
                rows.push(info("Router", g));
            }
            let dns = dns_for(&net, i);
            if !dns.is_empty() {
                rows.push(info("DNS servers", &dns.join(", ")));
            }
            if !net.search.is_empty() {
                rows.push(info("Search domains", &net.search.join(", ")));
            }
            for a in &i.ipv6 {
                rows.push(info("IPv6 address", a));
            }
        }
        if !i.mac.is_empty() {
            rows.push(info("Hardware address", &i.mac));
        }
        p.append(&w::section(kind, &rows));
    }
    if let Some(f) = firewall_section(e, snap.security.as_ref(), status) {
        p.append(&f);
    }
}

/// Reads `load` off the UI thread on every tick of `ticks` and calls `show` when the answer
/// differs from the last one shown (`first` is that last one). The pane followed no changes
/// before: unplugging a cable left "Connected" on screen until it was reopened (QA-NET-11).
pub(crate) async fn follow<T: PartialEq + Send + 'static>(
    ticks: async_channel::Receiver<()>,
    first: T,
    load: Arc<dyn Fn() -> T + Send + Sync>,
    show: impl Fn(&T),
) {
    let mut last = first;
    while ticks.recv().await.is_ok() {
        // Ticks that piled up while reading are one read.
        while ticks.try_recv().is_ok() {}
        let load = load.clone();
        let Ok(now) = gio::spawn_blocking(move || load()).await else {
            continue;
        };
        if now != last {
            show(&now);
            last = now;
        }
    }
}

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let p = w::page("Network");
    let body = w::vbox(w::S6);
    p.append(&body);
    let status = status_label();
    p.append(&status);
    let (m, e2) = (e.machine(), e.clone());
    let load: Arc<dyn Fn() -> Snap + Send + Sync> = Arc::new(move || Snap {
        net: m.net(),
        security: m.security(),
    });
    let first = load.clone();
    later(
        &body,
        move || first(),
        move |into, snap| {
            let holder = w::vbox(w::S6);
            into.append(&holder);
            render(&e2, &holder, snap.clone(), &status);
            if let Some(ticks) = e2.ctx.sys.watch() {
                let e3 = e2.clone();
                glib::spawn_future_local(follow(ticks, snap, load, move |s: &Snap| {
                    clear(&holder);
                    render(&e3, &holder, s.clone(), &status);
                }));
            }
        },
    );
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eth(up: bool) -> NetInfo {
        NetInfo {
            interfaces: vec![Interface {
                name: "enp0s1".into(),
                kind: IfKind::Ethernet,
                up,
                ipv4: if up {
                    vec!["10.0.2.15/24".into()]
                } else {
                    vec![]
                },
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    /// QA-NET-11: the open pane re-reads on every watch tick and shows only what changed;
    /// ticks that find the same answer do not redraw.
    #[test]
    fn watch_tick_rerenders() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let reads = Arc::new(AtomicUsize::new(0));
        // Still plugged in, pulled (read twice), plugged back.
        let script = [eth(true), eth(false), eth(false), eth(true)];
        let load: Arc<dyn Fn() -> NetInfo + Send + Sync> = {
            let reads = reads.clone();
            Arc::new(move || script[reads.fetch_add(1, Ordering::SeqCst).min(3)].clone())
        };
        let (tx, rx) = async_channel::unbounded();
        let shown = Rc::new(RefCell::new(Vec::new()));
        let ctx = glib::MainContext::new();
        let seen = shown.clone();
        ctx.block_on(async move {
            let f = follow(rx, eth(true), load, move |n: &NetInfo| {
                seen.borrow_mut().push(connected(&n.interfaces[0]));
            });
            let feed = async move {
                for _ in 0..4 {
                    tx.send(()).await.unwrap();
                    // Let the read finish before the next tick.
                    glib::timeout_future(std::time::Duration::from_millis(50)).await;
                }
                drop(tx);
            };
            futures_lite::future::zip(f, feed).await;
        });
        assert_eq!(reads.load(Ordering::SeqCst), 4, "one read per tick");
        assert_eq!(
            *shown.borrow(),
            [false, true],
            "unplug shown once, replug shown"
        );
    }

    /// QA-NET-28: an active WireGuard tunnel is a VPN that is connected, with its own DNS; the
    /// LAN's resolvers and router stay under the link that carries them.
    #[test]
    fn wireguard_is_a_connected_vpn_with_its_dns() {
        let wg = Interface {
            name: "qa-wg0".into(),
            kind: IfKind::Vpn,
            up: true,
            ipv4: vec!["10.77.0.2/32".into()],
            dns: vec!["10.77.0.1".into()],
            ..Default::default()
        };
        let mut net = eth(true);
        net.interfaces.push(wg.clone());
        net.dns = vec!["10.77.0.1".into(), "10.0.2.3".into()];
        net.gateway = Some("10.0.2.2".into());
        net.route_dev = Some("enp0s1".into());
        assert_eq!(kind_label(&wg), ("network-vpn-symbolic", "VPN"));
        assert!(connected(&wg));
        assert_eq!(dns_for(&net, &wg), ["10.77.0.1"]);
        assert!(!carries_route(&net, &wg), "the router is the LAN's");
        assert_eq!(link_state(&net, &wg, true), "Connected");
        let lan = net.interfaces[0].clone();
        assert_eq!(dns_for(&net, &lan).len(), 2);
        net.connectivity = wol_core::netinfo::Connectivity::Portal;
        assert_eq!(
            link_state(&net, &lan, true),
            crate::copy::settings::NETWORK_PORTAL
        );
    }

    #[test]
    fn firewall_row_says_what_the_kernel_says() {
        let g = crate::copy::settings::FIREWALL_ON;
        assert_eq!(firewall_sentence(true, Some(true)), g);
        assert_eq!(firewall_sentence(true, None), g);
        assert_eq!(
            firewall_sentence(false, Some(false)),
            g,
            "off is not 'not running now'"
        );
        assert_eq!(
            firewall_sentence(true, Some(false)),
            crate::copy::settings::FIREWALL_STOPPED
        );
    }

    #[test]
    fn cidr_to_mask() {
        for (a, ip, mask) in [
            ("192.168.1.24/24", "192.168.1.24", "255.255.255.0"),
            ("10.0.0.2/8", "10.0.0.2", "255.0.0.0"),
            ("10.0.0.2/30", "10.0.0.2", "255.255.255.252"),
            ("10.0.0.2/0", "10.0.0.2", "0.0.0.0"),
            ("10.0.0.2", "10.0.0.2", ""),
            ("10.0.0.2/99", "10.0.0.2", ""),
        ] {
            assert_eq!(split_cidr(a), (ip.to_string(), mask.to_string()), "{a}");
        }
    }
}
