// SPDX-License-Identifier: GPL-2.0-or-later
//! Unit tests of the Settings window (ui/settings/mod.rs), split out of the parent file (`#[cfg(test)] mod tests;`).
use super::*;

/// A change that never answers ends after a deadline; installs wait for their download.
#[test]
fn every_change_but_installs_has_a_deadline() {
    assert_eq!(answer_within(&Action::Wifi(true)), Some(ANSWER_WITHIN));
    assert_eq!(answer_within(&Action::PkgInstall("vim".into())), None);
    let a = admin("remote-login", &["on"]).unwrap();
    assert!(answer_within(&a).unwrap() >= ANSWER_WITHIN);
    assert!(crate::copy::lint(crate::copy::NO_ANSWER).is_empty());
}

/// QA-SET-1: Settings > `dark` > Return opens Appearance; clearing the field went back to
/// General, the pane shown before the search.
#[test]
fn clearing_query_keeps_opened_pane() {
    let mut before = None;
    assert_eq!(
        after_query("dark", Some("General"), &mut before),
        Nav::Results
    );
    assert_eq!(
        after_query("dark", Some(RESULTS), &mut before),
        Nav::Results
    );
    // Return (or a hit) opened Appearance; the field is cleared there.
    assert_eq!(after_query("", Some("Appearance"), &mut before), Nav::Stay);
    assert_eq!(before, None);
    // Cleared while the hits show: back where the search began.
    assert_eq!(
        after_query("so", Some("Wallpaper"), &mut before),
        Nav::Results
    );
    assert_eq!(
        after_query("", Some(RESULTS), &mut before),
        Nav::Back("Wallpaper".into())
    );
}

/// QA-HUNT-16: a Sound change and a Wallpaper change 3 s later showed "Wallpaper changed ·
/// Undo", and Undo reverted both.
#[test]
fn undo_burst_counts_changes() {
    let b = |v: &[(u64, &str)]| -> Vec<(u64, String)> {
        v.iter().map(|(i, w)| (*i, w.to_string())).collect()
    };
    assert_eq!(
        burst_line(&b(&[(1, "wallpaper")])),
        changed_line("wallpaper")
    );
    assert_eq!(
        burst_line(&b(&[(1, "effects"), (2, "effects"), (3, "effects")])),
        changed_line("effects"),
        "one topic, a drag of many saves"
    );
    assert_eq!(
        burst_line(&b(&[(1, "sound"), (2, "wallpaper")])),
        "2 changes"
    );
    assert_eq!(
        burst_line(&b(&[
            (1, "sound"),
            (2, "wallpaper"),
            (3, "sound"),
            (4, "wallpaper")
        ])),
        "2 changes",
        "distinct changes, not journal entries"
    );
}

/// QA-SET-5: Undo of a burst stopped at the first failing id; "Change undone" was a literal.
#[test]
fn undo_burst_tries_every_change() {
    let mut tried = Vec::new();
    let said = undo_all(&[3, 2, 1], |id| {
        tried.push(id);
        if id == 2 {
            Err("It was changed since.".into())
        } else {
            Ok(())
        }
    });
    assert_eq!(tried, [3, 2, 1], "every one tried");
    assert_eq!(said, "2 of 3 changes undone. It was changed since.");
    assert_eq!(undo_all(&[1], |_| Ok(())), "Change undone");
    assert_eq!(undo_all(&[2, 1], |_| Ok(())), "2 changes undone");
    assert_eq!(undo_all(&[1], |_| Err("Nope.".into())), "Nope.");
    // Toast lines, not error sentences: no lint (they start with a count).
    assert!(!said.contains("Change undone"));
}

