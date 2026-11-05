// SPDX-License-Identifier: GPL-2.0-or-later
//! App Store > Web Apps (what `wol webapp add` did only in the Terminal): the form above and
//! the web apps there already are below; one with its own sign-ins, one with a picture of its own.
//! Wide and narrow with a list, and the empty page.
use super::Case;
use gtk::prelude::*;
use shell::ui::store::{self, webapps};
use std::sync::Arc;

fn row(id: &str, name: &str, url: &str, own: bool) -> webapps::Row {
    webapps::Row {
        id: id.into(),
        name: name.into(),
        url: url.into(),
        icon: "web-browser".into(),
        own_profile: own,
    }
}

pub fn listing() -> webapps::Listing {
    webapps::Listing::Ready(vec![
        row(
            "calendar",
            "Calendar",
            "https://calendar.example.com/work",
            false,
        ),
        row("mail", "Mail", "https://mail.example.com", true),
        row("notes", "Notes", "https://notes.example.org", false),
    ])
}

type Make = fn() -> webapps::Listing;

fn page(l: Make) -> Box<dyn Fn() -> gtk::Widget> {
    Box::new(move || {
        let s = store::content(
            Arc::new(shell::system::Fake),
            store::Env {
                data_dirs: vec![],
                pkg_search: false,
                flatpaks: vec![],
                installed: Some(Default::default()),
            },
        );
        s.show_webapps(l());
        s.aw.open("Web Apps");
        s.aw.root.clone().upcast()
    })
}

pub fn cases() -> Vec<Case> {
    let mut v = Vec::new();
    for (sw, sh, tag) in [(900, 700, "wide"), (480, 500, "narrow")] {
        v.push(Case {
            name: format!("store-webapps-list-{tag}"),
            size: (sw, sh),
            class: "preview-app",
            wallpaper: false,
            build: page(listing),
        });
    }
    v.push(Case {
        name: "store-webapps-empty".into(),
        size: (900, 600),
        class: "preview-app",
        wallpaper: false,
        build: page(|| webapps::Listing::Ready(vec![])),
    });
    v
}

/// The form is the Terminal's verb: a wrong address is told on the page and runs nothing; a right
/// one runs `wol webapp add --json -- Mail https://mail.example.com` (a `wol` on PATH that writes
/// its words down), says the web app is ready and clears the form for the next one.
pub fn the_form_runs_the_terminal_verb() {
    use crate::{find_type, pump};
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("wol-webapps-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("log");
    let wol = dir.join("wol");
    std::fs::write(
        &wol,
        format!(
            "#!/bin/sh\necho \"$@\" >> {}\n[ \"$2\" = list ] && echo '{{\"webapps\":[]}}'\nexit 0\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&wol, std::fs::Permissions::from_mode(0o755)).unwrap();
    let old = std::env::var("PATH").unwrap_or_default();
    unsafe { std::env::set_var("PATH", format!("{}:{old}", dir.display())) };

    let s = store::content(
        Arc::new(shell::system::Fake),
        store::Env {
            data_dirs: vec![],
            pkg_search: false,
            flatpaks: vec![],
            installed: Some(Default::default()),
        },
    );
    s.show_webapps(webapps::Listing::Ready(vec![]));
    let root: gtk::Widget = s.aw.root.clone().upcast();
    let mut entries: Vec<gtk::Entry> = Vec::new();
    find_type(&root, &mut entries);
    let entry = |hint: &str| {
        entries
            .iter()
            .find(|e| e.placeholder_text().as_deref() == Some(hint))
            .unwrap_or_else(|| panic!("a field showing {hint}"))
            .clone()
    };
    let (name, address) = (entry("Mail"), entry("mail.example.com"));
    let make = || {
        let mut buttons: Vec<gtk::Button> = Vec::new();
        find_type(&root, &mut buttons);
        buttons
            .into_iter()
            .find(|b| b.label().as_deref() == Some("Make Web App"))
            .expect("the Make Web App button")
    };
    let said = |text: &str| {
        let mut labels: Vec<gtk::Label> = Vec::new();
        find_type(&root, &mut labels);
        labels
            .iter()
            .any(|l| l.is_visible() && l.label().contains(text))
    };

    // A wrong address: told, and nothing ran.
    name.set_text("Mail");
    address.set_text("not a url");
    make().emit_clicked();
    pump(100);
    assert!(said("not a web address"), "the wrong address is told");
    assert!(
        !log.exists(),
        "nothing ran: {:?}",
        std::fs::read_to_string(&log)
    );

    // A right one: the Terminal's words.
    address.set_text("mail.example.com");
    make().emit_clicked();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !std::fs::read_to_string(&log).is_ok_and(|t| t.contains("webapp add")) {
        assert!(std::time::Instant::now() < until, "the verb ran");
        pump(20);
    }
    pump(200);
    let ran = std::fs::read_to_string(&log).unwrap();
    assert!(
        ran.lines()
            .any(|l| l == "webapp add --json -- Mail https://mail.example.com"),
        "{ran}"
    );
    assert!(said("is ready"), "it says the web app is ready");
    assert_eq!(name.text(), "", "the form is ready for the next one");
    unsafe { std::env::set_var("PATH", old) };
    let _ = std::fs::remove_dir_all(&dir);
    eprintln!("store_webapps: the_form_runs_the_terminal_verb ok");
}
