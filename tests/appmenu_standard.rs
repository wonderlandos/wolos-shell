// SPDX-License-Identifier: GPL-2.0-or-later
//! E2E-MENU: the standard menus. An app that publishes no menus gets File, Edit and View out of
//! the keys the ⌘ shortcuts tap (`appmenu::send`), greyed where the app has none, then Window and
//! Help. A Wolkit app that publishes its own keeps precedence. This is the table of the twenty
//! commands in six kinds of app, and the rules around it (docs/modules/menus.md).
use shell::appmenu::{
    self, Act, Cmd, Handoff, Menus, StdEntry, argv, enabled, help_menu, menus, settings_key,
    standard_menus,
};

/// The verbs of [`Cmd::ALL`] a menu row is live for, per class: the table.
const TABLE: [(&str, &[&str]); 5] = [
    // Text Editor: every command but Reload and ⌘L (which is "go to line" there).
    (
        "org.gnome.TextEditor",
        &[
            "copy",
            "paste",
            "cut",
            "undo",
            "redo",
            "select-all",
            "save",
            "open",
            "print",
            "find",
            "find-next",
            "new-tab",
            "close",
            "new",
            "zoom-in",
            "zoom-out",
            "actual-size",
            "emoji",
        ],
    ),
    // Files: no Save, Print, Find Next, emoji.
    (
        "org.gnome.Nautilus",
        &[
            "copy",
            "paste",
            "cut",
            "undo",
            "redo",
            "select-all",
            "open",
            "find",
            "new-tab",
            "close",
            "new",
            "reload",
            "location",
            "zoom-in",
            "zoom-out",
            "actual-size",
        ],
    ),
    // A terminal: copy and paste are Ctrl+Shift, Ctrl+letter is a control character, so no Undo,
    // Save, Cut, Select All...
    (
        "foot",
        &[
            "copy",
            "paste",
            "find",
            "close",
            "new",
            "zoom-in",
            "zoom-out",
            "actual-size",
        ],
    ),
    // Atlas (the engine's window class is helium): everything but the emoji chooser.
    (
        "helium",
        &[
            "copy",
            "paste",
            "cut",
            "undo",
            "redo",
            "select-all",
            "save",
            "open",
            "print",
            "find",
            "find-next",
            "new-tab",
            "close",
            "new",
            "reload",
            "location",
            "zoom-in",
            "zoom-out",
            "actual-size",
        ],
    ),
    // Papers: a viewer: nothing to cut, paste, undo or create.
    (
        "org.gnome.Papers",
        &[
            "copy",
            "select-all",
            "save",
            "open",
            "print",
            "find",
            "find-next",
            "close",
            "reload",
            "zoom-in",
            "zoom-out",
            "actual-size",
        ],
    ),
];

#[test]
fn twenty_verbs_in_six_kinds_of_app() {
    for (class, want) in TABLE {
        let got: Vec<&str> = Cmd::ALL
            .into_iter()
            .filter(|c| enabled(*c, class))
            .map(Cmd::verb)
            .collect();
        let mut want_sorted: Vec<&str> = want.to_vec();
        // The table is written by hand: same set as the code's, in the code's order.
        want_sorted.sort_by_key(|v| Cmd::ALL.iter().position(|c| c.verb() == *v));
        assert_eq!(got, want_sorted, "{class}");
    }
    // The sixth: a Wolkit app that publishes its own menus. The bar shows those, not these.
    let publishes = |id: &str| id == "org.wolos.todos";
    assert!(matches!(
        appmenu::source("org.wolos.todos", publishes),
        appmenu::Source::Wolkit { .. }
    ));
    let own = appmenu::wolkit_menus(
        r#"[{"id":1,"title":"File","entries":[{"type":"item","key":"new","label":"New"}]}]"#,
    );
    assert!(matches!(menus("org.wolos.todos", own), Menus::Own(_)));
    // The same app answering nothing (or something invalid) gets the standard ones.
    assert!(matches!(
        menus("org.wolos.todos", Vec::new()),
        Menus::Standard(m) if m.len() == 3
    ));
}

#[test]
fn a_row_is_live_exactly_where_the_hand_off_has_a_key() {
    // Nothing a row offers may be a key the hand-off drops: the unit table above is a subset of it.
    for (class, _) in TABLE {
        for c in Cmd::ALL {
            if enabled(c, class) {
                assert_ne!(
                    appmenu::send(c, class),
                    Handoff::Nothing,
                    "{c:?} in {class}"
                );
            }
        }
    }
    // A terminal never gets a Ctrl+letter that is a control character.
    for c in Cmd::ALL {
        if let Handoff::Keys(m, k) = appmenu::send(c, "foot") {
            assert!(
                m.contains("SHIFT") || k.len() > 1 || !k.as_bytes()[0].is_ascii_alphabetic(),
                "{c:?}: {m} {k}"
            );
        }
    }
    // No window in front: nothing is live, and there are no menus.
    assert!(Cmd::ALL.into_iter().all(|c| !enabled(c, "")));
    assert!(standard_menus("").is_empty());
    assert_eq!(menus("", Vec::new()), Menus::Standard(Vec::new()));
    assert!(!menus("", Vec::new()).help_shown());
}

