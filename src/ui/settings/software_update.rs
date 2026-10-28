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

/// `wol update check --json` stdout -> state.
pub fn parse_check(out: Option<&str>) -> UpdateState {
    match out.and_then(|o| serde_json::from_str::<Vec<String>>(o.trim()).ok()) {
        Some(v) if v.is_empty() => UpdateState::UpToDate,
        Some(v) => UpdateState::Available(v),
        None => UpdateState::Failed(crate::copy::UPDATE_CHECK.into()),
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

/// Automatic updates: the main switch and its two options (shown while it is on).
fn automatic(e: &Rc<Env>, into: &gtk::Box) {
    let ctx = e.ctx.clone();
    let set_download: Rc<dyn Fn(bool)> = Rc::new(move |on| {
        if let Some(a) = admin("auto-update", &[if on { "on" } else { "off" }]) {
            ctx.act(a, Rc::new(|_| {}));
        }
    });
    let set_apps: Rc<dyn Fn(bool)> = Rc::new(|on| {
        std::thread::spawn(move || {
            wol_core::system::run_with_timeout(
                &wol_core::update::auto::apps_argv(on),
                std::time::Duration::from_secs(20),
            )
        });
    });
    let (download, apps) = e.machine().auto_updates();
    let main = w::toggle(auto_main(download, apps), |_| {});
    let dl = w::toggle(download, {
        let f = set_download.clone();
        move |on| f(on)
    });
    let ap = w::toggle(apps, {
        let f = set_apps.clone();
        move |on| f(on)
    });
    let dl_row = form::row(
        "Download new updates",
        "In the background. You choose when to install.",
        &dl,
    );
    let ap_row = form::row("Install app updates", "Flathub apps, once a day.", &ap);
    dl_row.set_visible(main.is_active());
    ap_row.set_visible(main.is_active());
    {
        let (dl, ap, dl_row, ap_row) = (dl.clone(), ap.clone(), dl_row.clone(), ap_row.clone());
        main.connect_active_notify(move |m| {
            let on = m.is_active();
            dl_row.set_visible(on);
            ap_row.set_visible(on);
            // Each switch's own handler applies the change.
            dl.set_active(on);
            ap.set_active(on);
        });
    }
    for s in [&dl, &ap] {
        let (main, dl, ap) = (main.clone(), dl.clone(), ap.clone());
        s.connect_active_notify(move |_| {
            if !auto_main(dl.is_active(), ap.is_active()) && main.is_active() {
                main.set_active(false);
            }
        });
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
            ap_row,
        ],
    ));
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
        let note = note.clone();
        Rc::new(move |s: &UpdateState| {
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
            let (t, d) = match s {
                UpdateState::Idle => (
                    "Software Update".to_string(),
                    "Check for new WolOS packages.".to_string(),
                ),
                UpdateState::Checking => ("Checking for updates…".into(), String::new()),
                UpdateState::UpToDate => (
                    "WolOS is up to date".into(),
                    match note.borrow().as_deref() {
                        Some(n) => format!("All packages are current. {n}"),
                        None => "All packages are current.".into(),
                    },
                ),
                UpdateState::Available(v) => {
                    names.set_label(&names_line(v, 6));
                    (
                        format!(
                            "{} update{} available",
                            v.len(),
                            if v.len() == 1 { "" } else { "s" }
                        ),
                        match note.borrow().as_deref() {
                            Some(n) => format!("{n} WolOS saves a restore point first."),
                            None => "WolOS saves a restore point before updating.".into(),
                        },
                    )
                }
                UpdateState::Installing => (
                    "Installing updates…".into(),
                    "You can keep working while this runs.".into(),
                ),
                UpdateState::Installed => (
                    "Updates installed".into(),
                    "Restart to finish updating.".into(),
                ),
                UpdateState::Failed(e) => ("Software Update".into(), e.clone()),
            };
            title.set_label(&t);
            detail.set_label(&d);
            detail.set_visible(!d.is_empty());
        })
    };
    show(&UpdateState::Idle);
    {
        let (show, note, m) = (show.clone(), note.clone(), e.machine());
        off_thread(
            move || m.updates(),
            move |u| {
                *note.borrow_mut() = u
                    .as_ref()
                    .map(|u| snapshot_line(u, wol_core::journal::now()));
                show(&from_snapshot(u.as_ref()));
            },
        );
    }
    {
        let (show, note) = (show.clone(), note.clone());
        check.connect_clicked(move |_| {
            *note.borrow_mut() = None;
            show(&UpdateState::Checking);
            let (tx, rx) = async_channel::bounded(1);
            std::thread::spawn(move || {
                let out = wol_core::system::run_with_timeout(
                    &["wol", "update", "check", "--json"],
                    std::time::Duration::from_secs(120),
                );
                let _ = tx.send_blocking(out);
            });
            let show = show.clone();
            glib::spawn_future_local(async move {
                if let Ok(out) = rx.recv().await {
                    show(&parse_check(out.as_deref()));
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
                        Err(e) => UpdateState::Failed(crate::copy::update(e).into()),
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

    #[test]
    fn automatic_updates_is_on_while_either_half_is() {
        assert!(auto_main(true, false));
        assert!(auto_main(false, true));
        assert!(!auto_main(false, false));
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
