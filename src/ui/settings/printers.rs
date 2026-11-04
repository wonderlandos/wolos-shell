// SPDX-License-Identifier: GPL-2.0-or-later
//! Printers: CUPS queues (default, state), Add Printer from the devices CUPS finds (driverless IPP
//! Everywhere), Set as Default and Remove (wol-admind `printer-add`, `printer-default`,
//! `printer-remove`; the Authenticate sheet asks first).
use super::*;
use wol_core::printers::{Printer, queue_name};

/// What the pane shows now: None while a load is under way (the watch waits for it).
type Shown = Rc<RefCell<Option<Option<Vec<Printer>>>>>;

/// Seconds between the open pane's looks at the queues (QA-LIVE3-15: a printer added from a
/// terminal with `lpadmin` showed up only after the pane was reopened).
const WATCH_SECS: u32 = 4;

/// What people call a device URI: "dnssd://HP%20LaserJet._ipp._tcp.local/" -> "HP LaserJet",
/// "ipp://10.0.0.5/ipp/print" -> "10.0.0.5".
pub(crate) fn device_name(uri: &str) -> String {
    let rest = uri.split_once("://").map_or(uri, |x| x.1);
    let host = rest.split('/').next().unwrap_or(rest);
    let name = host.split("._").next().unwrap_or(host);
    let mut out = String::new();
    let b = name.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(v) = name
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(char::from(v));
            i += 3;
            continue;
        }
        out.push(char::from(b[i]));
        i += 1;
    }
    out
}

/// "Brother_HL_L2350DW" -> "Brother HL L2350DW".
fn shown(queue: &str) -> String {
    queue.replace('_', " ")
}

fn state_line(p: &Printer) -> String {
    match (p.default, p.state.as_str()) {
        (true, s) => format!("Default · {s}"),
        (false, s) => s.to_string(),
    }
}

fn load(e: &Rc<Env>, into: &gtk::Box, status: &gtk::Label, seen: &Shown) {
    clear(into);
    *seen.borrow_mut() = None;
    let (m, e2, st, sh) = (e.machine(), e.clone(), status.clone(), seen.clone());
    later(
        into,
        move || m.printers(),
        move |into, p| {
            *sh.borrow_mut() = Some(p.clone());
            render(&e2, into, p, &st, &sh)
        },
    );
}

/// Looks at the queues every few seconds while the pane is on screen and draws it again only
/// when the list differs from the one seen.
fn watch(e: &Rc<Env>, into: &gtk::Box, status: &gtk::Label, seen: &Shown) {
    let (e, into, status, seen) = (e.clone(), into.downgrade(), status.clone(), seen.clone());
    glib::timeout_add_seconds_local(WATCH_SECS, move || {
        let Some(into) = into.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if into.root().is_none() {
            return glib::ControlFlow::Break;
        }
        let (m, e, status, seen) = (e.machine(), e.clone(), status.clone(), seen.clone());
        glib::spawn_future_local(async move {
            let Ok(now) = gio::spawn_blocking(move || m.printers()).await else {
                return;
            };
            if changed(&seen.borrow(), &now) {
                *seen.borrow_mut() = Some(now.clone());
                clear(&into);
                render(&e, &into, now, &status, &seen);
            }
        });
        glib::ControlFlow::Continue
    });
}

/// Whether `now` differs from what the pane shows; nothing is seen yet (None): no, a load is
/// still under way and draws it.
fn changed(seen: &Option<Option<Vec<Printer>>>, now: &Option<Vec<Printer>>) -> bool {
    seen.as_ref().is_some_and(|s| s != now)
}

