// SPDX-License-Identifier: GPL-2.0-or-later
//! ⇧⌘/: every keyboard shortcut in effect, on one glass sheet in columns (Kit parts only), as tall
//! as its rows; on a screen too short for them, on pages.
//! Read each time it opens, so a shortcut changed in Settings shows at once. Esc closes it.
use crate::kit::{data as d, pickers, widgets as w};
use crate::shortcut_sheet::{Section, sections};
use gtk::prelude::*;

/// Sheet width: three columns of section cards.
pub const WIDTH: i32 = 960;
/// The smallest screen height the sheet is laid out for when it does not know its own (1024 x 768).
pub const SMALL_SCREEN: i32 = 768;

/// How tall the whole sheet may be on a screen `screen_h` high: under the menu bar, with the
/// panel's air above and below it.
pub fn max_height(screen_h: i32) -> i32 {
    screen_h - w::MENUBAR_HEIGHT - 2 * w::S6
}

fn section(s: &Section) -> gtk::Box {
    let b = w::vbox(w::S1);
    b.add_css_class("shortcut-sheet-section");
    let t = d::text(s.title, d::Role::Caption, d::Tone::Accent);
    t.set_xalign(0.0);
    t.set_margin_bottom(w::S1);
    b.append(&t);
    for (name, keys) in &s.rows {
        b.append(&row(name, keys));
    }
    b
}

/// One shortcut: what it does, wrapping, and its keys.
fn row(name: &str, keys: &str) -> gtk::Box {
    let r = w::hbox(w::S3);
    let n = w::label(name, "");
    n.set_xalign(0.0);
    n.set_hexpand(true);
    n.set_wrap(true);
    n.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    r.append(&n);
    let k = pickers::keycap(keys);
    k.set_valign(gtk::Align::Center);
    r.append(&k);
    r
}

/// Three columns of whole sections, in reading order (a section, and so a row, is never split).
fn page(list: &[Section]) -> gtk::Box {
    let cols = w::hbox(w::S6);
    cols.set_homogeneous(true);
    // Columns start at the top, like a newspaper: not a row of centred items.
    cols.add_css_class("no-row-check");
    for ix in crate::shortcut_sheet::columns(list, 3) {
        let c = w::vbox(w::S4);
        c.set_valign(gtk::Align::Start);
        for i in ix {
            c.append(&section(&list[i]));
        }
        cols.append(&c);
    }
    cols
}

/// Natural height of `b` at `width`.
fn tall_at(b: &impl IsA<gtk::Widget>, width: i32) -> i32 {
    b.measure(gtk::Orientation::Vertical, width).1
}

/// Natural height at the width the columns get inside the sheet's insets.
fn tall(b: &impl IsA<gtk::Widget>) -> i32 {
    tall_at(b, WIDTH - 2 * w::S4)
}

/// The width of one of the three columns (the sheet's insets and the two gaps out).
const COLUMN: i32 = (WIDTH - 2 * w::S4 - 2 * w::S6) / 3;

/// Too tall for one page: the rows flow down the columns like a newspaper, each column at most
/// `budget` tall. A column ends between two rows, never inside one; a section that goes on in the
/// next column shows its title again. A row taller than a whole column (none is) gets one alone.
fn flow(list: &[Section], budget: i32) -> Vec<Vec<Section>> {
    // Heights at the column width, measured once: a section is its title, then rows S1 apart.
    let title = list.first().map_or(0, |s| {
        tall_at(
            &section(&Section {
                title: s.title,
                rows: vec![],
            }),
            COLUMN,
        )
    });
    let mut cols: Vec<Vec<Section>> = vec![vec![]];
    let mut used = 0;
    for s in list {
        let rows: Vec<i32> = s
            .rows
            .iter()
            .map(|(n, k)| tall_at(&row(n, k), COLUMN))
            .collect();
        let mut i = 0;
        while i < rows.len() {
            let fresh = cols.last().is_none_or(Vec::is_empty);
            let mut h = used + if fresh { 0 } else { w::S4 } + title;
            let mut n = 0;
            while i + n < rows.len() && h + w::S1 + rows[i + n] <= budget {
                h += w::S1 + rows[i + n];
                n += 1;
            }
            if n == 0 && !fresh {
                cols.push(vec![]);
                used = 0;
                continue;
            }
            if n == 0 {
                h += w::S1 + rows[i];
                n = 1;
            }
            if let Some(c) = cols.last_mut() {
                c.push(Section {
                    title: s.title,
                    rows: s.rows[i..i + n].to_vec(),
                });
            }
            used = h;
            i += n;
            if i < rows.len() {
                cols.push(vec![]);
                used = 0;
            }
        }
    }
    cols
}

