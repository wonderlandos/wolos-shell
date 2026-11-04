// SPDX-License-Identifier: GPL-2.0-or-later
//! Hardware: every device the hardware catalogue knows (`wol hw`, snapshot written at boot), what
//! drives it, and a Fix button for those that need a driver module or a firmware package
//! (wol-admind `hw-load`, App Store install; the Authenticate sheet asks first).
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

/// The row's second line, in Settings words. `Device::hint` is the `wol hw` line ("install …: wol
/// hw fix --yes"), terminal text that read as the subtitle on real machines (QA-SET-13): it is
/// never shown here.
fn subtitle(d: &Device) -> String {
    let fixable = !fix_actions(d).is_empty();
    match d.state {
        State::Ready => d
            .driver
            .as_ref()
            .map_or("Working".into(), |u| format!("Working · driver {u}")),
        State::NeedsDriver
            if fixable && !d.missing_klds.is_empty() && d.missing_packages.is_empty() =>
        {
            crate::copy::HW_DRIVER_OFF.into()
        }
        State::NeedsDriver if fixable => crate::copy::HW_NEEDS_DRIVER.into(),
        State::NeedsFirmware if fixable => crate::copy::HW_NEEDS_FIRMWARE.into(),
        _ if !d.notes.is_empty() => d.notes[0].clone(),
        State::NeedsDriver if d.driver.is_none() && d.missing_klds.is_empty() => {
            crate::copy::HW_DRIVER_DIDNT_START.into()
        }
        State::NeedsDriver => "Needs a driver".into(),
        State::NeedsFirmware => "Needs firmware".into(),
        State::Unsupported => "Not supported yet".into(),
    }
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

/// Runs `steps` one after another; `done` gets the first error, or Ok after the last.
fn run_steps(ctx: &Rc<Ctx>, mut steps: Vec<Action>, done: Rc<dyn Fn(Result<(), ActionError>)>) {
    if steps.is_empty() {
        return done(Ok(()));
    }
    let first = steps.remove(0);
    let c = ctx.clone();
    ctx.act(
        first,
        Rc::new(move |r| match r {
            Ok(()) => run_steps(&c, steps.clone(), done.clone()),
            Err(e) => done(Err(e)),
        }),
    );
}

fn device_row(e: &Rc<Env>, d: &Device, reload: &Rc<dyn Fn()>, status: &gtk::Label) -> gtk::Widget {
    let steps = fix_actions(d);
    if d.state == State::Ready || steps.is_empty() {
        return w::list_row(kind_icon(d.kind), &d.name, &subtitle(d), None).upcast();
    }
    let (ctx, st, rl) = (e.ctx.clone(), status.clone(), reload.clone());
    let fix = w::button("Fix", w::Kind::Default, || {});
    fix.connect_clicked(move |b| {
        b.set_sensitive(false);
        b.set_label("Fixing…");
        let (st, rl, b) = (st.clone(), rl.clone(), b.clone());
        run_steps(
            &ctx,
            steps.clone(),
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
    w::list_row(
        kind_icon(d.kind),
        &d.name,
        &subtitle(d),
        Some(fix.upcast_ref()),
    )
    .upcast()
}

fn render(
    e: &Rc<Env>,
    into: &gtk::Box,
    hw: HardwareSnapshot,
    reload: &Rc<dyn Fn()>,
    status: &gtk::Label,
) {
    if hw.devices.is_empty() {
        into.append(&w::empty_state(
            "wolos-chip-symbolic",
            "No Hardware Report Yet",
            "WolOS checks your devices each time it starts. Restart to see them here.",
            None,
        ));
        return;
    }
    let bad: Vec<&Device> = hw.problems().collect();
    let icon = if bad.is_empty() {
        "object-select-symbolic"
    } else {
        "dialog-warning-symbolic"
    };
    let line = about::hardware_summary(&hw);
    let summary = w::list_row(
        icon,
        &line,
        if bad.is_empty() {
            "Drivers and firmware are in place."
        } else if bad.iter().any(|d| !fix_actions(d).is_empty()) {
            "Fix installs what is missing. Nothing else changes."
        } else {
            "WolOS can’t make these work yet. Updates may add them."
        },
        None,
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
            .map(|d| device_row(e, d, reload, status))
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
            .map(|d| device_row(e, d, reload, status))
            .collect();
        into.append(&w::section("Devices", &rows));
    }
}

fn load(e: &Rc<Env>, into: &gtk::Box, status: &gtk::Label, after_fix: bool) {
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
    later(
        into,
        move || snapshot(&*m, after_fix),
        move |into, hw| render(&e2, into, hw, &reload, &st),
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
            let s = subtitle(&d);
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
        assert_eq!(subtitle(&Device::default()), "Working");
        let odd = Device {
            state: State::Unsupported,
            notes: vec!["This Wi-Fi card has no driver yet.".into()],
            ..Default::default()
        };
        assert_eq!(subtitle(&odd), "This Wi-Fi card has no driver yet.");
        assert_eq!(
            subtitle(&Device {
                notes: vec![],
                ..odd
            }),
            "Not supported yet"
        );
    }
}