fn render(
    e: &Rc<Env>,
    into: &gtk::Box,
    printers: Option<Vec<Printer>>,
    status: &gtk::Label,
    seen: &Shown,
) {
    let Some(printers) = printers else {
        into.append(&w::empty_state(
            "printer-symbolic",
            "Printing Is Not Set Up",
            "The printing service is not running on this computer.",
            None,
        ));
        return;
    };
    let can_add = admin("printer-add", &[]).is_some();
    let mut rows: Vec<gtk::Widget> = printers
        .iter()
        .map(|p| {
            let mut items: Vec<w::MenuItem> = Vec::new();
            let act = |verb: &'static str| {
                let (env, st, q, holder, sh) = (
                    e.clone(),
                    status.clone(),
                    p.name.clone(),
                    into.clone(),
                    seen.clone(),
                );
                let f: Box<dyn Fn()> = Box::new(move || {
                    let Some(a) = admin(verb, &[&q]) else { return };
                    let (env2, st2, h2, sh2) =
                        (env.clone(), st.clone(), holder.clone(), sh.clone());
                    env.ctx.act(
                        a,
                        Rc::new(move |r| match r {
                            Ok(()) => load(&env2, &h2, &st2, &sh2),
                            Err(x) => say(&st2, error_line(Err(x))),
                        }),
                    );
                });
                f
            };
            if !p.default && admin("printer-default", &[]).is_some() {
                items.push(("Set as Default", act("printer-default")));
            }
            if admin("printer-remove", &[]).is_some() {
                items.push(("Remove Printer", act("printer-remove")));
            }
            let more = form::actions_button(items);
            w::list_row(
                "printer-symbolic",
                &shown(&p.name),
                &state_line(p),
                more.as_ref().map(|m| m.upcast_ref()),
            )
            .upcast()
        })
        .collect();
    if rows.is_empty() {
        rows.push(
            w::list_row(
                "printer-symbolic",
                "No printers yet",
                "Printers on your network show up when you add one.",
                None,
            )
            .upcast(),
        );
    }
    into.append(&w::section("", &rows));
    if !can_add {
        return;
    }
    let (env, st, holder, seen, taken) = (
        e.clone(),
        status.clone(),
        into.clone(),
        seen.clone(),
        printers.iter().map(|p| p.uri.clone()).collect::<Vec<_>>(),
    );
    let add = w::button("Add Printer…", w::Kind::Default, || {});
    add.connect_clicked(move |b| {
        b.set_sensitive(false);
        b.set_label("Looking for printers…");
        let (m, env, st, holder, seen, taken, b) = (
            env.machine(),
            env.clone(),
            st.clone(),
            holder.clone(),
            seen.clone(),
            taken.clone(),
            b.clone(),
        );
        off_thread(
            move || m.discover_printers(),
            move |found| {
                b.set_sensitive(true);
                b.set_label("Add Printer…");
                let new: Vec<(String, String)> = found
                    .into_iter()
                    .filter(|u| !taken.contains(u))
                    .map(|u| (device_name(&u), u))
                    .collect();
                if new.is_empty() {
                    return say(
                        &st,
                        "No other printers found. Check that the printer is on and on the same network.",
                    );
                }
                let items: Vec<w::MenuItem> = new
                    .iter()
                    .map(|(name, uri)| {
                        let (env, st, holder, seen, q, uri) = (
                            env.clone(),
                            st.clone(),
                            holder.clone(),
                            seen.clone(),
                            queue_name(name),
                            uri.clone(),
                        );
                        let f: Box<dyn Fn()> = Box::new(move || {
                            let Some(a) = admin("printer-add", &[&q, &uri]) else { return };
                            say(&st, "Adding the printer…");
                            let (env2, st2, h2, sh2) =
                                (env.clone(), st.clone(), holder.clone(), seen.clone());
                            env.ctx.act(
                                a,
                                Rc::new(move |r| match r {
                                    Ok(()) => {
                                        say(&st2, "");
                                        load(&env2, &h2, &st2, &sh2)
                                    }
                                    Err(x) => say(&st2, error_line(Err(x))),
                                }),
                            );
                        });
                        (name.as_str(), f)
                    })
                    .collect();
                popup(&b, items);
            },
        );
    });
    add.set_halign(gtk::Align::End);
    into.append(&add);
}

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let p = w::page("Printers");
    let body = w::vbox(w::S6);
    p.append(&body);
    let status = status_label();
    p.append(&status);
    let seen: Shown = Rc::default();
    load(e, &body, &status, &seen);
    watch(e, &body, &status, &seen);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    /// QA-LIVE3-15: a printer added outside the pane shows up; the same list does not redraw, and
    /// a pane still loading is left to the load.
    #[test]
    fn the_watch_redraws_only_a_changed_list() {
        let p = |n: &str| Printer {
            name: n.into(),
            ..Printer::default()
        };
        let one = Some(vec![p("a")]);
        let two = Some(vec![p("a"), p("b")]);
        assert!(!changed(&None, &one), "load under way");
        assert!(!changed(&Some(one.clone()), &one), "same list");
        assert!(changed(&Some(one.clone()), &two), "lpadmin added one");
        assert!(changed(&Some(two), &None), "cups went away");
    }

    #[test]
    fn device_names_read_like_people_say_them() {
        assert_eq!(
            device_name("dnssd://HP%20Color%20LaserJet%20MFP._ipp._tcp.local/"),
            "HP Color LaserJet MFP"
        );
        assert_eq!(device_name("ipp://192.168.1.40/ipp/print"), "192.168.1.40");
        assert_eq!(shown("Brother_HL_L2350DW"), "Brother HL L2350DW");
        let p = Printer {
            name: "x".into(),
            uri: String::new(),
            default: true,
            state: "Idle".into(),
        };
        assert_eq!(state_line(&p), "Default · Idle");
    }
}
