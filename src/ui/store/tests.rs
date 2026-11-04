// SPDX-License-Identifier: GPL-2.0-or-later
//! Unit tests of the App Store: the details rows, button labels and search merging.
use super::*;

#[test]
fn details_information_rows() {
    let s = Subject::flathub(&wol_core::flatpak::Hit {
        id: "org.gimp.GIMP".into(),
        name: "GIMP".into(),
        summary: "Edit images".into(),
    });
    let l = Loaded {
        details: Some(wol_core::appstream::Details {
            developer: Some("The GIMP team".into()),
            ..Default::default()
        }),
        size: Some("312.4 MB".into()),
        system: true,
        ..Default::default()
    };
    assert_eq!(
        info_rows(&s, &l),
        [
            ("Developer", "The GIMP team".to_string()),
            ("Size", "312.4 MB".into()),
            ("Source", "Flathub".into()),
            ("Installed for", "All accounts".into()),
        ]
    );
    let native = Subject::catalog(store::get("gimp").unwrap());
    assert_eq!(native.pkg.as_deref(), Some("gimp"));
    assert_eq!(
        info_rows(&native, &Loaded::default()),
        [("Source", "System packages".to_string())]
    );
}

#[test]
fn get_button_shows_the_percentage() {
    assert_eq!(button_label(w::TileState::Get), "Get");
    assert_eq!(button_label(w::TileState::Progress(42)), "42%");
    assert_eq!(button_label(w::TileState::Open), "Open");
    assert!(w::TileState::Progress(0).busy() && w::TileState::Installing.busy());
    assert!(!w::TileState::Open.busy());
}

#[test]
fn kits_already_here_start_as_added() {
    use std::collections::HashSet;
    let codecs = store::kit("codecs-kit").unwrap();
    let craft = store::kit("craft-suite").unwrap();
    let all: HashSet<String> = codecs.packages().iter().map(|n| n.to_string()).collect();
    let mut some = all.clone();
    some.remove(codecs.packages()[0]);
    let none = HashSet::new();
    assert!(kit_installed(codecs, &all, &none));
    assert!(!kit_installed(codecs, &some, &none));
    let craft_recipe: HashSet<String> = ["craft-launcher".to_string()].into();
    assert!(kit_installed(craft, &none, &craft_recipe));
    assert!(!kit_installed(craft, &all, &none));
}

#[test]
fn kits_install_in_one_step_and_have_nothing_to_open() {
    assert_eq!(kit_label(w::TileState::Get), "Get");
    assert_eq!(kit_label(w::TileState::Progress(7)), "7%");
    assert_eq!(kit_label(w::TileState::Open), "Added");
    let codecs = store::kit("codecs-kit").unwrap();
    // One wol-admind request for the whole kit: the daemon resolves the kit id itself.
    assert_eq!(
        install_action(codecs.id, false, false),
        Action::PkgInstall("codecs-kit".into())
    );
    let icons: Vec<&str> = codecs.parts.iter().map(|p| part_icon(codecs, p)).collect();
    assert_eq!(
        icons,
        [
            "audio-x-generic-symbolic",
            "audio-x-generic-symbolic",
            "video-display-symbolic",
            "video-display-symbolic",
            "audio-x-generic-symbolic",
            "preferences-desktop-locale-symbolic",
        ]
    );
    let craft = store::kit("craft-suite").unwrap();
    assert_eq!(
        part_icon(craft, &craft.parts[0]),
        "applications-graphics-symbolic"
    );
    assert!(PAGES.contains(&"Kits") && PAGES.contains(&"Plugins"));
    // Every kit draws its own pixel tile, never the generic package icon.
    for k in store::KITS {
        assert!(k.icon.starts_with("wolos-kit-"), "{}", k.id);
        assert!(
            crate::kit::icon_file(k.icon).is_some(),
            "{} has no {}.svg",
            k.id,
            k.icon
        );
    }
}

#[test]
fn store_actions_by_scope() {
    assert_eq!(
        install_action("org.a.B", true, false),
        Action::FlatpakInstall("org.a.B".into())
    );
    assert!(matches!(
        install_action("org.a.B", true, true),
        Action::Admin(r) if r.action == shared::admin::AdminAction::FlatpakInstall
    ));
    assert!(matches!(
        remove_action("org.a.B", true, true),
        Action::Admin(r) if r.action == shared::admin::AdminAction::FlatpakRemove
    ));
    assert_eq!(
        install_action("gimp", false, true),
        Action::PkgInstall("gimp".into())
    );
    assert_eq!(
        progress_path("gimp", false).as_deref(),
        Some("/run/wolos/progress/gimp")
    );
}

#[test]
fn a_catalog_app_is_not_listed_again_under_more_packages() {
    let hit = |n: &str| store::PkgHit {
        name: n.into(),
        summary: String::new(),
    };
    let pkgs = [hit("gimp"), hit("gimp-help-en")];
    let left = beyond_catalog("gimp", &pkgs);
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].name, "gimp-help-en");
}

#[test]
fn native_packages_win_over_flathub() {
    let hit = |id: &str, name: &str| wol_core::flatpak::Hit {
        id: id.into(),
        name: name.into(),
        summary: String::new(),
    };
    let repo = [store::PkgHit {
        name: "drawing".into(),
        summary: String::new(),
    }];
    assert!(covered_natively(
        &hit("com.github.maoschanz.drawing", "Drawing"),
        &repo
    ));
    assert!(!covered_natively(
        &hit("com.github.maoschanz.drawing", "Drawing"),
        &[]
    ));
    let catalog = &store::CATALOG[0];
    assert!(covered_natively(
        &hit("org.example.Whatever", catalog.name),
        &[]
    ));
    assert!(!covered_natively(
        &hit("com.example.NoSuchApp", "No Such App Here"),
        &[]
    ));
}

/// QA-STORE-30: a repo package the catalog does not know but the package database lists is
/// installed; its details open in "Open"/"Installed", not "Get".
#[test]
fn start_state_from_loaded() {
    use w::TileState::{Get, Open};
    assert_eq!(start_state(false, None), Get);
    assert_eq!(
        start_state(false, Some("")),
        Get,
        "an empty version is not installed"
    );
    assert_eq!(
        start_state(false, Some("2.1-1")),
        Open,
        "pacman -Q knows it"
    );
    assert_eq!(start_state(true, None), Open, "the desktop file is there");
    // Nothing to start: say what is true.
    assert_eq!(details::open_label(Open, true), "Open");
    assert_eq!(details::open_label(Open, false), "Installed");
    assert_eq!(details::open_label(Get, false), "Get");
    assert!(can_open(true, None), "a Flathub app runs by its id");
    assert!(can_open(false, Some("vlc")));
    assert!(!can_open(false, None), "a library has no Open");
}

/// Live-1 H1: Telegram's catalog id ends in ".desktop" and gtk4-launch took it for a file name.
#[test]
fn launch_arg_always_names_the_desktop_file() {
    assert_eq!(launch_arg("vlc"), "vlc.desktop");
    assert_eq!(
        launch_arg("org.telegram.desktop"),
        "org.telegram.desktop.desktop"
    );
    for a in store::CATALOG.iter() {
        let d = a.desktop;
        assert_eq!(launch_arg(d), format!("{d}.desktop"));
        assert!(!d.contains(' '), "{d}");
    }
}
