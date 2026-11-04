// SPDX-License-Identifier: GPL-2.0-or-later
//! Software Update: what the daily check found (base system fixes, package updates, the ones that
//! fix security problems), Check Now, and Update Now into a new boot environment.
use super::*;
use wol_core::update::check::Updates;

// ---- Software Update ----

#[derive(Clone, Debug, PartialEq)]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    Available(Vec<String>),
    Installing,
    Installed,
    Failed(String),
}

/// Under "N updates available": the daily check's note, then what Update Now keeps. A computer
/// with one copy of WolOS has no restore point, and the page says so before the button is used.
pub fn before_update(note: Option<&str>, one_copy: bool) -> String {
    let keeps = if one_copy {
        crate::copy::settings::UPDATE_ONE_COPY
    } else {
        crate::copy::UPDATE_RESTORE_POINT
    };
    match note {
        Some(n) if one_copy => format!("{n} {keeps}"),
        Some(n) => format!("{n} WolOS saves a restore point first."),
        None => keeps.into(),
    }
}

/// Update Now failed: wol-admind's own sentence when it gave one (the restore point it kept, or
/// why none could be kept), else the Store's words for `e`.
pub fn update_failed(e: ActionError, said: Option<String>) -> String {
    match (e, said) {
        (ActionError::Failed, Some(m)) => m,
        (e, _) => crate::copy::update(e).into(),
    }
}

/// `wol update check --json` stdout -> state.
pub fn parse_check(out: Option<&str>) -> UpdateState {
    match out.and_then(|o| serde_json::from_str::<Vec<String>>(o.trim()).ok()) {
        Some(v) if v.is_empty() => UpdateState::UpToDate,
        Some(v) => UpdateState::Available(v),
        None => UpdateState::Failed(crate::copy::UPDATE_CHECK.into()),
    }
}

/// Free bytes on the disk `wol update check` measures. Blocking: off the GTK thread.
pub fn check_disk_free() -> Option<u64> {
    wol_core::system::run_with_timeout(
        &shared::update::FREE_ARGV,
        std::time::Duration::from_secs(5),
    )
    .and_then(|o| shared::update::free_from_df(&o))
}

/// [`parse_check`] for a check that got no answer on a computer whose check disk (`/var`, the
/// one `wol update check` measures, `shared::update`) has `free` bytes left: a full disk is said
/// (QA-FDAY F10), "are you online?" is not asked of it.
pub fn check_result(out: Option<&str>, free: Option<u64>) -> UpdateState {
    match (parse_check(out), free) {
        (UpdateState::Failed(_), free) if shared::update::no_room(free) => {
            UpdateState::Failed(crate::copy::settings::UPDATE_NO_ROOM.into())
        }
        (s, _) => s,
    }
}

/// What a finished check says about when: an answer was just got, a failure says nothing.
pub fn checked_note(s: &UpdateState) -> Option<String> {
    matches!(s, UpdateState::UpToDate | UpdateState::Available(_))
        .then(|| crate::copy::settings::UPDATE_CHECKED_NOW.to_string())
}

/// What the download will be, from `pacman -Sup --print-format %s` (bytes per package).
pub const SIZE_ARGV: [&str; 4] = ["pacman", "-Sup", "--print-format", "%s"];

/// The bytes of the downloads `pacman -Sup --print-format %s` lists (QA-ACC-19). None when
/// it said nothing usable.
pub fn download_bytes(out: &str) -> Option<u64> {
    let sizes: Vec<u64> = out
        .lines()
        .filter_map(|l| l.trim().parse::<u64>().ok())
        .collect();
    (!sizes.is_empty()).then(|| sizes.iter().sum())
}

/// The size `pacman -Sup` gave for `asked` is shown only while the page is still in that state:
/// not after Update Now or Check Now was clicked meanwhile.
pub fn weigh_applies(now: &UpdateState, asked: &UpdateState) -> bool {
    matches!(now, UpdateState::Available(_)) && now == asked
}

