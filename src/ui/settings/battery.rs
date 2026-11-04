// SPDX-License-Identifier: GPL-2.0-or-later
//! Battery (Energy on desktops): charge and health, the energy mode (Automatic, Low Power, High
//! Power: wol-admind `power-mode`, no password for the person at the computer) and what closing the
//! lid does, and Memory Saver. Rows whose hardware or verb is missing are not shown.
use super::*;
use wol_core::power::policy::{self, Cpu, Mode};
use wol_core::power::{BatteryInfo, Charge, Lid};

/// Energy modes in menu order.
pub(crate) const MODES: [(Mode, &str); 3] = [
    (Mode::Auto, "Automatic"),
    (Mode::Low, "Low Power"),
    (Mode::High, "High Power"),
];

/// What the current mode does, in one line.
pub(crate) fn mode_line(m: Mode) -> &'static str {
    match m {
        Mode::Auto => "Saves energy on battery, full speed when plugged in.",
        Mode::Low => "Uses less energy so the battery lasts longer.",
        Mode::High => "Full speed all the time; uses more energy.",
    }
}

/// "82% · Charging", "82% · 3 h 25 min left".
pub(crate) fn level_line(b: &BatteryInfo) -> String {
    let state = match (b.charge, b.minutes_left) {
        (Charge::Full, _) => "Fully charged".to_string(),
        (Charge::Charging, _) => "Charging".to_string(),
        (Charge::Discharging, Some(m)) if m >= 60 => format!("{} h {} min left", m / 60, m % 60),
        (Charge::Discharging, Some(m)) => format!("{m} min left"),
        (Charge::Discharging, None) => "On battery".to_string(),
    };
    format!("{}% · {state}", b.percent)
}

/// Health words, as on the Mac: Normal from 80 % of the design capacity.
pub(crate) fn condition(health: u8) -> &'static str {
    if health >= 80 {
        "Normal"
    } else {
        "Service recommended"
    }
}

struct Facts {
    battery: Option<BatteryInfo>,
    lid: Option<Lid>,
    power: Option<policy::Status>,
}

fn render(e: &Rc<Env>, into: &gtk::Box, f: Facts, status: &gtk::Label) {
    if let Some(b) = &f.battery {
        let mut rows = vec![info("Battery level", &level_line(b))];
        if let Some(h) = b.health {
            rows.push(info(
                "Battery health",
                &format!("{} ({h}% of new)", condition(h)),
            ));
        }
        if let Some(c) = b.cycles {
            rows.push(info("Charge cycles", &c.to_string()));
        }
        into.append(&w::section("Battery", &rows));
    }
    let mut energy = Vec::new();
    // Nothing to steer (virtual machine, firmware owns the clock): no mode to pick.
    if let (Some(p), Some(_)) = (&f.power, admin("power-mode", &[]))
        && p.cpu != Cpu::Fixed
    {
        let (ctx, st) = (e.ctx.clone(), status.clone());
        let at = MODES.iter().position(|m| m.0 == p.mode).unwrap_or(0);
        let names: Vec<&str> = MODES.iter().map(|m| m.1).collect();
        // The line under the menu says what the mode does once wol-admind set it; a refused or
        // cancelled change puts the menu back (QA-SET-19: both showed the new mode at once).
        let line_slot: Rc<RefCell<Option<gtk::Label>>> = Rc::default();
        let ls = line_slot.clone();
        let picker = form::pending_picker(&names, at, move |i, done| {
            let Some((mode, _)) = MODES.get(i) else {
                return done(false);
            };
            let Some(a) = admin("power-mode", &[mode.name()]) else {
                return done(false);
            };
            let (ls, mode) = (ls.clone(), *mode);
            ctx.act_switch(
                a,
                &st,
                Box::new(move |ok| {
                    if ok && let Some(l) = ls.borrow().as_ref() {
                        l.set_label(mode_line(mode));
                    }
                    done(ok)
                }),
            )
        });
        let (row, line) = form::status_row("", "Energy Mode", mode_line(p.mode), &picker);
        *line_slot.borrow_mut() = Some(line);
        energy.push(row);
    }
    if let (Some(lid), Some(_)) = (f.lid, admin("lid", &[])) {
        let (ctx, st) = (e.ctx.clone(), status.clone());
        energy.push(form::pending_picker_row(
            "When the lid is closed",
            "",
            &["Sleep", "Keep running"],
            usize::from(lid == Lid::Nothing),
            move |i, done| match admin("lid", &[if i == 0 { "sleep" } else { "nothing" }]) {
                Some(a) => ctx.act_switch(a, &st, done),
                None => done(false),
            },
        ));
    }
    if !energy.is_empty() {
        into.append(&w::section("Energy", &energy));
    }
    into.append(&super::memory_saver::section());
    let lock = form::nav_row(
        "system-lock-screen-symbolic",
        "Lock Screen",
        "When the screen locks and the display sleeps",
    );
    let open = e.opener("Lock Screen");
    lock.connect_clicked(move |_| open());
    into.append(&w::section("", &[lock.upcast()]));
}

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let laptop = e.sys().battery().is_some();
    let p = w::page(if laptop { "Battery" } else { "Energy" });
    let body = w::vbox(w::S6);
    p.append(&body);
    let status = status_label();
    p.append(&status);
    let (m, e2) = (e.machine(), e.clone());
    later(
        &body,
        move || Facts {
            battery: m.battery(),
            lid: m.lid(),
            power: m.power(),
        },
        move |into, f| render(&e2, into, f, &status),
    );
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_lines() {
        let b = |charge, minutes_left| BatteryInfo {
            percent: 82,
            charge,
            minutes_left,
            ..Default::default()
        };
        assert_eq!(level_line(&b(Charge::Charging, None)), "82% · Charging");
        assert_eq!(
            level_line(&b(Charge::Discharging, Some(205))),
            "82% · 3 h 25 min left"
        );
        assert_eq!(
            level_line(&b(Charge::Discharging, Some(42))),
            "82% · 42 min left"
        );
        assert_eq!(level_line(&b(Charge::Full, None)), "82% · Fully charged");
        assert_eq!(condition(94), "Normal");
        assert_eq!(condition(71), "Service recommended");
    }

    #[test]
    fn every_energy_mode_is_offered_and_explained() {
        for m in [Mode::Auto, Mode::Low, Mode::High] {
            assert!(MODES.iter().any(|x| x.0 == m), "{}", m.name());
            assert!(mode_line(m).ends_with('.'));
        }
    }
}
