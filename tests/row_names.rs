// SPDX-License-Identifier: GPL-2.0-or-later
//! QA-ACC-2: every switch, slider and menu of a Settings row is named by the row's title on the
//! AT-SPI bus (LabelledBy, the subtitle as DescribedBy); an icon button is named by its tooltip.
//! Needs the toolkit: runs on a private Broadway display, skips without one.
use gtk::prelude::*;
use gtk::{AccessibleProperty as P, AccessibleRelation as R};
use shell::kit::{form, widgets as w};
use std::path::Path;

/// Kills the private broadwayd on every exit path.
struct Kill(std::process::Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A free Broadway display (the golden harness's scheme, another port range), or None.
fn broadway() -> Option<Kill> {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let n = (0..200)
        .map(|i| 240 + (std::process::id() + i) % 200)
        .find(|n| {
            !Path::new(&runtime)
                .join(format!("broadway{}.socket", n + 1))
                .exists()
                && std::net::TcpListener::bind(("127.0.0.1", (18000 + n) as u16)).is_ok()
        })?;
    let display = format!(":{n}");
    let child = std::process::Command::new("gtk4-broadwayd")
        .args([display.as_str(), "--port", &(18000 + n).to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let kill = Kill(child);
    let sock = Path::new(&runtime).join(format!("broadway{}.socket", n + 1));
    for _ in 0..100 {
        if std::os::unix::net::UnixStream::connect(&sock).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    unsafe {
        std::env::set_var("GDK_BACKEND", "broadway");
        std::env::set_var("BROADWAY_DISPLAY", &display);
    }
    Some(kill)
}

fn all<T: IsA<gtk::Widget>>(root: &gtk::Widget) -> Vec<T> {
    let mut out = Vec::new();
    let mut c = root.first_child();
    while let Some(x) = c {
        if let Some(s) = x.downcast_ref::<T>() {
            out.push(s.clone());
        }
        out.extend(all::<T>(&x));
        c = x.next_sibling();
    }
    out
}

#[test]
fn row_controls_carry_the_row_title_as_their_name() {
    let _display = broadway();
    if gtk::init().is_err() {
        eprintln!("skipped: no display for the toolkit");
        return;
    }
    let has = |x: &gtk::Widget, r: R| gtk::test_accessible_has_relation(x, r);

    let t = form::toggle_row("Reduce motion", "Fewer animations", false, |_| {});
    let s = all::<gtk::Switch>(&t);
    assert_eq!(s.len(), 1);
    assert!(
        has(s[0].upcast_ref(), R::LabelledBy),
        "switch named by the title"
    );
    assert!(
        has(s[0].upcast_ref(), R::DescribedBy),
        "and described by the subtitle"
    );

    let t = form::toggle_row("Show date", "", false, |_| {});
    let s = all::<gtk::Switch>(&t);
    assert!(has(s[0].upcast_ref(), R::LabelledBy));
    assert!(
        !has(s[0].upcast_ref(), R::DescribedBy),
        "no subtitle, no description"
    );

    // The switch nested in the pending row's control box is found too.
    let t = form::pending_toggle_row("Remote Login", "", false, |_, _| {});
    let s = all::<gtk::Switch>(&t);
    assert!(has(s[0].upcast_ref(), R::LabelledBy));

    let t = form::picker_row("Layout", "", &["Dwindle", "Master"], 0, |_| {});
    let d = all::<gtk::DropDown>(&t);
    assert!(
        has(d[0].upcast_ref(), R::LabelledBy),
        "menu named by the title"
    );

    let t = form::slider_row(
        "Pointer speed",
        (0.0, 10.0, 1.0),
        5.0,
        ("Slow", "Fast"),
        |_| {},
    );
    let sc = all::<gtk::Scale>(&t);
    assert!(
        has(sc[0].upcast_ref(), R::LabelledBy),
        "slider named by the title"
    );

    let i = w::icon_button("list-add-symbolic", "Add Printer", || {});
    assert!(gtk::test_accessible_has_property(&i, P::Label));
}
