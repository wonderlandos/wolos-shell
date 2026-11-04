// SPDX-License-Identifier: GPL-2.0-or-later
//! Network > DNS over TLS (NET-3): systemd-resolved's DNSOverTLS (Off, When available, Always)
//! and the computer's own DNS servers, saved by wol-admind `dns-over-tls` (the Authenticate sheet
//! asks). Names go through resolved for every program (docs/modules/wifi.md "Names and the
//! firewall"), so this applies to all of them.
use super::*;
use wol_core::netconf::{self, DotConfig, DotMode};

/// The page's name.
pub(crate) const TITLE: &str = "DNS over TLS";

const MODES: [DotMode; 3] = [DotMode::Off, DotMode::Opportunistic, DotMode::Strict];

/// Network's DNS over TLS row subtitle: the mode, and the first server when there is one.
pub(crate) fn summary(c: &DotConfig) -> String {
    let mode = match c.mode {
        DotMode::Off => "Off",
        DotMode::Opportunistic => "When available",
        DotMode::Strict => "Always",
    };
    match c.servers.first() {
        Some(s) => {
            let name = s.split_once('#').map_or(s.as_str(), |(_, n)| n);
            format!("{mode}, {name}")
        }
        None => mode.into(),
    }
}

/// The typed fields -> a configuration, or the sentence that says what is wrong.
pub(crate) fn from_fields(mode: usize, servers: &str) -> Result<DotConfig, &'static str> {
    let bad = netconf::dot_problem(&DotConfig {
        mode: DotMode::Off,
        servers: vec!["x".into()],
    });
    let servers = netconf::parse_dot_servers(servers).ok_or(bad.unwrap_or_default())?;
    let c = DotConfig {
        mode: MODES.get(mode).copied().unwrap_or_default(),
        servers,
    };
    match netconf::dot_problem(&c) {
        Some(why) => Err(why),
        None => Ok(c),
    }
}

fn render(e: &Rc<Env>, into: &gtk::Box, c: DotConfig, status: &gtk::Label) {
    let servers = w::text_field("9.9.9.9#dns.quad9.net, 1.1.1.1#cloudflare-dns.com");
    servers.set_text(&c.servers.join(", "));
    servers.set_width_chars(w::VALUE_CHARS);
    let chosen = Rc::new(Cell::new(
        MODES.iter().position(|m| *m == c.mode).unwrap_or(0),
    ));
    let mode = {
        let chosen = chosen.clone();
        form::segmented_row(
            "DNS over TLS",
            "",
            &["Off", "When available", "Always"],
            chosen.get(),
            move |i| chosen.set(i),
        )
    };
    into.append(&w::section(
        "",
        &[mode, form::row("DNS servers", "", &servers)],
    ));
    into.append(&w::footnote(crate::copy::settings::DOT_FOOT));
    let Some(_) = admin("dns-over-tls", &[]) else {
        return;
    };
    let apply = w::button("Apply", w::Kind::Primary, || {});
    apply.set_halign(gtk::Align::End);
    let (ctx, st) = (e.ctx.clone(), status.clone());
    apply.connect_clicked(move |b| {
        let c = match from_fields(chosen.get(), &servers.text()) {
            Ok(c) => c,
            Err(why) => return say(&st, why),
        };
        let args = netconf::dot_args(&c);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let Some(a) = admin("dns-over-tls", &args) else {
            return;
        };
        b.set_sensitive(false);
        let (st, b) = (st.clone(), b.clone());
        ctx.act(
            a,
            Rc::new(move |r| {
                b.set_sensitive(true);
                say(
                    &st,
                    if r.is_ok() {
                        crate::copy::settings::DOT_SAVED
                    } else {
                        error_line(r)
                    },
                );
            }),
        );
    });
    into.append(&apply);
}

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let p = form::subpage(TITLE, e.opener("Network"));
    let body = w::vbox(w::S6);
    p.append(&body);
    let status = status_label();
    p.append(&status);
    let (m, e2) = (e.machine(), e.clone());
    later(
        &body,
        move || m.net_settings().dot,
        move |into, c| render(&e2, into, c, &status),
    );
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NET-21 DoT: Always is refused without a server of its own (every lookup would fail).
    #[test]
    fn typed_fields_become_dns_over_tls() {
        assert_eq!(
            from_fields(1, "9.9.9.9#dns.quad9.net, 2620:fe::fe"),
            Ok(DotConfig {
                mode: DotMode::Opportunistic,
                servers: vec!["9.9.9.9#dns.quad9.net".into(), "2620:fe::fe".into()],
            })
        );
        assert!(from_fields(2, "").is_err());
        assert!(from_fields(0, "dns.quad9.net").is_err());
        assert_eq!(from_fields(0, ""), Ok(DotConfig::default()));
        assert_eq!(
            summary(&from_fields(2, "9.9.9.9#dns.quad9.net").unwrap()),
            "Always, dns.quad9.net"
        );
        assert_eq!(summary(&DotConfig::default()), "Off");
    }
}
