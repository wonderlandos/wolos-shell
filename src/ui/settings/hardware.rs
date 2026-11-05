// SPDX-License-Identifier: GPL-2.0-or-later
//! Hardware: every device the hardware catalogue knows (`wol hw`, snapshot written at boot), what
//! drives it, and a Fix button for those that need a driver module or a firmware package
//! (wol-admind `hw-load`, App Store install; the Authenticate sheet asks first). Check Again reads
//! the devices now (detect), each row says what Fix would install before the password is asked
//! (offer), and Undo Fix removes the packages Fix installed here and no others (revert): the
//! Terminal's `wol hw status`, `wol hw fix --yes` and `pacman -R`.
use super::machine::{Device, HardwareSnapshot, Kind, State};
use super::*;

pub(crate) fn kind_icon(k: Kind) -> &'static str {
    match k {
        Kind::Cpu | Kind::Other => "wolos-chip-symbolic",
        Kind::Gpu => "video-display-symbolic",
        Kind::Wifi => "network-wireless-symbolic",
        Kind::Ethernet => "network-wired-symbolic",
        Kind::Bluetooth => "bluetooth-active-symbolic",
        Kind::Audio => "audio-speakers-symbolic",
        Kind::Webcam => "camera-photo-symbolic",
        Kind::Input => "input-keyboard-symbolic",
        Kind::CardReader => "drive-harddisk-symbolic",
        Kind::Backlight => "display-brightness-symbolic",
    }
}

/// The steps Fix takes: modules to load, then packages to install. Empty = nothing to fix here.
pub(crate) fn fix_actions(d: &Device) -> Vec<Action> {
    let mut v: Vec<Action> = d
        .missing_klds
        .iter()
        .filter_map(|k| admin("hw-load", &[k]))
        .collect();
    v.extend(
        d.missing_packages
            .iter()
            .map(|p| Action::PkgInstall(p.clone())),
    );
    v
}

/// The packages Fix installed on this account's say-so, kept so Undo Fix takes back only those:
/// one package per line in `$XDG_STATE_HOME/wolos/hardware-fixed`. The catalogue also lists
/// packages the image ships (Vulkan drivers) and ones the first start installs; those are never
/// offered for removal because Fix did not add them.
pub(crate) mod fixed {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    /// Most bytes read from the file.
    const MAX: u64 = 64 * 1024;

    pub fn path() -> Option<PathBuf> {
        let base = crate::envv::var("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| {
                crate::envv::var("HOME")
                    .filter(|h| h.starts_with('/'))
                    .map(|h| Path::new(&h).join(".local/state"))
            })?;
        Some(base.join("wolos/hardware-fixed"))
    }

    /// Package names in `text`: one per line, a name is letters, digits and `@._+-`.
    pub fn parse(text: &str) -> BTreeSet<String> {
        text.lines()
            .map(str::trim)
            .filter(|l| {
                !l.is_empty()
                    && l.len() <= 128
                    && !l.starts_with('-')
                    && l.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"@._+-".contains(&b))
            })
            .map(String::from)
            .collect()
    }

    pub fn read_in(file: &Path) -> BTreeSet<String> {
        use std::io::Read;
        let mut bytes = Vec::new();
        match std::fs::File::open(file).and_then(|f| f.take(MAX).read_to_end(&mut bytes)) {
            Ok(_) => parse(&String::from_utf8_lossy(&bytes)),
            Err(_) => BTreeSet::new(),
        }
    }

    fn write_in(file: &Path, set: &BTreeSet<String>) {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let text: String = set.iter().map(|p| format!("{p}\n")).collect();
        // Beside the file and renamed over it: a crash never leaves half a list.
        let tmp = file.with_extension("new");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, file);
        }
    }

    /// Fix installed `pkg` here.
    pub fn add_in(file: &Path, pkg: &str) {
        let mut set = read_in(file);
        if set.insert(pkg.to_string()) {
            write_in(file, &set);
        }
    }

    /// `pkg` is gone (Undo Fix, or something else removed it).
    pub fn drop_in(file: &Path, pkg: &str) {
        let mut set = read_in(file);
        if set.remove(pkg) {
            write_in(file, &set);
        }
    }

    pub fn read() -> BTreeSet<String> {
        path().map(|p| read_in(&p)).unwrap_or_default()
    }
}

