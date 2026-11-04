// SPDX-License-Identifier: GPL-2.0-or-later
//! General > Date & Time: network time and the time zone (wol-admind `ntp`, `timezone`; the
//! Authenticate sheet asks first). Controls whose verb this build lacks are not shown.
use super::*;
use wol_core::region::zone_label;

/// Row titles, as the page shows them and Settings search finds them (`super::search::ROWS`).
pub(super) mod rows {
    pub const AUTOMATIC: &str = "Set time and date automatically";
    pub const ZONE: &str = "Time zone";
}

struct Facts {
    zones: Vec<String>,
    zone: String,
    ntp: Option<bool>,
}

pub(super) const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// "Tuesday 27 October, 14:05".
pub(crate) fn clock_line(t: &crate::clock::Stamp) -> String {
    const DAYS: [&str; 7] = [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ];
    format!(
        "{} {} {}, {:02}:{:02}",
        DAYS[usize::from(t.weekday.clamp(1, 7) - 1)],
        t.day,
        MONTHS[usize::from(t.month.clamp(1, 12) - 1)],
        t.hour,
        t.minute
    )
}

fn render(e: &Rc<Env>, into: &gtk::Box, f: Facts, status: &gtk::Label) {
    let mut rows = vec![info("Date and time", &clock_line(&kit::now()))];
    if let (Some(on), Some(_)) = (f.ntp, admin("ntp", &[])) {
        let (ctx, st) = (e.ctx.clone(), status.clone());
        rows.push(form::pending_toggle_row(
            rows::AUTOMATIC,
            "From time servers on the internet.",
            on,
            move |v, done| match admin("ntp", &[if v { "on" } else { "off" }]) {
                Some(a) => ctx.act_switch(a, &st, done),
                None => done(false),
            },
        ));
    }
    into.append(&w::section("", &rows));
    if f.zones.is_empty() || admin("timezone", &[]).is_none() {
        into.append(&w::section("", &[info(rows::ZONE, &zone_label(&f.zone))]));
        return;
    }
    let (zones, at) = zone_choices(&f.zones, &f.zone);
    let labels: Vec<String> = zones.iter().map(|z| zone_label(z)).collect();
    let names: Vec<&str> = labels.iter().map(String::as_str).collect();
    let (ctx, st) = (e.ctx.clone(), status.clone());
    // Waits for wol-admind: a refused or cancelled zone goes back to the one in effect
    // (QA-SET-29: the menu kept it).
    into.append(&w::section(
        "",
        &[form::row(
            rows::ZONE,
            "Type a city to find it.",
            &form::pending_search_picker(&names, at, move |i, done| {
                match zones.get(i).and_then(|z| admin("timezone", &[z])) {
                    Some(a) => ctx.act_switch(a, &st, done),
                    None => done(false),
                }
            }),
        )],
    ));
}

/// The zones the menu offers and the one it opens on: the zone in effect is always one of them
/// (UTC or an old name like `US/Pacific` is not in the city list, and the menu opened on
/// "Abidjan", the first city, QA-SET-29).
fn zone_choices(zones: &[String], zone: &str) -> (Vec<String>, usize) {
    let mut v = zones.to_vec();
    let at = match v.iter().position(|z| z == zone) {
        Some(i) => i,
        None if zone.is_empty() => 0,
        None => {
            v.insert(0, zone.to_string());
            0
        }
    };
    (v, at)
}

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let p = general::subpage(e, "Date & Time");
    let body = w::vbox(w::S6);
    p.append(&body);
    let status = status_label();
    p.append(&status);
    let (m, e2) = (e.machine(), e.clone());
    later(
        &body,
        move || Facts {
            zones: m.zones(),
            zone: m.zone(),
            ntp: m.ntp(),
        },
        move |into, f| render(&e2, into, f, &status),
    );
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    /// QA-SET-29: a machine on UTC opened the time-zone menu on "Abidjan".
    #[test]
    fn zone_in_effect_is_offered() {
        let cities = vec!["Africa/Abidjan".to_string(), "Europe/Lisbon".to_string()];
        let (v, at) = zone_choices(&cities, "UTC");
        assert_eq!(v[at], "UTC");
        assert_eq!(v.len(), 3);
        let (v, at) = zone_choices(&cities, "Europe/Lisbon");
        assert_eq!((v.len(), v[at].as_str()), (2, "Europe/Lisbon"));
    }

    #[test]
    fn clock_line_reads_like_a_sentence() {
        let t = crate::clock::Stamp {
            weekday: 2,
            day: 27,
            month: 10,
            hour: 9,
            minute: 5,
            ..kit::FAKE_NOW
        };
        assert_eq!(clock_line(&t), "Tuesday 27 October, 09:05");
    }
}
