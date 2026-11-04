// SPDX-License-Identifier: GPL-2.0-or-later
//! Search: installed apps, Settings pages and file names under `$HOME`, ranked per keystroke
//! from prebuilt lowercase indexes.
use crate::files;
use crate::kit::{
    self,
    surface::{Keyboard, Spec},
    widgets as w,
};
use crate::search::{self, Item};
use gtk::{gdk, glib, prelude::*};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Kind shown on a Settings result.
pub const SETTINGS: &str = "System Settings";
/// Kind shown on a row an app provided, and what opens one.
pub const PROVIDED: &str = "From an app";
const OPEN_ROW: &str = "wol-open-provided-row";
/// Results shown at most; apps and Settings keep at most [`APPS`] of them when files match.
pub const ROWS: usize = 8;
/// Characters of a query that count for the meaning providers (as Settings search caps it).
const QUERY_MAX: usize = 64;
pub const APPS: usize = 5;

pub fn index(apps: Vec<crate::desktop::Entry>) -> Vec<Item> {
    // Pinned apps come from PINS below (WolOS name, icon, launcher), never twice.
    let mut v: Vec<Item> = apps
        .into_iter()
        .filter(|e| crate::apps::pin_for_entry(e, crate::apps::here()).is_none())
        .map(|e| {
            Item::new(&e.name, "Application", &e.icon, e.exec)
                .with_app(crate::apps::desktop_stem(&e.id))
        })
        .collect();
    for p in crate::apps::here() {
        if !v.iter().any(|i| i.title == p.name) {
            // Found by the real name too: Helium, Geary, Nautilus (QA-LN-20).
            v.push(
                Item::new(
                    p.name,
                    "Application",
                    p.icon,
                    p.exec.iter().map(|s| s.to_string()).collect(),
                )
                .with_words(&crate::apps::product_words(p))
                .with_app(p.id),
            );
        }
    }
    v.extend(actions(crate::apps::is_live()));
    // Every Settings page under the name Settings opens; a pane's search words find it too.
    let open = |t: &str| vec!["wol-settings".into(), t.into()];
    for p in crate::ui::settings::SIDEBAR.iter().flat_map(|g| g.iter()) {
        v.push(Item::new(p.title, SETTINGS, "wolos-settings", open(p.title)).with_words(p.words));
    }
    for t in crate::ui::settings::SUBPAGES {
        v.push(Item::new(t, SETTINGS, "wolos-settings", open(t)));
    }
    v
}

/// The keyboard mode Search's surface is created with. Hyprland 0.56 hands the keyboard to a layer
/// surface only on a change from `none` to `exclusive`; created Exclusive, a click on the menu bar's
/// Search icon over a focused app left the keystrokes with the window underneath, and the second
/// click closed it again (QA-LIVE1-11). Created with none, then switched on once it is on screen,
/// it is the same change `^F2` makes for the menu bar.
pub const FIRST_KEYBOARD: Keyboard = Keyboard::None;
/// Wait after the first frame before the keyboard is taken, so the compositor has seen the surface
/// without it.
pub const KEYBOARD_AFTER_MS: u64 = 30;
/// Longest a warm Search may take from the press to its first map (the budget the journey
/// `qa100-ln-launcher-toggle` measures).
pub const WARM_SHOW_BUDGET_MS: u64 = 150;

fn take_keyboard_when_mapped(win: &gtk::ApplicationWindow) {
    win.connect_map(|w| {
        let w = w.clone();
        glib::timeout_add_local_once(
            std::time::Duration::from_millis(KEYBOARD_AFTER_MS),
            move || {
                crate::kit::surface::set_keyboard(&w, Keyboard::Exclusive);
            },
        );
    });
}

/// Kind shown on a system action.
pub const ACTION: &str = "Action";

