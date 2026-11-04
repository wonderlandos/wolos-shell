// SPDX-License-Identifier: GPL-2.0-or-later
//! Printers: CUPS queues (default, state), Add Printer from the devices CUPS finds (driverless IPP
//! Everywhere), Set as Default and Remove (wol-admind `printer-add`, `printer-default`,
//! `printer-remove`; the Authenticate sheet asks first).
use super::*;
use crate::copy::settings as copy;
use wol_core::printers::{Printer, unique_queue_name};

/// What people call a device URI: "dnssd://HP%20LaserJet._ipp._tcp.local/" -> "HP LaserJet",
/// "ipp://10.0.0.5/ipp/print" -> "10.0.0.5".
pub(crate) fn device_name(uri: &str) -> String {
    let rest = uri.split_once("://").map_or(uri, |x| x.1);
    let host = rest.split('/').next().unwrap_or(rest);
    let name = host.split("._").next().unwrap_or(host);
    // Bytes first, then UTF-8: "Caf%C3%A9" is two bytes of one letter (QA-SET-14: each byte used to
    // become its own character, "CafÃ©").
    let mut out: Vec<u8> = Vec::new();
    let b = name.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(v) = name
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The queue name for each of `names`: none that `existing` queues or the earlier names of this
/// menu already have, so two printers of one model get "_2", never the first one's queue (QA-SET-14).
fn queue_names(names: &[&str], existing: &[String]) -> Vec<String> {
    let mut used = existing.to_vec();
    names
        .iter()
        .map(|n| {
            let q = unique_queue_name(n, &used);
            used.push(q.clone());
            q
        })
        .collect()
}

/// "Brother_HL_L2350DW" -> "Brother HL L2350DW".
fn shown(queue: &str) -> String {
    queue.replace('_', " ")
}

/// "Default · Printing · 2 jobs · Connecting to printer.": what CUPS says now, not only the
/// state word (QA-DEV-3: a printer that went away read "Printing" forever, with no reason).
fn state_line(p: &Printer) -> String {
    let mut parts: Vec<String> = Vec::new();
    if p.default {
        parts.push("Default".into());
    }
    parts.push(p.state.clone());
    match p.jobs {
        0 => {}
        1 => parts.push("1 job".into()),
        n => parts.push(format!("{n} jobs")),
    }
    if !p.message.is_empty() {
        parts.push(p.message.clone());
    }
    parts.join(" · ")
}

/// How often the list is read again while it is on screen (QA-DEV-3: it was read once).
const REFRESH_SECS: u32 = 5;

/// Runs `argv` (a user printing command) off the UI thread, says `done` or the failure, reloads.
fn user_job(
    e: &Rc<Env>,
    into: &gtk::Box,
    status: &gtk::Label,
    argv: Vec<String>,
    done: &'static str,
) {
    let (m, env, st, holder) = (e.machine(), e.clone(), status.clone(), into.clone());
    off_thread(
        move || m.printer_job(&argv),
        move |ok| {
            say(&st, if ok { done } else { copy::PRINTER_JOB_FAILED });
            load(&env, &holder, &st);
        },
    );
}

fn load(e: &Rc<Env>, into: &gtk::Box, status: &gtk::Label) {
    clear(into);
    let (m, e2, st) = (e.machine(), e.clone(), status.clone());
    later(
        into,
        move || m.printers(),
        move |into, p| render(&e2, into, p, &st),
    );
}

fn render(e: &Rc<Env>, into: &gtk::Box, printers: Option<Vec<Printer>>, status: &gtk::Label) {
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
                let (env, st, q, holder) =
                    (e.clone(), status.clone(), p.name.clone(), into.clone());
                let f: Box<dyn Fn()> = Box::new(move || {
                    let Some(a) = admin(verb, &[&q]) else { return };
                    let (env2, st2, h2) = (env.clone(), st.clone(), holder.clone());
                    env.ctx.act(
                        a,
                        Rc::new(move |r| match r {
                            Ok(()) => load(&env2, &h2, &st2),
                            Err(x) => say(&st2, error_line(Err(x))),
                        }),
                    );
                });
                f
            };
            if let Some(argv) = wol_core::printers::test_page_argv(&p.name) {
                let (env, st, holder) = (e.clone(), status.clone(), into.clone());
                items.push((
                    "Print Test Page",
                    Box::new(move || {
                        user_job(&env, &holder, &st, argv.clone(), copy::PRINTER_TEST_SENT)
                    }),
                ));
            }
            if p.jobs > 0
                && let Some(argv) = wol_core::printers::cancel_argv(&p.name)
            {
                let (env, st, holder) = (e.clone(), status.clone(), into.clone());
                items.push((
                    "Cancel All Jobs",
                    Box::new(move || user_job(&env, &holder, &st, argv.clone(), "")),
                ));
            }
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
    let (env, st, holder, taken, queues) = (
        e.clone(),
        status.clone(),
        into.clone(),
        printers.iter().map(|p| p.uri.clone()).collect::<Vec<_>>(),
        printers.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
    );
    let add = w::button("Add Printer…", w::Kind::Default, || {});
    add.connect_clicked(move |b| {
        b.set_sensitive(false);
        b.set_label("Looking for printers…");
        let (m, env, st, holder, taken, queues, b) = (
            env.machine(),
            env.clone(),
            st.clone(),
            holder.clone(),
            taken.clone(),
            queues.clone(),
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
                let names: Vec<&str> = new.iter().map(|n| n.0.as_str()).collect();
                let items: Vec<w::MenuItem> = new
                    .iter()
                    .zip(queue_names(&names, &queues))
                    .map(|((name, uri), q)| {
                        let (env, st, holder, uri) =
                            (env.clone(), st.clone(), holder.clone(), uri.clone());
                        let f: Box<dyn Fn()> = Box::new(move || {
                            let Some(a) = admin("printer-add", &[&q, &uri]) else { return };
                            say(&st, "Adding the printer…");
                            let (env2, st2, h2) = (env.clone(), st.clone(), holder.clone());
                            env.ctx.act(
                                a,
                                Rc::new(move |r| match r {
                                    Ok(()) => {
                                        say(&st2, "");
                                        load(&env2, &h2, &st2)
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
    load(e, &body, &status);
    // Read again while on screen, so a job that finished or a printer that went away shows; the
    // list is built again only when CUPS says something new (an open row menu stays open).
    let last: Rc<RefCell<Option<Vec<Printer>>>> = Rc::default();
    let (env, b, st) = (Rc::downgrade(e), body.downgrade(), status.clone());
    glib::timeout_add_seconds_local(REFRESH_SECS, move || {
        let (Some(env), Some(b)) = (env.upgrade(), b.upgrade()) else {
            return glib::ControlFlow::Break;
        };
        if b.is_mapped() {
            let (m, last, st) = (env.machine(), last.clone(), st.clone());
            off_thread(
                move || m.printers(),
                move |now| {
                    // The first read only remembers (the page has just shown the same).
                    let first = last.borrow().is_none();
                    if now.is_none() || *last.borrow() == now {
                        return;
                    }
                    *last.borrow_mut() = now.clone();
                    if !first {
                        clear(&b);
                        render(&env, &b, now, &st);
                    }
                },
            );
        }
        glib::ControlFlow::Continue
    });
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_names_read_like_people_say_them() {
        assert_eq!(
            device_name("dnssd://HP%20Color%20LaserJet%20MFP._ipp._tcp.local/"),
            "HP Color LaserJet MFP"
        );
        assert_eq!(device_name("ipp://192.168.1.40/ipp/print"), "192.168.1.40");
        // QA-SET-14: percent-encoded UTF-8 is decoded as bytes, and a stray byte is not a panic.
        assert_eq!(
            device_name("dnssd://Caf%C3%A9%20Printer._ipp._tcp.local/"),
            "Café Printer"
        );
        assert_eq!(
            device_name("dnssd://%E6%97%A5%E6%9C%AC._ipp._tcp.local/"),
            "日本"
        );
        assert_eq!(
            device_name("dnssd://Bad%FFName._ipp._tcp.local/"),
            "Bad\u{fffd}Name"
        );
        assert_eq!(device_name("dnssd://100%._ipp._tcp.local/"), "100%");
        assert_eq!(shown("Brother_HL_L2350DW"), "Brother HL L2350DW");
        // Two printers of one model, and one a queue of that name already has: three queues.
        assert_eq!(
            queue_names(
                &["HP LaserJet", "HP LaserJet"],
                &["HP_LaserJet".to_string()]
            ),
            ["HP_LaserJet_2", "HP_LaserJet_3"]
        );
        let mut p = Printer {
            name: "x".into(),
            uri: String::new(),
            default: true,
            state: "Idle".into(),
            ..Default::default()
        };
        assert_eq!(state_line(&p), "Default · Idle");
        // QA-DEV-3: what CUPS says, and the jobs waiting.
        (p.default, p.state, p.jobs) = (false, "Printing".into(), 2);
        p.message = "Connecting to printer.".into();
        assert_eq!(state_line(&p), "Printing · 2 jobs · Connecting to printer.");
        p.jobs = 1;
        assert!(state_line(&p).contains("1 job ·"));
    }
}
