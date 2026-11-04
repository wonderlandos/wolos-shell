// SPDX-License-Identifier: GPL-2.0-or-later
//! Unit tests of the install flow: answers, plans, encryption defaults and the steps of an install.
use super::*;
use wol_core::install::disk::{DATA_TYPE, ESP_TYPE, Part};
use wol_core::install::layout::Slots;

const GB: u64 = 1_000_000_000;

fn part(n: u32, start_gb: u64, gb: u64, kind: &str, fs: &str) -> Part {
    Part {
        number: n,
        path: format!("/dev/vda{n}"),
        start: start_gb * GB / 512 / 2048 * 2048 + 2048,
        sectors: gb * GB / 512 / 2048 * 2048,
        kind: kind.into(),
        uuid: format!("u{n}"),
        fstype: fs.into(),
        fsuuid: format!("f{n}"),
        ..Part::default()
    }
}

/// 500 GB: a 100 MB-ish ESP, Windows 300 GB with 100 GB used, a Linux /home of 100 GB.
fn dual() -> Disk {
    let mut esp = part(1, 0, 1, ESP_TYPE, "vfat");
    esp.free_bytes = Some(800 * MIB);
    let mut win = part(2, 2, 300, "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7", "ntfs");
    win.min_bytes = Some(100 * GB);
    let mut home = part(3, 310, 100, DATA_TYPE, "ext4");
    home.min_bytes = Some(20 * GB);
    Disk {
        path: "/dev/vda".into(),
        model: "WD Blue".into(),
        bytes: 500 * GB,
        sector: 512,
        table: Table::Gpt,
        id: "t".into(),
        first: 2048,
        last: 500 * GB / 512 - 34,
        parts: vec![esp, win, home],
    }
}

fn flow(d: Disk) -> Flow {
    let mut f = Flow::new(&Hints::default(), true, false);
    f.set_disks(vec![Found {
        disk: d,
        check: super::super::Check::default(),
    }]);
    f
}

fn to_method(f: &mut Flow) {
    for s in [Step::Language, Step::Zone, Step::Disk, Step::Method] {
        assert_eq!(f.advance(), Next::Go(s));
    }
}

fn fill_account(f: &mut Flow) {
    f.draft.set_full_name("Alice Liddell");
    f.draft.password = "curiouser".into();
    f.draft.verify = "curiouser".into();
}

#[test]
fn the_steps_of_an_erase_are_one_decision_each_and_back_retraces_them() {
    let mut f = flow(dual());
    assert_eq!(f.step, Step::Begin);
    to_method(&mut f);
    f.method = Method::Erase;
    assert_eq!(f.advance(), Next::Go(Step::Protect));
    // Encrypted by default: no chip in the test, so the disk password is asked for first.
    assert!(matches!(f.advance(), Next::Stay(Some(Field::Password), _)));
    f.disk_password = "white rabbit".into();
    f.disk_verify = "white rabbit".into();
    assert_eq!(f.advance(), Next::Go(Step::Account));
    assert!(matches!(f.advance(), Next::Stay(Some(Field::FullName), _)));
    fill_account(&mut f);
    // No module records in the test: the Modules page is skipped, the Extras page is not.
    assert_eq!(f.advance(), Next::Go(Step::Extras));
    assert_eq!(f.advance(), Next::Go(Step::Summary));
    assert_eq!(f.back(), Some(Step::Extras));
    assert_eq!(f.back(), Some(Step::Account));
    assert_eq!(f.back(), Some(Step::Protect));
    assert_eq!(f.back(), Some(Step::Method), "Alongside was never shown");
    for s in [Step::Protect, Step::Account, Step::Extras, Step::Summary] {
        assert_eq!(f.advance(), Next::Go(s));
    }
    assert_eq!(f.advance(), Next::Install);
    assert!(f.plan().unwrap().layout.wipe);
}

#[test]
fn from_the_live_desktop_there_is_no_begin_page() {
    let f = Flow::new(&Hints::default(), false, false);
    assert_eq!(f.step, Step::Language);
}

#[test]
fn the_zone_is_guessed_from_the_machine_then_the_language() {
    let h = |z: &str| Hints {
        zone: Some(z.into()),
        ..Hints::default()
    };
    assert_eq!(guess_zone(&h("Europe/Paris"), 0), "Europe/Paris");
    assert_eq!(
        guess_zone(&h("UTC"), 2),
        "America/Sao_Paulo",
        "live images say UTC"
    );
    assert_eq!(guess_zone(&Hints::default(), 6), "Europe/Berlin");
    let mut f = Flow::new(&Hints::default(), false, false);
    f.set_language(5);
    assert_eq!(f.zone, "Europe/Paris", "follows the language");
    f.set_zone("Asia/Tokyo");
    f.set_language(2);
    assert_eq!(f.zone, "Asia/Tokyo", "until the person chooses");
}