/// System actions Search offers by name (QA-LN-18: 'lock', 'shut down', 'log out', 'sleep' and
/// 'screenshot' gave Settings pages or nothing). Each runs the command the menu runs; the three
/// that end the session open its question first. Live has no Lock (nothing unlocks it).
pub fn actions(live: bool) -> Vec<Item> {
    let row = |title: &str, words: &str, icon: &str, argv: &[&str]| {
        Item::new(
            title,
            ACTION,
            icon,
            argv.iter().map(|s| s.to_string()).collect(),
        )
        .with_words(words)
    };
    let show = |t: &'static str| ["wol", "shell", "show", t];
    let mut v = Vec::new();
    if !live {
        v.push(row(
            "Lock Screen",
            "lock",
            "system-lock-screen-symbolic",
            &["wol-lock"],
        ));
    }
    v.extend([
        row(
            "Sleep",
            "suspend",
            "weather-clear-night-symbolic",
            &["wol", "power", "sleep"],
        ),
        row(
            "Restart",
            "reboot",
            "system-reboot-symbolic",
            &show("session-restart"),
        ),
        row(
            "Shut Down",
            "power off poweroff",
            "system-shutdown-symbolic",
            &show("session-shutdown"),
        ),
        row(
            "Log Out",
            "logout sign out",
            "system-log-out-symbolic",
            &show("session-logout"),
        ),
        row(
            "Take Screenshot",
            "screenshot capture screen grab",
            "wolos-photos",
            &show("screenshot"),
        ),
        row(
            "Record Screen",
            "screen recording record video",
            "media-record-symbolic",
            &show("screen-record"),
        ),
    ]);
    v
}

/// Seconds between two listings of `$HOME` by the shell (Search also lists it when stale).
const RELIST_SECS: u32 = 3600;

/// Keystrokes closer than this are one question to the apps (a worker thread per keystroke before).
pub const PROVIDER_DEBOUNCE_MS: u64 = 80;

/// Whether an answer from the apps is still for what is typed: no newer keystroke was asked
/// (`asked` is the newest) and the field still says the same.
pub fn still_wanted(asked: u64, newest: u64, asked_text: &str, text_now: &str) -> bool {
    asked == newest && asked_text == text_now
}

/// wol-shell: lists `$HOME` for Search at login and every hour, off the main thread.
pub fn keep_file_index() {
    let list = || {
        glib::spawn_future_local(async {
            let _ = gtk::gio::spawn_blocking(|| files::rebuild(&glib::home_dir())).await;
        });
    };
    list();
    glib::timeout_add_seconds_local(RELIST_SECS, move || {
        list();
        glib::ControlFlow::Continue
    });
}

/// Apps and Settings first, then file names, [`ROWS`] at most.
/// The calculator's answer, if any, comes first; Return copies it.
pub fn results(items: &[Item], files: &files::Index, query: &str) -> Vec<Item> {
    let sem = crate::search_sem::get();
    results_with(items, files, query, sem.as_deref())
}

/// [`results`] with the meaning providers given (tests use their own table).
pub fn results_with(
    items: &[Item],
    files: &files::Index,
    query: &str,
    sem: Option<&crate::search_sem::Providers<'_>>,
) -> Vec<Item> {
    if let Some(answer) = search::calc(query) {
        let mut v = vec![Item::new(
            &format!("= {answer}"),
            "Calculator",
            "edit-paste-symbolic",
            vec!["wl-copy".into(), answer],
        )];
        v.extend(search::search(items, query, ROWS - 1).into_iter().cloned());
        return v;
    }
    let mut top: Vec<Item> = search::search(items, query, ROWS)
        .into_iter()
        .cloned()
        .collect();
    // Word hits first; what the query means follows them (a Settings row, a manual section).
    if let Some(p) = sem.filter(|_| !query.trim().is_empty()) {
        let q: String = query.chars().take(QUERY_MAX).collect();
        let room = ROWS.saturating_sub(top.len());
        for it in p.extra(&q, !top.is_empty(), room) {
            if !top.iter().any(|t| t.title == it.title && t.kind == it.kind) {
                top.push(it);
            }
        }
    }
    let found = files.search(query, ROWS - top.len().min(APPS));
    top.truncate(ROWS - found.len());
    top.extend(found);
    top
}

/// Rows apps provided. The app's name comes from wold, so a row cannot pass for another's.
pub fn provided(rows: Vec<crate::app_session::ProviderRow>) -> Vec<Item> {
    let row = |r: crate::app_session::ProviderRow| {
        let title = match r.detail.as_str() {
            "" => format!("{} \u{b7} {}", r.title, r.app),
            d => format!("{} \u{2014} {d} \u{b7} {}", r.title, r.app),
        };
        let exec = vec![OPEN_ROW.into(), r.provider.to_string(), r.key];
        Item::new(&title, PROVIDED, "system-search-symbolic", exec)
    };
    rows.into_iter().map(row).collect()
}

