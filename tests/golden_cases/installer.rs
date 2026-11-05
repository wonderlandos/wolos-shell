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
    // QA-DESIGN-17: the recovery key and Restart on the smallest screen the installer takes.
    v.push(Case {
        name: "installer-done-key-640x400".into(),
        size: (640, 400),
        class: "lock-window",
        wallpaper: false,
        build: Box::new(|| installer_at("done-key").root.clone()),
    });
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
        // No network at install time: the Summary says the media formats and modules chosen are
        // added at first start (copy::installer::DEFERRED_BOTH).
        "summary-offline" => {
            i.set_online_probe(|| false);
            i.with_flow(|f| {
                f.method = Method::Erase;
                f.set_modules(&records());
                f.tick_module("dev", true);
                f.codecs = true;
                f.offline = true;
            });
            i.jump(Step::Summary);
            i.with_flow(|f| {
                assert_eq!(
                    f.deferred_line(),
                    Some(shell::copy::installer::DEFERRED_BOTH)
                )
            });
        }
        // R2B-9: a Windows with BitLocker stays and this install changes the Secure Boot keys.
        // A BitLocker Windows on the chosen disk now refuses Replace, so on the Summary the
        // Windows that stays is the one on the other disk (`bitlocker yes` in install-check).
        "summary-bitlocker" | "secure-boot-bitlocker" => {
            i.with_flow(|f| {
                if page == "summary-bitlocker" {
                    f.method = Method::Erase;
                    f.disks[1].check.others = vec!["Windows".into()];
                    f.disks[1].check.bitlocker = true;
                } else {
                    f.method = Method::Replace;
                    f.replace = Some("u3".into());
                    f.disks[0].disk.parts[1].fstype = "BitLocker".into();
                }
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

/// Every page the goldens draw, by the name [`at`] knows.
const PAGES: [&str; 45] = [
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
    "summary-offline",
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

/// The installer the goldens draw at `page` (a disk with Windows and Linux, a blank one).
fn installer_at(page: &str) -> Rc<Installer> {
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
    i
}

pub fn add(v: &mut Vec<Case>) {
    for (sw, sh, tag) in [(1280, 800, "1280"), (1024, 640, "1024")] {
        for page in PAGES {
            v.push(Case {
                name: format!("installer-{page}-{tag}"),
                size: (sw, sh),
                class: "lock-window",
                wallpaper: false,
                build: Box::new(move || installer_at(page).root.clone()),
            });
        }
    }
    add_fit(v);
    add_empty(v);
    add_done_live(v);
}

/// QA-DESIGN-17: on a 640x400 screen (the installer's minimum) every page keeps its foot, Back and
/// Continue, inside the window: the card was 588 high and the foot was cut off with the recovery
/// key of the last page.
pub(super) fn every_page_keeps_its_buttons_on_a_640x400_screen() {
    use gtk::prelude::*;
    use shell::kit::assistant::{CARD, card_height};
    assert_eq!(
        card_height(1080),
        CARD.1,
        "a tall screen: the card's own height"
    );
    assert_eq!(card_height(400), 400 - 2 * shell::kit::widgets::S6);
    assert_eq!(card_height(0), CARD.1, "no screen known yet");
    let (sw, sh) = (640, 400);
    let mut cut = Vec::new();
    for page in PAGES {
        let i = installer_at(page);
        let frame = gtk::Overlay::new();
        frame.set_child(Some(&i.root));
        frame.set_size_request(sw, sh);
        let win = gtk::Window::builder()
            .decorated(false)
            .default_width(sw)
            .default_height(sh)
            .child(&frame)
            .build();
        win.add_css_class("lock-window");
        win.present();
        super::pump(500);
        let mut boxes: Vec<gtk::Box> = Vec::new();
        super::find_type(&i.root, &mut boxes);
        for f in boxes
            .iter()
            .filter(|b| b.has_css_class("assistant-foot") && b.is_mapped())
        {
            let r = f.compute_bounds(&win).expect("the foot has bounds");
            if r.y() < 0.0 || r.y() + r.height() > win.height() as f32 + 0.5 {
                cut.push(format!(
                    "{page}: foot at y {}..{} in a {}-high window",
                    r.y(),
                    r.y() + r.height(),
                    win.height()
                ));
            }
        }
        win.destroy();
    }
    assert!(cut.is_empty(), "feet off screen: {cut:#?}");
}

/// QA-VIS2-7/8: on Protect Your Files with Password chosen, Tab from the chosen row goes to the
/// Password field, then Verify, then the foot; never to a hidden or empty widget, and never past
/// the fields to Back (Space typed into the password then pressed Back or flipped the choice).
pub(super) fn protect_tab_goes_through_the_password_fields() {
    use gtk::prelude::*;
    let i = installer_at("protect-password");
    let win = gtk::Window::builder()
        .decorated(false)
        .default_width(1280)
        .default_height(800)
        .child(&i.root)
        .build();
    win.add_css_class("lock-window");
    win.present();
    win.set_focus_visible(true);
    super::pump(500);
    let mut buttons: Vec<gtk::Button> = Vec::new();
    super::find_type(&i.root, &mut buttons);
    let chosen = buttons
        .iter()
        .find(|b| {
            b.has_css_class("assistant-choice") && b.is_mapped() && {
                let mut l: Vec<gtk::Label> = Vec::new();
                super::find_type(b.upcast_ref(), &mut l);
                l.iter().any(|l| l.label() == "Password")
            }
        })
        .expect("the Password row")
        .clone();
    chosen.grab_focus();
    let mut path = Vec::new();
    for _ in 0..5 {
        win.child_focus(gtk::DirectionType::TabForward);
        let f = gtk::prelude::GtkWindowExt::focus(&win).expect("something has the keyboard");
        let what = f
            .ancestor(gtk::PasswordEntry::static_type())
            .and_then(|p| p.downcast::<gtk::PasswordEntry>().ok())
            .and_then(|p| p.placeholder_text())
            .map(|t| format!("field {t}"))
            .or_else(|| {
                f.downcast_ref::<gtk::Button>()
                    .and_then(|b| b.label())
                    .map(|l| format!("button {l}"))
            })
            .unwrap_or_else(|| format!("{} {:?}", f.type_().name(), f.css_classes()));
        assert!(f.is_drawable(), "Tab landed on a hidden widget: {what}");
        path.push(what);
    }
    win.destroy();
    let at = |what: &str| path.iter().position(|p| p == what);
    let (pw, verify, back) = (
        at("field At least 8 characters"),
        at("field Type it again"),
        at("button Back"),
    );
    assert!(
        pw.is_some() && pw < verify && verify < back,
        "Tab goes through Password and Verify before Back: {path:?}"
    );
}

/// QA-VIS2-3: no scroller of the assistant is a Tab stop of its own. The card's scroller and the
/// screen's (`lock-fit`) took the keyboard before the buttons, an empty ringed area, so Tab,
/// Tab, Shift-Tab, Shift-Tab, Return landed one stop off and started the live desktop on Welcome.
/// Tab from Install WolOS goes to a button of the page, never to a scroller.
pub(super) fn welcome_tab_stops_are_its_two_buttons() {
    use gtk::prelude::*;
    let i = installer_at("begin");
    let win = gtk::Window::builder()
        .decorated(false)
        .default_width(1280)
        .default_height(800)
        .child(&i.root)
        .build();
    win.add_css_class("lock-window");
    win.present();
    win.set_focus_visible(true);
    super::pump(500);
    let mut scrollers: Vec<gtk::ScrolledWindow> = Vec::new();
    super::find_type(&i.root, &mut scrollers);
    let stops: Vec<String> = scrollers
        .iter()
        .filter(|s| s.is_mapped() && s.is_focusable())
        .map(|s| format!("{:?}", s.css_classes()))
        .collect();
    let mut buttons: Vec<gtk::Button> = Vec::new();
    super::find_type(&i.root, &mut buttons);
    buttons
        .iter()
        .find(|b| b.label().as_deref() == Some("Install WolOS"))
        .expect("Install WolOS")
        .grab_focus();
    let mut path = Vec::new();
    for _ in 0..4 {
        win.child_focus(gtk::DirectionType::TabForward);
        path.push(
            gtk::prelude::GtkWindowExt::focus(&win)
                .map(|f| match f.downcast_ref::<gtk::Button>() {
                    Some(b) => b.label().map(|l| l.to_string()).unwrap_or_default(),
                    None => format!("{} {:?}", f.type_().name(), f.css_classes()),
                })
                .unwrap_or_default(),
        );
    }
    win.destroy();
    assert!(stops.is_empty(), "a scroller takes the keyboard: {stops:?}");
    assert!(
        path.iter()
            .all(|p| p == "Install WolOS" || p == "Try WolOS" || p.is_empty()),
        "Tab left the buttons of the page: {path:?}"
    );
}

/// QA-VIS2-10: Continue on Create Your Account with no password says why, as Setup does, and
/// puts the keyboard in that field.
pub(super) fn account_without_a_password_says_why() {
    use gtk::prelude::*;
    let i = installer_at("account");
    let win = gtk::Window::builder()
        .decorated(false)
        .default_width(1280)
        .default_height(800)
        .child(&i.root)
        .build();
    win.add_css_class("lock-window");
    win.present();
    super::pump(300);
    i.type_into(Field::FullName, "Alice Liddell");
    i.next();
    super::pump(100);
    let mut labels: Vec<gtk::Label> = Vec::new();
    super::find_type(&i.root, &mut labels);
    let said = labels
        .iter()
        .any(|l| l.is_mapped() && l.has_css_class("problem") && l.label() == "Choose a password.");
    let in_field = gtk::prelude::GtkWindowExt::focus(&win)
        .and_then(|f| f.ancestor(gtk::PasswordEntry::static_type()))
        .and_then(|p| p.downcast::<gtk::PasswordEntry>().ok())
        .is_some();
    win.destroy();
    assert!(said, "no 'Choose a password.' line after Continue");
    assert!(in_field, "the keyboard moved to the password field");
}

/// QA-VIS2-12: the installer card is as tall on every page, so Back and Continue stay where the
/// pointer left them (Apps was ~500 px, Music and Video Formats ~600). Only the two pages that
/// carry a recovery key or a log may grow past [`CARD`]'s height.
pub(super) fn the_card_is_as_tall_on_every_page() {
    use gtk::prelude::*;
    use shell::kit::assistant::CARD;
    let grows = ["done-key-screen", "failed-details"];
    let mut off = Vec::new();
    for page in PAGES {
        let i = installer_at(page);
        let win = gtk::Window::builder()
            .decorated(false)
            .default_width(1280)
            .default_height(1000)
            .child(&i.root)
            .build();
        win.add_css_class("lock-window");
        win.present();
        super::pump(400);
        let mut boxes: Vec<gtk::Box> = Vec::new();
        super::find_type(&i.root, &mut boxes);
        for b in boxes
            .iter()
            .filter(|b| b.has_css_class("assistant-card") && b.is_mapped())
        {
            if b.height() != CARD.1 && !grows.contains(&page) {
                off.push(format!("{page}: {} high, not {}", b.height(), CARD.1));
            }
        }
        win.destroy();
    }
    assert!(off.is_empty(), "cards of another height: {off:#?}");
}
