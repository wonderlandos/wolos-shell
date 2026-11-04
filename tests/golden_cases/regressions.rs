// SPDX-License-Identifier: GPL-2.0-or-later
//! Regression checks that run before the screenshots: each one fails on the bug it was written for.
use super::*;

/// Settings > Effects & Looks keeps its state alive once built: picking a look and then Restore
/// Defaults checks the WolOS card again (every card, control and button did nothing when the
/// page's state was dropped at the end of building it).
pub(super) fn effects_page_answers_after_it_is_built() {
    let a = ui::settings::content(Rc::new(shell::system::Fake), &ui::settings::Host::demo());
    a.open("Effects & Looks");
    pump(50);
    let root: gtk::Widget = a.root.clone().upcast();
    let mut cards: Vec<gtk::ToggleButton> = Vec::new();
    find_type(&root, &mut cards);
    let card = |name: &str| {
        cards
            .iter()
            .filter(|b| b.has_css_class("preset-card"))
            .find(|b| b.tooltip_text().as_deref() == Some(name))
            .unwrap_or_else(|| panic!("no {name} card in {}", cards.len()))
            .clone()
    };
    card("Compact").set_active(true);
    let mut all = Vec::new();
    find_type::<gtk::Button>(&root, &mut all);
    let restore = all
        .into_iter()
        .find(|b| {
            let mut l = Vec::new();
            find_type::<gtk::Label>(b.upcast_ref(), &mut l);
            b.label().as_deref() == Some("Restore Defaults")
                || l.iter().any(|l| l.label() == "Restore Defaults")
        })
        .expect("Restore Defaults");
    restore.emit_clicked();
    pump(50);
    assert!(card("WolOS").is_active(), "Restore Defaults did nothing");
    assert!(!card("Compact").is_active());
}

/// Return is Continue, unless the keyboard ring is on a button: then it is that button's.
/// Audit5 S8-1/S9-5: the Dock follows `shell.json` like the menu bar. A layout with the Dock on
/// the left and a plugin item before the apps turns the shelf and puts the plugin box first; the
/// same layout again rebuilds nothing. The shelf hands the keyboard focus to its first button
/// (what `keyboard` relies on after a rebuild).
pub(super) fn the_dock_takes_a_new_layout() {
    use shared::layout::{DockItem, Edge};
    let d = ui::dock::content(&Default::default());
    assert_eq!(d.edge(), Edge::Bottom);
    let items = vec![
        DockItem::Plugin {
            plugin: "com.example.dock".into(),
            id: "x".into(),
        },
        DockItem::Apps,
        DockItem::Trash,
    ];
    assert!(d.set_layout(Edge::Left, &items), "new items: rebuilt");
    assert_eq!((d.edge(), d.items()), (Edge::Left, items.clone()));
    assert_eq!(d.root.orientation(), gtk::Orientation::Vertical);
    assert_eq!(
        (d.root.halign(), d.root.valign()),
        (gtk::Align::Start, gtk::Align::Center)
    );
    let first = d.root.first_child().expect("a first entry");
    assert!(
        first.is::<gtk::Box>() && !first.is::<gtk::Button>(),
        "the plugin box first"
    );
    let last = d.root.last_child().expect("a last entry");
    assert!(last.is::<gtk::Button>(), "Trash last");
    let n = {
        let mut n = 0;
        let mut c = d.root.first_child();
        while let Some(w) = c {
            n += 1;
            c = w.next_sibling();
        }
        n
    };
    assert!(!d.set_layout(Edge::Left, &items), "same items: kept");
    assert!(
        !d.set_layout(Edge::Right, &items),
        "an edge alone turns, no rebuild"
    );
    assert_eq!(d.root.halign(), gtk::Align::End);
    let mut m = 0;
    let mut c = d.root.first_child();
    while let Some(w) = c {
        m += 1;
        c = w.next_sibling();
    }
    assert_eq!(n, m, "nothing added by a turn");
    let win = gtk::Window::new();
    win.set_child(Some(&d.root));
    win.present();
    pump(20);
    let keys = kit::surface::keyboard_access(&win, &d.root, |_| {});
    keys(true);
    let focus = gtk::prelude::GtkWindowExt::focus(&win);
    assert!(
        focus.is_some_and(|f| f.is::<gtk::Button>() && f.is_ancestor(&d.root)),
        "the shelf hands the focus to one of its buttons"
    );
    keys(false);
    win.close();
}