/// Open a result: run it, or tell the app that provided it.
/// An app row goes to the Dock (manual webapps.md; QA-MANUAL-5/43): dragged onto it, it hands the
/// Dock its desktop id ([`crate::apps::DRAG_PREFIX`]); right-click offers Keep in Dock. The
/// Dock's own pinned apps are there already, so their rows offer nothing.
fn dock_handle(row: &gtk::Button, id: &str) {
    let drag = gtk::DragSource::new();
    drag.set_actions(gdk::DragAction::COPY);
    let text = format!("{}{id}", crate::apps::DRAG_PREFIX);
    drag.set_content(Some(&gdk::ContentProvider::for_value(&text.to_value())));
    {
        let icon = row
            .child()
            .and_then(|c| c.first_child())
            .and_downcast::<gtk::Image>();
        drag.connect_drag_begin(move |src, _| {
            if let Some(p) = icon.as_ref().and_then(|i| i.paintable()) {
                src.set_icon(Some(&p), 0, 0);
            }
        });
    }
    row.add_controller(drag);
    if crate::apps::here().iter().any(|p| p.id == id) {
        return;
    }
    let click = gtk::GestureClick::new();
    click.set_button(gdk::BUTTON_SECONDARY);
    let (b, id) = (row.clone(), id.to_string());
    click.connect_pressed(move |_, _, x, y| {
        let id = id.clone();
        crate::kit::data::popup_menu(
            &b,
            x,
            y,
            vec![(
                crate::ui::dock::KEEP_IN_DOCK.to_string(),
                None,
                Box::new(move || crate::ui::dock::keep_later(id.clone(), true)),
            )],
        );
    });
    row.add_controller(click);
}

fn launch(exec: &[String]) {
    match exec {
        [open, pid, key] if open == OPEN_ROW => {
            if let Ok(pid) = pid.parse() {
                crate::app_session::open_row(pid, key);
            }
        }
        _ => kit::spawn_app(exec),
    }
}

pub fn no_results(query: &str) -> String {
    format!("No results for \u{201c}{query}\u{201d}")
}