#[test]
fn zone_search_finds_cities_as_people_type_them() {
    let all: Vec<String> = [
        "America/New_York",
        "America/Sao_Paulo",
        "Europe/London",
        "Asia/Sao_Tome",
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(zones_matching(&all, "sao paulo"), [&all[1]]);
    assert_eq!(zones_matching(&all, "NEW"), [&all[0]]);
    assert_eq!(zones_matching(&all, "").len(), 4);
    assert!(zones_matching(&all, "atlantis").is_empty());
    assert_eq!(zones_matching(&all, "lon")[0], &all[2]);
}

#[test]
fn alongside_defaults_when_it_can_and_the_slider_changes_the_shrink() {
    let mut f = flow(dual());
    assert_eq!(f.method, Method::Alongside);
    assert_eq!(f.alongside_title(), "Install Alongside Windows");
    to_method(&mut f);
    assert_eq!(f.advance(), Next::Go(Step::Alongside));
    let (uuid, name, lo, hi) = f.alongside_slider().unwrap();
    assert_eq!((uuid.as_str(), name.as_str()), ("u2", "Windows"));
    assert!(lo >= 104 * GB && lo < hi && hi <= 300 * GB);
    assert!(f.keep > lo && f.keep < hi, "starts in the middle");
    f.keep = lo;
    let most = f.alongside_room().unwrap();
    f.keep = hi;
    assert!(f.alongside_room().unwrap() < most);
    assert!(
        f.alongside_room().unwrap() >= MIN_ROOT,
        "the end of the slider still fits"
    );
    assert_eq!(f.advance(), Next::Go(Step::Protect));
    let lines = f.summary();
    assert!(
        lines.iter().any(|l| l.starts_with("Shrink Windows")),
        "{lines:?}"
    );
    assert!(lines.iter().any(|l| l.contains("Linux stays")), "{lines:?}");
}

#[test]
fn alongside_says_why_not() {
    let mut d = dual();
    d.parts[1].blocked = Some(wol_core::install::disk::Blocked::BitLocker);
    d.parts[2].min_bytes = None;
    // No free space to fall back on.
    d.bytes = 411 * GB;
    d.last = d.bytes / 512 - 34;
    let f = flow(d);
    assert_eq!(f.method, Method::Erase, "the default falls back");
    assert!(f.alongside_problem().unwrap().contains("BitLocker"));
    let mut mbr = dual();
    mbr.table = Table::Other;
    assert!(
        flow(mbr)
            .alongside_problem()
            .unwrap()
            .contains("Erase Disk")
    );
    let mut blank = dual();
    blank.parts.clear();
    blank.table = Table::None;
    assert!(flow(blank).alongside_problem().is_some());
}

#[test]
fn manual_keeps_home_and_touches_only_what_the_person_chose() {
    let mut f = flow(dual());
    to_method(&mut f);
    f.method = Method::Manual;
    assert_eq!(f.advance(), Next::Go(Step::Manual));
    assert!(matches!(f.advance(), Next::Stay(None, _)), "no system yet");
    // Free space becomes the system.
    let lines = manual_lines(&f.manual, f.disk().unwrap());
    assert_eq!(lines.len(), 5, "{lines:?}"); // three partitions and two gaps
    let free = lines
        .iter()
        .filter_map(|l| match l.0 {
            Row::Free(b) => Some(b),
            _ => None,
        })
        .max()
        .unwrap();
    assert_eq!(f.open_edit(&Row::Free(free)), Next::Go(Step::Edit));
    assert_eq!(f.edit.as_ref().unwrap().usage, Use::System);
    assert_eq!(f.advance(), Next::Go(Step::Manual));
    // The startup partition stays, with WolOS added.
    f.open_edit(&Row::Part("u1".into()));
    f.edit.as_mut().unwrap().usage = Use::Startup;
    f.advance();
    // /home is kept.
    f.open_edit(&Row::Part("u3".into()));
    let e = f.edit.as_mut().unwrap();
    e.usage = Use::Home;
    assert!(!e.erase, "keeping is the default");
    f.advance();
    assert_eq!(f.advance(), Next::Go(Step::Protect));
    let l = f.layout();
    assert!(l.chosen().is_empty(), "nothing existing changes: {l:?}");
    assert_eq!(f.compiled().unwrap().slots, Slots::Two);
    let lines = f.summary();
    assert!(
        lines.iter().any(|l| l.contains("Windows stays")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.starts_with("Keep Linux")),
        "{lines:?}"
    );
    // The install is encrypted (a password: no chip in the test); the kept volume is not.
    assert_ne!(f.protect, Protect::Off);
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("Keep Linux") && l.ends_with("It stays unencrypted.")),
        "{lines:?}"
    );
    f.protect = Protect::Off;
    assert!(
        !f.summary().iter().any(|l| l.contains("unencrypted")),
        "nothing to say when nothing is encrypted"
    );
}

#[test]
fn a_layout_that_can_not_keep_two_copies_says_so_in_the_summary() {
    let mut f = flow(dual());
    f.method = Method::Manual;
    f.manual.create.push(New {
        bytes: Some(30 * GB),
        mount: "/".into(),
        fs: Fs::Ext4,
    });
    f.manual.assign.push(Assign {
        part: "u1".into(),
        mount: "/efi".into(),
        format: None,
    });
    assert_eq!(f.compiled().unwrap().slots, Slots::One);
    let lines = f.summary();
    assert!(lines.iter().any(|l| l.contains("one copy")), "{lines:?}");
}

#[test]
fn deleting_goes_through_the_edit_page_and_can_be_taken_back() {
    let mut f = flow(dual());
    to_method(&mut f);
    f.method = Method::Manual;
    f.advance();
    f.open_edit(&Row::Part("u2".into()));
    f.delete_edited();
    assert_eq!(f.step, Step::Manual);
    assert_eq!(f.manual.delete, ["u2"]);
    let lines = manual_lines(&f.manual, f.disk().unwrap());
    assert!(lines[1].2.starts_with("Will be deleted"), "{lines:?}");
    assert!(
        lines
            .iter()
            .filter(|l| matches!(l.0, Row::Free(_)))
            .any(|l| { matches!(l.0, Row::Free(b) if b > 250 * GB) }),
        "the room shows at once: {lines:?}"
    );
    // Opening it again and choosing a job takes the delete back.
    f.open_edit(&Row::Part("u2".into()));
    f.edit.as_mut().unwrap().usage = Use::Folder;
    f.edit.as_mut().unwrap().folder = "../etc".into();
    assert!(matches!(f.advance(), Next::Stay(None, _)), "bad name stays");
    f.edit.as_mut().unwrap().folder = "windows".into();
    assert_eq!(f.advance(), Next::Go(Step::Manual));
    assert!(f.manual.delete.is_empty());
}