fn item(m: &appmenu::StdMenu, label: &str) -> (Act, bool, String) {
    m.entries
        .iter()
        .find_map(|e| match e {
            StdEntry::Item {
                label: l,
                act,
                enabled,
                shortcut,
            } if *l == label => Some((*act, *enabled, shortcut.clone())),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no row {label}"))
}

#[test]
fn file_edit_and_view_rows_run_the_commands_with_their_shortcuts() {
    let m = standard_menus("org.gnome.TextEditor");
    assert_eq!(
        m.iter().map(|m| m.title).collect::<Vec<_>>(),
        ["File", "Edit", "View"]
    );
    let (edit, view) = (&m[1], &m[2]);
    assert_eq!(
        item(edit, "Copy"),
        (Act::Key(Cmd::Copy), true, "Super+C".to_string())
    );
    assert_eq!(item(edit, "Redo").2, "Super+Shift+Z");
    assert_eq!(item(edit, "Emoji & Symbols").2, "Super+Ctrl+Space");
    assert_eq!(item(view, "Zoom In").2, "Super+=");
    assert_eq!(item(view, "Zoom Out").2, "Super+-");
    // The row is the same command a keybind runs.
    assert_eq!(argv(Act::Key(Cmd::SelectAll)), ["wol-app", "select-all"]);
    // Greyed where the app has none.
    assert!(!item(view, "Reload").1);
    assert!(!item(view, "Open Location…").1);
    let t = standard_menus("foot");
    assert!(!item(&t[1], "Undo").1);
    assert!(item(&t[1], "Paste").1);
    assert!(!item(&t[0], "New Tab").1, "foot has no tabs");
    // Every row of every menu names a real command; separators are not items.
    for m in &t {
        for e in &m.entries {
            if let StdEntry::Item { act, .. } = e {
                assert!(matches!(act, Act::Key(_)));
            }
        }
    }
}

#[test]
fn the_menus_pass_the_same_checks_as_an_apps_own() {
    // The labels and shortcut texts of a standard menu are one short line each.
    for class in ["org.gnome.TextEditor", "foot", "helium"] {
        for m in standard_menus(class).iter().chain([&help_menu()]) {
            assert!(shared::view::label_ok(m.title));
            for e in &m.entries {
                if let StdEntry::Item {
                    label, shortcut, ..
                } = e
                {
                    assert!(shared::view::label_ok(label), "{label}");
                    assert!(shortcut.len() <= shared::view::MAX_SHORTCUT);
                }
            }
        }
    }
}

#[test]
fn help_searches_the_manual_and_shows_the_shortcut_sheet() {
    let h = help_menu();
    assert_eq!(h.title, "Help");
    assert_eq!(
        h.items()
            .iter()
            .map(|i| (i.0, i.1, i.2))
            .collect::<Vec<_>>(),
        [
            ("Search the manual…", Act::Manual, true),
            ("Keyboard Shortcuts", Act::Shortcuts, true)
        ]
    );
    assert_eq!(argv(Act::Manual), ["wol-search"]);
    assert_eq!(
        argv(Act::Shortcuts),
        ["wol", "shell", "toggle", "shortcuts"]
    );
    // Neither is a `wol-app` verb: they are not on the app in front.
    assert_eq!(appmenu::verb(Act::Manual), None);
    // Help is there for an app with its own menus too, unless it has a Help of its own.
    let own = |t: &str| {
        appmenu::wolkit_menus(&format!(
            r#"[{{"id":1,"title":"{t}","entries":[{{"type":"item","key":"a","label":"A"}}]}}]"#
        ))
    };
    assert!(menus("org.wolos.todos", own("File")).help_shown());
    assert!(!menus("org.wolos.todos", own("Help")).help_shown());
    assert!(menus("foot", Vec::new()).help_shown());
}

#[test]
fn settings_shows_for_every_class_with_a_known_key() {
    for class in [
        "org.gnome.TextEditor",
        "org.gnome.Nautilus",
        "org.gnome.Ptyxis",
    ] {
        assert_eq!(
            settings_key(class),
            Some(Handoff::Keys("CTRL", "comma")),
            "{class}"
        );
        let rows = appmenu::app_menu("App", class);
        assert!(rows.iter().any(|r| r.0 == "Settings…"), "{class}");
    }
    for class in ["foot", "helium", "org.gnome.Papers", "org.wolos.settings"] {
        assert_eq!(settings_key(class), None, "{class}");
        assert!(
            !appmenu::app_menu("App", class)
                .iter()
                .any(|r| r.0 == "Settings…"),
            "{class}"
        );
    }
    // The class may come in any case (the switcher lowercases it).
    assert!(settings_key("org.gnome.texteditor").is_some());
}