/// The web search for `query` (Chromium has no search flag: the browser opens the results page).
fn search_url(query: &str) -> String {
    let mut out = String::from("https://duckduckgo.com/?q=");
    for b in query.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The row under [`no_results`]: the browser searches for the query.
pub fn web_search(query: &str) -> Item {
    let browser = crate::apps::here().iter().find(|p| p.name == "Browser");
    let (icon, exec) = browser.map_or(("wolos-browser", "helium"), |p| (p.icon, p.exec[0]));
    Item::new(
        &format!("Search the web for \u{201c}{query}\u{201d}"),
        "Browser",
        icon,
        vec![exec.into(), search_url(query)],
    )
}

/// `wol-search [--warm]`; `warm`: the binary was given [`super::WARM_ARG`].
pub fn run(warm: bool) -> glib::ExitCode {
    super::run_panel_app(
        "org.wolos.search",
        "searchbar",
        // No keyboard at first, Exclusive once mapped ([`take_keyboard_when_mapped`]).
        Spec::panel(
            "wol-panel-searchbar",
            [true, false, false, false],
            FIRST_KEYBOARD,
        ),
        warm,
        |win, _| {
            take_keyboard_when_mapped(win);
            let items: Rc<RefCell<Vec<Item>>> = Default::default();
            let names: Rc<RefCell<files::Index>> = Default::default();
            let (c, render) = content(
                items.clone(),
                names.clone(),
                &std::env::var("WOL_SEARCHBAR_QUERY").unwrap_or_default(),
            );
            let _ = win;
            // Index off the main thread (parallel .desktop parse); typing before it lands re-renders when ready.
            {
                let render = render.clone();
                glib::spawn_future_local(async move {
                    // The shared list (read off the UI thread once, `ipc::installed`).
                    let apps = super::ipc::installed().await;
                    *items.borrow_mut() = index(apps.as_ref().clone());
                    render();
                    // The meaning providers: table, manual and Settings rows, off this thread.
                    let snapshot = items.borrow().clone();
                    if gtk::gio::spawn_blocking(move || crate::search_sem::build_for(&snapshot))
                        .await
                        .is_ok()
                    {
                        render();
                    }
                });
            }
            // File names: the saved list at once, a fresh one after when it is old.
            glib::spawn_future_local(async move {
                let home = glib::home_dir();
                let h = home.clone();
                let Ok((idx, stale)) = gtk::gio::spawn_blocking(move || files::load(&h)).await
                else {
                    return;
                };
                *names.borrow_mut() = idx;
                render();
                if stale
                    && let Ok(idx) = gtk::gio::spawn_blocking(move || files::rebuild(&home)).await
                {
                    *names.borrow_mut() = idx;
                    render();
                }
            });
            c
        },
    )
}

/// Search field + ranked hits. Returns the widget and a re-render hook (index arrived).
/// A shown result: its row and what it launches.
type Hit = (gtk::Button, Vec<String>);

/// The row to select when the pointer enters row `hovered` (of `len`) while `sel` is selected.
pub fn hover_select(sel: usize, hovered: usize, len: usize) -> Option<usize> {
    (hovered < len && hovered != sel).then_some(hovered)
}

pub fn content(
    items: Rc<RefCell<Vec<Item>>>,
    names: Rc<RefCell<files::Index>>,
    query: &str,
) -> (gtk::Widget, Rc<dyn Fn()>) {
    let panel = w::glass_panel(0);
    panel.add_css_class("searchbar-panel");
    panel.set_width_request(w::SEARCHBAR_WIDTH);
    panel.set_valign(gtk::Align::Start);
    panel.set_margin_top(w::PANEL_TOP);
    let bar = w::hbox(w::S3);
    bar.add_css_class("searchbar-bar");
    bar.append(&w::icon("system-search-symbolic", w::ICON_M));
    let entry = gtk::Text::builder()
        .placeholder_text("Search")
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    bar.append(&entry);
    panel.append(&bar);
    let hits = w::vbox(w::S1);
    hits.add_css_class("searchbar-hits");
    hits.set_visible(false);
    panel.append(&hits);

    let shown: Rc<RefCell<Vec<Hit>>> = Default::default();
    let sel = Rc::new(Cell::new(0usize));
    // Rows from apps, for the text they answered.
    let apps: Rc<RefCell<(String, Vec<Item>)>> = Default::default();
    let render: Rc<dyn Fn()> = {
        let from_apps = apps.clone();
        let (shown, hits, sel, entry) = (shown.clone(), hits.clone(), sel.clone(), entry.clone());
        Rc::new(move || {
            let t0 = std::time::Instant::now();
            while let Some(c) = hits.first_child() {
                hits.remove(&c);
            }
            let mut s = shown.borrow_mut();
            s.clear();
            sel.set(0);
            let q = entry.text();
            let mut found = results(&items.borrow(), &names.borrow(), &q);
            let more = from_apps.borrow();
            if more.0 == q.as_str() {
                found = search::merge_slow(found, vec![(0, more.1.clone())], 0, ROWS);
            }
            if found.is_empty() && !q.trim().is_empty() {
                let none = w::centered(&no_results(q.trim()), "secondary");
                none.add_css_class("searchbar-none");
                none.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
                hits.append(&none);
                if let Some(d) = crate::search_sem::get()
                    .and_then(|p| p.did_you_mean(&items.borrow(), wolsem::capped(q.trim())))
                {
                    found.push(d);
                }
                found.push(web_search(q.trim()));
            }
            for (i, it) in found.into_iter().enumerate() {
                let row = w::hbox(w::S3);
                row.append(&w::app_icon(&it.icon, w::ICON_M));
                let t = w::label(&it.title, "");
                t.set_hexpand(true);
                t.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
                row.append(&t);
                row.append(&w::label(it.kind, "secondary"));
                let b = gtk::Button::builder().child(&row).build();
                b.add_css_class("hit");
                if i == 0 {
                    b.add_css_class("selected");
                }
                if !it.app.is_empty() {
                    dock_handle(&b, &it.app);
                }
                let exec = it.exec.clone();
                b.connect_clicked(move |b| {
                    launch(&exec);
                    if let Some(w) = b.root().and_downcast::<gtk::Window>() {
                        w.close();
                    }
                });
                {
                    // The pointer moves the selection: Return opens what is lit.
                    let (shown, sel) = (shown.clone(), sel.clone());
                    // `motion`, not `enter`: a row rebuilt under a resting pointer gets an enter
                    // but no motion, so Return keeps opening the top hit (QA-LN-2: 'ls' + Return
                    // opened the row under the pointer).
                    let motion = gtk::EventControllerMotion::new();
                    motion.connect_motion(move |_, _, _| {
                        let Ok(s) = shown.try_borrow() else { return };
                        if let Some(n) = hover_select(sel.get(), i, s.len()) {
                            if let Some((o, _)) = s.get(sel.get()) {
                                o.remove_css_class("selected");
                            }
                            s[n].0.add_css_class("selected");
                            sel.set(n);
                        }
                    });
                    b.add_controller(motion);
                }
                hits.append(&b);
                s.push((b, it.exec));
            }
            hits.set_visible(!s.is_empty());
            // WOL_SHELL_TIMING: per-keystroke search + row build time (target < 16 ms).
            if std::env::var_os("WOL_SHELL_TIMING").is_some() {
                eprintln!("wol-search: keystroke {:?}", t0.elapsed());
            }
        })
    };
    // Local rows show at once on every keystroke. A slower provider's arrival (the file index
    // loading) re-renders only within the timeout of the keystroke, after the priority delay.
    let started = Rc::new(Cell::new(std::time::Instant::now()));
    {
        let (r, st) = (render.clone(), started.clone());
        entry.connect_changed(move |_| {
            st.set(std::time::Instant::now());
            r()
        });
    }
    let late: Rc<dyn Fn()> = {
        let (r, st, e) = (render.clone(), started, entry.clone());
        Rc::new(move || {
            let p = crate::ui::ipc::shell_prefs();
            let waited = u32::try_from(st.get().elapsed().as_millis()).unwrap_or(u32::MAX);
            let delay = p.priority_index_delay_ms;
            if e.text().is_empty() {
                return r();
            }
            if !crate::search::slow_in_time(
                waited.saturating_add(delay),
                p.searchbar_slow_timeout_ms,
            ) {
                return;
            }
            if delay == 0 {
                return r();
            }
            let r = r.clone();
            glib::timeout_add_local_once(
                std::time::Duration::from_millis(u64::from(delay)),
                move || r(),
            );
        })
    };
    {
        // Debounced, newest wins: the apps hear one question per pause in typing, and only an
        // answer to what is still typed is shown.
        let late = late.clone();
        let asked = Rc::new(Cell::new(0u64));
        entry.connect_changed(move |e| {
            let q = e.text().to_string();
            if q.trim().is_empty() {
                return;
            }
            let n = asked.get().wrapping_add(1);
            asked.set(n);
            let (e, late, apps, asked) = (e.clone(), late.clone(), apps.clone(), asked.clone());
            glib::timeout_add_local_once(
                std::time::Duration::from_millis(PROVIDER_DEBOUNCE_MS),
                move || {
                    if asked.get() != n {
                        return;
                    }
                    glib::spawn_future_local(async move {
                        let typed = q.clone();
                        let rows = gtk::gio::spawn_blocking(move || {
                            crate::app_session::provider_rows(&typed)
                        });
                        let Ok(rows) = rows.await else { return };
                        if rows.is_empty() || !still_wanted(n, asked.get(), &q, &e.text()) {
                            return;
                        }
                        *apps.borrow_mut() = (q, provided(rows));
                        late();
                    });
                },
            );
        });
    }
    {
        let (shown, sel) = (shown.clone(), sel.clone());
        entry.connect_activate(move |e| {
            if let Some((_, exec)) = shown.borrow().get(sel.get()) {
                launch(exec);
                if let Some(w) = e.root().and_downcast::<gtk::Window>() {
                    w.close();
                }
            }
        });
    }
    let keys = gtk::EventControllerKey::new();
    {
        let (shown, sel) = (shown.clone(), sel.clone());
        keys.connect_key_pressed(move |_, k, _, _| {
            let s = shown.borrow();
            if s.is_empty() {
                return glib::Propagation::Proceed;
            }
            let next = match k {
                gdk::Key::Down => (sel.get() + 1).min(s.len() - 1),
                gdk::Key::Up => sel.get().saturating_sub(1),
                _ => return glib::Propagation::Proceed,
            };
            s[sel.get()].0.remove_css_class("selected");
            s[next].0.add_css_class("selected");
            sel.set(next);
            glib::Propagation::Stop
        });
    }
    entry.add_controller(keys);
    entry.set_text(query);
    (panel.upcast(), late)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row under the pointer lights up like the keyboard selection (and becomes it, so Return
    /// opens what is highlighted).
    #[test]
    fn rows_light_up_under_the_pointer() {
        let css = include_str!("../kit/kit.css");
        assert!(
            css.contains("button.hit:hover"),
            "hover style for result rows"
        );
        assert_eq!(hover_select(0, 3, 5), Some(3));
        assert_eq!(hover_select(3, 3, 5), None);
        assert_eq!(hover_select(0, 7, 5), None);
    }

    /// A thread per keystroke asked the apps for every letter typed; now one question per pause,
    /// and a late answer to older text is dropped.
    #[test]
    fn apps_are_asked_once_per_pause_and_old_answers_are_dropped() {
        assert!(still_wanted(3, 3, "ter", "ter"));
        assert!(
            !still_wanted(2, 3, "te", "ter"),
            "a newer keystroke was asked"
        );
        assert!(!still_wanted(3, 3, "ter", "term"), "the field moved on");
    }

    #[test]
    fn a_provided_row_names_its_app_and_opens_through_wold() {
        let row = crate::app_session::ProviderRow {
            key: "k".into(),
            title: "A note".into(),
            detail: "Today".into(),
            app: "notes".into(),
            provider: 7,
        };
        let items = provided(vec![row]);
        assert_eq!(items[0].title, "A note \u{2014} Today \u{b7} notes");
        assert_eq!(items[0].kind, PROVIDED);
        assert_eq!(items[0].exec, [OPEN_ROW, "7", "k"]);
    }

    /// QA-LN-18: 'lock', 'shut down', 'log out', 'sleep', 'screenshot' find the action, first.
    #[test]
    fn session_actions_are_found_first() {
        let idx = index(vec![]);
        let top = |q: &str| {
            search::search(&idx, q, 1)
                .first()
                .map(|i| (i.title.clone(), i.kind))
        };
        assert_eq!(top("shut down"), Some(("Shut Down".into(), ACTION)));
        assert_eq!(top("sleep"), Some(("Sleep".into(), ACTION)));
        assert_eq!(top("log out"), Some(("Log Out".into(), ACTION)));
        assert_eq!(top("restart"), Some(("Restart".into(), ACTION)));
        assert_eq!(top("screenshot"), Some(("Take Screenshot".into(), ACTION)));
        let by = |t: &str| {
            actions(false)
                .into_iter()
                .find(|i| i.title == t)
                .unwrap()
                .exec
        };
        assert_eq!(
            by("Shut Down"),
            ["wol", "shell", "show", "session-shutdown"]
        );
        assert_eq!(
            by("Take Screenshot"),
            ["wol", "shell", "show", "screenshot"]
        );
        // Every shell target an action opens is one the wol CLI accepts.
        for a in actions(false)
            .iter()
            .filter(|a| a.exec.get(2).is_some_and(|v| v == "show"))
        {
            assert!(
                shared::shell::PANELS.contains(&a.exec[3].as_str()),
                "{:?}",
                a.exec
            );
        }
        // Live has no Lock Screen action (nothing unlocks the live account).
        assert!(actions(false).iter().any(|a| a.title == "Lock Screen"));
        assert!(!actions(true).iter().any(|a| a.title == "Lock Screen"));
    }

    /// QA-LN-20: the real names of the pinned apps find them.
    #[test]
    fn index_has_generic_and_product_names() {
        let idx = index(vec![]);
        let top = |q: &str| search::search(&idx, q, 1).first().map(|i| i.title.clone());
        assert_eq!(top("helium").as_deref(), Some("Browser"));
        assert_eq!(top("geary").as_deref(), Some("Mail"));
        assert_eq!(top("nautilus").as_deref(), Some("Files"));
        assert_eq!(top("amberol").as_deref(), Some("Music"));
        assert_eq!(top("shotwell").as_deref(), Some("Photos"));
        assert_eq!(top("gnote").as_deref(), Some("Notes"));
        assert_eq!(
            top("browser").as_deref(),
            Some("Browser"),
            "the WolOS name still finds it"
        );
    }

    /// QA-LN-2: 'ls' + Return opened the row under a resting pointer. Rows are rebuilt on every
    /// keystroke; GTK sends `enter` to the new row under the pointer, which lit it. Only real pointer
    /// motion moves the selection now, so Return opens the top hit.
    #[test]
    fn return_takes_top_hit_with_resting_pointer() {
        let src = include_str!("searchbar.rs");
        let code = &src[..src.find("#[cfg(test)]").unwrap()];
        assert!(
            code.contains("motion.connect_motion("),
            "hover follows motion"
        );
        assert!(
            !code.contains(concat!("motion.connect_", "enter(")),
            "an enter alone lights nothing"
        );
        // The first row is the selected one after every rebuild.
        assert!(code.contains("if i == 0 {\n                    b.add_css_class(\"selected\")"));
    }

    /// QA-LN-3: a key at +0.2 s after the press reaches the field: the warm panel maps within
    /// its budget and then takes the keyboard, both before the key.
    #[test]
    fn a_key_at_200ms_is_kept() {
        const KEY_AT_MS: u64 = 200;
        const { assert!(WARM_SHOW_BUDGET_MS + KEYBOARD_AFTER_MS < KEY_AT_MS) };
        // The warm process is what makes the budget possible: the app is activated, not started.
        let src = include_str!("mod.rs");
        assert!(src.contains("pub const WARM_ARG"));
    }

    /// QA-LIVE1-11: Search starts without the keyboard and takes it once mapped (the change
    /// Hyprland acts on).
    #[test]
    fn search_takes_keyboard_after_map() {
        assert_eq!(FIRST_KEYBOARD, Keyboard::None);
        let src = include_str!("searchbar.rs");
        assert!(src.contains("set_keyboard(&w, Keyboard::Exclusive)"));
    }

    #[test]
    fn index_has_settings_pages_and_pins() {
        let idx = index(vec![]);
        assert!(
            idx.iter().any(
                |i| i.title == "Users & Groups" && i.exec == ["wol-settings", "Users & Groups"]
            )
        );
        assert_eq!(search::search(&idx, "term", 1)[0].title, "Terminal");
    }

    /// "Menu Bar & Dock" and "Hardware" opened Appearance: every Settings result is a page
    /// Settings has, under the name it shows, and the pane's search words find it.
    #[test]
    fn every_settings_result_opens_its_pane() {
        use crate::ui::settings::{PAGES, SUBPAGES, canonical};
        let idx = index(vec![]);
        let rows: Vec<&Item> = idx.iter().filter(|i| i.kind == SETTINGS).collect();
        assert_eq!(rows.len(), PAGES.len() + SUBPAGES.len());
        for r in &rows {
            assert_eq!(r.exec, ["wol-settings", r.title.as_str()]);
            assert_eq!(canonical(&r.title), r.title, "an old name");
            assert!(PAGES.contains(&r.title.as_str()) || SUBPAGES.contains(&r.title.as_str()));
        }
        for (q, want) in [
            ("hardware", "Hardware"),
            ("menu bar", "Desktop & Dock"),
            ("dark", "Appearance"),
            ("wifi", "Wi-Fi"),
            ("storage", "Storage"),
        ] {
            assert_eq!(search::search(&idx, q, 1)[0].title, want, "{q}");
        }
    }

    /// Apps and Settings first, files fill the list; never more than [`ROWS`].
    #[test]
    fn files_fill_the_results_after_apps() {
        let idx = index(vec![]);
        let files = files::Index::parse(
            "Documents/Terminal notes.txt\nterm paper.pdf\n",
            std::path::Path::new("/home/alice"),
        );
        let got = results(&idx, &files, "term");
        assert_eq!(got[0].title, "Terminal");
        assert!(got.iter().any(|i| i.title == "term paper.pdf"));
        let many = files::Index::parse(
            &(0..20)
                .map(|i| format!("text {i}.txt\n"))
                .collect::<String>(),
            std::path::Path::new("/home/alice"),
        );
        let got = results(&idx, &many, "te");
        assert_eq!(got.len(), ROWS);
        assert!(got.iter().filter(|i| i.kind == "Text").count() >= ROWS - APPS);
        assert!(results(&idx, &files::Index::default(), "").is_empty());
    }

    /// The meaning providers on the wolsem fixture: the table, the manual's 505 sections and every
    /// Settings row. Built once per test (a table load and ~1 s of embedding unoptimised).
    fn with_providers<R>(f: impl FnOnce(&[Item], &crate::search_sem::Providers<'_>) -> R) -> R {
        let dir =
            std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../wolsem/tests/data"));
        let table = wolsem::Table::open(&dir.join("mini.wst")).unwrap_or_else(|e| panic!("{e}"));
        let manual = wolsem::Corpus::from_json(
            &std::fs::read_to_string(dir.join("sections.json")).unwrap_or_default(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let installed = wolsem::Installed::from_parts(table, manual, dir);
        let idx = index(vec![]);
        let p = crate::search_sem::Providers {
            sem: crate::search_sem::Semantic::build(&installed.table, &idx),
            installed: &installed,
        };
        f(&idx, &p)
    }

    /// Questions in other words and other languages find a Settings row or a manual section after
    /// the word hits; a name typed as it is still opens the same first row.
    #[test]
    fn meaning_follows_the_word_hits() {
        with_providers(|idx, p| {
            let none = files::Index::default();
            let got = |q: &str| results_with(idx, &none, q, Some(p));
            let titles = |q: &str| {
                got(q)
                    .into_iter()
                    .map(|i| format!("{} [{}]", i.title, i.kind))
                    .collect::<Vec<_>>()
            };
            // Words first, unchanged.
            assert_eq!(got("term")[0].title, "Terminal");
            assert_eq!(got("wifi")[0].title, "Wi-Fi");
            assert_eq!(
                got("wifi")[0].title,
                results_with(idx, &none, "wifi", None)[0].title
            );
            assert!(got("wifi").len() <= results_with(idx, &none, "wifi", None).len() + 2);
            // Meaning.
            let t = titles("make the cursor bigger");
            assert!(
                t.iter()
                    .any(|x| x.starts_with("Pointer size") || x.starts_with("Accessibility")),
                "{t:?}"
            );
            let t = titles("tirar uma captura da tela");
            assert!(
                t.iter()
                    .any(|x| x.contains("[Manual]") || x.starts_with("Take Screenshot")),
                "{t:?}"
            );
            let manual: Vec<_> = got("modo escuro")
                .into_iter()
                .filter(|i| i.kind == crate::search_sem::MANUAL)
                .collect();
            assert!(
                manual
                    .iter()
                    .any(|i| i.exec[1].starts_with("https://wolos.org/manual/dark-mode")),
                "{manual:?}"
            );
            assert!(manual.len() <= crate::search_sem::MANUAL_ROWS);
            // Nothing: nothing.
            assert!(got("qwertyuiop asdfgh").is_empty());
            assert!(got("").is_empty());
            assert!(got("   ").is_empty());
            assert!(got("2+2")[0].kind == "Calculator");
        });
    }

    /// A pasted page (100 000 characters) in the field: the meaning providers cut it before any
    /// work (the spelling pass alone took 6 s on the interface thread).
    #[test]
    fn a_pasted_page_does_not_stall_the_meaning_providers() {
        with_providers(|idx, p| {
            let none = files::Index::default();
            let paste = "pasword wireles blutooth firewal securty ".repeat(2500);
            assert!(paste.chars().count() >= 100_000);
            let t0 = std::time::Instant::now();
            let _ = p.did_you_mean(idx, wolsem::capped(paste.trim()));
            let _ = p.did_you_mean(idx, &paste);
            let _ = results_with(idx, &none, &paste, Some(p));
            let spent = t0.elapsed();
            assert!(spent.as_secs() < 3, "{spent:?}");
        });
    }

    /// "Did you mean": a misspelt word is put right; a near title is offered; nonsense offers nothing.
    #[test]
    fn did_you_mean_offers_the_nearest_title() {
        with_providers(|idx, p| {
            let d = p.did_you_mean(idx, "bluetoth").map(|i| (i.title, i.kind));
            assert_eq!(
                d,
                Some(("Bluetooth".into(), crate::search_sem::DID_YOU_MEAN)),
                "{d:?}"
            );
            let d = p.did_you_mean(idx, "securty").map(|i| i.title);
            assert!(d.is_some(), "{d:?}");
            assert_eq!(
                p.did_you_mean(idx, "qwertyuiop asdfgh").map(|i| i.title),
                None
            );
        });
    }

    /// Nothing found: say so, and offer the web.
    #[test]
    fn no_results_offers_the_web() {
        assert_eq!(no_results("qwzx"), "No results for \u{201c}qwzx\u{201d}");
        let web = web_search("tax forms");
        assert_eq!(
            web.exec,
            ["helium", "https://duckduckgo.com/?q=tax%20forms"]
        );
        assert_eq!(
            search_url("a&b=c/ü"),
            "https://duckduckgo.com/?q=a%26b%3Dc%2F%C3%BC"
        );
        assert_eq!(web.title, "Search the web for \u{201c}tax forms\u{201d}");
    }

    #[test]
    fn pinned_apps_appear_once_under_their_wolos_name() {
        // Apps listed "Geary" (its .desktop) and "Mail" (the pin) for the same app.
        let e = |id: &str, name: &str, exec: &str| crate::desktop::Entry {
            id: id.into(),
            name: name.into(),
            exec: vec![exec.into()],
            icon: id.trim_end_matches(".desktop").into(),
            wm_class: String::new(),
        };
        let idx = index(vec![
            e("org.gnome.Geary.desktop", "Geary", "geary"),
            e("helium.desktop", "Helium", "/usr/bin/helium"),
            e(
                "org.gnome.Calculator.desktop",
                "Calculator",
                "gnome-calculator",
            ),
        ]);
        let titles: Vec<&str> = idx.iter().map(|i| i.title.as_str()).collect();
        for (name, n) in [
            ("Mail", 1),
            ("Geary", 0),
            ("Browser", 1),
            ("Helium", 0),
            ("Calculator", 1),
        ] {
            assert_eq!(titles.iter().filter(|t| **t == name).count(), n, "{name}");
        }
        let mail = idx.iter().find(|i| i.title == "Mail").unwrap();
        assert_eq!(
            (mail.icon.as_str(), mail.exec.as_slice()),
            ("wolos-mail", &["geary".to_string()][..])
        );
        let web = idx.iter().find(|i| i.title == "Browser").unwrap();
        assert_eq!(web.exec, ["helium"]);
    }
}
