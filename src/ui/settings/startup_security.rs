// SPDX-License-Identifier: GPL-2.0-or-later
//! Privacy & Security > Startup Security (ADR 0025): what `wol boot status` says about Secure
//! Boot, this computer's key, kernel lockdown, the security chip and the sealed system, and
//! Finish Secure Boot Setup (`wol boot enroll` through wol-admind `boot-enroll`; the steps of the
//! firmware's key screen follow). Attested gaming mode is a placeholder, off and not switchable,
//! until docs/research/attested-gaming-mode.md becomes a decision.
use super::*;
use crate::copy::settings as copy;
use wol_core::boot::Status;

pub(super) const TITLE: &str = "Startup Security";

/// Which keys the firmware trusts WolOS with, in words (`$S/tier`).
pub fn keys_label(tier: &str) -> &'static str {
    match tier {
        "own-keys" => "This computer's own keys",
        "mok" => "This computer's enrolled key",
        "firmware" => "The firmware's keys",
        _ => "None",
    }
}

/// `/sys/kernel/security/lockdown`'s word, in words.
pub fn lockdown_label(word: &str) -> &'static str {
    match word {
        "integrity" => "On",
        "confidentiality" => "On, strict",
        _ => "Off",
    }
}

/// Finish Secure Boot Setup is offered when the key still has to reach the firmware: it is
/// queued, the install left the Firmware tier, or the enrolled-key chain runs without Secure Boot.
pub fn needs_finish(s: &Status, key_pending: bool) -> bool {
    key_pending || s.tier == "firmware" || (s.tier == "mok" && !s.secure_boot)
}

fn on_off(on: bool) -> &'static str {
    if on { "On" } else { "Off" }
}

fn render(e: &Rc<Env>, into: &gtk::Box, facts: (Option<Status>, bool), status: &gtk::Label) {
    let (boot, pending) = facts;
    let Some(s) = boot else {
        into.append(&w::section(
            "Secure Boot",
            &[info("Secure Boot", "Unknown")],
        ));
        return;
    };
    let mut rows = vec![
        info("Secure Boot", &s.summary()),
        info("Keys", keys_label(&s.tier)),
        info("Kernel lockdown", lockdown_label(&s.lockdown)),
        info("Security chip", if s.tpm2 { "TPM 2.0" } else { "None" }),
        info("Measured startup", on_off(s.measured)),
        info("Sealed system", on_off(s.sealed)),
    ];
    if let Some(epoch) = s.epoch {
        rows.push(info("Release", &format!("{epoch}")));
    }
    into.append(&w::section("Secure Boot", &rows));
    // Built hidden too, so a Finish Setup that queues the key can show it without a rebuild.
    let queued = w::footnote(copy::STARTUP_KEY_QUEUED);
    queued.set_visible(pending);
    into.append(&queued);
    if let Some(a) = admin("boot-enroll", &[]).filter(|_| needs_finish(&s, pending)) {
        let steps = w::vbox(w::S1);
        steps.set_visible(false);
        for line in wol_core::boot::key_screen_steps() {
            steps.append(&w::footnote(&line));
        }
        let (ctx, st, shown, m) = (e.ctx.clone(), status.clone(), steps.clone(), e.machine());
        let finish = w::button("Finish Setup", w::Kind::Default, || {});
        finish.connect_clicked(move |b| {
            // One enroll at a time; after a success the button stays off (the key is queued).
            b.set_sensitive(false);
            let (st, shown, b, q, m) = (
                st.clone(),
                shown.clone(),
                b.clone(),
                queued.clone(),
                m.clone(),
            );
            ctx.act(
                a.clone(),
                Rc::new(move |r| {
                    say(&st, r.as_ref().err().map_or("", |e| e.message()));
                    shown.set_visible(r.is_ok());
                    b.set_sensitive(r.is_err());
                    if r.is_ok() {
                        let (q, m) = (q.clone(), m.clone());
                        off_thread(move || m.boot_key_pending(), move |p| q.set_visible(p));
                    }
                }),
            );
        });
        into.append(&w::section(
            "",
            &[form::row("Finish Secure Boot setup", "", &finish)],
        ));
        into.append(&steps);
    }
    into.append(&w::footnote(copy::STARTUP_ABOUT));
}

/// Attested gaming mode: shown off and not switchable (research, not a decision).
fn attested() -> gtk::Box {
    let off = w::toggle(false, |_| {});
    off.set_sensitive(false);
    let b = w::vbox(w::S2);
    b.append(&w::section(
        "Gaming",
        &[row_sub("Attested gaming mode", "Not available yet", &off)],
    ));
    b.append(&w::footnote(copy::STARTUP_ATTESTED));
    b
}

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let p = form::subpage(TITLE, e.opener("Privacy & Security"));
    let body = w::vbox(w::S6);
    p.append(&body);
    p.append(&attested());
    let status = status_label();
    p.append(&status);
    let (m, e2) = (e.machine(), e.clone());
    later(
        &body,
        move || (m.boot(), m.boot_key_pending()),
        move |into, f| render(&e2, into, f, &status),
    );
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(secure_boot: bool, tier: &str) -> Status {
        Status {
            secure_boot,
            setup_mode: false,
            tier: tier.into(),
            lockdown: "integrity".into(),
            tpm2: true,
            measured: true,
            sealed: true,
            epoch: None,
            floor: 0,
        }
    }

    #[test]
    fn finish_setup_shows_only_while_the_key_has_to_reach_the_firmware() {
        assert!(needs_finish(&status(true, "mok"), true), "queued");
        assert!(needs_finish(&status(true, "firmware"), false));
        assert!(needs_finish(&status(false, "mok"), false));
        assert!(!needs_finish(&status(true, "mok"), false), "done");
        assert!(!needs_finish(&status(true, "own-keys"), false));
        assert!(
            !needs_finish(&status(false, "off"), false),
            "the person chose off"
        );
    }

    #[test]
    fn boot_words_read_plainly() {
        assert_eq!(keys_label("own-keys"), "This computer's own keys");
        assert_eq!(keys_label("off"), "None");
        assert_eq!(lockdown_label("integrity"), "On");
        assert_eq!(lockdown_label("none"), "Off");
    }
}
