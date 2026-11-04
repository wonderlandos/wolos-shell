// SPDX-License-Identifier: GPL-2.0-or-later
//! Restore: recent settings changes with Undo (`wol_core::journal`, same as `wol undo`), and restore
//! points (the root slots, `@wolos-a` and `@wolos-b`) to start the system from an earlier state.
use super::*;
use wol_core::journal::{Entry, is_undone};

/// What a change was about, in words.
pub(crate) fn topic(what: &str) -> &str {
    match what {
        "theme" => "Appearance",
        "wallpaper" => "Wallpaper",
        "focus" => "Focus",
        "shell" => "Desktop & Dock",
        "desktop" => "Desktop, windows and effects",
        "keyboard" | "shortcuts" => "Keyboard shortcuts",
        "displays" => "Displays",
        "idle" => "Lock Screen",
        "helpers" => "Assistants & Automation",
        "files" => "Tidied Downloads",
        "widgets" => "Widgets",
        "sound" => "Sound",
        other => other,
    }
}

/// Who made it: "System Settings", "Control Center", "Terminal", "a script", "an assistant".
pub(crate) fn actor(by: &str) -> String {
    match by {
        "settings" => "System Settings".into(),
        "control-center" => "Control Center".into(),
        "cli" => "Terminal".into(),
        b if b.starts_with("script:") => format!("Script “{}”", &b[7..]),
        b if b.starts_with("mcp:") => format!("Assistant “{}”", &b[4..]),
        b => b.into(),
    }
}

/// "Just now", "5 min ago", "3 h ago", "2 days ago".
pub(crate) fn ago(now: u64, t: u64) -> String {
    match now.saturating_sub(t) {
        s if s < 60 => "Just now".into(),
        s if s < 3600 => format!("{} min ago", s / 60),
        s if s < 86_400 => format!("{} h ago", s / 3600),
        s if s < 2 * 86_400 => "Yesterday".into(),
        s => format!("{} days ago", s / 86_400),
    }
}

/// The first thing in the journal is the computer being set up: the account's first theme, written
/// by `wol theme set` from the first-boot setup (journal id 1, `by: cli`, no file before it). It
/// is not a change the person made and Undo would only delete the file (QA-SET-R2-10).
fn is_setup(all: &[Entry], e: &Entry) -> bool {
    e.by == "cli"
        && all.iter().all(|o| o.id >= e.id)
        && !e.changes.is_empty()
        && e.changes
            .iter()
            .all(|c| matches!(c, wol_core::journal::Change::File { before: None, .. }))
}

/// Changes worth listing: not undo entries themselves, not already undone, not the setup of the
/// computer; newest first, at most `n`.
pub(crate) fn listed(all: &[Entry], n: usize) -> Vec<&Entry> {
    all.iter()
        .filter(|e| e.undoes.is_none() && !is_undone(all, e.id) && !is_setup(all, e))
        .take(n)
        .collect()
}

/// A restore point's name in words (QA-SET-30). The slots are `wolos-a` and `wolos-b`: which one
/// is not for the person to learn, only what each is for. The running system; the update that
/// starts next time; the system as it was before the last update.
pub(crate) fn point_name(b: &wol_core::update::be::BootEnv) -> String {
    if b.active {
        "Current system".into()
    } else if b.next_boot {
        "Next start".into()
    } else if b.name.starts_with("wol-before-update-") || wol_core::update::is_valid_be(&b.name) {
        "Before the last update".into()
    } else {
        b.name.clone()
    }
}

/// "2 November 2026" from a slot's release date ("2026/11/02", "2026.11.02" or "2026-11-02"),
/// with ", 18:40" when the snapshot list gave a time ("2026-10-20 18:40").
fn when(created: &str) -> Option<String> {
    let (date, time) = match created.split_once(' ') {
        Some((d, t)) => (d, Some(t)),
        None => (created, None),
    };
    let mut d = date.split(['-', '/', '.']);
    let (y, m, day) = (d.next()?, d.next()?.parse::<usize>().ok()?, d.next()?);
    if d.next().is_some() || y.len() != 4 || !y.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let month = super::datetime::MONTHS.get(m.checked_sub(1)?)?;
    let day = day.trim_start_matches('0');
    if day.is_empty() || !day.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(match time {
        Some(t) => format!("{day} {month} {y}, {t}"),
        None => format!("{day} {month} {y}"),
    })
}

