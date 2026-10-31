// SPDX-License-Identifier: GPL-2.0-or-later
//! Every page of Install WolOS, on a disk with Windows and Linux.
use super::Case;
use shell::install::flow::{Found, Method, Row, Step, Use};
use shell::install::{Check, Report, Stage};
use shell::setup::{Field, Hints};
use shell::ui::installer::Installer;
use std::rc::Rc;
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
        "account" => {
            i.jump(Step::Account);
            i.type_into(Field::FullName, "Alice Liddell");
        }
        "summary-alongside" => i.jump(Step::Summary),
        "summary-erase" => {
            i.with_flow(|f| {
                f.method = Method::Erase;
                f.protect = Protect::Tpm;
            });
            i.jump(Step::Summary)
        }
        "summary-manual" => {
            manual(i);
            i.jump(Step::Summary)
        }
        "secure-boot" => i.jump(Step::SecureBoot),
        "installing" => i.show_progress(&Report::new(Stage::Files, 53), Some(200.0)),
        "done" => i.show_done(None, None),
        "done-key" => i.show_done(
            Some("ABCDE-FGHIJ-KLMNO-PQRST-UVWXY-Z2345-67ABC-DEFGH"),
            None,
        ),
        _ => i.show_failed(Stage::Disk),
    }
}

pub fn add(v: &mut Vec<Case>) {
    const PAGES: [&str; 22] = [
        "begin",
        "language",
        "zone",
        "disk",
        "method",
        "method-erase",
        "alongside",
        "manual",
        "edit",
        "edit-new",
        "protect",
        "protect-password",
        "account",
        "summary-alongside",
        "summary-erase",
        "summary-manual",
        "secure-boot",
        "installing",
        "done",
        "done-key",
        "failed",
        "failed-erase",
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
                    let i = Installer::new(
                        &Hints::default(),
                        true,
                        true,
                        zones(),
                        Some(vec![found(dual()), found(blank())]),
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
}
