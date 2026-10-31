//! Memory Saver (Battery / Energy): apps you are not using give their memory back and get it back
//! the moment you return; per-app exclusions; App Nap (off by default). The user's choices go to
//! `wol_core::memsaver::Prefs` (wol-realtime forwards them to wold); what wold is doing comes from
//! its status file.
use super::*;
use shared::memsaver::{AppState, AppStatus, Status};
use wol_core::memsaver::{self as ms, Prefs};

/// What Memory Saver does, in plain words, with the memory it is saving right now.
pub(crate) fn summary(enabled: bool, st: Option<&Status>) -> String {
    const WHAT: &str =
        "Apps you aren't using give their memory back and get it back the moment you return.";
    match st.filter(|s| enabled && s.enabled && s.saved > 0) {
        Some(s) => format!("Saving {} now. {WHAT}", ms::size(s.saved)),
        None => WHAT.to_string(),
    }
}

/// One app's line under its name.
pub(crate) fn app_line(a: &AppStatus) -> String {
    match a.state {
        AppState::Active => "In use".into(),
        AppState::Waiting => "In the background".into(),
        AppState::Saving if a.saved > 0 => format!("Gave back {}", ms::size(a.saved)),
        AppState::Saving => "In the background".into(),
        AppState::Napping => format!("Paused · gave back {}", ms::size(a.saved)),
        AppState::Excluded => "Always keeps its memory".into(),
        AppState::Playing => "Playing sound, so it keeps its memory".into(),
        AppState::Game => "Games always keep their memory".into(),
    }
}

/// "discord" -> "Discord", "org.gnome.nautilus" -> "Nautilus".
pub(crate) fn app_title(id: &str) -> String {
    let last = id.rsplit('.').next().unwrap_or(id).replace(['_', '-'], " ");
    let last = last.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut c = last.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// Apps to list: those wold knows (running, with a window) plus excluded ones that are not running.
pub(crate) fn apps(st: Option<&Status>, prefs: &Prefs) -> Vec<(String, Option<AppStatus>)> {
    let mut out: Vec<(String, Option<AppStatus>)> = st
        .map(|s| {
            s.apps
                .iter()
                .map(|a| (a.app.clone(), Some(a.clone())))
                .collect()
        })
        .unwrap_or_default();
    for e in &prefs.exclude {
        if !out.iter().any(|(id, _)| id == e) {
            out.push((e.clone(), None));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.dedup_by(|a, b| a.0 == b.0);
    out
}

/// wold's status file (`WOL_MEMSAVER_STATUS` points elsewhere for demos and goldens).
fn status() -> Option<Status> {
    let path = std::env::var_os("WOL_MEMSAVER_STATUS")
        .map_or_else(|| PathBuf::from(shared::memsaver::STATUS), PathBuf::from);
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
}

fn home() -> PathBuf {
    PathBuf::from(kit::config_home())
}

fn change(f: impl FnOnce(&mut Prefs)) -> bool {
    let mut p = ms::load(&home());
    f(&mut p);
    let saved = ms::save(&home(), &p);
    if let Err(e) = &saved {
        eprintln!("wol: memory saver prefs: {e}");
    }
    saved.is_ok()
}

/// The Memory section.
pub(super) fn section() -> gtk::Box {
    let prefs = ms::load(&home());
    let st = status();
    let toggle = w::pending_toggle(prefs.enabled, |on, done| {
        done(change(|p| p.enabled = on));
    });
    let (main, line) = form::status_row(
        "",
        "Memory Saver",
        &summary(prefs.enabled, st.as_ref()),
        &toggle,
    );
    line.set_wrap(true);
    let mut rows = vec![main];
    let nap_toggle = w::pending_toggle(prefs.nap, |on, done| {
        done(change(|p| p.nap = on));
    });
    let (nap, nap_line) = form::status_row(
        "",
        "App Nap",
        "Pauses apps hidden for 10 minutes that aren't playing sound. They may show notifications late.",
        &nap_toggle,
    );
    nap_line.set_wrap(true);
    nap.set_sensitive(prefs.enabled);
    rows.push(nap.clone());
    let list = apps(st.as_ref(), &prefs);
    let per_app = w::vbox(0);
    if !list.is_empty() {
        let app_rows: Vec<gtk::Widget> = list
            .iter()
            .map(|(id, a)| {
                let sub = a
                    .as_ref()
                    .map(app_line)
                    .unwrap_or_else(|| "Not open".into());
                let id = id.clone();
                form::pending_toggle_row(
                    &app_title(&id),
                    &sub,
                    !prefs.exclude.contains(&id),
                    move |on, done| {
                        done(change(|p| {
                            p.set_excluded(&id, !on);
                        }))
                    },
                )
            })
            .collect();
        per_app.append(&w::section("Save Memory For", &app_rows));
        per_app.set_sensitive(prefs.enabled);
    }
    {
        let (line, nap, per_app) = (line.clone(), nap.clone(), per_app.clone());
        toggle.connect_active_notify(move |t| {
            let on = t.is_active();
            line.set_label(&summary(on, status().as_ref()));
            nap.set_sensitive(on);
            per_app.set_sensitive(on);
        });
    }
    let out = w::vbox(w::S6);
    out.append(&w::section("Memory", &rows));
    out.append(&per_app);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, state: AppState, saved: u64) -> AppStatus {
        AppStatus {
            app: id.into(),
            state,
            saved,
            memory: 0,
            swap: 0,
        }
    }

    #[test]
    fn the_summary_says_what_it_does_and_how_much_it_saves() {
        let st = Status {
            enabled: true,
            saved: 1_400_000_000,
            apps: vec![],
        };
        let s = summary(true, Some(&st));
        assert!(
            s.starts_with("Saving 1.4 GB now. Apps you aren't using"),
            "{s}"
        );
        assert!(
            !summary(false, Some(&st)).contains("Saving"),
            "off: no number"
        );
        assert!(!summary(true, None).contains("Saving"), "wold not running");
        assert!(summary(true, None).ends_with('.'));
    }

    #[test]
    fn every_state_reads_as_a_sentence_without_jargon() {
        use AppState::*;
        for s in [
            Active, Waiting, Saving, Napping, Excluded, Playing, Game,
        ] {
            let l = app_line(&app("x", s, 420_000_000));
            for jargon in ["cgroup", "zram", "swap", "reclaim", "memory.high"] {
                assert!(!l.contains(jargon), "{l}");
            }
            assert!(!l.is_empty());
        }
        assert_eq!(app_line(&app("x", Saving, 420_000_000)), "Gave back 420 MB");
        assert_eq!(app_line(&app("x", Saving, 0)), "In the background");
    }

    #[test]
    fn apps_list_running_apps_and_excluded_ones_once_each() {
        let st = Status {
            enabled: true,
            saved: 0,
            apps: vec![
                app("discord", AppState::Saving, 5),
                app("code", AppState::Active, 0),
            ],
        };
        let prefs = Prefs {
            exclude: vec!["spotify".into(), "code".into()],
            ..Prefs::default()
        };
        let l = apps(Some(&st), &prefs);
        let ids: Vec<&str> = l.iter().map(|x| x.0.as_str()).collect();
        assert_eq!(ids, vec!["code", "discord", "spotify"]);
        assert!(l[2].1.is_none(), "spotify is not open");
        assert_eq!(app_title("discord"), "Discord");
        assert_eq!(app_title("org.gnome.nautilus"), "Nautilus");
        assert_eq!(app_title("code_-_oss"), "Code oss");
    }
}