#[test]
fn a_new_partition_can_be_changed_or_removed() {
    let mut f = flow(dual());
    to_method(&mut f);
    f.method = Method::Manual;
    f.advance();
    f.open_edit(&Row::Free(40 * GB));
    f.edit.as_mut().unwrap().bytes = 30 * GB;
    f.advance();
    assert_eq!(f.manual.create[0].bytes, Some(30 * GB));
    f.open_edit(&Row::New(0));
    assert!(f.edit.as_ref().unwrap().max > 30 * GB);
    f.delete_edited();
    assert!(f.manual.create.is_empty());
}

#[test]
fn a_disk_password_is_asked_for_and_stays_out_of_the_plan() {
    let mut f = flow(dual());
    f.method = Method::Erase;
    f.step = Step::Protect;
    f.protect = Protect::Password;
    f.disk_password = "short".into();
    assert!(matches!(f.advance(), Next::Stay(Some(Field::Password), _)));
    f.disk_password = "white rabbit".into();
    f.disk_verify = "white rabbi".into();
    assert!(matches!(f.advance(), Next::Stay(Some(Field::Verify), _)));
    f.disk_verify = "white rabbit".into();
    assert_eq!(f.advance(), Next::Go(Step::Account));
    fill_account(&mut f);
    let json = serde_json::to_string(&f.plan().unwrap()).unwrap();
    assert!(
        !json.contains("rabbit") && !json.contains("curiouser"),
        "{json}"
    );
    let extras = format!("{:?}", f.plan_extras());
    assert!(
        !extras.contains("rabbit") && !extras.contains("curiouser"),
        "{extras}"
    );
}

#[test]
fn encryption_is_the_default_by_chip_when_there_is_one_else_by_password() {
    assert_eq!(
        Flow::new(&Hints::default(), false, true).protect,
        Protect::Tpm
    );
    assert_eq!(
        Flow::new(&Hints::default(), false, false).protect,
        Protect::Password
    );
    // Don't Encrypt stays a choice, and the summary says so once more.
    let mut f = flow(dual());
    f.method = Method::Erase;
    f.protect = Protect::Off;
    let w = f.summary_warning().unwrap();
    assert!(
        w.contains("Windows") && w.contains("won’t be encrypted"),
        "{w}"
    );
    f.protect = Protect::Tpm;
    assert!(!f.summary_warning().unwrap().contains("encrypted"));
    let mut blank = dual();
    blank.parts.clear();
    blank.table = Table::None;
    let mut f = flow(blank);
    f.method = Method::Erase;
    f.protect = Protect::Off;
    let w = f.summary_warning().unwrap();
    assert!(w.ends_with(copy::INSTALL_UNENCRYPTED), "{w}");
}

#[test]
fn the_disk_password_is_typable_before_any_desktop() {
    let mut f = flow(dual());
    f.method = Method::Erase;
    f.step = Step::Protect;
    f.protect = Protect::Password;
    f.disk_password = "café-au-lait".into();
    f.disk_verify = "café-au-lait".into();
    let n = f.advance();
    assert!(
        matches!(&n, Next::Stay(Some(Field::Password), why) if why == copy::INSTALL_PASSPHRASE_ASCII),
        "{n:?}"
    );
    f.disk_password = "white rabbit!".into();
    f.disk_verify = "white rabbit!".into();
    assert_eq!(f.advance(), Next::Go(Step::Account));
    assert!(passphrase_typable("Abc 123 ~!@#"));
    assert!(!passphrase_typable("tab\there"));
}

/// QA-SHADOW-7: Continue with no disk chosen asks for a disk; it does not judge a plan for an
/// empty default disk.
#[test]
fn no_disk_chosen_asks_for_a_disk_not_a_default_plan() {
    let mut f = flow(dual());
    to_method(&mut f);
    f.chosen = None;
    for step in [Step::Method, Step::Summary] {
        f.step = step;
        f.method = Method::Erase;
        assert_eq!(
            f.advance(),
            Next::Stay(None, copy::installer::CHOOSE_DISK.into()),
            "{step:?}"
        );
    }
}

/// QA-LIVE1-23: Replace on a blank disk says why on the How to Install page and Continue stays
/// there with the same sentence (it looked like a dead button).
#[test]
fn replace_unavailable_on_blank_disk() {
    let mut blank = dual();
    blank.parts.clear();
    blank.table = Table::None;
    let mut f = flow(blank);
    to_method(&mut f);
    f.method = Method::Replace;
    let why = f.replace_problem().expect("nothing to take over");
    assert_eq!(f.method_warning().as_deref(), Some(why.as_str()));
    assert_eq!(f.advance(), Next::Stay(None, why));
}

