// SPDX-License-Identifier: GPL-2.0-or-later
//! Network > IP Settings (QA-LIVE3-14): each link on a network gets its IPv4 address
//! automatically (DHCP) or typed in (address with prefix, router, DNS servers), saved on its
//! NetworkManager profile with a 15 s trial: a typed address that loses the network goes back to
//! Automatic by itself (`wol_core::controls::Controls::set_ipv4`). Read again each time the page
//! shows.
use super::*;
use wol_core::controls::ControlError;
use wol_core::netconf::{self, IpApplied, Ipv4Config};
use wol_core::netinfo::IfKind;

/// The page's name (`wol-settings "IP Settings"`, Network's Configure… buttons).
pub(crate) const TITLE: &str = "IP Settings";

/// The typed fields -> a configuration, or the sentence that says what is wrong.
pub(crate) fn from_fields(
    manual: bool,
    address: &str,
    router: &str,
    dns: &str,
) -> Result<Ipv4Config, &'static str> {
    if !manual {
        return Ok(Ipv4Config::default());
    }
    let dns = netconf::parse_dns_list(dns).ok_or(netconf::IpProblem::Dns.sentence())?;
    let c = Ipv4Config {
        manual: true,
        address: address.trim().into(),
        router: router.trim().into(),
        dns,
    };
    netconf::validate_ipv4(&c).map_err(|p| p.sentence())?;
    Ok(c)
}

/// The line under the page after a change.
pub(crate) fn result_line(r: Result<IpApplied, ControlError>) -> &'static str {
    match r {
        Ok(IpApplied::Kept) => crate::copy::settings::IP_KEPT,
        Ok(IpApplied::RolledBack) => crate::copy::settings::IP_ROLLED_BACK,
        Err(e) => ActionError::from(e).message(),
    }
}