pub(super) fn return_belongs_to_the_button_with_the_keyboard_ring() {
    use shell::kit::assistant::return_is_the_buttons;
    let win = gtk::Window::new();
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let button = gtk::Button::with_label("Try");
    let entry = gtk::Entry::new();
    body.append(&button);
    body.append(&entry);
    win.set_child(Some(&body));
    win.present();
    pump(20);
    GtkWindowExt::set_focus(&win, Some(&button));
    win.set_focus_visible(true);
    assert!(return_is_the_buttons(&win), "a ringed button takes Return");
    win.set_focus_visible(false);
    assert!(
        !return_is_the_buttons(&win),
        "focus nobody can see: Continue"
    );
    GtkWindowExt::set_focus(&win, Some(&entry));
    win.set_focus_visible(true);
    assert!(!return_is_the_buttons(&win), "a field: Continue");
    win.close();
}

/// A switch whose change takes time: insensitive while it runs, flipped back when it fails.
pub(super) fn a_toggle_waits_for_its_change_and_reverts_when_it_fails() {
    type Done = Box<dyn FnOnce(bool)>;
    let pending: Rc<std::cell::RefCell<Option<Done>>> = Default::default();
    let p = pending.clone();
    let s = w::pending_toggle(false, move |_on, done| *p.borrow_mut() = Some(done));
    s.set_active(true);
    assert!(
        !s.is_sensitive(),
        "the switch must wait while the change runs"
    );
    (pending.borrow_mut().take().expect("change started"))(false);
    assert!(s.is_sensitive());
    assert!(
        !s.is_active() && !s.state(),
        "a failed change must flip the switch back"
    );
    s.set_active(true);
    (pending.borrow_mut().take().expect("change started"))(true);
    assert!(s.is_active() && s.state());
}

/// Setup's cross-fade froze half way when the compositor stopped answering frame callbacks, and
/// the old page stayed drawn until the window closed. No tick is delivered here (the loop does
/// not run between the page change and the check), which is the same state: `settle` must leave
/// only the new page, fully shown.
/// A display plugged in and out over and over: its wallpaper and menu bar leave no minute tick,
/// reload hook or registered bar behind (a docking laptop must not grow all day).
pub(super) fn displays_coming_and_going_leave_nothing_behind() {
    let base = (
        shell::kit::lifetime::live_ticks(),
        ui::ipc::reload_hooks(),
        ui::menubar::bars().len(),
    );
    for _ in 0..25 {
        let wall = gtk::Window::new();
        ui::panels::dress_wallpaper(&wall);
        let bar_win = gtk::Window::new();
        let bar = ui::menubar::content(&shell::system::Fake);
        bar.life.tie(&bar_win);
        bar_win.set_child(Some(&bar.root));
        drop(bar);
        wall.present();
        bar_win.present();
        pump(10);
        wall.destroy();
        bar_win.destroy();
        pump(10);
    }
    let now = (
        shell::kit::lifetime::live_ticks(),
        ui::ipc::reload_hooks(),
        ui::menubar::bars().len(),
    );
    assert_eq!(
        now, base,
        "(minute ticks, reload hooks, bars) after 25 plug/unplug cycles"
    );
}