/// "3 updates available" with what they weigh when known: "3 updates, 312 MB".
pub fn available_title(n: usize, bytes: Option<u64>) -> String {
    match bytes.filter(|b| *b > 0) {
        Some(b) => format!(
            "{n} update{}, {}",
            if n == 1 { "" } else { "s" },
            wol_core::storage::human(b)
        ),
        None => crate::copy::updates_available(n),
    }
}

/// The page's title and the line under it for `s`: what the last check said, and when.
pub fn headline(
    s: &UpdateState,
    note: Option<&str>,
    one_copy: bool,
    bytes: Option<u64>,
) -> (String, String) {
    match s {
        UpdateState::Idle => (
            "Software Update".into(),
            // A check that finished leaves its time in `note`; none has finished here.
            match note {
                Some(n) => format!("Check for new WolOS packages. {n}"),
                None => crate::copy::settings::UPDATE_NEVER_CHECKED.into(),
            },
        ),
        UpdateState::Checking => ("Checking for updates…".into(), String::new()),
        UpdateState::UpToDate => (
            "WolOS is up to date".into(),
            match note {
                Some(n) => format!("All packages are current. {n}"),
                None => "All packages are current.".into(),
            },
        ),
        UpdateState::Available(v) => (
            available_title(v.len(), bytes),
            before_update(note, one_copy),
        ),
        UpdateState::Installing => (
            "Installing updates…".into(),
            "You can keep working while this runs.".into(),
        ),
        UpdateState::Installed => (
            "Updates installed".into(),
            "Restart to finish updating.".into(),
        ),
        UpdateState::Failed(e) => ("Software Update".into(), e.clone()),
    }
}

/// Adds Flathub apps with an update waiting (`flatpak remote-ls --updates`) to a check result.
pub fn with_flatpaks(s: UpdateState, ids: &[String]) -> UpdateState {
    let names = ids.iter().map(|id| format!("{id} (Flathub)"));
    match s {
        UpdateState::UpToDate if !ids.is_empty() => UpdateState::Available(names.collect()),
        UpdateState::Available(mut v) => {
            v.extend(names);
            UpdateState::Available(v)
        }
        other => other,
    }
}

/// The daily check's findings -> the state the page opens in: never checked asks for a check.
pub fn from_snapshot(u: Option<&Updates>) -> UpdateState {
    match u {
        None => UpdateState::Idle,
        Some(u) if u.count() == 0 => UpdateState::UpToDate,
        Some(u) => UpdateState::Available(
            u.base
                .iter()
                .map(|b| format!("WolOS {b}"))
                .chain(u.packages.iter().cloned())
                .collect(),
        ),
    }
}

/// "WolOS 2026.11, firefox and curl", at most `max` names then "and 3 more" (the title
/// already says how many).
pub fn names_line(names: &[String], max: usize) -> String {
    let shown: Vec<&str> = names.iter().take(max).map(String::as_str).collect();
    match (names.len().saturating_sub(max), shown.as_slice()) {
        (0, [one]) => (*one).to_string(),
        (0, [rest @ .., last]) => format!("{} and {last}", rest.join(", ")),
        (more, _) => format!("{} and {more} more", shown.join(", ")),
    }
}

/// "1 update fixes a security problem. Last checked 3 h ago."
pub fn snapshot_line(u: &Updates, now: u64) -> String {
    let fixes = match u.security.len() {
        0 => String::new(),
        1 => "1 update fixes a security problem. ".into(),
        n => format!("{n} updates fix security problems. "),
    };
    let when = super::restore::ago(now, u.checked).to_lowercase();
    format!("{fixes}Last checked {when}.")
}

/// "Automatic updates" is on while either half is; turning it on or off sets both.
pub fn auto_main(download: bool, apps: bool) -> bool {
    download || apps
}

