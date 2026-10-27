//! Menu Bar & Dock: shell prefs written to shell.json, applied with `reload`.
use super::*;

// ---- Menu Bar & Dock ----

/// Write shell.json and tell the running shell (`reload` on wol-shell.sock).
fn save_shell(p: ShellPrefs) {
    let _ = write(&config(prefs::SHELL_FILE), &p.to_json());
    let sock = crate::protocol::socket_path(&kit::runtime_dir());
    std::thread::spawn(move || {
        use std::io::Write;
        if let Ok(mut s) = std::os::unix::net::UnixStream::connect(sock) {
            let _ = s.write_all(b"reload\n");
        }
    });
}

pub(super) fn page() -> gtk::Box {
    let p = w::page("Menu Bar & Dock");
    let cur = Rc::new(std::cell::Cell::new(ShellPrefs::parse(
        read(&config(prefs::SHELL_FILE)).as_deref(),
    )));
    let t = |get: fn(&ShellPrefs) -> bool, set: fn(&mut ShellPrefs, bool)| {
        let c = cur.clone();
        w::toggle(get(&cur.get()), move |on| {
            let mut v = c.get();
            set(&mut v, on);
            c.set(v);
            save_shell(v);
        })
    };
    p.append(&w::section(
        "Menu Bar",
        &[
            row(
                "Show battery percentage",
                &t(|p| p.battery_percent, |p, v| p.battery_percent = v),
            ),
            row("Show date", &t(|p| p.show_date, |p, v| p.show_date = v)),
        ],
    ));
    p.append(&w::section(
        "Dock",
        &[
            row(
                "Automatically hide and show the Dock",
                &t(|p| p.dock_autohide, |p, v| p.dock_autohide = v),
            ),
            row(
                "Magnification",
                &t(|p| p.dock_magnify, |p, v| p.dock_magnify = v),
            ),
        ],
    ));
    p
}