#[test]
fn replace_takes_one_partition_and_keeps_the_rest() {
    let mut f = flow(dual());
    to_method(&mut f);
    f.method = Method::Replace;
    assert_eq!(f.replace_problem(), None);
    assert_eq!(f.advance(), Next::Go(Step::Replace));
    assert!(matches!(f.advance(), Next::Stay(None, _)), "nothing chosen");
    let c = f.replace_candidates();
    assert_eq!(c.len(), 2, "{c:?}");
    assert_eq!((c[0].1.as_str(), c[1].1.as_str()), ("Windows", "Linux"));
    assert!(c[1].2.contains("100 GB"), "{c:?}");
    f.replace = Some("u3".into());
    assert_eq!(f.advance(), Next::Go(Step::Protect));
    let l = f.layout();
    assert_eq!(l.assign.len(), 2, "{l:?}");
    assert!(l.create.is_empty(), "the startup partition is reused");
    assert_eq!(l.chosen().len(), 1);
    let lines = f.summary();
    assert!(
        lines.iter().any(|l| l.starts_with("Erase Linux")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("Windows stays")),
        "{lines:?}"
    );
    assert_eq!(f.back(), Some(Step::Replace));
    assert_eq!(f.back(), Some(Step::Method));
    // No room for a new startup partition, and nothing to take over: Replace says why.
    let mut blank = dual();
    blank.parts.clear();
    blank.table = Table::None;
    let f = flow(blank);
    assert!(f.replace_problem().is_some());
    let mut mbr = dual();
    mbr.table = Table::Other;
    assert!(flow(mbr).replace_problem().unwrap().contains("Erase Disk"));
}

#[test]
fn swap_name_and_automatic_login_reach_the_summary_and_the_run() {
    let mut f = flow(dual());
    f.method = Method::Erase;
    fill_account(&mut f);
    assert_eq!(f.hostname(), "aliceliddell-wolos", "follows the account");
    assert_eq!(f.swap(), Swap::Zram);
    let lines = f.summary();
    assert!(
        lines.iter().any(|l| l.contains("compressed memory")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("aliceliddell-wolos")),
        "{lines:?}"
    );
    f.set_memory(16 * GIB);
    f.swap = Swap::Swapfile(0);
    f.autologin = true;
    f.set_hostname(" rabbit-hole ");
    assert_eq!(f.swap(), Swap::Swapfile(8));
    let lines = f.summary();
    assert!(
        lines.iter().any(|l| l.contains("swap file of 8 GB")),
        "{lines:?}"
    );
    assert!(lines.iter().any(|l| l.contains("rabbit-hole")), "{lines:?}");
    assert!(
        lines.iter().any(|l| l.contains("automatically")),
        "{lines:?}"
    );
    for l in &lines {
        assert!(copy::lint(l).is_empty(), "{l}: {:?}", copy::lint(l));
    }
    let x = f.plan_extras();
    assert_eq!(
        x,
        [
            ("hostname", serde_json::json!("rabbit-hole")),
            ("autologin", serde_json::json!(true)),
            ("swap", serde_json::json!("swapfile:8")),
        ]
    );
    f.swap = Swap::None;
    assert_eq!(f.swap().word(), "none");
    assert!(!f.summary().iter().any(|l| l.contains("swap")));
    f.set_hostname("");
    assert_eq!(f.hostname(), "aliceliddell-wolos", "empty follows again");
    // A bad name stops at the account page, under its field.
    f.step = Step::Account;
    f.set_hostname("-bad-");
    assert!(matches!(f.advance(), Next::Stay(Some(Field::Hostname), _)));
    f.set_hostname("good-1");
    assert_eq!(f.advance(), Next::Go(Step::Extras));
    assert_eq!(swapfile_gib(GIB), 2);
    assert_eq!(swapfile_gib(64 * GIB), 8);
    assert_eq!(
        meminfo_bytes("MemTotal:       16318028 kB\nMemFree: 1 kB\n"),
        Some(16318028 * 1024)
    );
    assert_eq!(meminfo_bytes(""), None);
}

#[test]
fn try_again_goes_to_the_disk_page_with_a_way_back() {
    let mut f = flow(dual());
    to_method(&mut f);
    f.jump(Step::Failed);
    assert_eq!(f.back(), None, "nothing to go back to from Failed");
    f.restart();
    assert_eq!(f.step, Step::Disk);
    assert_eq!(f.back(), Some(Step::Zone));
    assert_eq!(f.back(), Some(Step::Language));
    assert_eq!(f.back(), Some(Step::Begin));
    let mut live = Flow::new(&Hints::default(), false, false);
    live.restart();
    assert_eq!(live.back(), Some(Step::Zone));
    assert_eq!(live.back(), Some(Step::Language));
    assert_eq!(live.back(), None);
}

#[test]
fn disks_say_their_device_and_kind() {
    let d = dual();
    assert_eq!(disk_detail(&d), "500 GB · /dev/vda · Virtual disk");
    let mut n = dual();
    n.path = "/dev/nvme0n1".into();
    assert_eq!(disk_detail(&n), "500 GB · /dev/nvme0n1 · NVMe");
    let mut sd = dual();
    sd.path = "/dev/sda".into();
    assert_eq!(disk_detail(&sd), "500 GB · /dev/sda");
}

/// QA-LIVE1-35: a disk without a model (a virtio disk) was titled "/dev/vda" over a line that said
/// "/dev/vda" again. The title says what kind of disk it is; the device name stays in the line
/// under it, once.
#[test]
fn a_disk_without_a_model_is_not_named_twice() {
    let mut v = dual();
    v.model = String::new();
    assert_eq!(v.title_text(), "Virtual disk");
    assert_eq!(disk_detail(&v), "500 GB · /dev/vda");
    let mut n = v.clone();
    n.path = "/dev/nvme0n1".into();
    assert_eq!(n.title_text(), "NVMe disk");
    assert_eq!(disk_detail(&n), "500 GB · /dev/nvme0n1");
    let mut sd = v.clone();
    sd.path = "/dev/sda".into();
    assert_eq!(sd.title_text(), "Disk");
    // With a model the title is the model and the kind stays in the line.
    assert_eq!(dual().title_text(), "WD Blue");
}

