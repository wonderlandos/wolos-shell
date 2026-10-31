// SPDX-License-Identifier: GPL-2.0-or-later
//! For the VM tests (`image/vm/test-installer.sh`): walks the installer's steps as a person would,
//! on a real disk's probe, and writes the plan the app would.
//! `install_scenario <scenario> <probe.json> <plan dir>`; prints the summary.
use shell::install::flow::{Flow, Found, Method, Next, Row, Step, Use, manual_lines};
use shell::install::{Check, write_plan};
use shell::setup::Hints;
use std::path::Path;
use wol_core::install::disk::Disk;
use wol_core::install::layout::Fs;
use wol_core::install::plan::Protect;

fn part_labelled(f: &Flow, label: &str) -> String {
    let d = f.disk().unwrap();
    let p = d.parts.iter().find(|p| p.label == label || p.name == label);
    p.unwrap_or_else(|| panic!("no partition {label}")).uuid.clone()
}

fn esp(f: &Flow) -> String {
    let d = f.disk().unwrap();
    d.parts.iter().find(|p| p.is_esp()).unwrap().uuid.clone()
}

fn must(f: &mut Flow, want: Step) {
    let n = f.advance();
    assert_eq!(n, Next::Go(want), "from {:?}", f.step);
}

fn use_part(f: &mut Flow, uuid: &str, how: Use) {
    assert_eq!(f.open_edit(&Row::Part(uuid.into())), Next::Go(Step::Edit));
    f.edit.as_mut().unwrap().usage = how;
    must(f, Step::Manual);
}

/// The largest free space becomes the system.
fn system_in_free_space(f: &mut Flow, fs: Fs) {
    let lines = manual_lines(&f.manual, f.disk().unwrap());
    let free = lines
        .iter()
        .filter_map(|l| match l.0 {
            Row::Free(b) => Some(b),
            _ => None,
        })
        .max()
        .expect("free space");
    assert_eq!(f.open_edit(&Row::Free(free)), Next::Go(Step::Edit));
    let e = f.edit.as_mut().unwrap();
    assert_eq!(e.usage, Use::System);
    e.fs = fs;
    must(f, Step::Manual);
}

fn manual(f: &mut Flow, fs: Fs) {
    f.method = Method::Manual;
    must(f, Step::Manual);
    let old = part_labelled(f, "linuxroot");
    f.open_edit(&Row::Part(old));
    f.delete_edited();
    system_in_free_space(f, fs);
    let e = esp(f);
    use_part(f, &e, Use::Startup);
    let home = part_labelled(f, "linuxhome");
    use_part(f, &home, Use::Home);
    must(f, Step::Protect);
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (scenario, probe, dir) = (a[1].as_str(), &a[2], Path::new(&a[3]));
    let disk: Disk = serde_json::from_str(&std::fs::read_to_string(probe).unwrap()).unwrap();
    let tpm = scenario == "tpm";
    let mut f = Flow::new(&Hints::default(), true, tpm);
    f.set_disks(vec![Found {
        disk,
        check: Check::default(),
    }]);
    f.set_zone("Europe/Lisbon");
    for s in [Step::Language, Step::Zone, Step::Disk, Step::Method] {
        must(&mut f, s);
    }
    match scenario {
        "erase" | "password" | "tpm" => {
            f.method = Method::Erase;
            must(&mut f, Step::Protect);
        }
        "alongside" => {
            f.method = Method::Alongside;
            must(&mut f, Step::Alongside);
            must(&mut f, Step::Protect);
        }
        "manual" => manual(&mut f, Fs::Btrfs),
        "manual-ext4" => manual(&mut f, Fs::Ext4),
        s => panic!("no scenario {s}"),
    }
    f.protect = match scenario {
        "password" => Protect::Password,
        "tpm" => Protect::Tpm,
        _ => Protect::Off,
    };
    f.disk_password = "white rabbit".into();
    f.disk_verify = "white rabbit".into();
    must(&mut f, Step::Account);
    f.draft.set_full_name("Alice Liddell");
    f.draft.password = "curiouser".into();
    f.draft.verify = "curiouser".into();
    must(&mut f, Step::Summary);
    for l in f.summary() {
        println!("summary: {l}");
    }
    assert_eq!(f.advance(), Next::Install);
    let pw = if f.protect == Protect::Password {
        f.disk_password.clone()
    } else {
        String::new()
    };
    write_plan(dir, &f.plan().unwrap(), &pw, &f.draft.password).unwrap();
}
