// SPDX-License-Identifier: GPL-2.0-or-later
//! Every page of Install WolOS, on a disk with Windows and Linux.
use super::Case;
use shell::install::flow::{Found, Method, Row, Step, Use};
use shell::install::{Check, Report, Stage};
use shell::setup::{Field, Hints};
use shell::ui::installer::Installer;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use wol_core::install::disk::{DATA_TYPE, Disk, ESP_TYPE, MIB, Part, Table};
use wol_core::install::layout::{Fs, New};
use wol_core::install::plan::Protect;

const GB: u64 = 1_000_000_000;

fn part(n: u32, start_gb: u64, gb: u64, kind: &str, fs: &str) -> Part {
    Part {
        number: n,
        path: format!("/dev/nvme0n1p{n}"),
        start: start_gb * GB / 512 / 2048 * 2048 + 2048,
        sectors: gb * GB / 512 / 2048 * 2048,
        kind: kind.into(),
        uuid: format!("u{n}"),
        fstype: fs.into(),
        fsuuid: format!("f{n}"),
        ..Part::default()
    }
}

fn dual() -> Disk {
    let mut esp = part(1, 0, 1, ESP_TYPE, "vfat");
    esp.free_bytes = Some(800 * MIB);
    let mut win = part(2, 2, 600, "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7", "ntfs");
    win.min_bytes = Some(180 * GB);
    let mut home = part(3, 610, 200, DATA_TYPE, "ext4");
    home.min_bytes = Some(40 * GB);
    home.os = Some("Ubuntu 24.04".into());
    Disk {
        path: "/dev/nvme0n1".into(),
        model: "Samsung SSD 980 PRO".into(),
        bytes: 1000 * GB,
        sector: 512,
        table: Table::Gpt,
        id: "t".into(),
        first: 2048,
        last: 1000 * GB / 512 - 34,
        parts: vec![esp, win, home],
    }
}

/// Windows' own startup partition, too small to share: WolOS gets its own and Windows leaves
/// its menu (the Summary and Done pages say how Windows starts).
fn dual_small_esp() -> Disk {
    let mut d = dual();
    d.parts[0].free_bytes = Some(60 * MIB);
    d
}

/// How often the disk page looked at disks (B7: every visit while none was found).
static PROBES: AtomicUsize = AtomicUsize::new(0);

fn no_disks() -> Vec<Found> {
    PROBES.fetch_add(1, Ordering::SeqCst);
    Vec::new()
}

