// SPDX-License-Identifier: GPL-2.0-or-later
//! Contract of the shipped Plugin Maker entry (ADR 0031, docs/modules/plugin-maker.md): the
//! desktop file starts the binary the shell launches from Settings and the Store, under the
//! app's id, with the icon the set draws.
use std::path::Path;

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn entry() -> String {
    let p = root().join(format!(
        "desktop/overlay/usr/share/applications/{}.desktop",
        shell::ui::maker::APP_ID
    ));
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn key(text: &str, k: &str) -> String {
    text.lines()
        .find_map(|l| l.strip_prefix(&format!("{k}=")))
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn the_entry_starts_the_maker_under_its_app_id() {
    let d = entry();
    assert_eq!(d.lines().next(), Some("[Desktop Entry]"));
    assert_eq!(key(&d, "Type"), "Application");
    assert_eq!(key(&d, "Name"), shell::ui::maker::APP_NAME);
    let exec = key(&d, "Exec");
    let bin = exec.split_whitespace().next().unwrap_or_default();
    assert_eq!(bin, shell::ui::maker::ARGV[0]);
    assert_eq!(bin, shell::ui::maker::THEME_ARGV[0]);
    assert!(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("src/bin/{bin}.rs"))
            .is_file(),
        "the binary is built from crates/shell/src/bin"
    );
    let icon = key(&d, "Icon");
    assert_eq!(icon, "wolos-plugin-maker");
    assert!(
        root()
            .join(format!("design/icons/apps/{icon}.svg"))
            .is_file()
    );
    assert!(key(&d, "Categories").ends_with(';'));
}