pub(super) fn a_stalled_page_fade_settles() {
    use shell::kit::assistant as a;
    let pager = a::pager();
    for n in ["a", "b", "c"] {
        pager.add_named(&gtk::Label::new(Some(n)), Some(n));
    }
    let win = gtk::Window::builder()
        .default_width(300)
        .default_height(200)
        .build();
    win.set_child(Some(&pager));
    win.present();
    pump(300);
    pager.set_visible_child_name("b");
    assert!(pager.is_transition_running(), "the fade has started");
    a::settle(&pager);
    assert!(!pager.is_transition_running(), "a stalled fade must end");
    let shown: Vec<bool> = ["a", "b", "c"]
        .iter()
        .map(|n| pager.child_by_name(n).unwrap().is_child_visible())
        .collect();
    assert_eq!(shown, [false, true, false], "only the new page stays drawn");
    assert_eq!(pager.visible_child_name().as_deref(), Some("b"));
    assert_eq!(pager.transition_type(), gtk::StackTransitionType::Crossfade);
    win.destroy();
}

/// Audit2 A2-4: below 640 px the sidebar collapsed with no way back. The toolbar toggle shows it in
/// place of the content, picking a row returns to the content, and widening the window brings the
/// two-pane layout back.
pub(super) fn a_narrow_window_can_bring_the_sidebar_back() {
    let a = ui::settings::content(Rc::new(shell::system::Fake), &ui::settings::Host::demo());
    let (aw, ah) = w::APP_NARROW;
    let win = gtk::Window::builder()
        .decorated(false)
        .default_width(aw)
        .default_height(ah)
        .build();
    win.set_child(Some(&a.root));
    win.present();
    pump(300);
    let side = a
        .sidebar
        .parent()
        .and_then(|v| v.parent())
        .expect("sidebar scroller");
    assert!(
        a.sidebar_toggle.is_visible(),
        "narrow: the toolbar offers the sidebar"
    );
    assert!(
        a.sidebar_toggle.is_focusable(),
        "the toggle takes the keyboard"
    );
    assert!(
        !side.is_visible() && a.content.is_visible(),
        "narrow starts on the content"
    );
    a.sidebar_toggle.emit_clicked();
    assert!(
        side.is_visible() && !a.content.is_visible(),
        "the toggle shows the sidebar"
    );
    assert!(
        a.sidebar_toggle.is_visible(),
        "the sidebar alone still offers the way back to the page"
    );
    a.sidebar_toggle.emit_clicked();
    assert!(
        !side.is_visible() && a.content.is_visible(),
        "the toggle goes back to the content"
    );
    a.sidebar_toggle.emit_clicked();
    assert!(side.is_visible(), "and shows the sidebar again");
    let mut i = 0;
    let row = loop {
        let r = a.sidebar.row_at_index(i).expect("a Sound row");
        if r.widget_name() == "Sound" {
            break r;
        }
        i += 1;
    };
    a.sidebar.select_row(Some(&row));
    row.emit_activate();
    assert!(
        !side.is_visible() && a.content.is_visible(),
        "picking a row shows its page"
    );
    assert_eq!(a.content.visible_child_name().as_deref(), Some("Sound"));
    assert!(a.sidebar_toggle.is_visible());
    win.set_default_size(900, 600);
    pump(300);
    assert!(
        side.is_visible() && a.content.is_visible(),
        "wide: both panes"
    );
    assert!(!a.sidebar_toggle.is_visible(), "wide: no toggle");
    win.destroy();
}

/// Audit4 S3-11: a tile re-sourced (Store refresh) showed "Flathub Flathub".
pub(super) fn an_app_tile_keeps_one_source_badge() {
    let t = w::AppTile::new(&w::store_icon("gimp", w::ICON_L), "GIMP", "Edit photos");
    assert_eq!(t.source(), "", "no badge until set");
    t.set_source("Flathub");
    t.set_source("Flathub");
    assert_eq!(t.source(), "Flathub");
    // Not in a window: walk the tree by hand (find_class wants drawn widgets).
    fn visible_badges(w: &gtk::Widget) -> usize {
        let here = usize::from(w.is_visible() && w.has_css_class("badge"));
        let mut n = here;
        let mut c = w.first_child();
        while let Some(ch) = c {
            n += visible_badges(&ch);
            c = ch.next_sibling();
        }
        n
    }
    let shown = visible_badges(t.root.upcast_ref());
    assert_eq!(shown, 1, "one badge, however often it is set");
    t.set_source("");
    assert_eq!(t.source(), "", "an empty source hides it");
}

