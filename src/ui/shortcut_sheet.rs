//! ⇧⌘/: every keyboard shortcut in effect, on one glass sheet in columns (Kit parts only).
//! Read each time it opens, so a shortcut changed in Settings shows at once. Esc closes it.
use crate::kit::{data as d, pickers, widgets as w};
use crate::shortcut_sheet::{Section, sections};
use gtk::prelude::*;

/// Sheet width: three columns of section cards.
pub const WIDTH: i32 = 960;
/// Taller than this, the columns scroll (small screens).
pub const MAX_HEIGHT: i32 = 640;

fn section(s: &Section) -> gtk::Box {
    let b = w::vbox(w::S1);
    b.add_css_class("shortcut-sheet-section");
    let t = d::text(s.title, d::Role::Caption, d::Tone::Accent);
    t.set_xalign(0.0);
    t.set_margin_bottom(w::S1);
    b.append(&t);
    for (name, keys) in &s.rows {
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
        b.append(&r);
    }
    b
}

/// The sheet's content for `list` (golden: a fixed list).
pub fn content(list: &[Section]) -> gtk::Box {
    let g = w::glass_panel(w::S4);
    g.add_css_class("shortcut-sheet");
    g.set_size_request(WIDTH, -1);
    g.append(&w::label("Keyboard Shortcuts", "title"));
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
    // As tall as the columns, up to a screen's worth; scrolls beyond.
    let (_, natural, _, _) = cols.measure(gtk::Orientation::Vertical, WIDTH - 2 * w::S4);
    let scroll = gtk::ScrolledWindow::builder()
        .child(&cols)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(natural.min(MAX_HEIGHT))
        .max_content_height(MAX_HEIGHT)
        .build();
    g.append(&scroll);
    let f = w::footnote("Change shortcuts in System Settings > Keyboard > Keyboard Shortcuts.");
    f.set_xalign(0.0);
    g.append(&f);
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
    win.connect_show(move |_| {
        while let Some(c) = holder.first_child() {
            holder.remove(&c);
        }
        holder.append(&content(&current()));
    });
    win
}
