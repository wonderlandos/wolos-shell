// SPDX-License-Identifier: GPL-2.0-or-later
//! Wi-Fi: the radio, the network in use (with Disconnect), saved networks and every one in range
//! (Look Again scans), a hidden network by name; secured networks ask for their password in the
//! Join sheet. Joining runs nmcli as the person (NetworkManager's polkit rules), docs/modules/wifi.md.
use super::*;
use crate::kit::controls::DeviceRow;
use crate::system::WifiNetwork;

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let p = w::page("Wi-Fi");
    let status = status_label();
    let ctx = &e.ctx;
    let wifi_on = ctx.sys.wifi_enabled();
    let lists = w::vbox(w::S6);
    // Waits for its change: on Cancel, NotAdmin or an nmcli failure the switch flips back and
    // the lists keep matching the radio (hunt 1 G1; Night Shift's pattern).
    let toggle = {
        let (ctx, st, lists) = (ctx.clone(), status.clone(), lists.clone());
        w::pending_toggle(wifi_on.unwrap_or(false), move |on, done| {
            let lists = lists.clone();
            ctx.act_switch(
                Action::Wifi(on),
                &st,
                Box::new(move |ok| {
                    lists.set_visible(on == ok);
                    done(ok)
                }),
            )
        })
    };
    toggle.set_sensitive(wifi_on.is_some());
    lists.set_visible(wifi_on.unwrap_or(false));
    let hint = if wifi_on.is_none() {
        "No Wi-Fi hardware was found."
    } else {
        ""
    };
    p.append(&w::section(
        "",
        &[w::list_row(
            "network-wireless-symbolic",
            "Wi-Fi",
            hint,
            Some(toggle.upcast_ref()),
        )
        .upcast()],
    ));
    // Saved networks and the Wi-Fi device, read off the UI thread; read again after a join or a
    // Forget (QA-SET-11: Known/Other never changed until Settings restarted).
    let saved: Rc<RefCell<Vec<String>>> = Rc::default();
    // None: the saved networks could not be read (no Wi-Fi device yet, NetworkManager silent).
    let saved_read: Rc<std::cell::Cell<bool>> = Rc::new(std::cell::Cell::new(true));
    let iface: Rc<RefCell<String>> = Rc::new(RefCell::new("wlan0".into()));
    let nets: Rc<RefCell<Vec<WifiNetwork>>> = Rc::default();
    let reread: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::default();
    // Weak: the read closure holds the list filler, which holds this (no cycle; the page keeps
    // `reread` alive, below).
    let after_join: Rc<dyn Fn()> = {
        let r = Rc::downgrade(&reread);
        Rc::new(move || {
            let Some(r) = r.upgrade() else { return };
            let f = r.borrow().clone();
            if let Some(f) = f {
                f()
            }
        })
    };
    let join = join_sheet(e, &status, after_join.clone());
    let current = w::vbox(0);
    let known = w::vbox(0);
    let others = w::vbox(0);
    let remembered = w::vbox(0);
    lists.append(&current);
    lists.append(&known);
    lists.append(&others);
    lists.append(&remembered);
    p.append(&lists);
    p.append(&status);
    let fill = {
        let (current, known, others, remembered, join, e) = (
            current.clone(),
            known.clone(),
            others.clone(),
            remembered.clone(),
            join.clone(),
            e.clone(),
        );
        let (saved, iface, nets, st, again, saved_read) = (
            saved.clone(),
            iface.clone(),
            nets.clone(),
            status.clone(),
            after_join.clone(),
            saved_read.clone(),
        );
        Rc::new(move || {
            let (nets, saved) = (nets.borrow().clone(), saved.borrow().clone());
            for b in [&current, &known, &others, &remembered] {
                clear(b);
            }
            let row = |n: &WifiNetwork| -> gtk::Widget {
                let sub = if n.connected { "Connected" } else { "" };
                let lock = n.secure.then_some("network-wireless-encrypted-symbolic");
                let r = DeviceRow::new(
                    crate::system::signal_icon(n.signal),
                    &n.ssid,
                    sub,
                    n.connected,
                    lock,
                );
                // A saved network joins with what it has: no password sheet again.
                let (n, join, known) = (n.clone(), join.clone(), saved.contains(&n.ssid));
                let (enterprise, st) = (e.ctx.sys.wifi_enterprise(&n.ssid), st.clone());
                r.button.connect_clicked(move |_| {
                    if n.connected {
                        return;
                    }
                    if enterprise && !known {
                        // QA-NET-9: a password field that can never work is not offered.
                        say(&st, crate::copy::settings::WIFI_ENTERPRISE);
                    } else {
                        join(n.ssid.clone(), asks_password(n.secure, known));
                    }
                });
                r.button.upcast()
            };
            if let Some(n) = nets.iter().find(|n| n.connected) {
                let details = form::nav_row(
                    "network-wired-symbolic",
                    "Network Details",
                    "Address, router and DNS",
                );
                details.connect_clicked({
                    let e = e.clone();
                    move |_| e.open("Network")
                });
                // QA-NET-17: leave the network without turning Wi-Fi off; it stays saved.
                let leave = {
                    let (m, st, again, iface) =
                        (e.machine(), st.clone(), again.clone(), iface.clone());
                    w::button("Disconnect", w::Kind::Default, move || {
                        let (m, st, again, dev) =
                            (m.clone(), st.clone(), again.clone(), iface.borrow().clone());
                        off_thread(
                            move || m.wifi_disconnect(&dev),
                            move |ok| {
                                if ok {
                                    again()
                                } else {
                                    say(&st, crate::copy::settings::WIFI_DISCONNECT_FAILED)
                                }
                            },
                        );
                    })
                };
                let leave_row = w::list_row("", "This Network", "", Some(leave.upcast_ref()));
                current.append(&w::section(
                    "",
                    &[row(n), details.upcast(), leave_row.upcast()],
                ));
            }
            // Saved networks in range are listed once, under Saved Networks with Join and
            // Forget; without the Forget verb they keep their plain rows here.
            let forget = admin("wifi-forget", &[]).is_some();
            let mine: Vec<gtk::Widget> = nets
                .iter()
                .filter(|n| !forget && !n.connected && saved.contains(&n.ssid))
                .map(row)
                .collect();
            if !mine.is_empty() {
                known.append(&w::section("Known Networks", &mine));
            }
            // QA-NET-15: every network in range (the page scrolls), never a silent cut at 12.
            let mut rest: Vec<gtk::Widget> =
                other_networks(&nets, &saved).iter().map(row).collect();
            if rest.is_empty() {
                rest.push(w::list_row("", "No other networks in range", "", None).upcast());
            }
            // QA-NET-16: ask NetworkManager for a fresh scan now.
            let look = {
                let m = e.machine();
                w::button("Look Again", w::Kind::Default, move || {
                    let m = m.clone();
                    off_thread(move || m.wifi_rescan(), |()| {});
                })
            };
            rest.push(w::list_row("", "Scan for Networks", "", Some(look.upcast_ref())).upcast());
            // QA-NET-10: a network that hides its name, joined by name.
            let hidden = DeviceRow::new("view-more-symbolic", "Other Network…", "", false, None);
            {
                let join = join.clone();
                hidden
                    .button
                    .connect_clicked(move |_| join(String::new(), true));
            }
            rest.push(hidden.button.upcast());
            others.append(&w::section("Other Networks", &rest));
            // QA-SHADOW-55: "could not read" is not "none saved".
            if !saved_read.get() {
                remembered.append(&w::section(
                    "Saved Networks",
                    &[
                        w::list_row("", crate::copy::settings::WIFI_SAVED_UNREAD, "", None)
                            .upcast(),
                    ],
                ));
            }
            // Every saved network, in range or not, with Forget (manual: control-center,
            // settings; QA-SET-11).
            if forget && !saved.is_empty() {
                let rows: Vec<gtk::Widget> = saved
                    .iter()
                    .map(|ssid| {
                        let (ctx, st, again, iface, ssid2) = (
                            e.ctx.clone(),
                            st.clone(),
                            again.clone(),
                            iface.clone(),
                            ssid.clone(),
                        );
                        let b = w::button("Forget", w::Kind::Default, || {});
                        b.connect_clicked(move |b| {
                            let Some(a) = forget_action(&iface.borrow(), &ssid2) else {
                                return;
                            };
                            b.set_sensitive(false);
                            let (again, b) = (again.clone(), b.downgrade());
                            ctx.act_switch(
                                a,
                                &st,
                                Box::new(move |ok| {
                                    if let Some(b) = b.upgrade() {
                                        b.set_sensitive(true);
                                    }
                                    if ok {
                                        again()
                                    }
                                }),
                            );
                        });
                        b.set_tooltip_text(Some(&crate::copy::wifi_forget_hint(ssid)));
                        let here = nets.iter().find(|n| &n.ssid == ssid);
                        let buttons = w::hbox(w::S2);
                        if let Some(n) = here.filter(|n| !n.connected) {
                            let (join, n) = (join.clone(), n.clone());
                            buttons.append(&w::button("Join", w::Kind::Default, move || {
                                join(n.ssid.clone(), asks_password(n.secure, true))
                            }));
                        }
                        buttons.append(&b);
                        let (icon, sub) = match here {
                            Some(n) => (
                                crate::system::signal_icon(n.signal),
                                if n.connected { "Connected" } else { "" },
                            ),
                            None => ("network-wireless-symbolic", ""),
                        };
                        w::list_row(icon, ssid, sub, Some(buttons.upcast_ref())).upcast()
                    })
                    .collect();
                remembered.append(&w::section("Saved Networks", &rows));
            }
            // Empty groups leave the layout (no doubled spacing).
            for b in [&current, &known, &others, &remembered] {
                b.set_visible(b.first_child().is_some());
            }
        })
    };
    {
        let (fill, saved, iface, m) = (fill.clone(), saved.clone(), iface.clone(), e.machine());
        *reread.borrow_mut() = Some(Rc::new(move || {
            let (fill, saved, iface, m) = (fill.clone(), saved.clone(), iface.clone(), m.clone());
            let saved_read = saved_read.clone();
            off_thread(
                move || (m.known_networks_read(), wifi_iface(&m.net())),
                move |(known, dev)| {
                    saved_read.set(known.is_some());
                    *saved.borrow_mut() = known.unwrap_or_default();
                    if let Some(d) = dev {
                        *iface.borrow_mut() = d;
                    }
                    fill()
                },
            );
        }));
    }
    {
        let (fill, sys, nets, reread) =
            (fill.clone(), ctx.sys.clone(), nets.clone(), reread.clone());
        *nets.borrow_mut() = sys.wifi_networks();
        if let Some(f) = reread.borrow().clone() {
            f()
        }
        // The page owns the reader: it goes when the page does.
        let keep = RefCell::new(Some(reread.clone()));
        p.connect_destroy(move |_| drop(keep.borrow_mut().take()));
        // One watcher per page, gone with it: an Env::refresh builds the page again, and the old
        // loop kept running (and its 1 s snapshots forking) for good (QA-SET-8).
        if let Some(rx) = sys.watch() {
            let task = glib::spawn_future_local(async move {
                while rx.recv().await.is_ok() {
                    let n = sys.wifi_networks();
                    if *nets.borrow() != n {
                        *nets.borrow_mut() = n;
                        fill();
                    }
                }
            });
            let task = RefCell::new(Some(task));
            p.connect_destroy(move |_| {
                if let Some(t) = task.borrow_mut().take() {
                    t.abort();
                }
            });
        }
    }
    p
}

