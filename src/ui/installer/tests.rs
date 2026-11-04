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

/// QA-SHADOW-1: the three ways the recovery key meets a USB stick. Only the last one lets the
/// person go on; the two others say so in the foot.
#[test]
fn the_key_to_a_usb_stick_has_three_outcomes() {
    let root = std::env::temp_dir().join(format!("wolos-keyusb-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let media = root.join("media");
    std::fs::create_dir_all(&media).unwrap();
    let roots = [media.clone()];
    let none = key_to_usb(&roots, "AAAA-BBBB");
    assert_eq!(
        none,
        KeySaved {
            text: copy::INSTALL_NO_USB.into(),
            problem: true,
            saved: false
        }
    );
    // A "stick" whose recovery file cannot be created: a directory sits where it goes.
    let stick = media.join("STICK");
    std::fs::create_dir_all(stick.join(install::RECOVERY_FILE)).unwrap();
    let unwritable = key_to_usb(&roots, "AAAA-BBBB");
    assert_eq!(
        unwritable,
        KeySaved {
            text: copy::INSTALL_KEY_NOT_SAVED.into(),
            problem: true,
            saved: false
        }
    );
    std::fs::remove_dir_all(stick.join(install::RECOVERY_FILE)).unwrap();
    let ok = key_to_usb(&roots, "AAAA-BBBB");
    assert_eq!(
        ok,
        KeySaved {
            text: copy::install_key_saved("STICK"),
            problem: false,
            saved: true
        }
    );
    assert!(
        std::fs::read_to_string(stick.join(install::RECOVERY_FILE))
            .unwrap()
            .contains("AAAA-BBBB")
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// QA-SHADOW-3, QA-SHADOW-4: a probe that cannot run or gives no JSON leaves that disk out
/// (and the others in); an empty allowed list is an empty list.
#[test]
fn a_disk_whose_probe_fails_is_left_out() {
    let disk_json = |name: &str| {
        serde_json::to_string(&wol_core::install::disk::Disk {
            path: format!("/dev/{name}"),
            bytes: 64_000_000_000,
            ..Default::default()
        })
        .unwrap()
    };
    let probe = |name: &str| {
        if name == "vdb" {
            "not json".into()
        } else if name == "vdc" {
            String::new()
        } else {
            disk_json(name)
        }
    };
    let found = disks_from("vda\nvdb\nvdc\n", probe, |_| String::new());
    assert_eq!(found.len(), 1, "only vda has a probe");
    assert_eq!(found[0].disk.path, "/dev/vda");
    assert!(disks_from("", |_| unreachable!(), |_| unreachable!()).is_empty());
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