/// E2E-OPEN: a file dropped on a Dock tile opens with that tile's app (`ui::dock::dropped`, what the
/// tile's drop target calls): all files of a `%F` line in one start, one start per file for
/// `%f`, nothing for an app whose line takes no file or whose entry is not read; the icon bounces
/// like a click's; right-click is wired for the app's actions.
pub(super) fn a_file_dropped_on_a_dock_tile_opens_with_its_app() {
    use shell::open_with::Handler;
    use std::cell::RefCell;
    let entry = |exec: &str| {
        let text = format!("[Desktop Entry]\nType=Application\nName=Viewer\nExec={exec}\n");
        ui::dock::entry_of(Handler::parse("v.desktop", "/p/v.desktop", &text))
    };
    let started: Rc<RefCell<Vec<Vec<String>>>> = Rc::default();
    let launch: ui::dock::Launch = {
        let log = started.clone();
        Rc::new(move |argv| log.borrow_mut().push(argv.to_vec()))
    };
    let drop_on = |t: &w::DockTile, e: &ui::dock::Entry, paths: &[&str]| -> bool {
        let files: Vec<gtk::gio::File> = paths.iter().map(gtk::gio::File::for_path).collect();
        ui::dock::dropped(e, t, &launch, &files)
    };
    let tile = |exec: &str| {
        let t = w::DockTile::new("folder", "Viewer", || {});
        let e = entry(exec);
        ui::dock::wire_tile(&t, &e, &launch);
        (t, e)
    };
    let words = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();

    let (t, e) = tile("viewer --new %F");
    assert!(drop_on(&t, &e, &["/h/a b.png", "/h/c.png"]));
    assert_eq!(
        *started.borrow(),
        [words(&["viewer", "--new", "/h/a b.png", "/h/c.png"])],
        "one start with every file"
    );
    assert!(t.is_launching(), "the icon bounces like a click's");

    started.borrow_mut().clear();
    let (t2, e2) = tile("viewer %f");
    assert!(drop_on(&t2, &e2, &["/h/a.png", "/h/b.png"]));
    assert_eq!(
        *started.borrow(),
        [
            words(&["viewer", "/h/a.png"]),
            words(&["viewer", "/h/b.png"])
        ],
        "%f: one start per file"
    );

    started.borrow_mut().clear();
    let (t3, e3) = tile("viewer");
    assert!(!drop_on(&t3, &e3, &["/h/a.png"]), "no file code: refused");
    // A file with no path on this computer, or no file at all: `%F` has nothing to pass, no bounce.
    let (t4, e4) = tile("viewer %F");
    let remote = gtk::gio::File::for_uri("smb://nas/pic.png");
    assert!(
        !ui::dock::dropped(&e4, &t4, &launch, &[remote]),
        "no local path for %F: refused"
    );
    assert!(
        !ui::dock::dropped(&e4, &t4, &launch, &[]),
        "an empty drop: refused"
    );
    assert!(!t4.is_launching(), "a refused drop does not bounce");
    let unread = w::DockTile::new("folder", "Apps", || {});
    let none = ui::dock::entry_of(None);
    ui::dock::wire_tile(&unread, &none, &launch);
    assert!(!drop_on(&unread, &none, &["/h/a.png"]), "no entry: refused");
    assert!(started.borrow().is_empty(), "nothing started");

    let takes_files = t
        .button
        .observe_controllers()
        .into_iter()
        .flatten()
        .filter_map(|c| c.downcast::<gtk::DropTarget>().ok())
        .any(|d| {
            d.formats()
                .is_some_and(|f| f.contains_type(gdk::FileList::static_type()))
        });
    assert!(takes_files, "the tile takes dropped files");
    let menu = t
        .button
        .observe_controllers()
        .into_iter()
        .flatten()
        .any(|c| {
            c.downcast::<gtk::GestureClick>()
                .is_ok_and(|g| g.button() == 3)
        });
    assert!(menu, "right-click opens the tile menu");
}