/// The links this page offers: Ethernet and Wi-Fi that are up, with their configuration (None:
/// no active profile).
type Links = Vec<(String, &'static str, Option<Ipv4Config>)>;

fn load(m: &Arc<dyn Machine>) -> Links {
    m.net()
        .interfaces
        .into_iter()
        .filter(|i| i.up && matches!(i.kind, IfKind::Ethernet | IfKind::WiFi))
        .map(|i| {
            let kind = if i.kind == IfKind::WiFi {
                "Wi-Fi"
            } else {
                "Ethernet"
            };
            let c = m.ipv4(&i.name);
            (i.name, kind, c)
        })
        .collect()
}

fn link_section(
    e: &Rc<Env>,
    iface: &str,
    kind: &str,
    c: Ipv4Config,
    status: &gtk::Label,
    reload: Rc<dyn Fn()>,
) -> gtk::Box {
    let field = |placeholder: &str, text: &str| {
        let f = w::text_field(placeholder);
        f.set_text(text);
        f.set_width_chars(w::VALUE_CHARS);
        f
    };
    let address = field("192.168.1.20/24", &c.address);
    let router = field("192.168.1.1", &c.router);
    let dns = field("1.1.1.1, 9.9.9.9", &c.dns.join(", "));
    let typed: Vec<gtk::Widget> = vec![
        form::row("Address", "", &address),
        form::row("Router", "", &router),
        form::row("DNS servers", "", &dns),
    ];
    for r in &typed {
        r.set_visible(c.manual);
    }
    let manual = Rc::new(Cell::new(c.manual));
    let mode = {
        let (manual, typed) = (manual.clone(), typed.clone());
        form::segmented_row(
            "Configure IPv4",
            "",
            &["Automatic", "Manual"],
            usize::from(c.manual),
            move |i| {
                manual.set(i == 1);
                for r in &typed {
                    r.set_visible(i == 1);
                }
            },
        )
    };
    let mut rows = vec![mode];
    rows.extend(typed);
    let apply = w::button("Apply", w::Kind::Primary, || {});
    apply.set_halign(gtk::Align::End);
    {
        let (m, st, iface) = (e.machine(), status.clone(), iface.to_string());
        apply.connect_clicked(move |b| {
            let c = match from_fields(manual.get(), &address.text(), &router.text(), &dns.text()) {
                Ok(c) => c,
                Err(why) => return say(&st, why),
            };
            say(
                &st,
                if c.manual {
                    crate::copy::settings::IP_TRYING
                } else {
                    ""
                },
            );
            b.set_sensitive(false);
            let (m, st, b, iface, reload) = (
                m.clone(),
                st.clone(),
                b.clone(),
                iface.clone(),
                reload.clone(),
            );
            off_thread(
                move || m.set_ipv4(&iface, &c),
                move |r| {
                    b.set_sensitive(true);
                    reload();
                    say(&st, result_line(r));
                },
            );
        });
    }
    let b = w::vbox(w::S2);
    b.append(&w::section(&format!("{kind} ({iface})"), &rows));
    b.append(&apply);
    b
}

fn render(e: &Rc<Env>, into: &gtk::Box, links: Links, status: &gtk::Label, reload: Rc<dyn Fn()>) {
    let mut any = false;
    for (iface, kind, c) in links {
        let Some(c) = c else { continue };
        any = true;
        into.append(&link_section(e, &iface, kind, c, status, reload.clone()));
    }
    if !any {
        into.append(&w::empty_state(
            "network-offline-symbolic",
            "No Network Connections",
            crate::copy::settings::IP_NO_LINKS,
            None,
        ));
    }
}

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let p = form::subpage(TITLE, e.opener("Network"));
    let body = w::vbox(w::S6);
    p.append(&body);
    let status = status_label();
    p.append(&status);
    let fill: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Default::default();
    let reload: Rc<dyn Fn()> = {
        let (e, body, status, fill) = (e.clone(), body.clone(), status.clone(), fill.clone());
        Rc::new(move || {
            clear(&body);
            let m = e.machine();
            let (e, status, fill) = (e.clone(), status.clone(), fill.clone());
            later(
                &body,
                move || load(&m),
                move |into, links| {
                    let again = fill.borrow().clone();
                    if let Some(again) = again {
                        render(&e, into, links, &status, again);
                    }
                },
            );
        })
    };
    *fill.borrow_mut() = Some(reload.clone());
    reload();
    // Shown again later (another Configure…): read again, the profile may have changed.
    let first = Cell::new(true);
    p.connect_map(move |_| {
        if !first.replace(false) {
            reload();
        }
    });
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    /// QA-LIVE3-14: the typed fields become the configuration, or the sentence that says what
    /// to fix; Automatic needs nothing typed.
    #[test]
    fn typed_fields_become_a_configuration() {
        assert_eq!(
            from_fields(false, "junk", "", ""),
            Ok(Ipv4Config::default())
        );
        assert_eq!(
            from_fields(true, " 10.0.2.50/24 ", "10.0.2.2", "1.1.1.1, 9.9.9.9"),
            Ok(Ipv4Config {
                manual: true,
                address: "10.0.2.50/24".into(),
                router: "10.0.2.2".into(),
                dns: vec!["1.1.1.1".into(), "9.9.9.9".into()],
            })
        );
        assert_eq!(
            from_fields(true, "10.0.2.50", "10.0.2.2", ""),
            Err(netconf::IpProblem::Address.sentence())
        );
        assert_eq!(
            from_fields(true, "10.0.2.50/24", "10.0.3.1", ""),
            Err(netconf::IpProblem::RouterOutside.sentence())
        );
        assert_eq!(
            from_fields(true, "10.0.2.50/24", "", "quad9"),
            Err(netconf::IpProblem::Dns.sentence())
        );
        assert_eq!(
            result_line(Ok(IpApplied::RolledBack)),
            crate::copy::settings::IP_ROLLED_BACK
        );
        assert_eq!(
            result_line(Ok(IpApplied::Kept)),
            crate::copy::settings::IP_KEPT
        );
    }
}