/// "20 October 2026, 18:40 · 412 MB" from the snapshot list's "2026-10-20 18:40" and "412M"; a
/// boot-menu slot gives "2 November 2026" and "2 tries left" (kept as it is: it says the slot has
/// not started well yet).
pub(crate) fn point_line(created: &str, space: &str) -> String {
    let when = when(created).unwrap_or_else(|| created.to_string());
    let size = match space.char_indices().last() {
        Some((i, u @ ('K' | 'M' | 'G' | 'T'))) => format!("{} {u}B", &space[..i]),
        _ => space.to_string(),
    };
    if size.is_empty() {
        when
    } else {
        format!("{when} · {size}")
    }
}

fn history(e: &Rc<Env>, into: &gtk::Box, status: &gtk::Label) {
    clear(into);
    let all = e.sys().history();
    let now = wol_core::journal::now();
    let rows: Vec<gtk::Widget> = listed(&all, 8)
        .into_iter()
        .map(|x| {
            let (env, h, st, id) = (e.clone(), into.clone(), status.clone(), x.id);
            let undo = w::button("Undo", w::Kind::Default, move || {
                // `undo` also reloads what the change touched (`system::after_undo`, per topic).
                match env.sys().undo(Some(id)) {
                    Ok(_) => {
                        say(&st, "");
                        // The other open pages show the restored files too, as after the toast's
                        // Undo; once this click is over, since this page is built again with them.
                        let env = Rc::downgrade(&env);
                        glib::idle_add_local_once(move || {
                            if let Some(env) = env.upgrade() {
                                env.refresh()
                            }
                        });
                    }
                    Err(m) => say(&st, &m),
                }
                history(&env, &h, &st);
            });
            w::list_row(
                "",
                topic(&x.what),
                &format!("{} · {}", actor(&x.by), ago(now, x.t)),
                Some(undo.upcast_ref()),
            )
            .upcast()
        })
        .collect();
    if rows.is_empty() {
        into.append(&w::section(
            "Recent Changes",
            &[w::list_row(
                "wolos-restore-symbolic",
                "No changes to undo",
                "Changes to settings show here, so you can take any of them back.",
                None,
            )
            .upcast()],
        ));
    } else {
        into.append(&w::section("Recent Changes", &rows));
    }
}