/// QA-SHADOW-10: /etc/passwd unreadable gave an empty Users card with no word.
#[test]
fn users_page_unreadable_passwd_says_so() {
    let shells = "/bin/zsh\n";
    let passwd = "alice:x:1000:1000:Alice:/home/alice:/bin/zsh\n";
    assert_eq!(login_users(None, Some(shells)), None);
    assert_eq!(login_users(Some(passwd), None), None);
    assert_eq!(
        login_users(Some(passwd), Some(shells)),
        Some(crate::login::users(passwd, shells))
    );
    assert_eq!(no_users_line(false), crate::copy::USERS_UNREADABLE);
    assert_eq!(no_users_line(true), "No other accounts");
    assert!(crate::copy::lint(crate::copy::USERS_UNREADABLE).is_empty());
}

/// Hunt 1 E1: Cancel on the Authenticate sheet ended every waiting change as `Denied`, and
/// its status line said "Incorrect password. Try again." with no password typed.
#[test]
fn cancel_ends_every_waiting_change_quietly() {
    let log: Rc<RefCell<Vec<Result<(), ActionError>>>> = Rc::default();
    let all = ["remote", "airplay"]
        .map(|_| {
            let log = log.clone();
            Waiting {
                a: Action::Wifi(true),
                done: Rc::new(move |r| log.borrow_mut().push(r)),
                status: None,
                why: None,
                started: None,
            }
        })
        .into();
    cancelled(all);
    assert_eq!(*log.borrow(), [Err(ActionError::Cancelled); 2]);
    assert_eq!(error_line(Err(ActionError::Cancelled)), "");
    assert_eq!(error_line(Ok(())), "");
    // The other answers keep their sentence; only a wrong password names the password.
    assert_eq!(
        error_line(Err(ActionError::NotAdmin)),
        ActionError::NotAdmin.message()
    );
    assert!(
        !ActionError::Cancelled
            .message()
            .to_lowercase()
            .contains("password")
    );
}

/// wol-admind as the sheet meets it: NeedsAuth without a password, Ok with "right",
/// Denied with any other; `pkg` actions are wold's refusal (no password helps).
#[derive(Default)]
struct Asking {
    sent: std::sync::Mutex<Vec<(Action, Option<String>)>>,
}

impl System for Asking {
    fn battery(&self) -> Option<crate::status::Battery> {
        None
    }
    fn audio(&self) -> crate::status::Audio {
        Default::default()
    }
    fn network(&self) -> crate::status::Network {
        Default::default()
    }
    fn brightness(&self) -> Option<u8> {
        None
    }
    fn spawn_with(&self, a: Action, pw: Option<&str>) -> crate::system::Done {
        self.sent
            .lock()
            .unwrap()
            .push((a.clone(), pw.map(String::from)));
        let r = match (&a, pw) {
            (Action::PkgInstall(_), _) => Err(ActionError::Refused),
            (_, None) => Err(ActionError::NeedsAuth),
            (_, Some("right")) => Ok(()),
            (_, Some(_)) => Err(ActionError::Denied),
        };
        let (tx, rx) = async_channel::bounded(1);
        let _ = tx.try_send(r);
        rx
    }
}

type Log = Rc<RefCell<Vec<(&'static str, Result<(), ActionError>)>>>;

fn waiting(a: Action, name: &'static str, log: &Log) -> Waiting {
    let log = log.clone();
    Waiting {
        a,
        done: Rc::new(move |r| log.borrow_mut().push((name, r))),
        status: None,
        why: None,
        started: None,
    }
}

/// One send and its answer through the queue, as `Ctx::run` does (blocking here).
fn send(
    q: &mut AuthQueue,
    sys: &Asking,
    w: Waiting,
    pw: Option<(&str, u32)>,
) -> Option<&'static str> {
    let retry = pw.map(|(_, n)| n);
    q.sent(retry);
    let r = sys
        .spawn_with(w.a.clone(), pw.map(|(p, _)| p))
        .recv_blocking()
        .unwrap();
    match q.answered(w, r, retry) {
        Next::Finish(w, r) => {
            (w.done)(r);
            None
        }
        Next::Ask(m) => Some(m),
        Next::GiveUp(all) => {
            for w in all {
                (w.done)(Err(ActionError::Denied));
            }
            Some("gave up")
        }
    }
}