/// The flowed columns, three a page (a short last page keeps the column width).
fn pages(list: &[Section], budget: i32) -> Vec<gtk::Box> {
    flow(list, budget)
        .chunks(3)
        .map(|three| {
            let cols = w::hbox(w::S6);
            cols.set_homogeneous(true);
            cols.add_css_class("no-row-check");
            for k in 0..3 {
                let c = w::vbox(w::S4);
                c.set_valign(gtk::Align::Start);
                for s in three.get(k).into_iter().flatten() {
                    c.append(&section(s));
                }
                cols.append(&c);
            }
            cols
        })
        .collect()
}

/// The sheet's content for `list`, at most `max` tall when it can be (see [`max_height`]). It is
/// as tall as its columns: nothing scrolls, so no row is ever cut. When the columns do not fit,
/// the rows flow onto pages ([`flow`]), picked beside the footnote with a Kit segmented control.
pub fn content(list: &[Section], max: i32) -> gtk::Box {
    let g = w::glass_panel(w::S4);
    g.add_css_class("shortcut-sheet");
    g.set_size_request(WIDTH, -1);
    g.append(&w::label("Keyboard Shortcuts", "title"));
    let f = w::footnote("Change shortcuts in System Settings > Keyboard > Keyboard Shortcuts.");
    f.set_xalign(0.0);
    let one = page(list);
    // The sheet without its columns: title, footnote, insets and the gaps between them.
    g.append(&f);
    let chrome = g.measure(gtk::Orientation::Vertical, WIDTH).1 + w::S4;
    g.remove(&f);
    if chrome + tall(&one) <= max {
        g.append(&one);
        g.append(&f);
        return g;
    }
    let picker_row = |sel: &gtk::Box| {
        let r = w::hbox(w::S3);
        r.append(&f);
        f.set_hexpand(true);
        r.append(sel);
        r
    };
    // The pager costs no height: it shares the footnote's row.
    let probe = picker_row(&w::segmented(&["1", "2"], 0, |_| {}));
    let footer = tall(&probe) - tall(&f);
    while let Some(c) = probe.first_child() {
        probe.remove(&c);
    }
    let cut = pages(list, max - chrome - footer.max(0));
    let stack = gtk::Stack::new();
    stack.set_vhomogeneous(true);
    stack.set_transition_type(gtk::StackTransitionType::None);
    let names: Vec<String> = (1..=cut.len()).map(|n| n.to_string()).collect();
    for (n, p) in names.iter().zip(&cut) {
        stack.add_named(p, Some(n));
    }
    g.append(&stack);
    let labels: Vec<&str> = names.iter().map(String::as_str).collect();
    let sel = {
        let (stack, names) = (stack.clone(), names.clone());
        w::segmented(&labels, 0, move |i| {
            if let Some(n) = names.get(i) {
                stack.set_visible_child_name(n);
            }
        })
    };
    g.append(&picker_row(&sel));
    g
}

/// The shortcuts in effect now: shipped defaults with the user's changes.
pub fn current() -> Vec<Section> {
    let home = std::path::PathBuf::from(crate::kit::config_home());
    let defaults = wol_core::shortcuts::load_defaults(|p| std::fs::read_to_string(p).ok());
    let over = wol_core::hyprprefs::load_overrides(&home);
    let tiling = wol_core::hyprprefs::load_desktop(&home).tiling;
    sections(&wol_core::shortcuts::effective(&defaults, &over), tiling)
}

/// Resident panel, centred; rebuilt from the current shortcuts on every show.
pub fn build(app: &gtk::Application) -> gtk::Window {
    let holder = w::vbox(0);
    let win = super::panels::panel(
        app,
        "shortcut-sheet-window",
        "wol-panel-shortcuts",
        [false; 4],
        &holder,
    );
    win.connect_show(move |win| {
        while let Some(c) = holder.first_child() {
            holder.remove(&c);
        }
        let screen = crate::kit::surface::screen_of(win).map_or(SMALL_SCREEN, |(_, h)| h);
        holder.append(&content(&current(), max_height(screen)));
    });
    win
}
