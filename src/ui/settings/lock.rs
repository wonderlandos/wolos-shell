// SPDX-License-Identifier: GPL-2.0-or-later
//! Lock Screen: when the screen locks (hypridle timeouts, journaled so the change can be undone).
//! Both controls wait for the save and go back, with the reason, when it fails (QA-SET-4).
use super::*;

/// Restarts hypridle on its new file ([`wol_core::journal::IDLE_RESTART`], the same steps an
/// undo takes): it reads its config only at start.
pub(super) fn restart_hypridle() {
    let [stop, start] = wol_core::journal::IDLE_RESTART;
    background(vec![argv(stop)]);
    glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
        kit::spawn(&argv(start))
    });
}

/// The menu position of `secs`: one of the choices, or a row of its own for a value written by
/// hand ("5 minutes" was shown for any odd value, QA-SET-4).
pub(super) fn choices(secs: u32) -> (Vec<String>, Vec<u32>, usize) {
    let mut labels: Vec<String> = prefs::IDLE_CHOICES
        .iter()
        .map(|c| c.0.to_string())
        .collect();
    let mut values: Vec<u32> = prefs::IDLE_CHOICES.iter().map(|c| c.1).collect();
    let at = match values.iter().position(|v| *v == secs) {
        Some(i) => i,
        None => {
            labels.push(crate::copy::idle_after(secs));
            values.push(secs);
            values.len() - 1
        }
    };
    (labels, values, at)
}

pub(super) fn page(_: &Rc<Env>) -> gtk::Box {
    let p = w::page("Lock Screen");
    let file = config(prefs::IDLE_FILE);
    let (secs, sleep) = prefs::parse_hypridle(read(&file).as_deref());
    let cur = Rc::new(std::cell::Cell::new((secs, sleep)));
    let status = status_label();
    // Saves (secs, sleep); true when the file has it.
    let save = Rc::new({
        let (cur, st) = (cur.clone(), status.clone());
        move |want: (u32, bool)| {
            let ok = save_journaled(
                "idle",
                prefs::IDLE_FILE,
                Some(&prefs::hypridle_conf(want.0, want.1)),
            )
            .is_ok();
            say(&st, if ok { "" } else { SAVE_FAILED });
            if ok {
                cur.set(want);
                restart_hypridle();
            }
            ok
        }
    });
    let (labels, values, sel) = choices(secs);
    let names: Vec<&str> = labels.iter().map(String::as_str).collect();
    let (c1, s1, c2, s2) = (cur.clone(), save.clone(), cur.clone(), save.clone());
    p.append(&w::section(
        "",
        &[
            form::pending_picker_row(
                "Lock screen when inactive",
                "",
                &names,
                sel,
                move |i, done| done(s1((values[i], c1.get().1))),
            ),
            form::pending_toggle_row(
                "Lock when the computer sleeps",
                "",
                sleep,
                move |on, done| done(s2((c2.get().0, on))),
            ),
        ],
    ));
    p.append(&status);
    p.append(&w::footnote(
        "The display turns off one minute after the screen locks.",
    ));
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    /// QA-SET-4: a timeout written by hand showed as "5 minutes".
    #[test]
    fn odd_timeout_gets_its_own_row() {
        let (labels, values, at) = choices(prefs::IDLE_CHOICES[0].1);
        assert_eq!((labels.len(), at), (prefs::IDLE_CHOICES.len(), 0));
        let (labels, values2, at) = choices(17 * 60);
        assert_eq!(values2[at], 17 * 60);
        assert_eq!(labels[at], "17 minutes");
        assert_eq!(choices(90).0.last().unwrap(), "90 seconds");
        assert_eq!(labels.len(), values.len() + 1);
    }
}