/// Runs the main loop until `done`, at most two seconds.
fn pump_until(done: impl Fn() -> bool) {
    let until = Instant::now() + Duration::from_secs(2);
    while !done() && Instant::now() < until {
        while gtk::glib::MainContext::default().iteration(false) {}
        std::thread::sleep(Duration::from_millis(5));
    }
    for _ in 0..20 {
        while gtk::glib::MainContext::default().iteration(false) {}
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// No disk found: the page looks again on every visit and offers Look Again.
fn add_empty(v: &mut Vec<Case>) {
    v.push(Case {
        name: "installer-disk-empty-1280".into(),
        size: (1280, 800),
        class: "lock-window",
        wallpaper: false,
        build: Box::new(|| {
            let i = Installer::new(&Hints::default(), true, true, zones(), Some(vec![]));
            i.set_probe(no_disks);
            let before = PROBES.load(Ordering::SeqCst);
            i.jump(Step::Disk);
            pump_until(|| PROBES.load(Ordering::SeqCst) > before);
            i.jump(Step::Disk);
            pump_until(|| PROBES.load(Ordering::SeqCst) > before + 1);
            assert_eq!(
                PROBES.load(Ordering::SeqCst),
                before + 2,
                "every visit to an empty disk page looks again"
            );
            i.root.clone()
        }),
    });
}

/// audit6 I6-5: on the live desktop an encrypted install's Done page shows the recovery key, which
/// `install-check --result` has already deleted. Restart and Restart Later both wait for "I wrote
/// the key down"; with no key on the page both are free.
fn add_done_live(v: &mut Vec<Case>) {
    v.push(Case {
        name: "installer-done-live-key-1280".into(),
        size: (1280, 800),
        class: "lock-window",
        wallpaper: false,
        build: Box::new(|| {
            let i = Installer::new(&Hints::default(), false, true, zones(), Some(vec![]));
            i.show_done(
                Some("ABCDE-FGHIJ-KLMNO-PQRST-UVWXY-Z2345-67ABC-DEFGH"),
                None,
                false,
            );
            assert_eq!(
                i.leave_buttons(),
                (false, Some(false)),
                "key unacknowledged"
            );
            i.set_acknowledged(true);
            assert_eq!(i.leave_buttons(), (true, Some(true)), "acknowledged");
            i.set_acknowledged(false);
            assert_eq!(i.leave_buttons(), (false, Some(false)), "toggled back off");
            // A second install in the same window starts unacknowledged again.
            i.set_acknowledged(true);
            i.show_done(Some("ZZZZZ-FGHIJ"), None, false);
            assert_eq!(
                i.leave_buttons(),
                (false, Some(false)),
                "a new key resets it"
            );
            i.show_done(None, None, false);
            assert_eq!(i.leave_buttons(), (true, Some(true)), "no key on the page");
            // The golden shows the key page (what the person must read first).
            i.show_done(
                Some("ABCDE-FGHIJ-KLMNO-PQRST-UVWXY-Z2345-67ABC-DEFGH"),
                None,
                false,
            );
            i.root.clone()
        }),
    });
}

fn blank() -> Disk {
    Disk {
        path: "/dev/sda".into(),
        model: "WD Blue SA510".into(),
        bytes: 500 * GB,
        sector: 512,
        last: 500 * GB / 512 - 34,
        first: 2048,
        ..Disk::default()
    }
}

fn zones() -> Vec<String> {
    [
        "America/New_York",
        "America/Sao_Paulo",
        "Asia/Tokyo",
        "Europe/Berlin",
        "Europe/Lisbon",
        "Europe/London",
        "Europe/Paris",
    ]
    .map(String::from)
    .to_vec()
}

/// The first screen on 4:3, 1366x768 and a screen smaller than the card (1280x800 at scale 2):
/// the card keeps its size and the screen scrolls, the buttons stay reachable.
fn add_fit(v: &mut Vec<Case>) {
    for (sw, sh) in [(1024, 768), (1366, 768), (1024, 600), (640, 400)] {
        v.push(Case {
            name: format!("installer-begin-{sw}x{sh}"),
            size: (sw, sh),
            class: "lock-window",
            wallpaper: false,
            build: Box::new(move || {
                let i = Installer::new(&Hints::default(), true, true, zones(), Some(vec![]));
                at(&i, "begin");
                i.root.clone()
            }),
        });
    }
}

/// The module records the Modules page reads on the image (lead's order: developer, AI, creative,
/// gaming).
fn records() -> Vec<wol_core::modules::Module> {
    [
        ("dev", 900),
        ("ai-claude", 300),
        ("ai-local", 4000),
        ("creative", 3500),
        ("gaming", 6000),
        ("console", 50),
    ]
    .into_iter()
    .map(|(id, mb)| wol_core::modules::Module {
        id: id.into(),
        name: id.into(),
        size_mb: mb,
        ..Default::default()
    })
    .collect()
}

/// The Secure Boot page's computers: `tier` with Secure Boot on, `off` without it.
fn secure(i: &Rc<Installer>, tier: &str, off: &str, on: bool) {
    i.with_flow(|f| {
        f.disks[0].check.tier = Some(tier.into());
        f.disks[0].check.tier_off = Some(off.into());
        f.secure = on;
    });
}

fn at(i: &Rc<Installer>, page: &str) {
    let manual = |i: &Rc<Installer>| {
        i.with_flow(|f| {
            f.method = Method::Manual;
            f.manual.create.push(New {
                bytes: Some(100 * GB),
                mount: "/".into(),
                fs: Fs::Btrfs,
            });
            f.manual.delete.push("u3".into());
        })
    };
    match page {
        "begin" => i.jump(Step::Begin),
        "language" => i.jump(Step::Language),
        "zone" => i.jump(Step::Zone),
        "disk" => i.jump(Step::Disk),
        "method" => i.jump(Step::Method),
        "method-erase" => {
            i.with_flow(|f| f.method = Method::Erase);
            i.jump(Step::Method)
        }
        "replace" => {
            i.with_flow(|f| {
                f.method = Method::Replace;
                f.replace = Some("u3".into());
            });
            i.jump(Step::Replace)
        }
        "alongside" => i.jump(Step::Alongside),
        "manual" => {
            manual(i);
            i.jump(Step::Manual)
        }
        "edit" => {
            i.with_flow(|f| {
                f.open_edit(&Row::Part("u3".into()));
                let e = f.edit.as_mut().unwrap();
                e.usage = Use::Home;
            });
            i.jump(Step::Edit)
        }
        "edit-new" => {
            i.with_flow(|f| {
                f.open_edit(&Row::Free(150 * GB));
            });
            i.jump(Step::Edit)
        }
        "protect" => i.jump(Step::Protect),
        "protect-password" => {
            i.with_flow(|f| f.protect = Protect::Password);
            i.jump(Step::Protect)
        }
        "protect-off" => {
            i.with_flow(|f| f.protect = Protect::Off);
            i.jump(Step::Protect)
        }
        "account" => {
            i.jump(Step::Account);
            i.type_into(Field::FullName, "Alice Liddell");
        }
        "summary-alongside" => i.jump(Step::Summary),
        "summary-windows" => i.jump(Step::Summary),
        "summary-erase" => {
            i.with_flow(|f| {
                f.method = Method::Erase;
                f.protect = Protect::Tpm;
            });
            i.jump(Step::Summary)
        }
        "summary-unencrypted" => {
            i.with_flow(|f| {
                f.method = Method::Erase;
                f.protect = Protect::Off;
            });
            i.jump(Step::Summary)
        }
        "summary-manual" => {
            manual(i);
            i.jump(Step::Summary)
        }
        "secure-boot" => i.jump(Step::SecureBoot),
        // Lead 2026-11-02: Secure Boot opt-in, default off, the key screen pictured first.
        "secure-boot-choice" => {
            secure(i, "mok", "off", false);
            i.jump(Step::SecureBootChoice)
        }
        "secure-boot-choice-on" => {
            secure(i, "mok", "off", true);
            i.jump(Step::SecureBootChoice)
        }
        "secure-boot-already-on" => {
            secure(i, "mok", "firmware", false);
            i.jump(Step::SecureBootChoice)
        }
        // BitLocker's recovery key is said only while the switch is on.
        "secure-boot-own-keys" | "secure-boot-own-keys-on" => {
            i.with_flow(|f| {
                f.method = Method::Replace;
                f.replace = Some("u3".into());
                f.disks[0].disk.parts[1].fstype = "BitLocker".into();
            });
            secure(i, "own-keys", "off", page.ends_with("-on"));
            i.jump(Step::SecureBootChoice)
        }
        "modules" => {
            i.with_flow(|f| f.set_modules(&records()));
            i.jump(Step::Modules)
        }
        "modules-ticked" => {
            i.with_flow(|f| {
                f.set_modules(&records());
                f.tick_module("dev", true);
                f.tick_module("creative", true);
            });
            i.jump(Step::Modules)
        }
        "extras" => i.jump(Step::Extras),
        "extras-on" => {
            i.with_flow(|f| f.codecs = true);
            i.jump(Step::Extras)
        }
        "summary-choices" => {
            i.with_flow(|f| {
                f.method = Method::Erase;
                f.set_modules(&records());
                f.tick_module("dev", true);
                f.tick_module("gaming", true);
                f.codecs = true;
            });
            secure(i, "mok", "off", true);
            i.jump(Step::Summary)
        }
        // R2B-9: a Windows with BitLocker stays and this install changes the Secure Boot keys.
        "summary-bitlocker" | "secure-boot-bitlocker" => {
            i.with_flow(|f| {
                f.method = Method::Replace;
                f.replace = Some("u3".into());
                f.disks[0].disk.parts[1].fstype = "BitLocker".into();
                // Own keys only when the person turned Secure Boot on (opt-in).
                f.secure = true;
                f.disks[0].check.tier = Some(
                    if page == "summary-bitlocker" {
                        "own-keys"
                    } else {
                        "firmware"
                    }
                    .into(),
                );
            });
            i.jump(if page == "summary-bitlocker" {
                Step::Summary
            } else {
                Step::SecureBoot
            })
        }
        "installing" => i.show_progress(&Report::new(Stage::Encrypt, 45), Some(200.0)),
        "done" => i.show_done(None, None, false),
        "done-windows" => i.show_done(None, None, false),
        "done-key" => i.show_done(
            Some("ABCDE-FGHIJ-KLMNO-PQRST-UVWXY-Z2345-67ABC-DEFGH"),
            Some(wol_core::boot::note(wol_core::boot::Tier::Off)),
            false,
        ),
        "done-key-screen" => i.show_done(
            None,
            Some(wol_core::boot::note(wol_core::boot::Tier::Mok)),
            true,
        ),
        // Secure Boot left off: the key screen is not repeated, whatever was queued.
        "done-secure-off" => {
            secure(i, "mok", "off", false);
            i.show_done(
                None,
                Some(wol_core::boot::note(wol_core::boot::Tier::Off)),
                true,
            )
        }
        "failed-refused" => i.show_failed_with(
            Stage::Prepare,
            "the disk is not what the plan was made for (refuse UntypablePassword)",
            "+ wol install check /run/wolos/plan/plan.json
refuse UntypablePassword
",
        ),
        "key-unread" => {
            i.show_key_unread(
                "sudo -n /usr/lib/wolos/install-check --result: exit status: 1 sudo: a password is required",
                "+ wol encrypt enroll /dev/nvme0n1p4
",
            );
            i.toggle_details();
        }
        "failed-details" => {
            i.show_failed_with(
                Stage::Encrypt,
                "cryptsetup: Cannot format device /dev/nvme0n1p4, it is in use",
                "+ cryptsetup luksFormat /dev/nvme0n1p4\nDevice /dev/nvme0n1p4 is in use. Cannot proceed with format operation.\n",
            );
            i.toggle_details();
        }
        _ => {
            i.show_failed_with(Stage::Disk, "sfdisk: cannot open /dev/nvme0n1", "");
            // The tool's words are behind Details; the foot keeps one plain sentence.
            assert_eq!(
                i.failed_texts(),
                (
                    shell::copy::installer::FAILED_SEE_DETAILS.to_string(),
                    "sfdisk: cannot open /dev/nvme0n1".to_string()
                )
            );
        }
    }
}

pub fn add(v: &mut Vec<Case>) {
    const PAGES: [&str; 44] = [
        "begin",
        "language",
        "zone",
        "disk",
        "method",
        "method-erase",
        "replace",
        "alongside",
        "manual",
        "edit",
        "edit-new",
        "protect",
        "protect-password",
        "protect-off",
        "account",
        "summary-alongside",
        "summary-windows",
        "summary-erase",
        "summary-unencrypted",
        "summary-manual",
        "summary-bitlocker",
        "summary-choices",
        "secure-boot",
        "secure-boot-bitlocker",
        "secure-boot-choice",
        "secure-boot-choice-on",
        "secure-boot-already-on",
        "secure-boot-own-keys",
        "secure-boot-own-keys-on",
        "modules",
        "modules-ticked",
        "extras",
        "extras-on",
        "installing",
        "done",
        "done-windows",
        "done-key",
        "done-key-screen",
        "done-secure-off",
        "failed",
        "failed-details",
        "failed-refused",
        "failed-erase",
        "key-unread",
    ];
    for (sw, sh, tag) in [(1280, 800, "1280"), (1024, 640, "1024")] {
        for page in PAGES {
            v.push(Case {
                name: format!("installer-{page}-{tag}"),
                size: (sw, sh),
                class: "lock-window",
                wallpaper: false,
                build: Box::new(move || {
                    let found = |disk| Found {
                        disk,
                        check: Check::default(),
                    };
                    let first = if page.ends_with("-windows") {
                        dual_small_esp()
                    } else {
                        dual()
                    };
                    let i = Installer::new(
                        &Hints::default(),
                        true,
                        true,
                        zones(),
                        Some(vec![found(first), found(blank())]),
                    );
                    i.with_flow(|f| {
                        f.pick_disk(0);
                        f.draft.set_full_name("Alice Liddell");
                    });
                    at(&i, page);
                    i.root.clone()
                }),
            });
        }
    }
    add_fit(v);
    add_empty(v);
    add_done_live(v);
}