fn points(e: &Rc<Env>, into: &gtk::Box, status: &gtk::Label) {
    clear(into);
    let mut bes = e.sys().boot_environments();
    if bes.is_empty() {
        // One copy of WolOS says so, in the place the list would be (QA-STORE-5).
        if e.machine().single_copy() {
            into.append(&w::section(
                "Restore Points",
                &[w::list_row(
                    "drive-harddisk-symbolic",
                    "One copy of WolOS",
                    crate::copy::settings::RESTORE_ONE_COPY,
                    None,
                )
                .upcast()],
            ));
        }
        return;
    }
    bes.sort_by(|a, b| b.created.cmp(&a.created));
    let rows: Vec<gtk::Widget> = bes
        .iter()
        .map(|b| {
            let sub = point_line(&b.created, &b.space);
            let trailing: gtk::Widget = if b.active {
                w::label("Running now", "secondary").upcast()
            } else if b.next_boot {
                w::label("Starts next time", "secondary").upcast()
            } else {
                let (env, h, st, name) = (e.clone(), into.clone(), status.clone(), b.name.clone());
                w::button("Start From This", w::Kind::Default, move || {
                    let (env2, h2, st2) = (env.clone(), h.clone(), st.clone());
                    env.ctx.act(
                        Action::BootEnvironment(name.clone()),
                        Rc::new(move |r| match r {
                            Ok(()) => {
                                say(&st2, "Restart to use this restore point.");
                                points(&env2, &h2, &st2);
                            }
                            Err(x) => say(&st2, x.message()),
                        }),
                    );
                })
                .upcast()
            };
            w::list_row(
                "drive-harddisk-symbolic",
                &point_name(b),
                &sub,
                Some(&trailing),
            )
            .upcast()
        })
        .collect();
    into.append(&w::section("Restore Points", &rows));
    into.append(&w::footnote(
        "A restore point is made before every update. Starting from one keeps your files and brings back the system as it was.",
    ));
}

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let p = w::page("Restore");
    let status = status_label();
    let changes = w::vbox(0);
    history(e, &changes, &status);
    p.append(&changes);
    let bes = w::vbox(w::S6);
    points(e, &bes, &status);
    p.append(&bes);
    p.append(&status);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_read_as_sentences() {
        assert_eq!(topic("theme"), "Appearance");
        assert_eq!(actor("mcp:claude"), "Assistant “claude”");
        assert_eq!(actor("script:tidy"), "Script “tidy”");
        assert_eq!(ago(1000, 990), "Just now");
        assert_eq!(ago(10_000, 10_000 - 300), "5 min ago");
        assert_eq!(ago(100_000, 100_000 - 7200), "2 h ago");
        assert_eq!(ago(1_000_000, 1_000_000 - 3 * 86_400), "3 days ago");
        let e = |id, undoes| Entry {
            id,
            t: 0,
            by: "settings".into(),
            what: "theme".into(),
            changes: vec![],
            undoes,
        };
        let all = vec![e(3, Some(1)), e(2, None), e(1, None)];
        let ids: Vec<u64> = listed(&all, 8).iter().map(|x| x.id).collect();
        assert_eq!(
            ids,
            [2],
            "undo entries and undone changes are not offered again"
        );
    }

    /// QA-SET-R2-10: the theme written while the computer was set up is not a change to undo; a
    /// later one from the terminal still is.
    #[test]
    fn setup_is_not_a_change_to_undo() {
        use wol_core::journal::Change;
        let file = |id, by: &str, before: Option<&str>| Entry {
            id,
            t: id,
            by: by.into(),
            what: "theme".into(),
            changes: vec![Change::File {
                path: "wolos/theme".into(),
                before: before.map(String::from),
                after: Some("wol-light".into()),
            }],
            undoes: None,
        };
        let first = file(1, "cli", None);
        let all = vec![file(2, "cli", Some("wol-light")), first.clone()];
        assert_eq!(
            listed(&all, 8).iter().map(|x| x.id).collect::<Vec<_>>(),
            [2]
        );
        assert!(
            listed(&[first], 8).is_empty(),
            "nothing to undo after setup"
        );
        let by_settings = vec![file(1, "settings", None)];
        assert_eq!(
            listed(&by_settings, 8).len(),
            1,
            "the person's own first change"
        );
    }

    /// QA-SET-30: the names and dates of bootctl's slots, not ZFS-shaped ones.
    #[test]
    fn point_name_table() {
        let be = |name: &str, active, next_boot, created: &str, space: &str| {
            wol_core::update::be::BootEnv {
                name: name.into(),
                active,
                next_boot,
                mountpoint: None,
                space: space.into(),
                created: created.into(),
            }
        };
        for (b, name, line) in [
            (
                be("wolos-a", true, false, "2026/10/20", ""),
                "Current system",
                "20 October 2026",
            ),
            (
                be("wolos-b", false, true, "2026/11/02", "2 tries left"),
                "Next start",
                "2 November 2026 · 2 tries left",
            ),
            (
                be("wolos-b", false, false, "2026.11.02", ""),
                "Before the last update",
                "2 November 2026",
            ),
            (
                be("wolos-a", false, false, "", ""),
                "Before the last update",
                "",
            ),
        ] {
            assert_eq!(point_name(&b), name, "{b:?}");
            assert_eq!(point_line(&b.created, &b.space), line, "{b:?}");
        }
        assert_eq!(
            point_line("2026-10-20 18:40", "412M"),
            "20 October 2026, 18:40 · 412 MB"
        );
        assert_eq!(
            point_line("2026/13/02", ""),
            "2026/13/02",
            "not a date: shown as it is"
        );
    }

    /// QA-STORE-5: Restore lists what `System::boot_environments` reads, which drops a boot entry
    /// whose root subvolume is gone; the page then names the slots that are really there.
    #[test]
    fn ghost_slot_filtered() {
        let h = wol_core::system::fake::FakeHost::new(false);
        h.command(
            "bootctl list --json=short",
            r#"[{"id":"wolos-a.conf","version":"2026/10/20","isSelected":true,"isDefault":true},
                {"id":"wolos-b+3.conf","version":"2026/11/02","isSelected":false,"isDefault":false,"triesLeft":3}]"#,
        );
        h.command("ls /run/wolos/top", "@wolos-a\n");
        let names: Vec<String> = wol_core::update::be::read(&h)
            .unwrap()
            .iter()
            .map(point_name)
            .collect();
        assert_eq!(
            names,
            ["Current system"],
            "the half-made wolos-b is not a restore point"
        );
    }

    #[test]
    fn restore_points_read_as_words() {
        let be = |name: &str, active| wol_core::update::be::BootEnv {
            name: name.into(),
            active,
            next_boot: active,
            mountpoint: None,
            space: "412M".into(),
            created: "2026-10-02 18:40".into(),
        };
        assert_eq!(point_name(&be("default", true)), "Current system");
        assert_eq!(
            point_name(&be("wol-before-update-1792", false)),
            "Before the last update"
        );
        assert_eq!(point_name(&be("my-test", false)), "my-test");
        assert_eq!(
            point_line("2026-10-02 18:40", "412M"),
            "2 October 2026, 18:40 · 412 MB"
        );
        assert_eq!(point_line("odd", "9.8G"), "odd · 9.8 GB");
        assert_eq!(point_line("odd", ""), "odd");
    }
}
