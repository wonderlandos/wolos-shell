//! Event sources on the GTK main loop (no threads, no polling): the shell socket and Hyprland's event socket.
use super::{Panels, dock::Dock, menubar::Bar, panels::Osd};
use crate::{hypr, protocol};
use gtk::{gio, glib, prelude::*};
use std::rc::Rc;

/// `$XDG_RUNTIME_DIR/wol-shell.sock`: `toggle|show|hide <panel>`, `reload`.
pub fn shell_socket(panels: Panels, osd: Rc<Osd>, sys: Rc<dyn crate::system::System>) {
    let path = protocol::socket_path(&crate::kit::runtime_dir());
    let _ = std::fs::remove_file(&path);
    let listener = gio::SocketListener::new();
    if let Err(e) = listener.add_address(
        &gio::UnixSocketAddress::new(&path),
        gio::SocketType::Stream,
        gio::SocketProtocol::Default,
        None::<&glib::Object>,
    ) {
        eprintln!("wol-shell: {}: {e}", path.display());
        return;
    }
    glib::spawn_future_local(async move {
        while let Ok((conn, _)) = listener.accept_future().await {
            let (panels, osd, sys) = (panels.clone(), osd.clone(), sys.clone());
            glib::spawn_future_local(async move {
                let input = gio::DataInputStream::new(&conn.input_stream());
                while let Ok(Some(line)) =
                    input.read_line_utf8_future(glib::Priority::DEFAULT).await
                {
                    let Some(cmd) = protocol::parse(&line) else {
                        eprintln!("wol-shell: bad command '{line}'");
                        continue;
                    };
                    run(&cmd, &panels, &osd, &*sys);
                }
            });
        }
    });
}

fn run(cmd: &protocol::Command, panels: &Panels, osd: &Rc<Osd>, sys: &dyn crate::system::System) {
    if cmd.verb == protocol::Verb::Reload {
        return;
    } // theme reloads through its file monitor
    if cmd.target == "osd" && cmd.verb == protocol::Verb::Show {
        let a = sys.audio();
        return osd.show(crate::status::volume(a), a.volume.unwrap_or(0));
    }
    match panels.borrow().get(cmd.target.as_str()) {
        Some(w) => w.set_visible(protocol::apply(cmd.verb, w.is_visible())),
        None => eprintln!("wol-shell: no panel '{}'", cmd.target),
    }
}

/// Hyprland `.socket2.sock`: active window -> menu bar name; open/close -> dock running dots.
pub fn hypr_events(bar: Rc<Bar>, dock: Rc<Dock>) {
    let Some(path) = hypr::socket(".socket2.sock") else {
        return;
    };
    let installed = Rc::new(std::cell::OnceCell::<Vec<crate::desktop::Entry>>::new());
    glib::spawn_future_local(async move {
        let Ok(conn) = gio::SocketClient::new()
            .connect_future(&gio::UnixSocketAddress::new(&path))
            .await
        else {
            return;
        };
        let input = gio::DataInputStream::new(&conn.input_stream());
        while let Ok(Some(line)) = input
            .read_line_utf8_future(glib::Priority::DEFAULT_IDLE)
            .await
        {
            match hypr::parse_event(&line) {
                Some(hypr::Event::ActiveWindow { class, title }) => {
                    let inst = installed.get_or_init(crate::desktop::load_all);
                    bar.set_app(crate::apps::frontmost_name(
                        &class,
                        &title,
                        crate::apps::PINS,
                        inst,
                    ));
                }
                Some(hypr::Event::OpenWindow { .. } | hypr::Event::CloseWindow { .. }) => {
                    dock.set_running(&hypr::running_classes(&hypr::clients()));
                }
                _ => {}
            }
        }
    });
}
