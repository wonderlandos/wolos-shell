// SPDX-License-Identifier: GPL-2.0-or-later
//! General > About: product mark, version, kernel and a hardware summary (`wol_core::hardware`).
use super::machine::{HardwareSnapshot, Kernel, Kind};
use super::*;
use wol_core::storage::{Storage, human};

/// Everything About shows that is read off the UI thread.
struct Facts {
    kernel: Kernel,
    hw: HardwareSnapshot,
    storage: Option<Storage>,
    /// os-release VERSION_ID.
    version: Option<String>,
}

/// The version About names: the installed system's (os-release), else this build's. The shell
/// crate's own version said 0.12.0 on a 0.12.1 system (QA-SET-R2-03).
pub(crate) fn version_of(os: Option<&str>) -> String {
    os.filter(|v| !v.is_empty())
        .unwrap_or(env!("CARGO_PKG_VERSION"))
        .to_string()
}

fn device(hw: &HardwareSnapshot, k: Kind) -> Option<String> {
    hw.devices
        .iter()
        .find(|d| d.kind == k)
        .map(|d| d.name.clone())
}

/// "All 8 devices work" / "2 devices need attention".
pub(crate) fn hardware_summary(hw: &HardwareSnapshot) -> String {
    let bad = hw.problems().count();
    match (bad, hw.devices.len()) {
        (_, 0) => "No devices found".into(),
        (0, n) => format!("All {n} devices are working"),
        (1, _) => "1 device needs attention".into(),
        (b, _) => format!("{b} devices need attention"),
    }
}

fn render(e: &Rc<Env>, into: &gtk::Box, f: Facts) {
    let h = &e.host;
    let or = |s: &str| {
        if s.is_empty() {
            "Unknown".to_string()
        } else {
            s.to_string()
        }
    };
    let mut rows = vec![info("Name", &or(&e.hostname()))];
    if !h.processor.is_empty() {
        rows.push(info("Processor", &h.processor));
    }
    if let Some(g) = device(&f.hw, Kind::Gpu) {
        rows.push(info("Graphics", &g));
    }
    if !h.memory.is_empty() {
        rows.push(info("Memory", &h.memory));
    }
    if let Some(s) = &f.storage {
        rows.push(info(
            "Startup disk",
            &format!(
                "{} ({} available)",
                human(s.total),
                human(s.total.saturating_sub(s.used))
            ),
        ));
    }
    if let Some(b) = crate::system::battery_label(e.sys().battery()) {
        rows.push(info("Battery", &b));
    }
    into.append(&w::section("", &rows));
    into.append(&w::section(
        "Software",
        &[
            info(
                "System",
                &format!("WolOS {}", version_of(f.version.as_deref())),
            ),
            info("Kernel", &f.kernel.label()),
        ],
    ));
    let hw = form::nav_row("wolos-chip-symbolic", "Hardware", &hardware_summary(&f.hw));
    let open = e.opener("Hardware");
    hw.connect_clicked(move |_| open());
    into.append(&w::section("", &[hw.upcast()]));
}

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let p = general::subpage(e, "About");
    let mark = w::app_icon("wolos-mark", w::ICON_XL);
    mark.set_halign(gtk::Align::Center);
    p.append(&mark);
    let head = w::vbox(w::S1);
    head.append(&w::centered("WolOS", "large-title"));
    let version = w::centered(&format!("Version {}", version_of(None)), "secondary");
    head.append(&version);
    p.append(&head);
    let body = w::vbox(w::S6);
    p.append(&body);
    let m = e.machine();
    let e2 = e.clone();
    later(
        &body,
        move || Facts {
            kernel: m.kernel(),
            hw: m.hardware(),
            storage: m.storage(),
            version: m.os_version(),
        },
        move |into, f| {
            version.set_label(&format!("Version {}", version_of(f.version.as_deref())));
            render(&e2, into, f)
        },
    );
    p
}

#[cfg(test)]
mod tests {
    use super::super::machine::{Device, State};
    use super::*;

    /// QA-SET-R2-03: About said "Version 0.12.0" (the shell crate's) on a 0.12.1 system.
    #[test]
    fn version_is_the_installed_one() {
        assert_eq!(version_of(Some("0.12.1")), "0.12.1");
        assert_eq!(version_of(None), env!("CARGO_PKG_VERSION"));
        assert_eq!(version_of(Some("")), env!("CARGO_PKG_VERSION"));
        assert_eq!(
            wol_core::base::version("NAME=WolOS\nID=wolos\nVERSION_ID=0.12.1\n").as_deref(),
            Some("0.12.1")
        );
    }

    #[test]
    fn summary_counts_problems() {
        let d = |state| Device {
            state,
            ..Default::default()
        };
        let hw = |v: Vec<Device>| HardwareSnapshot { devices: v };
        assert_eq!(hardware_summary(&hw(vec![])), "No devices found");
        assert_eq!(
            hardware_summary(&hw(vec![d(State::Ready), d(State::Ready)])),
            "All 2 devices are working"
        );
        assert_eq!(
            hardware_summary(&hw(vec![d(State::Ready), d(State::NeedsDriver)])),
            "1 device needs attention"
        );
        assert_eq!(
            hardware_summary(&hw(vec![d(State::NeedsFirmware), d(State::Unsupported)])),
            "2 devices need attention"
        );
    }
}
