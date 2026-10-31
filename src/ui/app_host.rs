//! Kit host (ADR 0020). The shell listens on `$XDG_RUNTIME_DIR/wol-kit.sock` (mode 0600, own
//! account only) and starts one `wol-kit-host` per app connection, so each app's windows carry
//! its own window class and a misbehaving app takes down only its own host. The host draws the
//! app's view trees with Kit parts (`ui::app_view`) and sends what the person does back.
use super::app_view::{Act, Sink, View, app_frame};
use crate::app_host::{Session, Step, gtk_app_id, read_line, socket_path};
use crate::kit;
use gtk::{gio, glib, prelude::*};
use shared::view::{FromHost, ToHost};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{BufReader, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// The host binary: next to the running shell, else on PATH.
fn host_binary() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("wol-kit-host")))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| "wol-kit-host".into())
}

fn bind(path: &Path) -> std::io::Result<UnixListener> {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::remove_file(path);
    let l = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(l)
}

/// Shell side: accept app connections for the session (a thread; never touches GTK).
pub fn listen() {
    let Some(path) = socket_path() else {
        eprintln!("wol-shell: Kit host: no XDG_RUNTIME_DIR");
        return;
    };
    let l = match bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("wol-shell: Kit host: {}: {e}", path.display());
            return;
        }
    };
    let me = shared::sys::euid();
    let host = host_binary();
    std::thread::spawn(move || {
        for conn in l.incoming().flatten() {
            if shared::sys::peer(&conn).map(|p| p.uid) != Some(me) {
                continue;
            }
            let fd: OwnedFd = conn.into();
            match std::process::Command::new(&host)
                .stdin(fd)
                .stdout(std::process::Stdio::null())
                .spawn()
            {
                Ok(mut child) => {
                    std::thread::spawn(move || child.wait());
                }
                Err(e) => eprintln!("wol-shell: Kit host: {}: {e}", host.display()),
            }
        }
    });
}

/// One open app window.
struct Win {
    window: gtk::ApplicationWindow,
    view: View,
    title: gtk::Label,
}

/// `wol-kit-host`: the app connection is stdin. Reads the hello, then draws until the app goes.
pub fn run_host() -> glib::ExitCode {
    // SAFETY: fd 0 is ours (the shell passed the connection as stdin) and nothing else uses it.
    let conn = unsafe { UnixStream::from_raw_fd(0) };
    let mut session = Session::default();
    let mut reader = BufReader::new(conn.try_clone().expect("clone app connection"));
    let first = read_line(&mut reader).and_then(|l| ToHost::parse(&l));
    let Some(Ok(_)) = first.map(|m| session.on(m)) else {
        return glib::ExitCode::FAILURE;
    };
    let app_id = session.app().unwrap_or_default().to_owned();
    let app = gtk::Application::builder()
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    if let Some(id) = gtk_app_id(&app_id) {
        app.set_application_id(Some(id));
    }
    let (tx, rx) = async_channel::unbounded::<ToHost>();
    std::thread::spawn(move || {
        while let Some(line) = read_line(&mut reader) {
            // A message that does not parse ends the connection (the app is not trusted).
            let Some(m) = ToHost::parse(&line) else { break };
            if tx.send_blocking(m).is_err() {
                break;
            }
        }
    });
    let session = Rc::new(RefCell::new(session));
    app.connect_activate(move |app| {
        kit::style::install("");
        let hold = app.hold();
        let wins: Rc<RefCell<HashMap<u32, Win>>> = Default::default();
        let send = {
            let conn = conn.try_clone().expect("clone app connection");
            Rc::new(move |e: FromHost| {
                let _ = (&conn).write_all(e.to_line().as_bytes());
            })
        };
        let (app, rx, session) = (app.clone(), rx.clone(), session.clone());
        glib::spawn_future_local(async move {
            while let Ok(m) = rx.recv().await {
                let step = session.borrow_mut().on(m);
                match step {
                    Err(why) => {
                        eprintln!("wol-kit-host: {why}");
                        break;
                    }
                    Ok(Step::Open {
                        window,
                        title,
                        content,
                    }) => {
                        let s = send.clone();
                        let sink = Sink::new(move |a| {
                            s(match a {
                                Act::Clicked(widget) => FromHost::Clicked { window, widget },
                                Act::Edited(widget, text) => FromHost::Edited {
                                    window,
                                    widget,
                                    text,
                                },
                                Act::Toggled(widget, on) => {
                                    FromHost::Toggled { window, widget, on }
                                }
                                Act::Slid(widget, value) => FromHost::Slid {
                                    window,
                                    widget,
                                    value,
                                },
                            })
                        });
                        let view = View::new(&content, sink);
                        let aw = app_frame(&title, &view);
                        let win = gtk::ApplicationWindow::builder()
                            .application(&app)
                            .title(title.as_str())
                            .decorated(false)
                            .build();
                        win.add_css_class("app");
                        win.set_child(Some(&aw.root));
                        let (s, session, wins2) = (send.clone(), session.clone(), wins.clone());
                        win.connect_close_request(move |_| {
                            if session.borrow_mut().forget(window) {
                                s(FromHost::Closed { window });
                            }
                            wins2.borrow_mut().remove(&window);
                            glib::Propagation::Proceed
                        });
                        win.present();
                        wins.borrow_mut().insert(
                            window,
                            Win {
                                window: win,
                                view,
                                title: aw.title.clone(),
                            },
                        );
                    }
                    Ok(Step::Update {
                        window,
                        title,
                        content,
                    }) => {
                        if let Some(w) = wins.borrow().get(&window) {
                            w.view.update(&content);
                            w.title.set_label(&title);
                            w.window.set_title(Some(&title));
                        }
                    }
                    Ok(Step::Close { window }) => {
                        let w = wins.borrow_mut().remove(&window);
                        if let Some(w) = w {
                            w.window.destroy();
                        }
                    }
                    Ok(Step::Nothing) => {}
                }
            }
            // The app went away: so do its windows.
            for (_, w) in wins.borrow_mut().drain() {
                w.window.destroy();
            }
            drop(hold);
            app.quit();
        });
    });
    app.run_with_args::<&str>(&[])
}
