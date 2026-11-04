// SPDX-License-Identifier: GPL-2.0-or-later
//! Wi-Fi: the radio, the network in use, known networks and the ones in range; secured networks ask
//! for their password in the Join sheet. Joining goes through wol-admind (wpa_supplicant.conf).
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
        let (saved, iface, nets, st, again) = (
            saved.clone(),
            iface.clone(),
            nets.clone(),
            status.clone(),
            after_join.clone(),
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
                r.button.connect_clicked(move |_| {
                    if !n.connected {
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
                current.append(&w::section("", &[row(n), details.upcast()]));
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
            let rest: Vec<gtk::Widget> = nets
                .iter()
                .filter(|n| !n.connected && !saved.contains(&n.ssid))
                .take(12)
                .map(row)
                .collect();
            if rest.is_empty() {
                others.append(&w::section(
                    "Other Networks",
                    &[w::list_row("", "No other networks in range", "", None).upcast()],
                ));
            } else {
                others.append(&w::section("Other Networks", &rest));
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
            off_thread(
                move || (m.known_networks(), wifi_iface(&m.net())),
                move |(known, dev)| {
                    *saved.borrow_mut() = known;
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
    let pw = w::password_field("Password");
    let body = w::vbox(w::S3);
    body.append(&title);
    body.append(&pw);
    let caps = w::caps_lock_note();
    caps.set_halign(gtk::Align::Start);
    body.append(&caps);
    let buttons = w::hbox(w::S2);
    buttons.append(&w::spacer());
    let ssid: Rc<RefCell<String>> = Default::default();
    {
        let r = sheet.clone();
        buttons.append(&w::button("Cancel", w::Kind::Default, move || {
            r.set_reveal_child(false)
        }));
    }
    let join: Rc<dyn Fn(String, Option<String>)> = {
        let (ctx, st) = (e.ctx.clone(), status.clone());
        Rc::new(move |id: String, pass: Option<String>| {
            say(&st, &format!("Joining “{id}”…"));
            let st = st.clone();
            let (id2, joined) = (id.clone(), joined.clone());
            ctx.act(
                Action::WifiJoin(id, pass),
                Rc::new(move |r| match r {
                    Ok(()) => {
                        say(&st, &format!("Connected to “{id2}”."));
                        joined()
                    }
                    Err(e) => say(&st, &crate::copy::wifi_join(e, &id2)),
                }),
            );
        })
    };
    {
        let (r, pwc, id, join) = (sheet.clone(), pw.clone(), ssid.clone(), join.clone());
        let go = Rc::new(move || {
            let pass = pwc.text().to_string();
            pwc.set_text("");
            r.set_reveal_child(false);
            join(id.borrow().clone(), Some(pass));
        });
        let g = go.clone();
        pw.connect_activate(move |_| g());
        buttons.append(&w::button("Join", w::Kind::Primary, move || go()));
    }
    body.append(&buttons);
    sheet_body(&sheet, &body);
    Rc::new(move |name: String, secure: bool| {
        if secure {
            *ssid.borrow_mut() = name.clone();
            title.set_label(&format!("Enter the password for “{name}”"));
            sheet.set_reveal_child(true);
            pw.grab_focus();
        } else {
            join(name, None);
        }
    })
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
