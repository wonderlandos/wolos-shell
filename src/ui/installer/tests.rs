// SPDX-License-Identifier: GPL-2.0-or-later
//! Unit tests of the installer window: launching a run, the pages and the flow they follow.
use super::*;

/// Audit6 S6-4: a Prepare that failed (the plan could not be written, `install-run` could not
/// start) left the folder with both passwords until power-off.
#[test]
fn a_failed_prepare_leaves_no_password_files() {
    let root = std::env::temp_dir().join(format!("wolos-launch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let dir = root.join("wolos-install");
    let plan = wol_core::install::plan::Plan::default();
    let write = || install::write_plan(&dir, &plan, &[], "disk pw", "account pw");
    let gone = |what: &str| {
        assert!(!dir.join(install::ACCOUNT_PASSWORD).exists(), "{what}");
        assert!(!dir.join(install::DISK_PASSWORD).exists(), "{what}");
        assert!(!dir.exists(), "{what}");
    };
    // The command cannot start.
    let child = launch(&dir, Some(write()), |_| {
        vec!["/nonexistent/install-run".into()]
    });
    assert!(child.is_none());
    gone("unspawnable command");
    // The plan could not be written: whatever was put down is removed.
    write().unwrap();
    let err = Some(Err(std::io::Error::other("disk full")));
    assert!(launch(&dir, err, |_| unreachable!("no plan, no command")).is_none());
    gone("plan not written");
    // A command that starts keeps the plan for it (install-run removes it itself).
    let mut child = launch(&dir, Some(write()), |_| vec!["true".into()]).expect("starts");
    assert!(child.wait().unwrap().success());
    assert!(dir.join(install::ACCOUNT_PASSWORD).exists());
    install::remove_plan(&dir);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_sliders_land_on_whole_mib_inside_their_range() {
    let (lo, hi) = (10u64 << 30, 100u64 << 30);
    assert_eq!(keep_at(lo, hi, 0.0), hi);
    assert_eq!(keep_at(lo, hi, 100.0), lo);
    assert_eq!(keep_at(lo, hi, 50.0) % (1 << 20), 0);
    assert_eq!(bytes_at(lo, hi, 100.0), hi);
    assert_eq!(bytes_at(lo, hi, 0.0), lo);
    assert_eq!(percent_of(lo, hi, hi), 100.0);
    assert_eq!(percent_of(hi, hi, hi), 100.0);
}