fn toggles() -> [Action; 2] {
    use shared::admin::{AdminAction as A, AdminRequest};
    let req = |action, on: &str| {
        Action::Admin(AdminRequest {
            action,
            args: vec![on.into()],
        })
    };
    [req(A::RemoteLogin, "on"), req(A::AirPlayReceiver, "on")]
}

/// QA 0.12.0 bug 6: two Sharing switches asked at once; the second overwrote the first,
/// whose switch stayed grey. Both wait now: one Allow runs both with the password, in order.
#[test]
fn two_queued_toggles_both_run_after_one_allow() {
    let (sys, log, mut q) = (Asking::default(), Log::default(), AuthQueue::default());
    let [a, b] = toggles();
    assert_eq!(
        send(&mut q, &sys, waiting(a.clone(), "remote", &log), None),
        Some("")
    );
    assert_eq!(
        send(&mut q, &sys, waiting(b.clone(), "airplay", &log), None),
        Some("")
    );
    assert_eq!(q.len(), 2, "both wait");
    assert!(log.borrow().is_empty());
    let (all, n) = q.allow();
    assert_eq!(q.len(), 0);
    for w in all {
        assert_eq!(send(&mut q, &sys, w, Some(("right", n))), None);
    }
    assert_eq!(q.checking(), 0);
    assert_eq!(*log.borrow(), [("remote", Ok(())), ("airplay", Ok(()))]);
    let sent = sys.sent.lock().unwrap();
    assert_eq!(
        sent[2..],
        [(a, Some("right".into())), (b, Some("right".into()))]
    );
}

/// Cancel flips every waiting switch back, not just the last one.
#[test]
fn cancel_fails_every_waiting_change() {
    let (sys, log, mut q) = (Asking::default(), Log::default(), AuthQueue::default());
    let [a, b] = toggles();
    send(&mut q, &sys, waiting(a, "remote", &log), None);
    send(&mut q, &sys, waiting(b, "airplay", &log), None);
    // What Ctx::cancel does with them (QA-HUNT-1: Cancelled, never Denied).
    cancelled(q.cancel());
    assert_eq!(q.len(), 0);
    assert_eq!(
        *log.borrow(),
        [
            ("remote", Err(ActionError::Cancelled)),
            ("airplay", Err(ActionError::Cancelled))
        ]
    );
}

/// QA 0.12.0 bug 4: the sheet asked forever. A wrong password asks again with its sentence
/// (once per Allow, however many changes it re-ran); the third gives up and fails them all.
#[test]
fn three_wrong_passwords_end_the_sheet() {
    let (sys, log, mut q) = (Asking::default(), Log::default(), AuthQueue::default());
    let [a, b] = toggles();
    send(&mut q, &sys, waiting(a, "remote", &log), None);
    send(&mut q, &sys, waiting(b, "airplay", &log), None);
    for round in 1..=3 {
        let (all, n) = q.allow();
        let said: Vec<_> = all
            .into_iter()
            .map(|w| send(&mut q, &sys, w, Some(("wrong", n))))
            .collect();
        if round < 3 {
            assert_eq!(said, [Some(ActionError::Denied.message()); 2], "{round}");
            assert_eq!(q.len(), 2);
        } else {
            // The second retry of that Allow was still out: it fails too, no sheet.
            assert_eq!(said, [Some("gave up"), None]);
        }
    }
    assert_eq!(q.len(), 0);
    assert_eq!(log.borrow().len(), 2, "{:?}", log.borrow());
    assert!(
        log.borrow()
            .iter()
            .all(|(_, r)| *r == Err(ActionError::Denied))
    );
}