/// The networks under Other Networks: in range, not in use, not saved. All of them (QA-NET-15).
pub(crate) fn other_networks(nets: &[WifiNetwork], saved: &[String]) -> Vec<WifiNetwork> {
    nets.iter()
        .filter(|n| !n.connected && !saved.contains(&n.ssid))
        .cloned()
        .collect()
}

/// A secured network asks for its password only when it is not saved (QA-SET-11).
pub(crate) fn asks_password(secure: bool, saved: bool) -> bool {
    secure && !saved
}

/// wol-admind's Forget for `ssid` on the Wi-Fi device `iface`.
pub(crate) fn forget_action(iface: &str, ssid: &str) -> Option<Action> {
    admin("wifi-forget", &[iface, ssid])
}

/// The Wi-Fi device's name (`wlan0`, `wlp2s0`), if there is one.
fn wifi_iface(n: &wol_core::netinfo::NetInfo) -> Option<String> {
    n.interfaces
        .iter()
        .find(|i| i.kind == wol_core::netinfo::IfKind::WiFi)
        .map(|i| i.name.clone())
}

/// The Join sheet: `join(ssid, secure)` asks for the password of secured networks, joins open ones
/// at once. Progress and errors show on `status`.
fn join_sheet(e: &Rc<Env>, status: &gtk::Label, joined: Rc<dyn Fn()>) -> Rc<dyn Fn(String, bool)> {
    let sheet = e.sheets.join.clone();
    let title = w::label("", "title");
    title.set_wrap(true);
    // Only for Other Network (a hidden network): its name.
    let name = w::text_field("Network name");
    name.set_visible(false);
    let pw = w::password_field("Password");
    let note = w::label("", "assistant-status");
    note.add_css_class("problem");
    note.set_wrap(true);
    note.set_visible(false);
    let body = w::vbox(w::S3);
    body.append(&title);
    body.append(&name);
    body.append(&pw);
    body.append(&note);
    let caps = w::caps_lock_note();
    caps.set_halign(gtk::Align::Start);
    body.append(&caps);
    let buttons = w::hbox(w::S2);
    buttons.append(&w::spacer());
    let ssid: Rc<RefCell<String>> = Default::default();
    let hidden: Rc<std::cell::Cell<bool>> = Default::default();
    // QA-SET-16: one join at a time; a second click waits for the first's answer.
    let busy: Rc<std::cell::Cell<bool>> = Default::default();
    {
        let r = sheet.clone();
        buttons.append(&w::button("Cancel", w::Kind::Default, move || {
            r.set_reveal_child(false)
        }));
    }
    let reopen: Rc<RefCell<Option<Rc<dyn Fn(String, String)>>>> = Default::default();
    let join: Rc<dyn Fn(String, Option<String>, bool)> = {
        let (ctx, st, busy, reopen) = (e.ctx.clone(), status.clone(), busy.clone(), reopen.clone());
        Rc::new(move |id: String, pass: Option<String>, hidden: bool| {
            if busy.replace(true) {
                return;
            }
            say(&st, &crate::copy::wifi_joining(&id));
            let (st, busy, reopen) = (st.clone(), busy.clone(), reopen.clone());
            let (id2, joined, secured) = (id.clone(), joined.clone(), pass.is_some());
            ctx.act(
                Action::WifiJoin {
                    ssid: id,
                    password: pass,
                    hidden,
                },
                Rc::new(move |r| {
                    busy.set(false);
                    match r {
                        Ok(()) => {
                            say(&st, &crate::copy::settings::wifi_connected(&id2));
                            joined()
                        }
                        Err(e) => {
                            let why = crate::copy::wifi_join(e, &id2);
                            say(&st, &why);
                            // QA-SET-16: a refused password asks again, with the reason.
                            if e == ActionError::WrongPassword && secured && !hidden {
                                let f = reopen.borrow().clone();
                                if let Some(f) = f {
                                    f(id2.clone(), why)
                                }
                            }
                        }
                    }
                }),
            );
        })
    };
    {
        let (r, pwc, id, join, namec, hid) = (
            sheet.clone(),
            pw.clone(),
            ssid.clone(),
            join.clone(),
            name.clone(),
            hidden.clone(),
        );
        let go = Rc::new(move || {
            let pass = pwc.text().to_string();
            let hidden = hid.get();
            let net = if hidden {
                namec.text().trim().to_string()
            } else {
                id.borrow().clone()
            };
            if net.is_empty() {
                return;
            }
            pwc.set_text("");
            r.set_reveal_child(false);
            // A hidden network may be open: no password then.
            let pass = (!hidden || !pass.is_empty()).then_some(pass);
            join(net, pass, hidden);
        });
        let g = go.clone();
        pw.connect_activate(move |_| g());
        let g = go.clone();
        name.connect_activate(move |_| g());
        buttons.append(&w::button("Join", w::Kind::Primary, move || go()));
    }
    body.append(&buttons);
    sheet_body(&sheet, &body);
    let why_note = note.clone();
    let open: Rc<dyn Fn(String, bool)> = Rc::new(move |net: String, secure: bool| {
        note.set_visible(false);
        // An empty name: Other Network (hidden), asked for by name.
        hidden.set(net.is_empty());
        name.set_visible(net.is_empty());
        if net.is_empty() {
            name.set_text("");
            title.set_label("Join Other Network");
            sheet.set_reveal_child(true);
            name.grab_focus();
        } else if secure {
            *ssid.borrow_mut() = net.clone();
            title.set_label(&format!("Enter the password for “{net}”"));
            sheet.set_reveal_child(true);
            pw.grab_focus();
        } else {
            join(net, None, false);
        }
    });
    {
        let open = open.clone();
        *reopen.borrow_mut() = Some(Rc::new(move |net: String, why: String| {
            open(net, true);
            why_note.set_label(&why);
            why_note.set_visible(true);
        }));
    }
    open
}

