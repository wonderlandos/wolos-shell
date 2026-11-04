// SPDX-License-Identifier: GPL-2.0-or-later
//! Network > Proxy (NET-3): Off, Automatic (a PAC file) or Manual (HTTP, HTTPS and SOCKS
//! servers), with the hosts reached directly. Saved for the whole computer by wol-admind `proxy`
//! (the Authenticate sheet asks), then set for this session: the desktop's proxy keys and
//! NetworkManager (`Machine::apply_proxy_session`).
use super::*;
use wol_core::netconf::{self, ProxyConfig, ProxyMode};

/// The page's name.
pub(crate) const TITLE: &str = "Proxy";

/// Network's Proxy row subtitle.
pub(crate) fn summary(c: &ProxyConfig) -> String {
    match c.mode {
        ProxyMode::Off => "Off".into(),
        ProxyMode::Auto => "Automatic".into(),
        ProxyMode::Manual => {
            let server = [&c.http, &c.https, &c.socks]
                .into_iter()
                .find(|s| !s.is_empty())
                .cloned()
                .unwrap_or_default();
            format!("Manual, {server}")
        }
    }
}

/// The segmented choice's index of `m`, and back.
pub(crate) fn mode_index(m: ProxyMode) -> usize {
    match m {
        ProxyMode::Off => 0,
        ProxyMode::Auto => 1,
        ProxyMode::Manual => 2,
    }
}

fn mode_of(i: usize) -> ProxyMode {
    match i {
        1 => ProxyMode::Auto,
        2 => ProxyMode::Manual,
        _ => ProxyMode::Off,
    }
}

/// The typed fields -> a configuration, or the sentence that says what is wrong. Fields of the
/// other modes are left out.
pub(crate) fn from_fields(
    mode: usize,
    pac: &str,
    http: &str,
    https: &str,
    socks: &str,
    direct: &str,
) -> Result<ProxyConfig, &'static str> {
    let mode = mode_of(mode);
    let t = |s: &str| s.trim().to_string();
    let c = match mode {
        ProxyMode::Off => ProxyConfig::default(),
        _ => ProxyConfig {
            mode,
            pac_url: if mode == ProxyMode::Auto {
                t(pac)
            } else {
                String::new()
            },
            http: if mode == ProxyMode::Manual {
                t(http)
            } else {
                String::new()
            },
            https: if mode == ProxyMode::Manual {
                t(https)
            } else {
                String::new()
            },
            socks: if mode == ProxyMode::Manual {
                t(socks)
            } else {
                String::new()
            },
            no_proxy: netconf::parse_no_proxy(direct).unwrap_or_else(|| vec![direct.into()]),
        },
    };
    match netconf::proxy_problem(&c) {
        Some(why) => Err(why),
        None => Ok(c),
    }
}

fn render(e: &Rc<Env>, into: &gtk::Box, c: ProxyConfig, status: &gtk::Label) {
    let field = |placeholder: &str, text: &str| {
        let f = w::text_field(placeholder);
        f.set_text(text);
        f.set_width_chars(w::VALUE_CHARS);
        f
    };
    let pac = field("https://wpad.example.com/proxy.pac", &c.pac_url);
    let http = field("proxy.example.com:3128", &c.http);
    let https = field("proxy.example.com:3128", &c.https);
    let socks = field("proxy.example.com:1080", &c.socks);
    let direct = field(
        "localhost, .example.com, 10.0.0.0/8",
        &c.no_proxy.join(", "),
    );
    let auto_rows: Vec<gtk::Widget> = vec![form::row("Configuration address", "", &pac)];
    let manual_rows: Vec<gtk::Widget> = vec![
        form::row("Web proxy (HTTP)", "", &http),
        form::row("Secure web proxy (HTTPS)", "", &https),
        form::row("SOCKS proxy", "", &socks),
    ];
    let direct_row = form::row("Don’t use the proxy for", "", &direct);
    let show = {
        let (a, m, d) = (auto_rows.clone(), manual_rows.clone(), direct_row.clone());
        move |i: usize| {
            for r in &a {
                r.set_visible(i == 1);
            }
            for r in &m {
                r.set_visible(i == 2);
            }
            d.set_visible(i != 0);
        }
    };
    let chosen = Rc::new(Cell::new(mode_index(c.mode)));
    show(chosen.get());
    let mode = {
        let chosen = chosen.clone();
        form::segmented_row(
            "Proxy",
            "",
            &["Off", "Automatic", "Manual"],
            chosen.get(),
            move |i| {
                chosen.set(i);
                show(i);
            },
        )
    };
    let mut rows = vec![mode];
    rows.extend(auto_rows);
    rows.extend(manual_rows);
    rows.push(direct_row);
    into.append(&w::section("", &rows));
    into.append(&w::footnote(crate::copy::settings::PROXY_FOOT));
    let apply = w::button("Apply", w::Kind::Primary, || {});
    apply.set_halign(gtk::Align::End);
    let (ctx, m, st) = (e.ctx.clone(), e.machine(), status.clone());
    apply.connect_clicked(move |b| {
        let c = match from_fields(
            chosen.get(),
            &pac.text(),
            &http.text(),
            &https.text(),
            &socks.text(),
            &direct.text(),
        ) {
            Ok(c) => c,
            Err(why) => return say(&st, why),
        };
        let args = netconf::proxy_args(&c);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let Some(a) = admin("proxy", &args) else {
            return;
        };
        b.set_sensitive(false);
        let (m, st, b) = (m.clone(), st.clone(), b.clone());
        ctx.act(
            a,
            Rc::new(move |r| {
                if r.is_err() {
                    b.set_sensitive(true);
                    return say(&st, error_line(r));
                }
                let (m, st, b, c) = (m.clone(), st.clone(), b.clone(), c.clone());
                off_thread(
                    move || m.apply_proxy_session(&c),
                    move |r| {
                        b.set_sensitive(true);
                        say(
                            &st,
                            match r {
                                Ok(()) => crate::copy::settings::PROXY_SAVED,
                                Err(e) => ActionError::from(e).message(),
                            },
                        );
                    },
                );
            }),
        );
    });
    if admin("proxy", &[]).is_some() {
        into.append(&apply);
    }
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
        move || m.net_settings().proxy,
        move |into, c| render(&e2, into, c, &status),
    );
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NET-21 proxy: the fields of the chosen mode only, checked before the password is asked.
    #[test]
    fn typed_fields_become_a_proxy() {
        assert_eq!(
            from_fields(0, "junk", "junk", "", "", "a b"),
            Ok(ProxyConfig::default())
        );
        let m = from_fields(
            2,
            "http://x/p.pac",
            " proxy.lan:3128 ",
            "",
            "",
            "localhost, .corp",
        )
        .unwrap();
        assert_eq!(
            (
                m.mode,
                m.pac_url.as_str(),
                m.http.as_str(),
                m.no_proxy.len()
            ),
            (ProxyMode::Manual, "", "proxy.lan:3128", 2)
        );
        assert!(from_fields(2, "", "", "", "", "").is_err(), "no server");
        assert!(
            from_fields(1, "wpad", "", "", "", "").is_err(),
            "not a web address"
        );
        assert!(from_fields(2, "", "proxy:80", "", "", "a'b").is_err());
        assert_eq!(summary(&m), "Manual, proxy.lan:3128");
        assert_eq!(summary(&ProxyConfig::default()), "Off");
        assert_eq!(mode_index(ProxyMode::Auto), 1);
    }
}
