// SPDX-License-Identifier: GPL-2.0-or-later
//! Contract of the shipped System Monitor entry (docs/modules/monitor.md): the desktop file
//! starts the binary built from crates/shell/src/bin under the app's id, with the icon the set
//! draws, and the icon set maps the usual monitor names onto that icon.
use std::path::Path;

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn key(text: &str, k: &str) -> String {
    text.lines()
        .find_map(|l| l.strip_prefix(&format!("{k}=")))
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn the_entry_starts_the_monitor_under_its_app_id() {
    use shell::ui::monitor::{APP_ID, BIN};
    let p = root().join(format!(
        "desktop/overlay/usr/share/applications/{APP_ID}.desktop"
    ));
    let d = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    assert_eq!(d.lines().next(), Some("[Desktop Entry]"));
    assert_eq!(key(&d, "Type"), "Application");
    assert_eq!(key(&d, "Name"), shell::copy::monitor::TITLE);
    assert_eq!(key(&d, "Exec"), BIN);
    assert_eq!(key(&d, "StartupWMClass"), APP_ID);
    assert!(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("src/bin/{BIN}.rs"))
            .is_file()
    );
    let icon = key(&d, "Icon");
    assert_eq!(icon, "wolos-system-monitor");
    assert!(
        root()
            .join(format!("design/icons/apps/{icon}.svg"))
            .is_file()
    );
    assert!(key(&d, "Categories").ends_with(';'));
}

/// Docks, launchers and other apps ask for the app id or a common monitor name: all of them
/// resolve to the WolOS icon (design/icons/aliases.tsv, linked by install.sh).
#[test]
fn the_icon_set_names_the_monitor_every_usual_way() {
    let t = std::fs::read_to_string(root().join("design/icons/aliases.tsv")).unwrap();
    let row = t
        .lines()
        .find(|l| l.starts_with("system-monitor\t"))
        .expect("a system-monitor row");
    let names: Vec<&str> = row.split(['\t', ' ']).collect();
    for n in [
        shell::ui::monitor::APP_ID,
        "utilities-system-monitor",
        "org.gnome.SystemMonitor",
    ] {
        assert!(names.contains(&n), "{n} in {row}");
    }
}
