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
pub const APPS: usize = 5;

pub fn index(apps: Vec<crate::desktop::Entry>) -> Vec<Item> {
    // Pinned apps come from PINS below (WolOS name, icon, launcher), never twice.
    let mut v: Vec<Item> = apps
        .into_iter()
        .filter(|e| crate::apps::pin_for_entry(e, crate::apps::here()).is_none())
        .map(|e| Item::new(&e.name, "Application", &e.icon, e.exec))
        .collect();
    for p in crate::apps::here() {
        if !v.iter().any(|i| i.title == p.name) {
            v.push(Item::new(
                p.name,
                "Application",
                p.icon,
                p.exec.iter().map(|s| s.to_string()).collect(),
            ));
        }
    }
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

/// Seconds between two listings of `$HOME` by the shell (Search also lists it when stale).
const RELIST_SECS: u32 = 3600;

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
fn launch(exec: &[String]) {
    match exec {
        [open, pid, key] if open == OPEN_ROW => {
            if let Ok(pid) = pid.parse() {
                crate::app_session::open_row(pid, key);
            }
        }
        _ => kit::spawn(exec),
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

pub fn run() -> glib::ExitCode {
    super::run_panel_app(
        "org.wolos.search",
        "searchbar",
        Spec::panel(
            "wol-panel-searchbar",
            [true, false, false, false],
            Keyboard::Exclusive,
        ),
        |win, _| {
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
                    if let Ok(v) =
                        gtk::gio::spawn_blocking(|| index(crate::desktop::load_all())).await
                    {
                        *items.borrow_mut() = v;
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
        .placeholder_text("Search Search")
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
                let exec = it.exec.clone();
                b.connect_clicked(move |b| {
                    launch(&exec);
                    if let Some(w) = b.root().and_downcast::<gtk::Window>() {
                        w.close();
                    }
                });
                {
                    // The pointer moves the selection (macOS): Return opens what is lit.
                    let (shown, sel) = (shown.clone(), sel.clone());
                    let motion = gtk::EventControllerMotion::new();
                    motion.connect_enter(move |_, _, _| {
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
        let late = late.clone();
        entry.connect_changed(move |e| {
            let q = e.text().to_string();
            if q.trim().is_empty() {
                return;
            }
            let (e, late, apps) = (e.clone(), late.clone(), apps.clone());
            glib::spawn_future_local(async move {
                let asked = q.clone();
                let rows =
                    gtk::gio::spawn_blocking(move || crate::app_session::provider_rows(&asked));
                let Ok(rows) = rows.await else { return };
                if rows.is_empty() || e.text().as_str() != q {
                    return;
                }
                *apps.borrow_mut() = (q, provided(rows));
                late();
            });
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