/// Keeps the log of what Fix installed true after one step ran: a package installed is added, one
/// removed is dropped.
fn note_step(file: Option<&std::path::Path>, a: &Action) {
    let Some(file) = file else { return };
    match a {
        Action::PkgInstall(p) => fixed::add_in(file, p),
        Action::PkgRemove(p) => fixed::drop_in(file, p),
        _ => {}
    }
}

/// Packages of this device that are installed now and that Fix installed here. Firmware and
/// driver packages both count; one the log does not name never does.
fn fixed_here(d: &Device, fixed: &std::collections::BTreeSet<String>) -> Vec<String> {
    d.packages
        .iter()
        .chain(&d.firmware)
        .filter(|p| !d.missing_packages.contains(p) && fixed.contains(*p))
        .cloned()
        .collect()
}

/// What Undo Fix on `d` (one of `all`) takes back: [`fixed_here`], less any package another
/// device of `all` has installed and lists as its own. Firmware packages are leaves, so pacman
/// would remove a shared one without a word and the other device would stop (QA-A10-SD-05).
pub(crate) fn undo_packages(
    d: &Device,
    all: &[Device],
    fixed: &std::collections::BTreeSet<String>,
) -> Vec<String> {
    let used_elsewhere = |p: &String| {
        all.iter().filter(|o| !std::ptr::eq(*o, d)).any(|o| {
            !o.missing_packages.contains(p) && (o.packages.contains(p) || o.firmware.contains(p))
        })
    };
    fixed_here(d, fixed)
        .into_iter()
        .filter(|p| !used_elsewhere(p))
        .collect()
}

/// The steps Undo Fix takes: remove those packages.
pub(crate) fn undo_actions(
    d: &Device,
    all: &[Device],
    fixed: &std::collections::BTreeSet<String>,
) -> Vec<Action> {
    undo_packages(d, all, fixed)
        .into_iter()
        .map(Action::PkgRemove)
        .collect()
}

