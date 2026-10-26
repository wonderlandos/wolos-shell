//! Spotlight: installed apps + Settings pages, ranked per keystroke from a prebuilt lowercase index.
use crate::kit::{
    self,
    surface::{Keyboard, Spec},
    widgets as w,
};
use crate::search::{self, Item};
use gtk::{gdk, glib, prelude::*};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub const SETTINGS_PAGES: [&str; 7] = [
    "Appearance",
    "Menu Bar & Dock",
    "Keyboard",
    "Lock & Idle",
    "Hardware",
    "Accounts",
    "About",
];

pub fn index(apps: Vec<crate::desktop::Entry>) -> Vec<Item> {
    let mut v: Vec<Item> = apps
        .into_iter()
        .map(|e| Item::new(&e.name, "Application", &e.icon, e.exec))
        .collect();
    for p in crate::apps::PINS {
        if !v.iter().any(|i| i.title == p.name) {
            v.push(Item::new(
                p.name,
                "Application",
                p.icon,
                p.exec.iter().map(|s| s.to_string()).collect(),
            ));
        }
    }
    v.extend(SETTINGS_PAGES.iter().map(|p| {
        Item::new(
            p,
            "System Settings",
            "wolos-settings",
            vec!["wol-settings".into(), (*p).into()],
        )
    }));
    v
}

pub fn run() -> glib::ExitCode {
    super::run_panel_app(
        "org.wolos.spotlight",
        "spotlight",
        Spec::panel(
            "wol-panel-spotlight",
            [true, false, false, false],
            Keyboard::Exclusive,
        ),
        |win, _| {
            let items: Rc<RefCell<Vec<Item>>> = Default::default();
            let (c, render) = content(
                items.clone(),
                &std::env::var("WOL_SPOTLIGHT_QUERY").unwrap_or_default(),
            );
            let _ = win;
            // Index off the main thread (parallel .desktop parse); typing before it lands re-renders when ready.
            glib::spawn_future_local(async move {
                if let Ok(v) = gtk::gio::spawn_blocking(|| index(crate::desktop::load_all())).await
                {
                    *items.borrow_mut() = v;
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

pub fn content(items: Rc<RefCell<Vec<Item>>>, query: &str) -> (gtk::Widget, Rc<dyn Fn()>) {
    let panel = w::glass_panel(0);
    panel.add_css_class("spotlight-panel");
    panel.set_width_request(w::SPOTLIGHT_WIDTH);
    panel.set_valign(gtk::Align::Start);
    panel.set_margin_top(w::PANEL_TOP);
    let bar = w::hbox(w::S3);
    bar.add_css_class("spotlight-bar");
    bar.append(&w::icon("system-search-symbolic", w::ICON_M));
    let entry = gtk::Text::builder()
        .placeholder_text("Spotlight Search")
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    bar.append(&entry);
    panel.append(&bar);
    let hits = w::vbox(w::S1);
    hits.add_css_class("spotlight-hits");
    hits.set_visible(false);
    panel.append(&hits);

    let shown: Rc<RefCell<Vec<Hit>>> = Default::default();
    let sel = Rc::new(Cell::new(0usize));
    let render: Rc<dyn Fn()> = {
        let (shown, hits, sel, entry) = (shown.clone(), hits.clone(), sel.clone(), entry.clone());
        Rc::new(move || {
            let t0 = std::time::Instant::now();
            while let Some(c) = hits.first_child() {
                hits.remove(&c);
            }
            let mut s = shown.borrow_mut();
            s.clear();
            sel.set(0);
            let items = items.borrow();
            for (i, it) in search::search(&items, &entry.text(), 8)
                .into_iter()
                .enumerate()
            {
                let row = w::hbox(w::S3);
                row.append(&w::app_icon(&it.icon, w::ICON_M));
                let t = w::label(&it.title, "");
                t.set_hexpand(true);
                row.append(&t);
                row.append(&w::label(it.kind, "secondary"));
                let b = gtk::Button::builder().child(&row).build();
                b.add_css_class("hit");
                if i == 0 {
                    b.add_css_class("selected");
                }
                let exec = it.exec.clone();
                b.connect_clicked(move |b| {
                    kit::spawn(&exec);
                    if let Some(w) = b.root().and_downcast::<gtk::Window>() {
                        w.close();
                    }
                });
                hits.append(&b);
                s.push((b, it.exec.clone()));
            }
            hits.set_visible(!s.is_empty());
            // WOL_SHELL_TIMING: per-keystroke search + row build time (target < 16 ms).
            if std::env::var_os("WOL_SHELL_TIMING").is_some() {
                eprintln!("wol-spotlight: keystroke {:?}", t0.elapsed());
            }
        })
    };
    {
        let r = render.clone();
        entry.connect_changed(move |_| r());
    }
    {
        let (shown, sel) = (shown.clone(), sel.clone());
        entry.connect_activate(move |e| {
            if let Some((_, exec)) = shown.borrow().get(sel.get()) {
                kit::spawn(exec);
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
    (panel.upcast(), render)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_has_settings_pages_and_pins() {
        let idx = index(vec![]);
        assert!(
            idx.iter()
                .any(|i| i.title == "Accounts" && i.exec == ["wol-settings", "Accounts"])
        );
        assert_eq!(search::search(&idx, "term", 1)[0].title, "Terminal");
    }
}
