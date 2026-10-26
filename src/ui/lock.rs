//! Lock screen (ext-session-lock, ADR 0022/0023) and login window (ADR 0024): clock, avatar, password.
use crate::kit::{self, widgets as w};
use crate::login::{self, Verdict};
use gtk::{glib, prelude::*};
use std::rc::Rc;

pub fn body(panel: &gtk::Box, picker: Option<gtk::Box>) -> gtk::Box {
    let b = w::vbox(0);
    b.add_css_class("lock-body");
    b.append(&w::clock_header());
    let s = w::vbox(0);
    s.set_vexpand(true);
    b.append(&s);
    if let Some(p) = picker {
        b.append(&p);
    }
    b.append(panel);
    b
}

pub fn user_name() -> String {
    std::env::var("USER").unwrap_or_else(|_| "user".into())
}

/// Wires Enter -> blocking check off the main thread -> `ok` or shake.
fn wire(
    panel: Rc<w::UserPanel>,
    check: impl Fn(String) -> Verdict + Send + Sync + Clone + 'static,
    ok: impl Fn() + Clone + 'static,
) {
    let p2 = panel.clone();
    panel.field.connect_activate(move |f| {
        let pw = f.text().to_string();
        f.set_sensitive(false);
        p2.message.set_label("");
        let (p, check, ok) = (p2.clone(), check.clone(), ok.clone());
        glib::spawn_future_local(async move {
            let v = gtk::gio::spawn_blocking(move || check(pw))
                .await
                .unwrap_or(Verdict::Error);
            p.field.set_sensitive(true);
            if v == Verdict::Ok {
                ok()
            } else {
                p.reject(v.message());
                p.field.grab_focus();
            }
        });
    });
}

pub fn run_lock() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("org.wolos.lock")
        .build();
    app.connect_activate(|app| {
        kit::style::install("");
        let win = gtk::ApplicationWindow::new(app);
        win.add_css_class("lock-window");
        let panel = Rc::new(w::UserPanel::new(&user_name()));
        win.set_child(Some(&body(&panel.root, None)));
        kit::snapshot_on_map(&win, "lock");
        let unlock = lock_surface(app, &win);
        let app2 = app.clone();
        wire(panel.clone(), login::authcheck_owned, move || {
            unlock();
            app2.quit();
        });
        panel.field.grab_focus();
    });
    app.run_with_args::<&str>(&[])
}

/// Session lock on every monitor (this window on the first); returns the unlock action.
#[cfg(feature = "layer-shell")]
fn lock_surface(app: &gtk::Application, win: &gtk::ApplicationWindow) -> Rc<dyn Fn()> {
    use gtk::gdk;
    if !gtk4_session_lock::is_supported() {
        win.fullscreen();
        win.present();
        return Rc::new(|| {});
    }
    let lock = gtk4_session_lock::Instance::new();
    let (app, first) = (app.clone(), win.clone());
    let used = std::cell::Cell::new(false);
    lock.connect_monitor(move |l, m: &gdk::Monitor| {
        let w = if !used.replace(true) {
            first.clone()
        } else {
            let w = gtk::ApplicationWindow::new(&app);
            w.add_css_class("lock-window");
            w
        };
        l.assign_window_to_monitor(&w, m);
    });
    lock.lock();
    Rc::new(move || lock.unlock())
}

#[cfg(not(feature = "layer-shell"))]
fn lock_surface(_app: &gtk::Application, win: &gtk::ApplicationWindow) -> Rc<dyn Fn()> {
    eprintln!("wol-lock: built without layer-shell: NOT a secure lock (dev window)");
    win.present();
    Rc::new(|| {})
}

pub fn run_login() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("org.wolos.loginwindow")
        .build();
    app.connect_activate(|app| {
        kit::style::install("");
        let win = gtk::ApplicationWindow::new(app);
        win.add_css_class("lock-window");
        let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
        let shells = std::fs::read_to_string("/etc/shells").unwrap_or_default();
        let users = login::users(&passwd, &shells);
        let first = users.first().cloned().unwrap_or_default();
        let panel = Rc::new(w::UserPanel::new(&first));
        let selected = Rc::new(std::cell::RefCell::new(first));
        let picker = (users.len() > 1).then(|| {
            let row = w::hbox(w::S4);
            row.set_halign(gtk::Align::Center);
            row.set_margin_bottom(w::S6);
            let name_label = panel
                .root
                .observe_children()
                .item(1)
                .and_downcast::<gtk::Label>();
            for u in &users {
                let b = gtk::Button::builder()
                    .child(&w::avatar(u, w::AVATAR_L))
                    .tooltip_text(u)
                    .build();
                b.add_css_class("icon-button");
                let (u, sel, nl, f) = (
                    u.clone(),
                    selected.clone(),
                    name_label.clone(),
                    panel.field.clone(),
                );
                b.connect_clicked(move |_| {
                    if let Some(l) = &nl {
                        l.set_label(&u);
                    }
                    *sel.borrow_mut() = u.clone();
                    f.grab_focus();
                });
                row.append(&b);
            }
            row
        });
        win.set_child(Some(&body(&panel.root, picker)));
        let sel = selected.clone();
        let p2 = panel.clone();
        panel.field.connect_activate(move |f| {
            let (user, pw) = (sel.borrow().clone(), f.text().to_string());
            f.set_sensitive(false);
            let p = p2.clone();
            glib::spawn_future_local(async move {
                let v =
                    gtk::gio::spawn_blocking(move || login::login(login::LOGIN_SOCKET, &user, &pw))
                        .await
                        .unwrap_or(Verdict::Error);
                p.field.set_sensitive(true);
                if v == Verdict::Ok {
                    std::process::exit(0)
                } else {
                    p.reject(v.message());
                    p.field.grab_focus();
                }
            });
        });
        kit::snapshot_on_map(&win, "loginwindow");
        win.fullscreen();
        win.present();
        panel.field.grab_focus();
    });
    app.run_with_args::<&str>(&[])
}
