// SPDX-License-Identifier: GPL-2.0-or-later
//! Notifications: Focus (Do Not Disturb, for a while or until turned off) and Notification Center.
//! Same file as the Control Center Focus tile and `wol focus` (`crate::focus`).
use super::*;
use crate::focus::{self, Focus};

/// "Turn off" choices while Focus is on.
const UNTIL: [&str; 3] = ["When I turn it off", "In one hour", "Tomorrow morning"];

/// The "Turn off" choice that `f` reads back as at `now` (unix seconds), `tod` the local time of
/// day in seconds since midnight. Within the hour it is "In one hour", unless it ends exactly on
/// the morning boundary: "Tomorrow morning" chosen at 06:30 has 30 minutes left and is still that
/// (QA-SET-24; the file stores the end, not the choice).
fn until_index(f: Focus, now: u64, tod: u32) -> usize {
    match f {
        Focus::Until(u) if u.saturating_sub(now) <= focus::HOUR => {
            let ends = (u64::from(tod) + u.saturating_sub(now)) % 86_400;
            let morning = u64::from(focus::MORNING_HOUR) * 3600;
            // A second either way: `now` and the clock were read a moment apart.
            if ends.abs_diff(morning) <= 2 { 2 } else { 1 }
        }
        Focus::Until(_) => 2,
        _ => 0,
    }
}

/// Seconds since local midnight.
fn time_of_day() -> u32 {
    let t = kit::now();
    u32::from(t.hour) * 3600 + u32::from(t.minute) * 60 + u32::from(t.second)
}

fn focus_for(i: usize) -> Focus {
    let now = focus::unix_now();
    match i {
        1 => Focus::Until(now + focus::HOUR),
        2 => {
            let t = kit::now();
            Focus::Until(now + focus::secs_to_morning(t.hour, t.minute, t.second))
        }
        _ => Focus::On,
    }
}

pub(super) fn page(_: &Rc<Env>) -> gtk::Box {
    let p = w::page("Notifications");
    let status = status_label();
    let cur = focus::read();
    let now = focus::unix_now();
    let until = form::pending_picker_row(
        "Turn off",
        "",
        &UNTIL,
        until_index(cur, now, time_of_day()),
        {
            let st = status.clone();
            move |i, done| {
                let r = focus::write(focus_for(i));
                say(&st, if r.is_ok() { "" } else { SAVE_FAILED });
                done(r.is_ok())
            }
        },
    );
    until.set_visible(cur.active());
    let t = kit::now();
    let sub: Rc<RefCell<Option<gtk::Label>>> = Default::default();
    let toggle = {
        let (st, until, sub) = (status.clone(), until.clone(), sub.clone());
        w::pending_toggle(cur.active(), move |on, done| {
            let f = if on { Focus::On } else { Focus::Off };
            let r = focus::write(f);
            say(&st, if r.is_ok() { "" } else { SAVE_FAILED });
            if r.is_ok() {
                until.set_visible(on);
                let t = kit::now();
                if let Some(l) = &*sub.borrow() {
                    l.set_label(&state_line(f, focus::unix_now(), t.hour, t.minute));
                }
            }
            done(r.is_ok());
        })
    };
    let (dnd, line) = form::status_row(
        "weather-clear-night-symbolic",
        "Do Not Disturb",
        &state_line(cur, now, t.hour, t.minute),
        &toggle,
    );
    *sub.borrow_mut() = Some(line);
    p.append(&w::section("Focus", &[dnd, until]));
    p.append(&w::footnote(
        "While Focus is on, banners stay quiet and wait in Notification Center. Critical alerts, such as a low battery, still show.",
    ));
    p.append(&w::section(
        "",
        &[form::button_row(
            "Notification Center",
            "Every notification you missed, newest first.",
            "Show",
            || kit::spawn(&argv(&["wol", "shell", "show", "notifications"])),
        )],
    ));
    p.append(&status);
    p
}

/// "Off", "On until you turn it off", "On until 15:05".
fn state_line(f: Focus, now: u64, h: u8, m: u8) -> String {
    match f {
        Focus::Off => "Off".into(),
        Focus::On => "On until you turn it off".into(),
        Focus::Until(_) => format!("On {}", focus::label(f, now, h, m).to_lowercase()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_lines_and_choices() {
        assert_eq!(state_line(Focus::Off, 0, 9, 0), "Off");
        assert_eq!(state_line(Focus::On, 0, 9, 0), "On until you turn it off");
        assert_eq!(state_line(Focus::Until(3600), 0, 9, 0), "On until 10:00");
        assert_eq!(until_index(Focus::On, 0, 0), 0);
        assert_eq!(until_index(Focus::Until(1800), 0, 9 * 3600), 1);
        assert_eq!(until_index(Focus::Until(30_000), 0, 0), 2);
        // QA-SET-24: tomorrow morning chosen at 06:30 has 30 minutes left and still reads back as
        // tomorrow morning; one hour chosen then ends at 07:30 and reads back as one hour.
        let six_thirty = 6 * 3600 + 30 * 60;
        let to_morning = focus::secs_to_morning(6, 30, 0);
        assert_eq!(to_morning, 1800);
        assert_eq!(
            until_index(Focus::Until(1000 + to_morning), 1000, six_thirty),
            2
        );
        assert_eq!(
            until_index(Focus::Until(1000 + focus::HOUR), 1000, six_thirty),
            1
        );
        // A minute later, the same end: still the morning.
        assert_eq!(
            until_index(Focus::Until(1000 + to_morning), 1060, six_thirty + 60),
            2
        );
        assert_eq!(focus_for(0), Focus::On);
        assert!(matches!(focus_for(1), Focus::Until(_)));
    }
}