/// wold's refusal never opens the sheet: the change ends with its own sentence.
#[test]
fn a_refusal_ends_the_change_without_the_sheet() {
    let (sys, log, mut q) = (Asking::default(), Log::default(), AuthQueue::default());
    assert_eq!(
        send(
            &mut q,
            &sys,
            waiting(Action::PkgInstall("vim".into()), "vim", &log),
            None
        ),
        None
    );
    assert_eq!(q.len(), 0);
    assert_eq!(*log.borrow(), [("vim", Err(ActionError::Refused))]);
    assert!(!ActionError::Refused.message().contains("password."));
}

/// QA 0.12.0 bug 9: the toast's Undo reverted the newest entry of any topic. It reverts the
/// entry the change recorded; a save made in `wol_core` resolves to the newest of its topic.
#[test]
fn the_undo_toast_targets_the_changes_own_entry() {
    use wol_core::journal::Entry;
    let e = |id, what: &str, by: &str, undoes| Entry {
        id,
        t: 0,
        by: by.into(),
        what: what.into(),
        changes: vec![],
        undoes,
    };
    let all = [
        e(1, "desktop", "settings", None),
        e(2, "sound", "settings", None),
        e(3, "desktop", "cli", None),
        e(4, "desktop", "settings", Some(1)),
    ];
    assert_eq!(latest_entry(&all, "desktop"), Some(1));
    assert_eq!(latest_entry(&all, "sound"), Some(2));
    assert_eq!(latest_entry(&all, "theme"), None);
}

#[test]
fn pages_list_the_sidebar_once_each() {
    let side: Vec<&str> = SIDEBAR
        .iter()
        .flat_map(|g| g.iter().map(|p| p.title))
        .collect();
    assert_eq!(side, PAGES, "PAGES follows the sidebar");
    let mut uniq = PAGES.to_vec();
    uniq.sort();
    uniq.dedup();
    assert_eq!(uniq.len(), PAGES.len(), "each pane once");
    for s in SUBPAGES {
        assert!(!PAGES.contains(&s), "{s} is a subpage");
    }
}

#[test]
fn search_matches_titles_and_words() {
    for (q, want) in [
        ("blur", "Effects & Looks"),
        ("wifi", "Wi-Fi"),
        ("zsh", "Users & Groups"),
        ("time zone", "General"),
        ("pwsh", "General"),
        ("tile", "Desktop & Dock"),
        ("firewall", "Network"),
        ("UNDO", "Restore"),
        ("driver", "Hardware"),
    ] {
        assert!(search(q).contains(&want), "{q}: {:?}", search(q));
    }
    assert!(search("no such thing").is_empty());
    assert_eq!(
        search("").len(),
        PAGES.len(),
        "empty query lists every pane"
    );
    assert!(search("balance").contains(&"Sound"), "a row finds its pane");
    assert!(
        search("secure boot").contains(&"Privacy & Security"),
        "a subpage row finds the pane it is reached from"
    );
}

#[test]
fn old_names_still_open() {
    assert_eq!(canonical("lock & idle"), "Lock Screen");
    assert_eq!(canonical("Accounts"), "Users & Groups");
    assert_eq!(canonical("Wi-Fi"), "Wi-Fi");
}

/// Live: `wol-settings Wi-Fi` while Settings ran opened the first launch's page (the primary
/// instance read its own argv). The page comes from the launch's own arguments, under
/// today's name, and every alias lands on a page Settings has.
#[test]
fn a_launch_names_its_page_under_todays_name() {
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        requested_page(&v(&["wol-settings", "Lock & Idle"])),
        Some("Lock Screen")
    );
    assert_eq!(
        requested_page(&v(&["wol-settings", "Wi-Fi"])),
        Some("Wi-Fi")
    );
    assert_eq!(
        requested_page(&v(&["wol-settings", "About"])),
        Some("About")
    );
    assert_eq!(requested_page(&v(&["wol-settings"])), None);
    assert_eq!(requested_page(&[]), None);
    for (old, new) in ALIASES {
        assert!(
            PAGES.contains(&new) || SUBPAGES.contains(&new),
            "{old} -> {new}: not a page"
        );
    }
}