/// Automatic updates: the main switch and its two options (shown while it is on). What is on now
/// (`systemctl --user is-enabled`, `Machine::auto_updates`) is read off the GTK thread.
fn automatic(e: &Rc<Env>, into: &gtk::Box) {
    let holder = w::vbox(0);
    into.append(&holder);
    let (m, e) = (e.machine(), e.clone());
    later(
        &holder,
        move || {
            let (download, apps) = m.auto_updates();
            let file = m.auto_update_file();
            let metered = wol_core::update::auto::metered_allowed(file.as_deref());
            (download, apps, metered)
        },
        move |into, (download, apps, metered)| automatic_rows(&e, into, download, apps, metered),
    );
}

/// "Download on metered connections" is shown under a download switch that is there and on,
/// while automatic updates are on: it only says when the download may run.
pub fn metered_shown(can_download: bool, main_on: bool, download_on: bool) -> bool {
    can_download && main_on && download_on
}

/// The option rows stay while the main switch is on, while an option is on and while one waits
/// for its change: Cancel on the Authenticate sheet must find them where they were (QA-LIVE-42).
pub fn options_shown(main_on: bool, any_option_on: bool, any_waiting: bool) -> bool {
    main_on || any_option_on || any_waiting
}

/// What the main switch (now `on`) asks of an option: `Some(on)` when the option is shown
/// (`usable`), idle and not there yet; `None` for one that waits for its change or already
/// matches (a second `set_active` on a waiting switch starts a second change).
fn drive(on: bool, usable: bool, active: bool, idle: bool) -> Option<bool> {
    (usable && idle && active != on).then_some(on)
}

