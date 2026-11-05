// SPDX-License-Identifier: GPL-2.0-or-later
//! Live (lead, v0.12): Control Center > Wi-Fi > "Wi-Fi Settings…" opened Settings on Network, or
//! on whatever page the already-running Settings had been launched with. Two rules: every deep
//! link the shell spawns names a page Settings has, and a launch while Settings runs is routed
//! by the running instance from that launch's own arguments.
use shell::ui::settings::{PAGES, SUBPAGES, canonical, requested_page};

const SETTINGS: &str = include_str!("../src/ui/settings/mod.rs");

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

#[test]
fn the_wifi_tile_opens_the_wifi_pane() {
    assert_eq!(
        shell::ui::control_center::WIFI_SETTINGS,
        ["wol-settings", "wifi"]
    );
    assert_eq!(
        shell::ui::control_center::SOUND_SETTINGS,
        ["wol-settings", "sound"]
    );
    for argv in [
        shell::ui::control_center::WIFI_SETTINGS,
        shell::ui::control_center::SOUND_SETTINGS,
    ] {
        let argv = args(argv);
        let page = requested_page(&argv).unwrap();
        assert!(PAGES.contains(&page), "{page}");
        assert_eq!(canonical(page), page, "spelled under today's name");
    }
}

#[test]
fn every_launch_argument_routes_to_a_real_page() {
    for p in PAGES.iter().chain(SUBPAGES.iter()) {
        assert_eq!(requested_page(&args(&["wol-settings", p])), Some(*p));
    }
    assert_eq!(
        requested_page(&args(&["wol-settings", "Menu Bar & Dock"])),
        Some("Desktop & Dock")
    );
    assert_eq!(requested_page(&args(&["wol-settings"])), None);
}

/// QA-SEARCH-1: a launch names its page by id, and a row after `#`; the id reaches the same page
/// as the title does, and an unknown id is no page (Settings opens as it was).
#[test]
fn a_launch_by_page_id_reaches_its_page_and_row() {
    for (id, title) in shell::ui::settings::ids::PAGE_IDS {
        assert_eq!(requested_page(&args(&["wol-settings", id])), Some(title));
    }
    let t = shell::ui::settings::requested(&args(&["wol-settings", "privacy#screen-recording"]))
        .unwrap();
    assert_eq!(
        (t.page, t.row),
        ("Privacy & Security", Some("Screen Recording"))
    );
    assert_eq!(
        requested_page(&args(&["wol-settings", "no-such-page"])),
        None
    );
}

/// The running instance must see each launch's arguments: GApplication only forwards them with
/// HANDLES_COMMAND_LINE, and reading the process's own argv names the first launch's page.
#[test]
fn a_second_launch_is_routed_by_the_running_instance() {
    let run = &SETTINGS[SETTINGS.find("pub fn run()").unwrap()..];
    let run = &run[..run.find("\n}\n").unwrap()];
    assert!(run.contains("HANDLES_COMMAND_LINE"), "flag");
    assert!(run.contains("connect_command_line"), "handler");
    assert!(
        !SETTINGS.contains("std::env::args()"),
        "the primary instance's argv is the first launch's"
    );
}