#[cfg(test)]
mod tests {
    use super::*;

    /// QA-SET-11: clicking a saved secured network opened the password sheet again; and a saved
    /// network can be forgotten (wol-admind `wifi-forget <iface> <ssid>`).
    #[test]
    fn saved_network_joins_without_password_and_forgets() {
        assert!(asks_password(true, false));
        assert!(!asks_password(true, true), "saved: joins with what it has");
        assert!(!asks_password(false, false));
        match forget_action("wlp2s0", "Home") {
            Some(Action::Admin(r)) => assert_eq!(r.args, ["wlp2s0", "Home"]),
            other => panic!("{other:?}"),
        }
        let n = wol_core::netinfo::NetInfo {
            interfaces: vec![wol_core::netinfo::Interface {
                name: "wlp2s0".into(),
                kind: wol_core::netinfo::IfKind::WiFi,
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(wifi_iface(&n).as_deref(), Some("wlp2s0"));
    }

    /// QA-NET-15: with 20 networks in range Other Networks lists all 20 (it was cut at 12, the
    /// weakest unreachable); the one in use and the saved ones are listed elsewhere.
    #[test]
    fn wifi_20_networks() {
        let nets: Vec<WifiNetwork> = (0..20)
            .map(|i| WifiNetwork {
                ssid: format!("Net {i}"),
                signal: 95 - i as u8,
                secure: i % 2 == 0,
                connected: i == 0,
            })
            .collect();
        let saved = vec!["Net 1".to_string()];
        let v = other_networks(&nets, &saved);
        assert_eq!(v.len(), 18);
        assert_eq!(
            v.last().unwrap().ssid,
            "Net 19",
            "the weakest is listed too"
        );
        let src = include_str!("wifi.rs");
        let code = src.split("#[cfg(test)]").next().unwrap();
        assert!(!code.contains(".take(12)"));
    }

    /// QA-NET-21 / NET2-2: "hotspot" leads to the page that has it (General > Sharing > Internet
    /// Sharing), not to Wi-Fi, which has no hotspot control.
    #[test]
    fn hotspot_search_finds_sharing() {
        assert_eq!(super::super::search("hotspot"), ["General"]);
    }

    /// QA-HUNT-10 / QA-SET-9: the radio switch flipped on a click and showed or hid the lists
    /// before wol-admind answered, and never flipped back on an error. Both now wait.
    #[test]
    fn set_wifi_err_restores_switch() {
        let src = include_str!("wifi.rs");
        let code = src.split("#[cfg(test)]").next().unwrap();
        let power =
            &code[code.find("let toggle = {").unwrap()..code.find("toggle.set_sensitive").unwrap()];
        assert!(
            power.contains("w::pending_toggle(") && power.contains("act_switch("),
            "{power}"
        );
        assert!(
            power.contains("lists.set_visible(on == ok)"),
            "lists follow the answer"
        );
        assert!(!power.contains("w::toggle("));
    }
}