/// The switches of [`automatic`]. Each option waits for its change and flips back, with the
/// reason under the section, when it fails (the Authenticate sheet cancelled, wol-admind refused,
/// the user timer could not be changed); the main switch follows them. Without the
/// `auto-update` verb in this build the download option is not shown.
fn automatic_rows(e: &Rc<Env>, into: &gtk::Box, download: bool, apps: bool, metered: bool) {
    let status = status_label();
    let can_download = admin("auto-update", &["on"]).is_some();
    let download = download && can_download;
    let dl = {
        let (ctx, st) = (e.ctx.clone(), status.clone());
        w::pending_toggle(download, move |on, done| {
            match admin("auto-update", &[if on { "on" } else { "off" }]) {
                Some(a) => ctx.act_switch(a, &st, done),
                None => done(false),
            }
        })
    };
    // `auto-update metered on|off`: the timer also downloads on a metered connection (B6).
    let met = {
        let (ctx, st) = (e.ctx.clone(), status.clone());
        w::pending_toggle(metered, move |on, done| {
            match admin("auto-update", &["metered", if on { "on" } else { "off" }]) {
                Some(a) => ctx.act_switch(a, &st, done),
                None => done(false),
            }
        })
    };
    let ap = {
        let st = status.clone();
        w::pending_toggle(apps, move |on, done| {
            let st = st.clone();
            off_thread(
                move || {
                    wol_core::system::run_with_timeout(
                        &wol_core::update::auto::apps_argv(on),
                        std::time::Duration::from_secs(20),
                    )
                    .is_some()
                },
                move |ok| {
                    say(
                        &st,
                        if ok {
                            ""
                        } else {
                            crate::copy::settings::AUTO_APPS_FAILED
                        },
                    );
                    done(ok)
                },
            );
        })
    };
    // Shows what the options are; turning it on or off sets both, their handlers apply it.
    let main = w::toggle(auto_main(download, apps), |_| {});
    let dl_row = form::row(
        "Download new updates",
        "In the background. You choose when to install.",
        &dl,
    );
    let met_row = form::row(
        "Download on metered connections",
        crate::copy::settings::AUTO_METERED_HINT,
        &met,
    );
    let ap_row = form::row("Install app updates", "Flathub apps, once a day.", &ap);
    dl_row.set_visible(can_download && main.is_active());
    met_row.set_visible(metered_shown(
        can_download,
        main.is_active(),
        dl.is_active(),
    ));
    ap_row.set_visible(main.is_active());
    // Set while the main switch follows an option that went back (a cancelled change): it must
    // not drive the other option on the way.
    let following = Rc::new(std::cell::Cell::new(false));
    // Shows the option rows as `options_shown` says; called after every change of a switch.
    let show: Rc<dyn Fn()> = {
        let (main, dl, ap) = (main.clone(), dl.clone(), ap.clone());
        let (dl_row, met_row, ap_row) = (dl_row.clone(), met_row.clone(), ap_row.clone());
        Rc::new(move || {
            let waiting = !dl.is_sensitive() || !ap.is_sensitive();
            let shown = options_shown(
                main.is_active(),
                (can_download && dl.is_active()) || ap.is_active(),
                waiting,
            );
            dl_row.set_visible(can_download && shown);
            met_row.set_visible(metered_shown(can_download, shown, dl.is_active()));
            ap_row.set_visible(shown);
        })
    };
    {
        let (dl, ap, show, following) = (dl.clone(), ap.clone(), show.clone(), following.clone());
        main.connect_active_notify(move |m| {
            let on = m.is_active();
            // Only an option that is idle and differs is driven: one that waits for its change
            // would run a second one on top of the first (audit6 S6-3).
            if !following.get() {
                for (o, usable) in [(&dl, can_download), (&ap, true)] {
                    if let Some(want) = drive(on, usable, o.is_active(), o.is_sensitive()) {
                        o.set_active(want);
                    }
                }
            }
            show();
        });
    }
    // The main switch waits while an option waits for its change: a second answer to either
    // would race the first (audit6 S6-3).
    for s in [&dl, &ap] {
        let (main, dl, ap) = (main.clone(), dl.clone(), ap.clone());
        s.connect_sensitive_notify(move |_| {
            main.set_sensitive(dl.is_sensitive() && ap.is_sensitive())
        });
    }
    // The main switch follows the options once they settle: off when neither is on (turned off,
    // or a change that failed), back on when a cancelled change put an option back.
    for s in [&dl, &ap] {
        let (main, dl, ap, show, following) = (
            main.clone(),
            dl.clone(),
            ap.clone(),
            show.clone(),
            following.clone(),
        );
        let settle = Rc::new(move || {
            let any = auto_main(dl.is_active(), ap.is_active());
            if any != main.is_active() && dl.is_sensitive() && ap.is_sensitive() {
                following.set(true);
                main.set_active(any);
                following.set(false);
            }
            show();
        });
        let s2 = settle.clone();
        s.connect_active_notify(move |_| s2());
        s.connect_sensitive_notify(move |_| settle());
    }
    into.append(&w::section(
        "",
        &[
            form::row(
                "Automatic updates",
                "Keep WolOS and your apps current.",
                &main,
            ),
            dl_row,
            met_row,
            ap_row,
        ],
    ));
    into.append(&status);
}

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let ctx = &e.ctx;
    let p = general::subpage(e, "Software Update");
    let icon = w::icon("software-update-available-symbolic", w::ICON_M);
    icon.add_css_class("accent-icon");
    let title = w::label("", "title");
    title.set_wrap(true);
    let detail = w::label("", "secondary");
    detail.set_wrap(true);
    let text = w::vbox(0);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    text.append(&title);
    text.append(&detail);
    let spin = w::spinner();
    spin.set_spinning(false);
    spin.set_vexpand(false);
    let head = w::hbox(w::S3);
    head.add_css_class("list-row");
    head.add_css_class("hero-row");
    head.append(&icon);
    head.append(&text);
    head.append(&spin);
    let bar = w::progress();
    let progress_row = w::hbox(0);
    progress_row.add_css_class("list-row");
    progress_row.append(&bar);
    let names = w::label("", "secondary");
    names.set_wrap(true);
    names.set_wrap_mode(gtk::pango::WrapMode::Word);
    names.set_hexpand(true);
    let names_row = w::hbox(0);
    names_row.add_css_class("list-row");
    names_row.append(&names);
    p.append(&w::section(
        "",
        &[
            head.upcast(),
            progress_row.clone().upcast(),
            names_row.clone().upcast(),
        ],
    ));
    let buttons = w::hbox(w::S2);
    buttons.append(&w::spacer());
    let check = w::button("Check Now", w::Kind::Default, || {});
    let install = w::button("Update Now", w::Kind::Primary, || {});
    buttons.append(&check);
    buttons.append(&install);
    p.append(&buttons);
    automatic(e, &p);

    // What the daily check adds under the title (security fixes, when it ran); a fresh Check Now
    // clears it.
    let note: Rc<RefCell<Option<String>>> = Default::default();
    // What the waiting updates weigh, once `pacman -Sup` has said (QA-ACC-19).
    let size: Rc<std::cell::Cell<Option<u64>>> = Default::default();
    // The state the page shows now: an answer that comes late applies only to the state it asked about.
    let shown: Rc<RefCell<UpdateState>> = Rc::new(RefCell::new(UpdateState::Idle));
    let show: Rc<dyn Fn(&UpdateState)> = {
        let (title, detail, spin, progress_row, names_row, names, check, install, bar) = (
            title.clone(),
            detail.clone(),
            spin.clone(),
            progress_row.clone(),
            names_row.clone(),
            names.clone(),
            check.clone(),
            install.clone(),
            bar.clone(),
        );
        let pulse: Rc<RefCell<Option<glib::SourceId>>> = Default::default();
        let (note, size) = (note.clone(), size.clone());
        let one_copy = e.machine().single_copy();
        let shown = shown.clone();
        Rc::new(move |s: &UpdateState| {
            *shown.borrow_mut() = s.clone();
            let busy = matches!(s, UpdateState::Checking | UpdateState::Installing);
            spin.set_spinning(busy);
            spin.set_visible(busy);
            progress_row.set_visible(*s == UpdateState::Installing);
            if let Some(id) = pulse.borrow_mut().take() {
                id.remove();
            }
            if *s == UpdateState::Installing {
                let b = bar.clone();
                *pulse.borrow_mut() = Some(glib::timeout_add_local(
                    std::time::Duration::from_millis(120),
                    move || {
                        b.pulse();
                        glib::ControlFlow::Continue
                    },
                ));
            }
            check.set_sensitive(!busy);
            install.set_visible(matches!(s, UpdateState::Available(_)));
            names_row.set_visible(matches!(s, UpdateState::Available(_)));
            if let UpdateState::Available(v) = s {
                names.set_label(&names_line(v, 6));
            }
            let (t, d) = headline(s, note.borrow().as_deref(), one_copy, size.get());
            title.set_label(&t);
            detail.set_label(&d);
            detail.set_visible(!d.is_empty());
        })
    };
    show(&UpdateState::Idle);
    // The size of the waiting downloads, asked once the page shows updates.
    let weigh: Rc<dyn Fn(&UpdateState)> = {
        let (show, size, shown) = (show.clone(), size.clone(), shown.clone());
        Rc::new(move |s: &UpdateState| {
            if !matches!(s, UpdateState::Available(_)) {
                return;
            }
            let (show, size, shown, s) = (show.clone(), size.clone(), shown.clone(), s.clone());
            off_thread(
                || {
                    wol_core::system::run_with_timeout(
                        &SIZE_ARGV,
                        std::time::Duration::from_secs(30),
                    )
                    .and_then(|o| download_bytes(&o))
                },
                move |bytes| {
                    if bytes.is_some() && weigh_applies(&shown.borrow(), &s) {
                        size.set(bytes);
                        show(&s);
                    }
                },
            );
        })
    };
    {
        let (show, note, weigh, m) = (show.clone(), note.clone(), weigh.clone(), e.machine());
        off_thread(
            move || m.updates(),
            move |u| {
                *note.borrow_mut() = u
                    .as_ref()
                    .map(|u| snapshot_line(u, wol_core::journal::now()));
                let s = from_snapshot(u.as_ref());
                show(&s);
                weigh(&s);
            },
        );
    }
    {
        let (show, note, size, weigh) = (show.clone(), note.clone(), size.clone(), weigh.clone());
        check.connect_clicked(move |_| {
            *note.borrow_mut() = None;
            size.set(None);
            show(&UpdateState::Checking);
            let (tx, rx) = async_channel::bounded(1);
            std::thread::spawn(move || {
                // `CHECK_SECS` outwaits the refresh `wol update check` makes (QA-STORE-14).
                let out = wol_core::system::run_with_timeout(
                    &["wol", "update", "check", "--json"],
                    std::time::Duration::from_secs(wol_core::update::check::CHECK_SECS),
                );
                let free = out.is_none().then(check_disk_free).flatten();
                let _ = tx.send_blocking((out, free));
            });
            let (show, note, weigh) = (show.clone(), note.clone(), weigh.clone());
            glib::spawn_future_local(async move {
                if let Ok((out, free)) = rx.recv().await {
                    let s = check_result(out.as_deref(), free);
                    *note.borrow_mut() = checked_note(&s);
                    show(&s);
                    weigh(&s);
                }
            });
        });
    }
    {
        let (show, ctx) = (show.clone(), ctx.clone());
        install.connect_clicked(move |_| {
            show(&UpdateState::Installing);
            let show = show.clone();
            ctx.act(
                Action::PkgUpgrade,
                Rc::new(move |r| {
                    show(&match r {
                        Ok(()) => UpdateState::Installed,
                        Err(e) => UpdateState::Failed(update_failed(
                            e,
                            wol_core::controls::take_failure(
                                shared::admin::AdminAction::PkgUpgrade,
                            ),
                        )),
                    })
                }),
            );
        });
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    /// QA-LIVE-42: turning Automatic updates off removed the option rows before the Authenticate
    /// sheet was answered; Cancel then found them gone. They stay while an option is on or waits.
    #[test]
    fn subrows_stay_until_confirmed() {
        assert!(options_shown(true, false, false), "main on");
        assert!(options_shown(false, true, false), "an option still on");
        assert!(
            options_shown(false, false, true),
            "a change waits for the sheet"
        );
        assert!(!options_shown(false, false, false), "settled off");
    }

    /// B6 follow-up: the metered option sits under the download switch: shown only while the
    /// download is there and on and automatic updates are on.
    #[test]
    fn metered_option_follows_the_download_switch() {
        assert!(metered_shown(true, true, true));
        assert!(!metered_shown(true, true, false), "download off");
        assert!(!metered_shown(true, false, true), "automatic updates off");
        assert!(!metered_shown(false, true, true), "no auto-update verb");
    }

    #[test]
    fn flathub_updates_join_the_list() {
        let ids = vec!["org.kde.krita".to_string()];
        assert_eq!(
            with_flatpaks(UpdateState::UpToDate, &ids),
            UpdateState::Available(vec!["org.kde.krita (Flathub)".into()])
        );
        assert_eq!(
            with_flatpaks(UpdateState::Available(vec!["firefox".into()]), &ids),
            UpdateState::Available(vec!["firefox".into(), "org.kde.krita (Flathub)".into()])
        );
        assert_eq!(
            with_flatpaks(UpdateState::UpToDate, &[]),
            UpdateState::UpToDate
        );
        assert!(matches!(
            with_flatpaks(UpdateState::Failed("x".into()), &ids),
            UpdateState::Failed(_)
        ));
    }

    /// The page promises a restore point only where Update Now keeps one (ADR 0023: one copy).
    #[test]
    fn the_page_says_what_update_now_keeps() {
        assert_eq!(
            before_update(None, false),
            crate::copy::UPDATE_RESTORE_POINT
        );
        assert_eq!(
            before_update(Some("2 fix security problems."), false),
            "2 fix security problems. WolOS saves a restore point first."
        );
        assert_eq!(
            before_update(None, true),
            "This computer has one copy of WolOS, so there is no restore point; the update changes the running system."
        );
        assert_eq!(
            before_update(Some("Checked today."), true),
            format!("Checked today. {}", crate::copy::settings::UPDATE_ONE_COPY)
        );
    }

    /// wol-admind's sentence wins over the generic words; without one, nothing changes.
    #[test]
    fn a_failed_update_shows_what_wol_admind_said() {
        let said = Some("The update did not finish, so nothing was changed.".to_string());
        assert_eq!(
            update_failed(ActionError::Failed, said.clone()),
            "The update did not finish, so nothing was changed."
        );
        assert_eq!(
            update_failed(ActionError::Failed, None),
            crate::copy::update(ActionError::Failed)
        );
        assert_eq!(
            update_failed(ActionError::Denied, said),
            crate::copy::update(ActionError::Denied)
        );
    }

    #[test]
    fn automatic_updates_is_on_while_either_half_is() {
        assert!(auto_main(true, false));
        assert!(auto_main(false, true));
        assert!(!auto_main(false, false));
    }

    /// Audit6 S6-3: the main switch started a second change on an option that was still waiting
    /// for its first (a cancelled Authenticate then left the options and the main switch apart).
    #[test]
    fn the_main_switch_drives_only_idle_options_that_differ() {
        assert_eq!(drive(true, true, false, true), Some(true));
        assert_eq!(drive(false, true, true, true), Some(false));
        assert_eq!(drive(true, true, true, true), None, "already there");
        assert_eq!(
            drive(true, true, false, false),
            None,
            "still waiting for its change"
        );
        assert_eq!(
            drive(true, false, false, true),
            None,
            "not offered in this build"
        );
        // Both off; the main switch goes on: both options start their change and wait.
        let (mut dl, mut ap) = ((false, true), (false, true)); // (active, idle)
        for o in [&mut dl, &mut ap] {
            if let Some(w) = drive(true, true, o.0, o.1) {
                *o = (w, false);
            }
        }
        assert_eq!((dl, ap), ((true, false), (true, false)));
        // The Authenticate sheet is cancelled: `dl` flips back. `ap` is still waiting and on, so
        // the main switch stays on, and nothing re-drives `ap`.
        dl = (false, true);
        assert!(auto_main(dl.0, ap.0));
        assert_eq!(drive(true, true, ap.0, ap.1), None);
        // `ap` ends on: main on, `ap` on, `dl` off.
        ap = (true, true);
        assert!(auto_main(dl.0, ap.0) && ap.0 && !dl.0);
    }

    /// QA-FDAY-4: the page always says when it last looked, or that it has not.
    #[test]
    fn software_update_idle_last_checked() {
        let (t, d) = headline(&UpdateState::Idle, None, false, None);
        assert_eq!(t, "Software Update");
        assert_eq!(d, "Check for new WolOS packages. It has not checked yet.");
        let u = Updates {
            checked: 10_000,
            ..Default::default()
        };
        let note = snapshot_line(&u, 10_000 + 120);
        let (t, d) = headline(&from_snapshot(Some(&u)), Some(&note), false, None);
        assert_eq!(
            (t.as_str(), d.as_str()),
            (
                "WolOS is up to date",
                "All packages are current. Last checked 2 min ago."
            )
        );
        // Check Now: the answer just got says so.
        let s = parse_check(Some("[]"));
        let note = checked_note(&s);
        assert_eq!(note.as_deref(), Some("Last checked just now."));
        assert_eq!(
            headline(&s, note.as_deref(), false, None).1,
            "All packages are current. Last checked just now."
        );
        assert_eq!(
            checked_note(&parse_check(None)),
            None,
            "a failed check has no time"
        );
        for sentence in [
            crate::copy::settings::UPDATE_NEVER_CHECKED,
            crate::copy::settings::UPDATE_CHECKED_NOW,
            crate::copy::settings::UPDATE_NO_ROOM,
        ] {
            assert!(!sentence.is_empty());
        }
    }

    /// A late size does not bring the update list back over an install or a new check.
    #[test]
    fn a_late_size_applies_only_to_the_state_it_asked_about() {
        let a = UpdateState::Available(vec!["curl".into()]);
        assert!(weigh_applies(&a, &a));
        for now in [
            UpdateState::Installing,
            UpdateState::Installed,
            UpdateState::Checking,
            UpdateState::UpToDate,
            UpdateState::Available(vec!["git".into()]),
            UpdateState::Failed("x".into()),
        ] {
            assert!(!weigh_applies(&now, &a), "{now:?}");
        }
    }

    /// QA-ACC-19: what the update weighs is in the title.
    #[test]
    fn update_title_names_the_download() {
        assert_eq!(download_bytes("1000000\n311400000\n"), Some(312_400_000));
        assert_eq!(download_bytes("error: nothing\n"), None);
        assert_eq!(download_bytes(""), None);
        assert_eq!(available_title(3, Some(312_400_000)), "3 updates, 312 MB");
        assert_eq!(available_title(1, Some(5_000_000)), "1 update, 5 MB");
        assert_eq!(available_title(2, None), "2 updates available");
        assert_eq!(available_title(2, Some(0)), "2 updates available");
        let v = vec!["curl".to_string(), "git".to_string()];
        assert_eq!(
            headline(&UpdateState::Available(v), None, false, Some(312_400_000)).0,
            "2 updates, 312 MB"
        );
    }

    /// QA-FDAY F10: a check that failed on a full disk says so; with room left it is the network.
    #[test]
    fn a_full_disk_is_not_blamed_on_the_network() {
        let full = check_result(None, Some(0));
        assert_eq!(
            full,
            UpdateState::Failed(crate::copy::settings::UPDATE_NO_ROOM.into())
        );
        assert_eq!(
            check_result(None, Some(40 << 30)),
            UpdateState::Failed(crate::copy::UPDATE_CHECK.into())
        );
        assert_eq!(
            check_result(None, None),
            UpdateState::Failed(crate::copy::UPDATE_CHECK.into()),
            "free space unknown: the old sentence"
        );
        assert_eq!(check_result(Some("[]"), Some(0)), UpdateState::UpToDate);
    }

    /// Live-4 F1: no sentence of ours blames the network for a failure it cannot name (Check Now
    /// outwaiting the refresh is `check.rs::check_now_outwaits_the_refresh`).
    #[test]
    fn update_failure_does_not_guess_the_network() {
        let said = crate::copy::update(ActionError::Failed);
        assert!(!said.to_lowercase().contains("online"), "{said}");
    }

    #[test]
    fn update_check_states() {
        assert_eq!(parse_check(Some("[]\n")), UpdateState::UpToDate);
        assert_eq!(
            parse_check(Some(r#"["curl","git"]"#)),
            UpdateState::Available(vec!["curl".into(), "git".into()])
        );
        assert!(matches!(parse_check(None), UpdateState::Failed(_)));
    }

    #[test]
    fn update_names_read_as_a_list() {
        let n = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(names_line(&n(&["curl"]), 6), "curl");
        assert_eq!(names_line(&n(&["a", "b", "c"]), 6), "a, b and c");
        assert_eq!(names_line(&n(&["a", "b", "c", "d"]), 2), "a, b and 2 more");
    }

    #[test]
    fn the_daily_check_opens_the_page() {
        assert_eq!(from_snapshot(None), UpdateState::Idle);
        let mut u = Updates {
            checked: 10_000,
            ..Default::default()
        };
        assert_eq!(from_snapshot(Some(&u)), UpdateState::UpToDate);
        assert_eq!(
            snapshot_line(&u, 10_000 + 3 * 3600),
            "Last checked 3 h ago."
        );
        u.packages = vec!["curl".into(), "git".into()];
        u.security = vec!["curl".into()];
        u.base = Some("2026.11".into());
        assert_eq!(
            from_snapshot(Some(&u)),
            UpdateState::Available(vec!["WolOS 2026.11".into(), "curl".into(), "git".into()])
        );
        assert_eq!(
            snapshot_line(&u, 10_300),
            "1 update fixes a security problem. Last checked 5 min ago."
        );
    }
}
