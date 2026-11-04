// SPDX-License-Identifier: GPL-2.0-or-later
//! Network > VPN (NET-3, QA-NET-21): every VPN profile with Connect or Disconnect and Forget,
//! and Add VPN, which imports a WireGuard `.conf` or OpenVPN `.ovpn` file picked in the file
//! chooser (its key goes to the login keyring). An OpenVPN profile that asks for its password
//! gets a password row; the password is remembered in the keyring once it worked.
use super::machine::VpnChange;
use super::*;
use wol_core::controls::ControlError;
use wol_core::vpn::Vpn;

/// "WireGuard, connected".
pub(crate) fn subtitle(v: &Vpn) -> String {
    let state = if v.active {
        "Connected"
    } else {
        "Not connected"
    };
    format!("{}, {}", v.kind.label(), state.to_lowercase())
}

/// The sentence for a VPN change or import that did not work.
pub(crate) fn error_sentence(e: ControlError) -> &'static str {
    match e {
        ControlError::Unsupported => crate::copy::settings::VPN_NO_OPENVPN,
        ControlError::Invalid => crate::copy::settings::VPN_BAD_FILE,
        ControlError::WrongPassword => crate::copy::settings::VPN_NEEDS_PASSWORD,
        ControlError::Failed => crate::copy::settings::VPN_DID_NOT_CONNECT,
        e => ActionError::from(e).message(),
    }
}

/// The VPN section of the Network pane, filled off the UI thread and again after each change.
pub(super) fn section(e: &Rc<Env>) -> gtk::Box {
    let b = w::vbox(w::S2);
    let body = w::vbox(w::S2);
    b.append(&body);
    let status = status_label();
    b.append(&status);
    let fill: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Default::default();
    let reload: Rc<dyn Fn()> = {
        let (e, body, status, fill) = (e.clone(), body.clone(), status.clone(), fill.clone());
        Rc::new(move || {
            clear(&body);
            let m = e.machine();
            let (e, status, fill) = (e.clone(), status.clone(), fill.clone());
            later(
                &body,
                move || m.net_settings().vpns,
                move |into, vpns| {
                    let again = fill.borrow().clone();
                    if let (Some(vpns), Some(again)) = (vpns, again) {
                        render(&e, into, vpns, &status, again);
                    }
                },
            );
        })
    };
    *fill.borrow_mut() = Some(reload.clone());
    reload();
    b
}

fn render(e: &Rc<Env>, into: &gtk::Box, vpns: Vec<Vpn>, status: &gtk::Label, reload: Rc<dyn Fn()>) {
    let mut rows: Vec<gtk::Widget> = Vec::new();
    let password = w::password_field("Password");
    let pw_box = w::vbox(w::S1);
    pw_box.append(&password);
    let caps = w::caps_lock_note();
    caps.set_halign(gtk::Align::Start);
    pw_box.append(&caps);
    let pw_row = form::row("VPN password", "", &pw_box);
    pw_row.set_visible(false);
    for v in &vpns {
        let change = {
            let (m, st, uuid, reload) =
                (e.machine(), status.clone(), v.uuid.clone(), reload.clone());
            let (pw, pw_row) = (password.clone(), pw_row.clone());
            Rc::new(move |c: VpnChange| {
                let (m, st, uuid, reload, pw, pw_row) = (
                    m.clone(),
                    st.clone(),
                    uuid.clone(),
                    reload.clone(),
                    pw.clone(),
                    pw_row.clone(),
                );
                say(&st, "");
                off_thread(
                    move || m.vpn_change(&uuid, c),
                    move |r| match r {
                        Ok(()) => {
                            pw.set_text("");
                            reload()
                        }
                        Err(e) => {
                            pw_row.set_visible(e == ControlError::WrongPassword);
                            say(&st, error_sentence(e))
                        }
                    },
                );
            })
        };
        let control = w::hbox(w::S1);
        let (label, act) = if v.active {
            ("Disconnect", VpnChange::Disconnect)
        } else {
            ("Connect", VpnChange::Connect(None))
        };
        {
            let (change, pw) = (change.clone(), password.clone());
            control.append(&w::button(label, w::Kind::Default, move || {
                let typed = pw.text().to_string();
                change(match &act {
                    VpnChange::Connect(_) if !typed.is_empty() => VpnChange::Connect(Some(typed)),
                    a => a.clone(),
                })
            }));
        }
        let forget = change.clone();
        if let Some(more) = form::actions_button(vec![(
            "Forget VPN",
            Box::new(move || forget(VpnChange::Forget)),
        )]) {
            control.append(&more);
        }
        rows.push(
            w::list_row(
                "network-vpn-symbolic",
                &v.name,
                &subtitle(v),
                Some(control.upcast_ref()),
            )
            .upcast(),
        );
    }
    rows.push(pw_row);
    {
        let (m, st, reload) = (e.machine(), status.clone(), reload.clone());
        let anchor = into.clone();
        rows.push(form::button_row(
            "Add VPN",
            "A WireGuard or OpenVPN file",
            "Import…",
            move || {
                let (m, st, reload) = (m.clone(), st.clone(), reload.clone());
                crate::kit::pickers::choose_file_of(
                    &anchor,
                    "Add VPN",
                    "WireGuard and OpenVPN files",
                    &["conf", "ovpn"],
                    move |path| {
                        off_thread(
                            move || m.vpn_import(&path),
                            move |r| match r {
                                Ok(v) => {
                                    reload();
                                    say(&st, &crate::copy::settings::vpn_added(&v.name));
                                }
                                Err(e) => say(&st, error_sentence(e)),
                            },
                        );
                    },
                );
            },
        ));
    }
    into.append(&w::section("VPN", &rows));
    if vpns.is_empty() {
        into.append(&w::footnote(crate::copy::settings::VPN_NONE));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wol_core::vpn::VpnKind;

    /// NET-21 VPN: each row says its kind and whether it is connected; a missing plugin and a
    /// password it asks for have their own sentence.
    #[test]
    fn vpn_rows_say_kind_state_and_cause() {
        let v = Vpn {
            name: "Office".into(),
            uuid: "6a3f0c2e-1111-4a5b-9c8d-0123456789ab".into(),
            kind: VpnKind::WireGuard,
            active: true,
        };
        assert_eq!(subtitle(&v), "WireGuard, connected");
        assert_eq!(
            subtitle(&Vpn {
                active: false,
                kind: VpnKind::OpenVpn,
                ..v
            }),
            "OpenVPN, not connected"
        );
        assert_eq!(
            error_sentence(ControlError::Unsupported),
            crate::copy::settings::VPN_NO_OPENVPN
        );
        assert_eq!(
            error_sentence(ControlError::WrongPassword),
            crate::copy::settings::VPN_NEEDS_PASSWORD
        );
    }
}
