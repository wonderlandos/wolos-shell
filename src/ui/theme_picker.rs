// SPDX-License-Identifier: GPL-2.0-or-later
//! Theme picker (ADR 0031 themes v2): every theme of the catalogue as a preview card, the
//! catalogue's own themes first and then the theme packs of enabled plugins, each with its
//! plugin's name. One gallery for Settings > Appearance and the picker panel (Ctrl+Cmd+T); a
//! pick restyles the desktop and every app (`kit::style::set_active`, journaled).
use crate::copy;
use crate::kit::{self, widgets as w};
use gtk::{gio, glib, prelude::*};
use std::rc::Rc;
use wol_core::theme::{ThemeCatalog, packs};

/// The WolOS looks are the Light / Dark / Auto tiles and the accent colours, not gallery cards.
pub(crate) fn is_wolos_look(id: &str) -> bool {
    id.starts_with("wol-")
}

/// One card.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    pub id: String,
    pub name: String,
    /// The plugin that brought it (theme packs only).
    pub from: Option<String>,
}

/// (catalogue themes, theme packs), each by name, WolOS looks left out.
pub(crate) fn gallery(cat: &ThemeCatalog, packs: &[packs::Pack]) -> (Vec<Entry>, Vec<Entry>) {
    let from_pack = |id: &str| packs.iter().find(|p| p.spec.id == id);
    let (mut own, mut packed): (Vec<Entry>, Vec<Entry>) = cat
        .list()
        .into_iter()
        .filter(|s| !is_wolos_look(&s.id))
        .map(|s| Entry {
            id: s.id.clone(),
            name: s.name.clone(),
            // A pack id the catalogue already had stays the catalogue's (`packs::add`).
            from: from_pack(&s.id)
                .filter(|p| p.spec == *s)
                .map(|p| p.plugin_name.clone()),
        })
        .partition(|e| e.from.is_none());
    for v in [&mut own, &mut packed] {
        v.sort_by_key(|e| e.name.to_lowercase());
    }
    (own, packed)
}

/// Every installed theme and the packs they came from (built-ins, theme folders, plugins).
pub fn load() -> (ThemeCatalog, Vec<packs::Pack>) {
    let home = wol_core::theme::config_home();
    let data = wol_core::theme::apps::data_home(|k| std::env::var(k).ok());
    let packs = data
        .as_deref()
        .map(|d| packs::themes(&home, d))
        .unwrap_or_default();
    let cat = packs::add(
        ThemeCatalog::load(&wol_core::theme::search_paths(&home)),
        &packs,
    );
    (cat, packs)
}

/// Preview cards for `entries` in one row set; `current` is checked. `join` puts each card in
/// the caller's toggle group; `pick` runs when a card turns on.
pub(crate) fn cards(
    cat: &ThemeCatalog,
    entries: &[Entry],
    current: Option<&str>,
    join: &dyn Fn(&gtk::ToggleButton),
    pick: Rc<dyn Fn(&str)>,
) -> gtk::FlowBox {
    let row = w::wrap_row(w::S3, 4);
    for e in entries {
        let Ok(t) = cat.resolve(&e.id) else { continue };
        let card = w::theme_preview(
            vec![w::Palette::of(&t)],
            &e.name,
            current == Some(e.id.as_str()),
        );
        card.set_tooltip_text(Some(&match &e.from {
            Some(p) => copy::theme_from_plugin(&e.name, p),
            None => e.name.clone(),
        }));
        join(&card);
        let (id, pick) = (e.id.clone(), pick.clone());
        card.connect_toggled(move |c| {
            if c.is_active() {
                pick(&id);
            }
        });
        row.append(&card);
    }
    row
}

/// A toggle group: the first card joined leads it.
pub(crate) fn group() -> impl Fn(&gtk::ToggleButton) {
    let lead: Rc<std::cell::RefCell<Option<gtk::ToggleButton>>> = Default::default();
    move |t| {
        let mut l = lead.borrow_mut();
        match &*l {
            Some(f) => t.set_group(Some(f)),
            None => *l = Some(t.clone()),
        }
    }
}

/// The picker's content: the catalogue is read off the GTK thread (theme folders and plugin
/// packs; audit5 S8-7), then [`fill_with`]. Run at build and each time the panel shows (a plugin
/// enabled meanwhile is there). The shell then draws with what is listed (audit5 S9-2).
fn fill(list: &gtk::Box, done: Rc<dyn Fn()>) {
    let list = list.clone();
    glib::spawn_future_local(async move {
        let Ok((cat, packs)) = gio::spawn_blocking(load).await else {
            return;
        };
        crate::theme::adopt(cat.clone());
        fill_with(&list, &cat, &packs, done);
    });
}

