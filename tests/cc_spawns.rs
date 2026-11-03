// SPDX-License-Identifier: GPL-2.0-or-later
//! Live (real GPU): the Control Center Screenshot/Record tiles spawned `wol capture <verb>` and the
//! CLI answered "unknown command 'capture'". Every argv the Control Center spawns must resolve to a
//! real command: a shell panel both the CLI and wol-shell accept, or a binary WolOS installs.
use std::path::Path;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const SHELL_UI: &str = include_str!("../src/ui/mod.rs");

fn installed(prog: &str) -> bool {
    Path::new(ROOT)
        .join("desktop/overlay/usr/bin")
        .join(prog)
        .is_file()
        || Path::new(ROOT)
            .join("crates/shell/src/bin")
            .join(format!("{prog}.rs"))
            .is_file()
}

/// wol-shell answers `target`: a registered panel or a surface with its own handler.
fn shell_answers(target: &str) -> bool {
    SHELL_UI.contains(&format!("p.insert(\"{target}\"")) || target == shell::ui::capture::RECORD
}

#[test]
fn every_control_center_spawn_is_a_real_command() {
    let spawns = shell::ui::control_center::SPAWNS;
    assert!(spawns.len() >= 2, "{spawns:?}");
    for argv in spawns {
        match argv {
            ["wol", "shell", rest @ ..] => {
                let cmd = shell::protocol::parse(&rest.join(" "))
                    .unwrap_or_else(|| panic!("{argv:?}: wol-shell cannot parse it"));
                assert!(
                    shared::shell::PANELS.contains(&cmd.target.as_str()),
                    "{argv:?}: the wol CLI rejects '{}'",
                    cmd.target
                );
                assert!(
                    shell_answers(&cmd.target),
                    "{argv:?}: wol-shell has no '{}'",
                    cmd.target
                );
            }
            // A desktop toggle tile (Stay Awake): `wol toggle <word>` with a word the CLI takes.
            ["wol", "toggle", word] => assert!(
                wol_core::toggles::words().contains(word),
                "{argv:?}: `wol toggle` has no '{word}'"
            ),
            ["wol", sub, ..] => {
                panic!("{argv:?}: only `wol shell` and `wol toggle` are spawned, not `wol {sub}`")
            }
            ["wol-settings", page] => {
                assert!(
                    shell::ui::settings::PAGES.contains(page),
                    "{argv:?}: Settings has no page '{page}'"
                );
                assert_eq!(
                    shell::ui::settings::canonical(page),
                    *page,
                    "{argv:?}: an old page name"
                );
            }
            [prog, ..] => assert!(installed(prog), "{argv:?}: {prog} is not installed"),
            [] => panic!("empty argv"),
        }
    }
}

/// Live (lead, v0.12): "Wi-Fi Settings…" on the Wi-Fi page opened Network (the wired pane).
/// Each "… Settings…" link opens the pane it is named after.
#[test]
fn settings_links_open_the_pane_they_are_named_after() {
    assert_eq!(
        shell::ui::control_center::WIFI_SETTINGS,
        ["wol-settings", "Wi-Fi"]
    );
    assert_eq!(
        shell::ui::control_center::SOUND_SETTINGS,
        ["wol-settings", "Sound"]
    );
}

/// The Control Center is a resident wol-shell panel (`wol shell toggle control-center`); the
/// separate `wol-control-center` process (a second, leaking copy) is gone, and nothing spawns it.
#[test]
fn the_control_center_is_the_shells_panel_not_a_process() {
    assert!(
        !Path::new(ROOT)
            .join("crates/shell/src/bin/wol-control-center.rs")
            .exists(),
        "wol-control-center binary is back"
    );
    assert!(SHELL_UI.contains("p.insert(\"control-center\""));
    for argv in shell::ui::control_center::SPAWNS {
        assert_ne!(argv.first(), Some(&"wol-control-center"), "{argv:?}");
    }
}
