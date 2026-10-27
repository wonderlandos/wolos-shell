//! Software Update: check and install pkg upgrades into a new boot environment.
use super::*;

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

pub(super) fn page(ctx: &Rc<Ctx>) -> gtk::Box {
    let p = w::page("Software Update");
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
    let spin = gtk::Spinner::new();
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
                    "All packages are current.".into(),
                ),
                UpdateState::Available(v) => {
                    names.set_label(&wol_core::update::summary(v, 6));
                    (
                        format!(
                            "{} update{} available",
                            v.len(),
                            if v.len() == 1 { "" } else { "s" }
                        ),
                        "WolOS saves a restore point before updating.".into(),
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
        let show = show.clone();
        check.connect_clicked(move |_| {
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
    fn update_check_states() {
        assert_eq!(parse_check(Some("[]\n")), UpdateState::UpToDate);
        assert_eq!(
            parse_check(Some(r#"["curl","git"]"#)),
            UpdateState::Available(vec!["curl".into(), "git".into()])
        );
        assert!(matches!(parse_check(None), UpdateState::Failed(_)));
    }
}