/// "a", "a and b", "a, b and c".
fn names(v: &[String]) -> String {
    match v {
        [] => String::new(),
        [a] => a.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// The row's second line, in Settings words. `Device::hint` is the `wol hw` line ("install …: wol
/// hw fix --yes"), terminal text that read as the subtitle on real machines (QA-SET-13): it is
/// never shown here. A device Fix can mend says what Fix would install or turn on, so the person
/// sees the change before the password sheet; a working one says when its packages came from Fix.
fn subtitle(d: &Device, fixed: &std::collections::BTreeSet<String>) -> String {
    let fixable = !fix_actions(d).is_empty();
    let say = |text: &str, what: &[String]| text.replace("{what}", &names(what));
    match d.state {
        State::Ready => {
            let base = d
                .driver
                .as_ref()
                .map_or("Working".into(), |u| format!("Working · driver {u}"));
            if fixed_here(d, fixed).is_empty() {
                base
            } else {
                format!("{base} · {}", crate::copy::visual::HW_ADDED_BY_FIX)
            }
        }
        State::NeedsDriver
            if fixable && !d.missing_klds.is_empty() && d.missing_packages.is_empty() =>
        {
            say(crate::copy::visual::HW_DRIVER_OFF_NAMED, &d.missing_klds)
        }
        State::NeedsDriver if fixable => {
            say(crate::copy::visual::HW_NEEDS_DRIVER_NAMED, &fix_names(d))
        }
        State::NeedsFirmware if fixable => {
            say(crate::copy::visual::HW_NEEDS_FIRMWARE_NAMED, &fix_names(d))
        }
        _ if !d.notes.is_empty() => d.notes[0].clone(),
        State::NeedsDriver if d.driver.is_none() && d.missing_klds.is_empty() => {
            crate::copy::HW_DRIVER_DIDNT_START.into()
        }
        State::NeedsDriver => "Needs a driver".into(),
        State::NeedsFirmware => "Needs firmware".into(),
        State::Unsupported => "Not supported yet".into(),
    }
}

/// What Fix would work with, modules first: the names its steps carry.
fn fix_names(d: &Device) -> Vec<String> {
    d.missing_klds
        .iter()
        .chain(&d.missing_packages)
        .cloned()
        .collect()
}

/// The devices to show: the boot snapshot when the page opens, the live devices after a Fix
/// (the snapshot still has the device broken, QA-SET-7).
pub(crate) fn snapshot(m: &dyn Machine, after_fix: bool) -> HardwareSnapshot {
    if after_fix {
        m.hardware_live()
    } else {
        m.hardware()
    }
}

/// Runs `steps` one after another; `started` hears when each runs (see [`super::Started`]);
/// `done` gets the first error, or Ok after the last. A package step that worked is written to the
/// log of what Fix installed ([`fixed`]), so Undo Fix knows what is its to take back.
fn run_steps(
    ctx: &Rc<Ctx>,
    mut steps: Vec<Action>,
    started: super::Started,
    done: Rc<dyn Fn(Result<(), ActionError>)>,
) {
    if steps.is_empty() {
        return done(Ok(()));
    }
    let first = steps.remove(0);
    let (c, go) = (ctx.clone(), started.clone());
    let step = first.clone();
    ctx.act_asking(
        first,
        None,
        Some(started),
        Rc::new(move |r| match r {
            Ok(()) => {
                note_step(fixed::path().as_deref(), &step);
                run_steps(&c, steps.clone(), go.clone(), done.clone())
            }
            Err(e) => done(Err(e)),
        }),
    );
}

fn device_row(
    e: &Rc<Env>,
    d: &Device,
    all: &[Device],
    fixed: &std::collections::BTreeSet<String>,
    reload: &Rc<dyn Fn()>,
    status: &gtk::Label,
) -> gtk::Widget {
    let steps = fix_actions(d);
    let undo = undo_actions(d, all, fixed);
    let control = w::hbox(w::S1);
    if d.state != State::Ready && !steps.is_empty() {
        let (ctx, st, rl) = (e.ctx.clone(), status.clone(), reload.clone());
        let fix = w::button("Fix", w::Kind::Default, || {});
        fix.connect_clicked(move |b| {
            // Off at once (one Fix at a time); "Fixing…" only once it runs, not before the password
            // (QA-VIS2-39).
            b.set_sensitive(false);
            let (st, rl, b) = (st.clone(), rl.clone(), b.clone());
            let face = b.clone();
            run_steps(
                &ctx,
                steps.clone(),
                Rc::new(move |on| face.set_label(if on { "Fixing…" } else { "Fix" })),
                Rc::new(move |r| match r {
                    Ok(()) => rl(),
                    Err(x) => {
                        b.set_sensitive(true);
                        b.set_label("Fix");
                        say(&st, error_line(Err(x)));
                    }
                }),
            );
        });
        control.append(&fix);
    }
    if !undo.is_empty() {
        let (ctx, st, rl) = (e.ctx.clone(), status.clone(), reload.clone());
        if let Some(more) = form::actions_button(vec![(
            crate::copy::visual::HW_UNDO_FIX,
            Box::new(move || {
                let (st, rl) = (st.clone(), rl.clone());
                run_steps(
                    &ctx,
                    undo.clone(),
                    Rc::new(|_| {}),
                    Rc::new(move |r| match r {
                        Ok(()) => rl(),
                        Err(x) => say(
                            &st,
                            match error_line(Err(x)) {
                                "" => "",
                                _ => crate::copy::visual::HW_UNDO_NOT_DONE,
                            },
                        ),
                    }),
                );
            }),
        )]) {
            control.append(&more);
        }
    }
    let trailing = control
        .first_child()
        .map(|_| control.upcast_ref::<gtk::Widget>());
    w::list_row(kind_icon(d.kind), &d.name, &subtitle(d, fixed), trailing).upcast()
}

/// The page's facts: the devices, and which packages Fix installed here.
pub(crate) struct Facts {
    pub hw: HardwareSnapshot,
    pub fixed: std::collections::BTreeSet<String>,
}

fn render(
    e: &Rc<Env>,
    into: &gtk::Box,
    facts: Facts,
    live: bool,
    reload: &Rc<dyn Fn()>,
    again: &Rc<dyn Fn()>,
    status: &gtk::Label,
) {
    let Facts { hw, fixed } = facts;
    if hw.devices.is_empty() {
        into.append(&w::empty_state(
            "wolos-chip-symbolic",
            "No Hardware Report Yet",
            "WolOS checks your devices each time it starts. Restart to see them here.",
            None,
        ));
        let again = again.clone();
        let b = w::button(
            crate::copy::visual::HW_CHECK_AGAIN,
            w::Kind::Default,
            move || again(),
        );
        b.set_halign(gtk::Align::Center);
        into.append(&b);
        return;
    }
    let bad: Vec<&Device> = hw.problems().collect();
    let icon = if bad.is_empty() {
        "object-select-symbolic"
    } else {
        "dialog-warning-symbolic"
    };
    let line = about::hardware_summary(&hw);
    let verdict = if bad.is_empty() {
        "Drivers and firmware are in place."
    } else if bad.iter().any(|d| !fix_actions(d).is_empty()) {
        "Fix installs what is missing. Nothing else changes."
    } else {
        "WolOS can’t make these work yet. Updates may add them."
    };
    let when = if live {
        crate::copy::visual::HW_CHECKED_NOW
    } else {
        crate::copy::visual::HW_FROM_START
    };
    // Detect: the list is the boot snapshot until the person asks WolOS to look again.
    let check = {
        let again = again.clone();
        w::button(
            crate::copy::visual::HW_CHECK_AGAIN,
            w::Kind::Default,
            move || again(),
        )
    };
    let summary = w::list_row(
        icon,
        &line,
        &format!("{verdict} {when}"),
        Some(check.upcast_ref()),
    );
    summary.add_css_class(if bad.is_empty() {
        "all-good"
    } else {
        "attention"
    });
    into.append(&w::section("", &[summary.upcast()]));
    if !bad.is_empty() {
        let rows: Vec<gtk::Widget> = bad
            .iter()
            .map(|d| device_row(e, d, &hw.devices, &fixed, reload, status))
            .collect();
        into.append(&w::section("Needs Attention", &rows));
    }
    let mut ready: Vec<&Device> = hw
        .devices
        .iter()
        .filter(|d| d.state == State::Ready)
        .collect();
    ready.sort_by_key(|d| d.kind);
    if !ready.is_empty() {
        let rows: Vec<gtk::Widget> = ready
            .iter()
            .map(|d| device_row(e, d, &hw.devices, &fixed, reload, status))
            .collect();
        into.append(&w::section("Devices", &rows));
    }
}

/// Reads the devices (live after a Fix, an Undo Fix or Check Again) and puts them on the page.
fn load(e: &Rc<Env>, into: &gtk::Box, status: &gtk::Label, live: bool) {
    clear(into);
    let reload: Rc<dyn Fn()> = {
        let (e, into, st) = (e.clone(), into.downgrade(), status.clone());
        Rc::new(move || {
            if let Some(i) = into.upgrade() {
                load(&e, &i, &st, true)
            }
        })
    };
    let (m, e2, st) = (e.machine(), e.clone(), status.clone());
    let again = reload.clone();
    later(
        into,
        move || Facts {
            hw: snapshot(&*m, live),
            fixed: fixed::read(),
        },
        move |into, facts| render(&e2, into, facts, live, &reload, &again, &st),
    );
}

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let p = w::page("Hardware");
    let body = w::vbox(w::S6);
    p.append(&body);
    let status = status_label();
    p.append(&status);
    load(e, &body, &status, false);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    /// QA-SET-13: a device missing a package or a module showed `wol hw`'s terminal line
    /// ("install …: wol hw fix --yes") as its subtitle.
    #[test]
    fn subtitle_has_no_wol_hw() {
        let hint = |state, klds: &[&str], pkgs: &[&str]| Device {
            state,
            hint: "install linux-firmware-iwlwifi: wol hw fix --yes".into(),
            missing_klds: klds.iter().map(|s| s.to_string()).collect(),
            missing_packages: pkgs.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        for d in [
            hint(State::NeedsFirmware, &[], &["linux-firmware-iwlwifi"]),
            hint(State::NeedsDriver, &[], &["broadcom-wl"]),
            hint(State::NeedsDriver, &["coretemp"], &[]),
            hint(State::NeedsDriver, &[], &[]),
            hint(State::Unsupported, &[], &[]),
        ] {
            let s = subtitle(&d, &Default::default());
            assert!(
                !s.contains("wol") && !s.contains("--") && !s.contains(':'),
                "{s}"
            );
        }
    }

    /// A machine whose boot snapshot still lists the device Fix just mended.
    struct Stale;
    impl Machine for Stale {
        fn hardware(&self) -> HardwareSnapshot {
            HardwareSnapshot {
                devices: vec![Device {
                    name: "Wi-Fi".into(),
                    state: State::NeedsDriver,
                    missing_klds: vec!["iwlwifi".into()],
                    ..Default::default()
                }],
            }
        }
        fn hardware_live(&self) -> HardwareSnapshot {
            HardwareSnapshot {
                devices: vec![Device {
                    name: "Wi-Fi".into(),
                    state: State::Ready,
                    ..Default::default()
                }],
            }
        }
        super::machine::demo_but_hardware!();
    }

    /// QA-SET-7: after a Fix that worked the page reloaded the boot snapshot and showed the
    /// device still broken.
    #[test]
    fn after_fix_reads_live() {
        assert_eq!(
            snapshot(&Stale, false).problems().count(),
            1,
            "opened: the snapshot"
        );
        assert_eq!(
            snapshot(&Stale, true).problems().count(),
            0,
            "after Fix: live"
        );
    }

    #[test]
    fn fix_loads_modules_then_installs_packages() {
        let d = Device {
            missing_klds: vec!["if_iwlwifi".into()],
            missing_packages: vec!["wifi-firmware-iwlwifi-kmod".into()],
            state: State::NeedsFirmware,
            ..Default::default()
        };
        let steps = fix_actions(&d);
        assert_eq!(steps.len(), 2);
        assert!(matches!(&steps[0], Action::Admin(r) if r.args == ["if_iwlwifi"]));
        assert!(matches!(&steps[1], Action::PkgInstall(p) if p == "wifi-firmware-iwlwifi-kmod"));
        assert!(fix_actions(&Device::default()).is_empty());
        assert_eq!(subtitle(&Device::default(), &Default::default()), "Working");
        let odd = Device {
            state: State::Unsupported,
            notes: vec!["This Wi-Fi card has no driver yet.".into()],
            ..Default::default()
        };
        assert_eq!(
            subtitle(&odd, &Default::default()),
            "This Wi-Fi card has no driver yet."
        );
        assert_eq!(
            subtitle(
                &Device {
                    notes: vec![],
                    ..odd
                },
                &Default::default()
            ),
            "Not supported yet"
        );
    }

    fn set(v: &[&str]) -> std::collections::BTreeSet<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// Offer: the row names what Fix will install or turn on, before the password sheet.
    #[test]
    fn the_row_says_what_fix_installs() {
        let none = Default::default();
        let fw = Device {
            state: State::NeedsFirmware,
            missing_packages: vec!["linux-firmware-realtek".into()],
            ..Default::default()
        };
        assert_eq!(
            subtitle(&fw, &none),
            "Needs firmware. Fix installs linux-firmware-realtek."
        );
        let both = Device {
            state: State::NeedsDriver,
            missing_klds: vec!["nvidia".into()],
            missing_packages: vec!["nvidia-open-dkms".into(), "nvidia-utils".into()],
            ..Default::default()
        };
        assert_eq!(
            subtitle(&both, &none),
            "Needs a driver. Fix installs nvidia, nvidia-open-dkms and nvidia-utils."
        );
        let off = Device {
            state: State::NeedsDriver,
            missing_klds: vec!["uvcvideo".into()],
            ..Default::default()
        };
        assert_eq!(
            subtitle(&off, &none),
            "Its driver is installed but not turned on. Fix turns on uvcvideo."
        );
        for d in [&fw, &both, &off] {
            let s = subtitle(d, &none);
            assert!(
                crate::copy::lint(&s).is_empty(),
                "{s}: {:?}",
                crate::copy::lint(&s)
            );
        }
    }

    /// Revert: Undo Fix removes the packages Fix installed on this device and nothing else.
    #[test]
    fn undo_fix_takes_back_only_what_fix_added() {
        let gpu = Device {
            state: State::Ready,
            driver: Some("nvidia".into()),
            packages: vec![
                "nvidia-open-dkms".into(),
                "nvidia-utils".into(),
                "vulkan-intel".into(),
            ],
            firmware: vec!["linux-firmware-nvidia".into()],
            missing_packages: vec!["nvidia-utils".into()],
            ..Default::default()
        };
        // Fix installed two; one of them is not installed now, one is shipped (never logged).
        let log = set(&["nvidia-open-dkms", "nvidia-utils", "linux-firmware-nvidia"]);
        assert_eq!(
            undo_packages(&gpu, &[], &log),
            ["nvidia-open-dkms", "linux-firmware-nvidia"],
            "installed now and logged"
        );
        let acts = undo_actions(&gpu, &[], &log);
        assert!(
            matches!(&acts[..], [Action::PkgRemove(a), Action::PkgRemove(b)]
                if a == "nvidia-open-dkms" && b == "linux-firmware-nvidia"),
            "{acts:?}"
        );
        // Nothing logged: nothing to undo, so no menu.
        assert!(undo_actions(&gpu, &[], &set(&[])).is_empty());
        // The log is about this device's packages only.
        assert!(undo_actions(&Device::default(), &[], &log).is_empty());
        assert!(
            subtitle(&gpu, &log).ends_with("added by Fix"),
            "{}",
            subtitle(&gpu, &log)
        );
        assert_eq!(subtitle(&gpu, &set(&[])), "Working · driver nvidia");
    }

    /// QA-A10-SD-05: two devices share one firmware package (one split for a chip's Wi-Fi and
    /// Bluetooth). Fix on the Wi-Fi installed and logged it; the Bluetooth works with it now.
    /// Undo Fix on the Wi-Fi keeps it (removing it would stop the Bluetooth with no word), and
    /// still takes back what only the Wi-Fi uses. Once no other device needs it, it goes.
    #[test]
    fn undo_fix_keeps_a_package_another_device_uses() {
        let wifi = Device {
            state: State::Ready,
            packages: vec!["iwd-helper".into()],
            firmware: vec!["linux-firmware-intel".into()],
            ..Default::default()
        };
        let bt = Device {
            kind: Kind::Bluetooth,
            state: State::Ready,
            firmware: vec!["linux-firmware-intel".into()],
            ..Default::default()
        };
        let log = set(&["iwd-helper", "linux-firmware-intel"]);
        let all = [wifi.clone(), bt.clone()];
        assert_eq!(undo_packages(&all[0], &all, &log), ["iwd-helper"]);
        assert!(
            matches!(&undo_actions(&all[0], &all, &log)[..], [Action::PkgRemove(a)] if a == "iwd-helper")
        );
        // The Bluetooth row does not offer to remove it either: the Wi-Fi uses it.
        assert!(undo_packages(&all[1], &all, &log).is_empty());
        // A device that still lacks the package does not hold it.
        let mut lacking = bt;
        lacking.missing_packages = vec!["linux-firmware-intel".into()];
        let all = [wifi, lacking];
        assert_eq!(
            undo_packages(&all[0], &all, &log),
            ["iwd-helper", "linux-firmware-intel"]
        );
    }

    /// The log of what Fix installed: a step that worked is added (installed) or dropped (removed),
    /// the file survives a restart, and a hand-edited line cannot smuggle a flag into a removal.
    #[test]
    fn the_fix_log_follows_the_steps() {
        let dir = std::env::temp_dir().join(format!("wol-hwfix-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("wolos/hardware-fixed");
        assert!(fixed::read_in(&file).is_empty(), "no file yet");
        note_step(Some(&file), &Action::PkgInstall("broadcom-wl".into()));
        note_step(
            Some(&file),
            &Action::PkgInstall("linux-firmware-realtek".into()),
        );
        note_step(Some(&file), &Action::PkgInstall("broadcom-wl".into()));
        assert_eq!(
            fixed::read_in(&file),
            set(&["broadcom-wl", "linux-firmware-realtek"])
        );
        note_step(Some(&file), &Action::PkgRemove("broadcom-wl".into()));
        assert_eq!(fixed::read_in(&file), set(&["linux-firmware-realtek"]));
        // A module load is no package.
        note_step(Some(&file), &Action::Wifi(true));
        assert_eq!(fixed::read_in(&file), set(&["linux-firmware-realtek"]));
        assert_eq!(
            fixed::parse("good-pkg\n--noconfirm\n-Rns\nsp ace\n\nlib32-x.y+z\n"),
            set(&["good-pkg", "lib32-x.y+z"])
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