#[test]
fn secure_boot_is_said_before_anything_is_written() {
    let mut f = flow(dual());
    f.disks[0].check.secure_boot = true;
    f.method = Method::Erase;
    f.step = Step::Summary;
    assert_eq!(f.advance(), Next::Go(Step::SecureBoot));
    assert_eq!(f.advance(), Next::Install);
    // R2B-3: the shim is on the image, so the setting is changed after this install, never
    // by a second one, and the keys are never cleared (that trips BitLocker, R2B-9).
    let mut said = vec![Step::SecureBoot.subtitle()];
    said.extend(f.secure_boot_notes());
    let t = said.join(" ").to_lowercase();
    assert!(!t.contains("install again"), "{t}");
    assert!(!t.contains("clear the secure boot keys"), "{t}");
    assert!(
        t.contains("third-party certificates") && t.contains("blue screen"),
        "{t}"
    );
}

/// R2B-9: a Windows with BitLocker that stays is warned about when this install changes the
/// Secure Boot keys Windows measures, and only then.
#[test]
fn bitlocker_is_said_when_the_secure_boot_keys_change_next_to_it() {
    let mut d = dual();
    d.parts[1].fstype = "BitLocker".into();
    d.parts[1].blocked = Some(wol_core::install::disk::Blocked::BitLocker);
    let mut f = flow(d);
    f.method = Method::Replace;
    f.replace = Some("u3".into());
    f.disks[0].check.tier = Some("own-keys".into());
    // Own keys only when the person turns Secure Boot on (lead, 2026-11-02: opt-in).
    f.secure = true;
    assert!(f.windows_stays() && f.bitlocker_stays());
    // This disk's BitLocker Windows now makes Replace a refusal (FIX-install, WindowsUnchecked), so
    // there is no plan to summarise; the notes themselves are what the person reads next to a
    // Windows on another disk, and they are checked here as they are built.
    assert!(f.summary().is_empty());
    let lines: Vec<String> = f
        .bitlocker_notes(false)
        .into_iter()
        .map(String::from)
        .collect();
    for want in [
        copy::installer::BITLOCKER_RECOVERY,
        copy::installer::BITLOCKER_KEY_WHERE,
        copy::installer::BITLOCKER_AVOID_OWN_KEYS,
    ] {
        assert!(lines.iter().any(|l| l == want), "{lines:?}");
    }
    assert!(lines.iter().any(|l| l.contains("BitLocker")), "{lines:?}");
    assert_eq!(
        f.bitlocker_notes(true).len(),
        2,
        "after the install: the key only"
    );
    f.disks[0].check.tier = Some("firmware".into());
    assert!(
        f.bitlocker_notes(true)
            .contains(&copy::installer::BITLOCKER_AVOID_SETTING)
    );
    // The key screen (mok) leaves Windows' keys alone.
    f.disks[0].check.tier = Some("mok".into());
    assert!(f.bitlocker_notes(false).is_empty());
    // Erased: no Windows stays on this disk...
    f.disks[0].check.tier = Some("own-keys".into());
    f.method = Method::Erase;
    assert!(!f.windows_stays() && f.bitlocker_notes(false).is_empty());
    // ...unless another disk has a locked Windows.
    let mut other = Found {
        disk: Disk::default(),
        check: super::super::Check::default(),
    };
    other.check.others = vec!["Windows".into()];
    other.check.bitlocker = true;
    f.disks.push(other);
    assert!(f.windows_stays() && !f.bitlocker_notes(false).is_empty());
    for s in copy::installer::bitlocker("own-keys", false)
        .into_iter()
        .chain(copy::installer::bitlocker("firmware", false))
    {
        assert!(copy::lint(s).is_empty(), "{s}: {:?}", copy::lint(s));
    }
}

/// R2B-12: next to Windows the clock keeps local time (install-run writes it), and the summary says so.
#[test]
fn the_clock_follows_windows_when_it_stays() {
    let mut f = flow(dual());
    f.method = Method::Replace;
    f.replace = Some("u3".into());
    assert!(f.windows_stays());
    assert!(
        f.summary()
            .iter()
            .any(|l| l == copy::installer::CLOCK_LOCAL)
    );
    f.method = Method::Erase;
    assert!(
        !f.summary()
            .iter()
            .any(|l| l == copy::installer::CLOCK_LOCAL)
    );
    assert!(copy::lint(copy::installer::CLOCK_LOCAL).is_empty());
}

#[test]
fn a_refused_disk_stops_at_the_disk_page() {
    let mut f = flow(dual());
    f.disks[0].check.refused = Some("not-listed".into());
    f.step = Step::Disk;
    assert!(matches!(f.advance(), Next::Stay(None, _)));
}

#[test]
fn low_space_and_other_systems_are_warned_about() {
    let mut f = flow(dual());
    f.method = Method::Erase;
    assert!(f.summary_warning().unwrap().contains("Windows"));
    let mut small = dual();
    small.bytes = 30 * GB;
    small.parts.clear();
    small.table = Table::None;
    let mut f = flow(small);
    f.method = Method::Erase;
    assert!(f.summary_warning().unwrap().contains("30 GB"));
}

#[test]
fn every_title_and_sentence_here_passes_the_lint() {
    for s in STEPS {
        assert!(!s.title().is_empty());
        if !s.subtitle().is_empty() && s != Step::SecureBoot {
            let l = crate::copy::lint(s.subtitle());
            assert!(l.is_empty(), "{s:?}: {l:?}");
        }
    }
    for (_, title, detail) in SWAPS {
        assert!(!title.is_empty() && !detail.is_empty());
    }
    assert!(Step::Done.subtitle().contains("USB stick"));
}