/// Replaces `list`'s cards with both galleries of `cat`; `done` runs after a pick. No disk work.
pub fn fill_with(list: &gtk::Box, cat: &ThemeCatalog, packs: &[packs::Pack], done: Rc<dyn Fn()>) {
    while let Some(c) = list.first_child() {
        list.remove(&c);
    }
    let (own, packed) = gallery(cat, packs);
    let current = kit::style::active_id();
    let join = group();
    let pick: Rc<dyn Fn(&str)> = Rc::new(move |id: &str| {
        if let Err(e) = kit::style::set_active(id) {
            eprintln!("wol-shell: theme: {e}");
        }
        done();
    });
    list.append(&cards(cat, &own, current.as_deref(), &join, pick.clone()));
    if !packed.is_empty() {
        list.append(&w::label(copy::THEMES_FROM_PLUGINS, "secondary"));
        list.append(&cards(cat, &packed, current.as_deref(), &join, pick));
    }
}

/// The picker's glass card: a title over the galleries, empty until [`fill`] or [`fill_with`].
/// Returns the card and the list to fill.
pub fn content() -> (gtk::Box, gtk::Box) {
    let g = w::glass_panel(w::S2);
    let head = w::hbox(w::S2);
    head.append(&w::label(copy::THEMES_TITLE, "title"));
    head.append(&w::spacer());
    let list = w::vbox(w::S2);
    // A scroller, not a window: RU-045 (window titles) does not apply.
    let scroll = gtk::ScrolledWindow::default();
    scroll.set_child(Some(&list));
    scroll.add_css_class("theme-gallery");
    scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroll.set_min_content_height(w::PREVIEW.1 * 3);
    // About four rows of cards; the rest scrolls.
    scroll.set_max_content_height(w::PREVIEW.1 * 6);
    scroll.set_propagate_natural_height(true);
    // As wide as a row of cards, not one card.
    scroll.set_propagate_natural_width(true);
    g.append(&head);
    g.append(&scroll);
    (g, list)
}

/// The picker panel (`wol shell toggle themes`): a pick closes it, like a menu item.
pub fn panel(app: &gtk::Application) -> gtk::Window {
    let hide: Rc<std::cell::RefCell<Option<glib::WeakRef<gtk::Window>>>> = Default::default();
    let done: Rc<dyn Fn()> = {
        let hide = hide.clone();
        Rc::new(move || {
            if let Some(w) = hide.borrow().as_ref().and_then(|w| w.upgrade()) {
                w.set_visible(false);
            }
        })
    };
    let (g, list) = content();
    fill(&list, done.clone());
    let win = super::panels::panel(app, "themes", "wol-panel-themes", [false; 4], &g);
    *hide.borrow_mut() = Some(win.downgrade());
    win.connect_map(move |_| fill(&list, done.clone()));
    win
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gallery_lists_every_other_builtin_theme_by_name() {
        let (g, packed) = gallery(&ThemeCatalog::builtin(), &[]);
        assert_eq!(g.len(), 21, "23 built-ins minus WolOS Light and Dark");
        assert!(packed.is_empty());
        assert!(
            g.iter()
                .all(|e| !e.id.starts_with("wol-") && e.from.is_none())
        );
        let names: Vec<&str> = g.iter().map(|x| x.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_by_key(|n| n.to_lowercase());
        assert_eq!(names, sorted, "alphabetical");
    }

    #[test]
    fn plugin_themes_get_their_own_row_with_the_plugin_name() {
        let spec = wol_core::theme::ThemeSpec::parse(
            r##"{"id":"sunset","name":"Sunset","variant":"dark","tokens":{"accent":"#ff7a59"}}"##,
            std::path::Path::new(""),
        )
        .unwrap();
        // A pack that reuses a built-in id is not the pack's.
        let nord = ThemeCatalog::builtin()
            .list()
            .into_iter()
            .find(|s| s.id == "nord")
            .unwrap()
            .clone();
        let mut fake_nord = nord.clone();
        fake_nord.name = "Not Nord".into();
        let p = |spec: wol_core::theme::ThemeSpec| packs::Pack {
            plugin: "com.example.pack".into(),
            plugin_name: "Example Pack".into(),
            spec,
        };
        let list = [p(spec), p(fake_nord)];
        let cat = packs::add(ThemeCatalog::builtin(), &list);
        let (own, packed) = gallery(&cat, &list);
        assert_eq!(own.len(), 21);
        assert_eq!(
            packed,
            [Entry {
                id: "sunset".into(),
                name: "Sunset".into(),
                from: Some("Example Pack".into())
            }]
        );
        assert!(own.iter().any(|e| e.id == "nord" && e.name == nord.name));
    }
}
