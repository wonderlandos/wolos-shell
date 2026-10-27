//! Network: Wi-Fi radio, networks, join sheet.
use super::*;

// ---- Network ----

pub(super) fn page(ctx: &Rc<Ctx>, join_sheet: &gtk::Revealer) -> gtk::Box {
    let p = w::page("Network");
    let status = status_label();
    let wifi_on = ctx.sys.wifi_enabled();
    let toggle = {
        let (ctx, st) = (ctx.clone(), status.clone());
        w::toggle(wifi_on.unwrap_or(false), move |on| {
            let st = st.clone();
            ctx.act(
                Action::Wifi(on),
                Rc::new(move |r| say(&st, r.err().map_or("", |e| e.message()))),
            );
        })
    };
    toggle.set_sensitive(wifi_on.is_some());
    let connected = ctx
        .sys
        .wifi_networks()
        .into_iter()
        .find(|n| n.connected)
        .map_or_else(
            || crate::system::wifi_subtitle(ctx.sys.network(), &[]),
            |n| n.ssid,
        );
    p.append(&w::section(
        "",
        &[w::list_row(
            "network-wireless-symbolic",
            "Wi-Fi",
            &connected,
            Some(toggle.upcast_ref()),
        )
        .upcast()],
    ));

    // Join sheet: password for a secured network.
    let title = w::label("", "title");
    title.set_wrap(true);
    let pw = w::password_field("Password");
    let body = w::vbox(w::S3);
    body.append(&title);
    body.append(&pw);
    let buttons = w::hbox(w::S2);
    buttons.append(&w::spacer());
    let ssid: Rc<RefCell<String>> = Default::default();
    {
        let r = join_sheet.clone();
        buttons.append(&w::button("Cancel", w::Kind::Default, move || {
            r.set_reveal_child(false)
        }));
    }
    let join: Rc<dyn Fn(String, Option<String>)> = {
        let (ctx, st) = (ctx.clone(), status.clone());
        Rc::new(move |id: String, pass: Option<String>| {
            say(&st, &format!("Joining “{id}”…"));
            let st = st.clone();
            let id2 = id.clone();
            ctx.act(
                Action::WifiJoin(id, pass),
                Rc::new(move |r| match r {
                    Ok(()) => say(&st, &format!("Connected to “{id2}”.")),
                    Err(ActionError::Denied) => {
                        say(&st, "Could not join. Check the password and try again.")
                    }
                    Err(e) => say(&st, e.message()),
                }),
            );
        })
    };
    {
        let (r, pwc, id, join) = (join_sheet.clone(), pw.clone(), ssid.clone(), join.clone());
        let go = Rc::new(move || {
            let pass = pwc.text().to_string();
            pwc.set_text("");
            r.set_reveal_child(false);
            join(id.borrow().clone(), Some(pass));
        });
        let g = go.clone();
        let pw2 = pw.clone();
        pw2.connect_activate(move |_| g());
        buttons.append(&w::button("Join", w::Kind::Primary, move || go()));
    }
    body.append(&buttons);
    sheet_body(join_sheet, &body);

    let list = w::vbox(0);
    let fill = {
        let (list, sheet, title, pw, ssid, join) = (
            list.clone(),
            join_sheet.clone(),
            title.clone(),
            pw.clone(),
            ssid.clone(),
            join.clone(),
        );
        move |nets: Vec<crate::system::WifiNetwork>| {
            while let Some(c) = list.first_child() {
                list.remove(&c);
            }
            if nets.is_empty() {
                list.append(&row("No networks found", &w::label("", "secondary")));
            }
            for n in nets.iter().take(10) {
                let sub = if n.connected {
                    "Connected"
                } else if n.secure {
                    "Secured"
                } else {
                    "Open"
                };
                let b = w::choice_row(
                    crate::system::signal_icon(n.signal),
                    &n.ssid,
                    sub,
                    n.connected,
                );
                let (n, sheet, title, pw, ssid, join) = (
                    n.clone(),
                    sheet.clone(),
                    title.clone(),
                    pw.clone(),
                    ssid.clone(),
                    join.clone(),
                );
                b.connect_clicked(move |_| {
                    if n.connected {
                        return;
                    }
                    if n.secure {
                        *ssid.borrow_mut() = n.ssid.clone();
                        title.set_label(&format!("Enter the password for “{}”", n.ssid));
                        sheet.set_reveal_child(true);
                        pw.grab_focus();
                    } else {
                        join(n.ssid.clone(), None);
                    }
                });
                list.append(&b);
            }
        }
    };
    fill(ctx.sys.wifi_networks());
    p.append(&w::section("Networks", &[list.upcast()]));
    p.append(&status);
    if let Some(rx) = ctx.sys.watch() {
        let sys = ctx.sys.clone();
        let last = RefCell::new(sys.wifi_networks());
        glib::spawn_future_local(async move {
            while rx.recv().await.is_ok() {
                let n = sys.wifi_networks();
                if *last.borrow() != n {
                    fill(n.clone());
                    *last.borrow_mut() = n;
                }
            }
        });
    }
    p
}