/// B2: the live system runs from the stick; it comes out after Restart, not before.
#[test]
fn the_medium_comes_out_only_after_the_screen_goes_dark() {
    for s in [
        Step::Done.subtitle(),
        copy::INSTALL_KEY_SCREEN_MEDIA,
        copy::INSTALL_OWN_KEYS_MEDIA,
    ] {
        let l = s.to_lowercase();
        assert!(l.contains("screen goes dark"), "{s}");
        assert!(!l.contains("before") && !l.contains(", first"), "{s}");
    }
}

/// B8: the page setting the keyboard list to follow a language is no choice of the person's.
#[test]
fn a_language_click_moves_the_keyboard_until_the_person_picks_one() {
    let at = |k: &str| KEYBOARDS.iter().position(|x| x.0 == k).unwrap();
    let mut f = Flow::new(&Hints::default(), true, false);
    f.set_language(2);
    assert_eq!(f.draft.keyboard, at("br"));
    // The list echoes the index the language set: not a pick.
    assert!(!f.keyboard_shown(f.draft.keyboard, false));
    f.set_language(6);
    assert_eq!(
        f.draft.keyboard,
        at("de"),
        "the second language moves it too"
    );
    assert!(f.keyboard_shown(at("us"), true));
    f.set_language(2);
    assert_eq!(f.draft.keyboard, at("us"), "a picked keyboard stays");
}

/// R2B-8: picking the very layout the language suggested is still the person's pick.
#[test]
fn picking_the_suggested_keyboard_is_a_pick_too() {
    let at = |k: &str| KEYBOARDS.iter().position(|x| x.0 == k).unwrap();
    let mut f = Flow::new(&Hints::default(), true, false);
    f.set_language(2);
    assert_eq!(f.draft.keyboard, at("br"));
    assert!(f.keyboard_shown(at("br"), true));
    f.set_language(6);
    assert_eq!(f.draft.keyboard, at("br"), "the picked keyboard stays");
}

/// B9: with the security chip the account password is the disk's fallback at the console.
#[test]
fn the_chip_fallback_password_is_typable_before_any_desktop() {
    let mut f = flow(dual());
    f.method = Method::Erase;
    f.protect = Protect::Tpm;
    f.step = Step::Account;
    fill_account(&mut f);
    f.draft.password = "café12345".into();
    f.draft.verify = "café12345".into();
    let n = f.advance();
    assert!(
        matches!(&n, Next::Stay(Some(Field::Password), why) if why == copy::INSTALL_TPM_FALLBACK),
        "{n:?}"
    );
    // A disk password of its own: the account password never opens the disk.
    f.protect = Protect::Password;
    assert_eq!(f.advance(), Next::Go(Step::Extras));
    f.step = Step::Account;
    f.protect = Protect::Tpm;
    fill_account(&mut f);
    assert_eq!(f.advance(), Next::Go(Step::Extras));
}

/// B3: next to a Windows startup partition too small to share, Windows leaves the menu.
#[test]
fn the_summary_says_how_windows_starts_when_it_leaves_the_menu() {
    let mut small = dual();
    small.parts[0].free_bytes = Some(60 * MIB);
    let mut f = flow(small);
    assert_eq!(f.method, Method::Alongside);
    assert_eq!(
        f.windows_start().as_deref(),
        Some(copy::INSTALL_WINDOWS_START)
    );
    let lines = f.summary();
    assert!(
        lines.iter().any(|l| l == copy::INSTALL_WINDOWS_START),
        "{lines:?}"
    );
    // Shared startup partition: Windows stays in the menu.
    assert_eq!(flow(dual()).windows_start(), None);
    // Erased or taken over: no Windows to start.
    f.method = Method::Erase;
    assert_eq!(f.windows_start(), None);
    f.method = Method::Replace;
    f.replace = Some("u2".into());
    // A plain data partition starts nothing: nothing to say.
    assert_eq!(f.windows_start(), None);
    f.replace = Some("u3".into());
    assert_eq!(
        f.windows_start().as_deref(),
        Some(copy::INSTALL_WINDOWS_START)
    );
}

/// R2B-11: another Linux keeps its own startup partition next to WolOS's new one: it is named.
#[test]
fn the_summary_says_how_another_linux_starts_when_it_leaves_the_menu() {
    let mut d = dual();
    d.parts[0].free_bytes = Some(60 * MIB);
    d.parts[2].os = Some("Fedora Linux 41".into());
    let mut f = flow(d);
    f.method = Method::Replace;
    f.replace = Some("u2".into());
    let want = copy::installer::other_start("Fedora Linux 41");
    assert_eq!(f.windows_start().as_deref(), Some(want.as_str()));
    assert!(f.summary().contains(&want), "{:?}", f.summary());
}

/// An encrypted install with no recovery key read back never reaches Done's Restart; an
/// unencrypted one has no key to miss. The page cannot be left with Back or Escape.
#[test]
fn an_encrypted_install_without_its_recovery_key_does_not_offer_restart() {
    let mut f = flow(dual());
    for p in [Protect::Tpm, Protect::Password] {
        f.protect = p;
        assert!(f.key_unread(None), "{p:?}");
        assert!(f.key_unread(Some("  ")), "{p:?}");
        assert!(!f.key_unread(Some("1234-5678")), "{p:?}");
    }
    f.protect = Protect::Off;
    assert!(!f.key_unread(None));
    f.jump(Step::KeyUnread);
    assert_eq!(f.back(), None);
    assert_eq!(f.advance(), Next::Go(Step::KeyUnread));
    let t = Step::KeyUnread.subtitle();
    assert!(
        t.contains("Try Again") && crate::copy::lint(t).is_empty(),
        "{t}"
    );
}

/// R2B-10: a Windows that did not shut down fully stops every layout but Erase Disk, with a
/// sentence that says how to shut it down.
#[test]
fn a_hibernated_windows_is_said_before_the_disk_is_touched() {
    let mut d = dual();
    d.parts[1].blocked = Some(wol_core::install::disk::Blocked::NotShutDown);
    let mut f = flow(d);
    f.method = Method::Replace;
    f.replace = Some("u3".into());
    f.step = Step::Replace;
    match f.advance() {
        Next::Stay(None, why) => {
            assert!(why.contains("Fast Startup"), "{why}");
            assert!(copy::lint(&why).is_empty(), "{why}");
        }
        n => panic!("went on: {n:?}"),
    }
    f.method = Method::Erase;
    f.step = Step::Method;
    assert_eq!(f.advance(), Next::Go(Step::Protect));
}

fn records() -> Vec<wol_core::modules::Module> {
    [
        ("creative", 3500),
        ("dev", 900),
        ("console", 50),
        ("ai-claude", 300),
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

/// Lead 2026-11-02: Secure Boot is opt-in where leaving it off changes nothing at the start,
/// default off; the key screen is pictured on its page and repeated on Done only when chosen.
#[test]
fn secure_boot_is_a_choice_default_off_where_off_still_starts() {
    let mut f = flow(dual());
    assert_eq!(f.secure_offer(), SecureOffer::Nothing, "no tier said");
    f.disks[0].check.tier = Some("mok".into());
    f.disks[0].check.tier_off = Some("off".into());
    assert_eq!(f.secure_offer(), SecureOffer::Choice { screen: true });
    assert!(!f.secure && !f.secure_wanted() && !f.key_screen_chosen());
    f.method = Method::Erase;
    f.protect = Protect::Off;
    f.step = Step::Protect;
    assert_eq!(f.advance(), Next::Go(Step::SecureBootChoice));
    assert_eq!(f.advance(), Next::Go(Step::Account));
    assert_eq!(f.back(), Some(Step::SecureBootChoice));
    assert_eq!(f.plan().unwrap().secure_boot, Some(false));
    assert_eq!(f.secure_line(), Some(copy::installer::SUMMARY_SECURE_OFF));
    f.secure = true;
    assert!(f.secure_wanted() && f.key_screen_chosen());
    assert_eq!(f.plan().unwrap().secure_boot, Some(true));
    assert!(
        f.summary()
            .iter()
            .any(|l| l == copy::installer::SUMMARY_SECURE_SCREEN)
    );
    let intro = f.intro(Step::SecureBootChoice);
    assert!(
        intro.contains("blue screen") && intro.contains("optional"),
        "{intro}"
    );
    // The computer's own Secure Boot is on: off would not start, so it is on and explained.
    f.disks[0].check.tier_off = Some("firmware".into());
    f.secure = false;
    assert_eq!(f.secure_offer(), SecureOffer::AlreadyOn);
    assert!(f.secure_wanted() && f.key_screen_chosen());
    assert!(
        f.intro(Step::SecureBootChoice)
            .contains("already has Secure Boot")
    );
    // An install-check that does not say keeps today's answer.
    f.disks[0].check.tier_off = None;
    assert_eq!(f.secure_offer(), SecureOffer::AlreadyOn);
    // Setup Mode: own keys, nothing to click.
    f.disks[0].check.tier = Some("own-keys".into());
    assert_eq!(f.secure_offer(), SecureOffer::Choice { screen: false });
    assert!(f.intro(Step::SecureBootChoice).contains("nothing to click"));
    // Nothing to choose: the page is skipped.
    for t in ["off", "firmware"] {
        f.disks[0].check.tier = Some(t.into());
        assert_eq!(f.secure_offer(), SecureOffer::Nothing, "{t}");
        assert!(f.secure_wanted() && f.secure_line().is_none());
        f.step = Step::Protect;
        assert_eq!(f.advance(), Next::Go(Step::Account), "{t}");
    }
}

/// Off by choice in Setup Mode: Windows' Secure Boot keys stay, so BitLocker asks nothing.
#[test]
fn leaving_secure_boot_off_says_nothing_about_bitlocker() {
    let mut d = dual();
    d.parts[1].fstype = "BitLocker".into();
    let mut f = flow(d);
    f.method = Method::Replace;
    f.replace = Some("u3".into());
    f.disks[0].check.tier = Some("own-keys".into());
    assert!(f.bitlocker_stays());
    assert!(
        f.bitlocker_notes(false).is_empty(),
        "off: Windows' keys stay"
    );
    f.secure = true;
    assert!(!f.bitlocker_notes(false).is_empty());
}

/// ADR 0032 B: the Modules page offers the image's records in the lead's order, none ticked;
/// the ticked ones, their size and the media formats reach the summary and the plan.
#[test]
fn modules_and_media_formats_are_offered_never_chosen_and_reach_the_plan() {
    let mut f = flow(dual());
    f.set_modules(&records());
    let ids: Vec<&str> = f.module_list.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        ["dev", "ai-claude", "creative"],
        "words for each, lead's order"
    );
    assert!(f.modules.is_empty() && !f.codecs, "nothing by default");
    f.method = Method::Erase;
    f.protect = Protect::Off;
    f.step = Step::Account;
    fill_account(&mut f);
    assert_eq!(f.advance(), Next::Go(Step::Modules));
    assert_eq!(f.advance(), Next::Go(Step::Extras));
    assert_eq!(f.advance(), Next::Go(Step::Summary));
    let p = f.plan().unwrap();
    assert!(p.modules.is_empty() && p.extras.is_empty());
    f.tick_module("creative", true);
    f.tick_module("dev", true);
    f.tick_module("console", true);
    f.tick_module("nonsense", true);
    assert_eq!(f.modules_mb(), 4400);
    f.codecs = true;
    let p = f.plan().unwrap();
    assert_eq!(
        p.modules,
        ["dev", "creative"],
        "page order, offered ids only"
    );
    assert_eq!(p.extras, [CODECS_KIT]);
    assert!(wol_core::store::kit(CODECS_KIT).is_some());
    let s = f.summary();
    assert!(
        s.iter()
            .any(|l| l == "Adds Developer Tools and Creative Apps, about 4 GB to download."),
        "{s:?}"
    );
    assert!(s.iter().any(|l| l == copy::installer::SUMMARY_CODECS));
    assert_eq!(f.deferred_line(), None, "online: nothing waits");
    // Offline: Summary says what waits for the first start with a network.
    f.offline = true;
    let s = f.summary();
    assert_eq!(
        s.last().map(String::as_str),
        Some(copy::installer::DEFERRED_BOTH)
    );
    f.codecs = false;
    assert_eq!(f.deferred_line(), Some(copy::installer::DEFERRED_MODULES));
    f.codecs = true;
    f.offline = false;
    f.tick_module("dev", false);
    assert_eq!(f.plan().unwrap().modules, ["creative"]);
    // An image with no records: the page is skipped.
    f.set_modules(&[]);
    assert!(f.modules.is_empty());
    f.step = Step::Account;
    assert_eq!(f.advance(), Next::Go(Step::Extras));
}

/// Lead 2026-11-02: every page explains itself in plain words and names the next one; each
/// sentence passes the lint and no page uses a tool's word.
#[test]
fn every_page_explains_itself_in_plain_words() {
    let mut f = flow(dual());
    f.set_modules(&records());
    f.disks[0].check.tier = Some("mok".into());
    f.disks[0].check.tier_off = Some("off".into());
    let mut said: Vec<String> = Vec::new();
    for st in STEPS {
        said.extend(copy::installer::explain(st).iter().map(|s| s.to_string()));
        if let Some(n) = copy::installer::next_words(st) {
            said.push(n.into());
        }
        let intro = f.intro(st);
        for jargon in [
            "ESP",
            "LUKS",
            "UKI",
            "TPM",
            "MOK",
            "initramfs",
            "zram",
            "EFI",
        ] {
            assert!(
                !intro
                    .split(|c: char| !c.is_alphanumeric())
                    .any(|w| w == jargon),
                "{st:?}: {jargon} in {intro}"
            );
        }
    }
    for st in [
        Step::Begin,
        Step::Disk,
        Step::Method,
        Step::Protect,
        Step::SecureBootChoice,
        Step::Account,
        Step::Modules,
        Step::Extras,
        Step::Summary,
    ] {
        assert!(
            f.intro(st).len() > st.subtitle().len() + 20,
            "{st:?} explains more than its line"
        );
    }
    let next = |s: Step| copy::installer::next_words(s).unwrap();
    assert!(
        f.intro(Step::Protect)
            .ends_with(next(Step::SecureBootChoice))
    );
    assert!(f.intro(Step::Account).ends_with(next(Step::Modules)));
    assert!(f.intro(Step::Extras).ends_with(next(Step::Summary)));
    assert_eq!(f.next_after(Step::Method), None, "the method decides");
    for c in [
        &copy::installer::SECURE_CHOICE_SCREEN[..],
        &copy::installer::SECURE_CHOICE_OWN_KEYS[..],
        &copy::installer::SECURE_ALREADY_ON[..],
    ] {
        said.extend(c.iter().map(|s| s.to_string()));
    }
    said.extend(
        [
            copy::installer::KEY_SCREEN_LOOKS,
            copy::installer::SUMMARY_SECURE_SCREEN,
            copy::installer::SUMMARY_SECURE_OWN_KEYS,
            copy::installer::SUMMARY_SECURE_OFF,
            copy::installer::SUMMARY_CODECS,
            copy::installer::DEFERRED_BOTH,
            copy::installer::DEFERRED_CODECS,
            copy::installer::DEFERRED_MODULES,
        ]
        .map(String::from),
    );
    said.push(copy::installer::modules_download(0));
    said.push(copy::installer::modules_download(4400));
    said.extend(copy::installer::summary_modules(&["Gaming"], 9000));
    said.extend(copy::installer::summary_modules(&["A", "B", "C"], 1));
    for st in [Step::SecureBootChoice, Step::Modules, Step::Extras] {
        said.push(st.subtitle().into());
    }
    for s in &said {
        let l = crate::copy::lint(s);
        assert!(l.is_empty(), "{s}: {l:?}");
    }
}

/// S8-2: the offline look reads both route tables like install-run's `ip route show default`.
#[test]
fn a_default_route_means_online() {
    let v4_head = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\n";
    let v4 = format!("{v4_head}eth0\t00000000\t0102A8C0\t0003\t0\t0\t100\t00000000\n");
    let v4_lan = format!("{v4_head}eth0\t0002A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\n");
    let zero = "00000000000000000000000000000000";
    let v6 = format!(
        "{zero} 00 {zero} 00 fe800000000000000000000000000001 00000400 00000001 00000000 00000003 wlan0\n"
    );
    let v6_reject = format!("{zero} 00 {zero} 00 {zero} ffffffff 00000001 00000000 00200200 lo\n");
    assert!(has_default_route(&v4, ""));
    assert!(has_default_route("", &v6));
    assert!(
        !has_default_route(&v4_lan, &v6_reject),
        "a LAN route and lo's reject are not"
    );
    assert!(!has_default_route("", ""));
}
