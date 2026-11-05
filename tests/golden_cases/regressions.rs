// SPDX-License-Identifier: GPL-2.0-or-later
//! Regression checks that run before the screenshots: each one fails on the bug it was written for.
use super::*;
use std::sync::Arc;

/// Settings > Effects & Looks keeps its state alive once built: picking a look and then Restore
/// Defaults checks the WolOS card again (every card, control and button did nothing when the
/// page's state was dropped at the end of building it).
pub(super) fn effects_page_answers_after_it_is_built() {
    let a = ui::settings::content(Arc::new(shell::system::Fake), &ui::settings::Host::demo());
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
    let a = ui::settings::content(Arc::new(shell::system::Fake), &ui::settings::Host::demo());
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

/// Changes [`AsksFirst`] was handed with a password.
static SENT_WITH_PASSWORD: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A System whose changes need the password: NeedsAuth with none, Ok with one (what wol-admind
/// answers for an install the person is allowed to make).
pub(super) struct AsksFirst {
    /// What it answers once the password came: Ok, or this error.
    pub(super) then: Option<shell::system::ActionError>,
}

impl shell::system::System for AsksFirst {
    fn battery(&self) -> Option<shell::status::Battery> {
        shell::system::Fake.battery()
    }
    fn audio(&self) -> shell::status::Audio {
        shell::system::Fake.audio()
    }
    fn network(&self) -> shell::status::Network {
        shell::system::Fake.network()
    }
    fn brightness(&self) -> Option<u8> {
        shell::system::Fake.brightness()
    }
    fn perform(&self, _a: &shell::system::Action) -> Result<(), shell::system::ActionError> {
        Err(shell::system::ActionError::NeedsAuth)
    }
    fn spawn_with(&self, _a: shell::system::Action, pw: Option<&str>) -> shell::system::Done {
        let (tx, rx) = async_channel::bounded(1);
        let _ = tx.try_send(match pw {
            None => Err(shell::system::ActionError::NeedsAuth),
            Some(_) => {
                SENT_WITH_PASSWORD.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                self.then.map_or(Ok(()), Err)
            }
        });
        rx
    }
}

/// QA-VIS2-46/47: Get on an official app asks for the password with the install's own sentence
/// (not the Community tab's warning) and its tile still offers Get until the password is given;
/// then it installs.
pub(super) fn store_get_waits_for_the_password() {
    let (_s, root, get) = store_get_asked(None);
    assert!(
        get.is_visible() && get.label().as_deref() == Some("Get"),
        "no Installing before the password"
    );
    give_password(&root);
    // A tile hides its pill for the ring, the hero's button says Installing….
    assert!(
        !get.is_visible() || get.label().as_deref() != Some("Get"),
        "Installing once the password went"
    );
    pump(100);
    assert_eq!(get.label().as_deref(), Some("Open"), "installed");
}

/// QA-VIS2-48: an install that failed after the password says so in one sentence on the page
/// (it went back to Get with no word while wol-admind logged the failure).
pub(super) fn a_failed_store_install_says_so() {
    let (_s, root, get) = store_get_asked(Some(shell::system::ActionError::Failed));
    give_password(&root);
    pump(100);
    assert_eq!(get.label().as_deref(), Some("Get"), "offered again");
    let mut labels: Vec<gtk::Label> = Vec::new();
    find_type(&root, &mut labels);
    let line = labels
        .iter()
        .find(|l| l.label().contains("couldn’t be installed"))
        .expect("the failure's sentence");
    let shown = line.is_visible()
        && line
            .ancestor(gtk::Revealer::static_type())
            .and_downcast::<gtk::Revealer>()
            .is_none_or(|r| r.reveals_child());
    assert!(shown, "the sentence is shown");
}

/// QA-STORE-59: each way an install ends without the app says why, in the system's own sentence
/// (offline, disk full, a package that does not check, an update holding the lock, and a failure
/// that names nothing), on the status sheet that is shown, and the tile offers Get again. The
/// sentence is what wol-admind answered (`failed <message>`, the Store reads it with
/// `wol_core::controls::take_failure`); before, every kind got the one guess "Check that you’re
/// online", when it got a word at all.
pub(super) fn every_failed_install_says_why() {
    use wol_core::pkg_failure::{Cause, sentence, unknown};
    let kinds = [
        sentence(Cause::Download, true),
        sentence(Cause::NoRoom, true),
        sentence(Cause::Damaged, true),
        sentence(Cause::UpdateRunning, true),
        unknown(true),
    ];
    for said in kinds {
        let (_s, root, get) = store_get_asked(Some(shell::system::ActionError::Failed));
        wol_core::controls::note_failure(
            shared::admin::AdminAction::PkgInstall,
            Some(said.clone()),
        );
        give_password(&root);
        pump(150);
        assert_eq!(get.label().as_deref(), Some("Get"), "offered again: {said}");
        let mut labels: Vec<gtk::Label> = Vec::new();
        find_type(&root, &mut labels);
        let line = labels
            .iter()
            .find(|l| l.has_css_class("store-status"))
            .expect("the status line");
        assert_eq!(line.label(), said);
        let sheet = line
            .ancestor(gtk::Revealer::static_type())
            .and_downcast::<gtk::Revealer>()
            .expect("the status sheet");
        assert!(sheet.reveals_child() && line.is_visible(), "shown: {said}");
        assert!(
            !line.label().contains("online") || said.contains("download failed"),
            "only a failed download blames the connection: {said}"
        );
        // Taken once: a later failure without a sentence does not repeat this one.
        assert_eq!(
            wol_core::controls::take_failure(shared::admin::AdminAction::PkgInstall),
            None
        );
    }
}

/// A Store on [`AsksFirst`] (answering `then` after the password) where Get was clicked on an
/// official app: the Authenticate sheet is up with the install's sentence. Returns the window
/// and that Get button.
fn store_get_asked(
    then: Option<shell::system::ActionError>,
) -> (ui::store::Store, gtk::Widget, gtk::Button) {
    let s = ui::store::content(
        std::sync::Arc::new(AsksFirst { then }),
        ui::store::Env {
            data_dirs: vec![],
            pkg_search: false,
            flatpaks: vec![],
            installed: Some(Default::default()),
        },
    );
    s.aw.select("Discover");
    pump(100);
    let root: gtk::Widget = s.aw.root.clone().upcast();
    let mut buttons: Vec<gtk::Button> = Vec::new();
    find_type(&root, &mut buttons);
    let get = buttons
        .into_iter()
        .find(|b| b.label().as_deref() == Some("Get") && b.is_visible() && b.is_sensitive())
        .expect("a tile offers Get");
    get.emit_clicked();
    pump(500);
    let mut labels: Vec<gtk::Label> = Vec::new();
    find_type(&root, &mut labels);
    let says = |t: &str| labels.iter().any(|l| l.label().starts_with(t));
    assert!(
        says("Enter your password to install "),
        "the sheet names the install"
    );
    assert!(
        !says("Community packages are not reviewed"),
        "an official app is not warned about as a Community package"
    );
    (s, root, get)
}

/// Types a password into the Authenticate sheet that is up and presses Return.
fn give_password(root: &gtk::Widget) {
    let mut fields: Vec<gtk::PasswordEntry> = Vec::new();
    find_type(root, &mut fields);
    let field = fields
        .into_iter()
        .find(|f| {
            f.ancestor(gtk::Revealer::static_type())
                .and_downcast::<gtk::Revealer>()
                .is_some_and(|r| r.reveals_child())
        })
        .expect("the sheet is up with its password field");
    field.set_text("secret");
    field.emit_by_name::<()>("activate", &[]);
}

/// QA-VIS2-37: a page's sheet (Users & Groups > Add User…) closes on Esc and when another page
/// opens; it stayed through Hardware, Keyboard and Printers and only its Cancel closed it.
pub(super) fn a_page_sheet_closes_on_esc_and_on_another_page() {
    let a = ui::settings::content(
        std::sync::Arc::new(shell::system::Fake),
        &ui::settings::Host::demo(),
    );
    a.open("Users & Groups");
    pump(300);
    let root: gtk::Widget = a.root.clone().upcast();
    let sheet = || {
        let mut labels: Vec<gtk::Label> = Vec::new();
        find_type(&root, &mut labels);
        labels
            .into_iter()
            .find(|l| l.label() == "New account")
            .and_then(|l| l.ancestor(gtk::Revealer::static_type()))
            .and_downcast::<gtk::Revealer>()
            .expect("the New account sheet")
    };
    let add = || {
        let mut all: Vec<gtk::Button> = Vec::new();
        find_type(&root, &mut all);
        all.into_iter()
            .find(|b| {
                let mut l = Vec::new();
                find_type::<gtk::Label>(b.upcast_ref(), &mut l);
                b.label().as_deref() == Some("Add User…")
                    || l.iter().any(|l| l.label() == "Add User…")
            })
            .expect("Add User…")
    };
    add().emit_clicked();
    assert!(sheet().reveals_child(), "Add User… opens the sheet");
    let esc = a
        .root
        .observe_controllers()
        .iter::<glib::Object>()
        .filter_map(|c| c.ok()?.downcast::<gtk::EventControllerKey>().ok())
        .any(|k| {
            k.emit_by_name::<bool>(
                "key-pressed",
                &[&gdk::Key::Escape, &9u32, &gdk::ModifierType::empty()],
            )
        });
    assert!(esc, "Esc is taken");
    assert!(!sheet().reveals_child(), "Esc closes the sheet");
    add().emit_clicked();
    assert!(sheet().reveals_child());
    a.open("Hardware");
    pump(50);
    assert!(!sheet().reveals_child(), "another page closes the sheet");
}

/// The Store after a failed install of an official app (golden store-install-failed-*): its
/// sentence under the toolbar, Get offered again.
pub(super) fn store_after_a_failed_install() -> gtk::Widget {
    let (s, root, _get) = store_get_asked(Some(shell::system::ActionError::Failed));
    give_password(&root);
    pump(400);
    // The window outlives this function (the case renders it); the Store's state goes with it.
    std::mem::forget(s);
    root
}

/// QA-DESIGN-27: arriving on the Community tab asks for nothing (the sheet came up over the
/// tab's own Enter Password button); that button is the way in.
pub(super) fn the_community_tab_asks_only_from_its_button() {
    let s = ui::store::content(
        std::sync::Arc::new(AsksFirst { then: None }),
        ui::store::Env {
            data_dirs: vec![],
            pkg_search: true,
            flatpaks: vec![],
            installed: Some(Default::default()),
        },
    );
    let root: gtk::Widget = s.aw.root.clone().upcast();
    let win = gtk::Window::builder().child(&root).build();
    win.present();
    s.aw.open("Community");
    pump(300);
    let sheet_up = || {
        let mut fields: Vec<gtk::PasswordEntry> = Vec::new();
        find_type(&root, &mut fields);
        fields.iter().any(|f| {
            f.ancestor(gtk::Revealer::static_type())
                .and_downcast::<gtk::Revealer>()
                .is_some_and(|r| r.reveals_child())
        })
    };
    assert!(!sheet_up(), "no sheet on arrival");
    let mut buttons: Vec<gtk::Button> = Vec::new();
    find_type(&root, &mut buttons);
    buttons
        .into_iter()
        .find(|b| b.label().as_deref() == Some("Enter Password"))
        .expect("Enter Password")
        .emit_clicked();
    pump(100);
    assert!(sheet_up(), "Enter Password asks");
    win.destroy();
}

/// QA-STORE-78: typed results leave no sidebar row lit ('Kits' stayed over 'Results for gimp'),
/// and picking a page again empties the search field.
pub(super) fn store_search_and_sidebar_agree() {
    let s = ui::store::content(
        std::sync::Arc::new(shell::system::Fake),
        ui::store::Env {
            data_dirs: vec![],
            pkg_search: false,
            flatpaks: vec![],
            installed: Some(Default::default()),
        },
    );
    s.aw.select("Kits");
    pump(50);
    s.search.set_text("gimp");
    pump(700);
    assert_eq!(
        s.aw.content.visible_child_name().as_deref(),
        Some("Search"),
        "the results show"
    );
    assert!(
        s.aw.sidebar.selected_row().is_none(),
        "no page row lit over the results"
    );
    s.aw.select("Play");
    pump(400);
    assert_eq!(s.search.text(), "", "a page picked leaves the search");
    assert_eq!(s.aw.content.visible_child_name().as_deref(), Some("Play"));
}

/// QA-DESIGN-19: a 640-wide window (the smallest screen) collapses the sidebar: with it the content
/// had 400 px, less than the 480 a narrow window gives it. At the default width it stays.
pub(super) fn a_640_window_collapses_the_sidebar() {
    for (width, collapsed) in [(640, true), (w::NARROW - 1, true), (w::APP_SIZE.0, false)] {
        let a = ui::settings::content(
            std::sync::Arc::new(shell::system::Fake),
            &ui::settings::Host::demo(),
        );
        let frame = gtk::Overlay::new();
        frame.set_child(Some(&a.root));
        frame.set_size_request(width, 400);
        let win = gtk::Window::builder()
            .decorated(false)
            .default_width(width)
            .default_height(400)
            .child(&frame)
            .build();
        win.present();
        pump(300);
        assert_eq!(
            a.sidebar_toggle.is_visible(),
            collapsed,
            "{width} px: sidebar collapsed = {collapsed}"
        );
        win.destroy();
    }
}

/// QA-STORE-65: two Gets within a moment share one password sheet and both reach wol-admind (the
/// second ring used to reset to Get with no request behind it).
pub(super) fn two_gets_in_a_row_both_reach_the_system() {
    let s = ui::store::content(
        std::sync::Arc::new(AsksFirst { then: None }),
        ui::store::Env {
            data_dirs: vec![],
            pkg_search: false,
            flatpaks: vec![],
            installed: Some(Default::default()),
        },
    );
    s.aw.select("Discover");
    pump(100);
    let root: gtk::Widget = s.aw.root.clone().upcast();
    let mut buttons: Vec<gtk::Button> = Vec::new();
    find_type(&root, &mut buttons);
    let gets: Vec<gtk::Button> = buttons
        .into_iter()
        .filter(|b| b.label().as_deref() == Some("Get") && b.is_visible() && b.is_sensitive())
        .take(2)
        .collect();
    assert_eq!(gets.len(), 2, "two apps offer Get");
    SENT_WITH_PASSWORD.store(0, std::sync::atomic::Ordering::SeqCst);
    for g in &gets {
        g.emit_clicked();
        pump(30);
    }
    pump(400);
    give_password(&root);
    pump(400);
    assert_eq!(
        SENT_WITH_PASSWORD.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "both installs were sent"
    );
    for g in &gets {
        assert_eq!(g.label().as_deref(), Some("Open"), "both ended installed");
    }
}

/// The button labelled `text` under `root`.
fn button_named(root: &gtk::Widget, text: &str) -> gtk::Button {
    let mut all: Vec<gtk::Button> = Vec::new();
    find_type(root, &mut all);
    all.into_iter()
        .find(|b| b.label().as_deref() == Some(text))
        .unwrap_or_else(|| panic!("a button {text}"))
}

/// QA-VIS1-22 / QA-VIS1-12: Clear is off while the clipboard history and the Notification Center
/// are empty (and the Center says so), on again with a note in them.
pub(super) fn clear_is_off_while_there_is_nothing_to_clear() {
    use shell::clipboard::History;
    let mut h = History::with_pins(vec![]);
    let p = ui::clipboard::Panel::new(History::with_pins(vec![]));
    let root: gtk::Widget = p.root.clone().upcast();
    assert!(
        !button_named(&root, "Clear").is_sensitive(),
        "empty clipboard history"
    );
    h.push("hello");
    *p.history.borrow_mut() = h;
    p.refresh();
    assert!(button_named(&root, "Clear").is_sensitive(), "one entry");

    let rows: Rc<std::cell::RefCell<ui::notify::Center<gtk::Box>>> = Default::default();
    let (g, list) = ui::notify::center_panel(rows.clone(), || {});
    let root: gtk::Widget = g.clone().upcast();
    let mut labels: Vec<gtk::Label> = Vec::new();
    find_type(&root, &mut labels);
    let says = labels
        .iter()
        .find(|l| l.label() == "No notifications")
        .expect("the empty sentence")
        .clone();
    assert!(says.is_visible() && !button_named(&root, "Clear").is_sensitive());
    let row = gtk::Box::new(gtk::Orientation::Vertical, 0);
    rows.borrow_mut().push(1, row.clone());
    list.prepend(&row);
    assert!(!says.is_visible() && button_named(&root, "Clear").is_sensitive());
    button_named(&root, "Clear").emit_clicked();
    assert!(says.is_visible() && !button_named(&root, "Clear").is_sensitive());
}

/// QA-STORE-81 / QA-STORE-34: the Store window has something to show at once (a spinner) while
/// `Env::read` runs on a worker thread, and the Store takes its place when it is done. A slow
/// read (here 400 ms; `flatpak list` waits up to 5 s) never holds the GTK thread: the main loop
/// still runs during it.
pub(super) fn the_store_window_shows_a_spinner_while_the_env_is_read() {
    let win = gtk::Window::builder()
        .default_width(900)
        .default_height(600)
        .build();
    win.present();
    let ticks = Rc::new(std::cell::Cell::new(0));
    let t = ticks.clone();
    let tick = glib::timeout_add_local(std::time::Duration::from_millis(20), move || {
        t.set(t.get() + 1);
        glib::ControlFlow::Continue
    });
    ui::store::show_when_ready(
        &win,
        None,
        false,
        || {
            std::thread::sleep(std::time::Duration::from_millis(400));
            ui::store::Env {
                data_dirs: vec![],
                pkg_search: false,
                flatpaks: vec![],
                installed: Some(Default::default()),
            }
        },
        std::sync::Arc::new(shell::system::Fake),
    );
    assert!(
        win.child().is_some_and(|c| c.is::<gtk::Spinner>()),
        "a spinner until the read is done"
    );
    pump(200);
    assert!(
        ticks.get() >= 5,
        "the GTK thread kept running: {} ticks",
        ticks.get()
    );
    assert!(
        win.child().is_some_and(|c| c.is::<gtk::Spinner>()),
        "still reading"
    );
    pump(800);
    tick.remove();
    let store = win.child().expect("the Store");
    assert!(
        !store.is::<gtk::Spinner>(),
        "the Store replaced the spinner"
    );
    let mut entries: Vec<gtk::SearchEntry> = Vec::new();
    find_type(&store, &mut entries);
    assert!(!entries.is_empty(), "it is the Store (has its search)");
    win.destroy();
}

/// INT-1 sheet 3: an installed package with nothing to open showed "Installed" as the filled
/// accent pill next to Remove, so a state read as an action. HIG: the state is a quiet Kit pill
/// (`label.badge`, not a button) and "Open" stays the one primary button.
pub(super) fn installed_is_a_state_and_open_the_action() {
    // Shown: the widget and every ancestor visible (the page is not in a window).
    fn shown(w: &gtk::Widget) -> bool {
        w.is_visible() && w.parent().is_none_or(|p| shown(&p))
    }
    fn text_of(w: &gtk::Widget) -> Option<String> {
        if let Some(l) = w.downcast_ref::<gtk::Label>() {
            return Some(l.label().to_string());
        }
        w.downcast_ref::<gtk::Button>()
            .and_then(|b| b.label())
            .map(|l| l.to_string())
    }
    fn all(w: &gtk::Widget, out: &mut Vec<gtk::Widget>) {
        out.push(w.clone());
        let mut c = w.first_child();
        while let Some(ch) = c {
            all(&ch, out);
            c = ch.next_sibling();
        }
    }
    let page = |flatpaks: Vec<String>, subject: ui::store::Subject, installed: Option<&str>| {
        let s = ui::store::content(
            std::sync::Arc::new(shell::system::Fake),
            ui::store::Env {
                data_dirs: vec![],
                pkg_search: false,
                flatpaks,
                installed: Some(Default::default()),
            },
        );
        s.show_details(
            subject,
            ui::store::Loaded {
                installed: installed.map(Into::into),
                ..Default::default()
            },
        );
        pump(50);
        let mut v = Vec::new();
        all(s.aw.root.upcast_ref(), &mut v);
        (s, v)
    };
    let named = |v: &[gtk::Widget], t: &str| -> Vec<gtk::Widget> {
        v.iter()
            .filter(|w| shown(w) && text_of(w).as_deref() == Some(t))
            .cloned()
            .collect()
    };
    // A package without a desktop id, installed: nothing to open.
    let hit = wol_core::store::PkgHit {
        name: "demo".into(),
        summary: "An app from three places".into(),
    };
    let (_keep, v) = page(vec![], ui::store::Subject::pkg(&hit), Some("3.0-1"));
    let state = named(&v, "Installed");
    assert_eq!(state.len(), 1, "one \"Installed\" on the page");
    let state = &state[0];
    assert!(
        state.is::<gtk::Label>() && state.has_css_class("badge"),
        "\"Installed\" is a Kit pill (label.badge), got {} {:?}",
        state.type_().name(),
        state.css_classes()
    );
    assert!(
        !state.ancestor(gtk::Button::static_type()).is_some(),
        "\"Installed\" is not inside a button"
    );
    assert!(
        !v.iter()
            .any(|w| shown(w) && w.is::<gtk::Button>() && w.has_css_class("get-primary")),
        "no primary pill when there is nothing to do"
    );
    assert!(
        named(&v, "Remove")
            .iter()
            .any(|w| w.is::<gtk::Button>() || w.ancestor(gtk::Button::static_type()).is_some()),
        "Remove stays a button"
    );
    // A Flatpak that is installed opens: "Open" is the primary button, no state pill.
    let fh = wol_core::flatpak::Hit {
        id: "org.gimp.GIMP".into(),
        name: "GIMP".into(),
        summary: "Edit images".into(),
    };
    let (_keep, v) = page(
        vec!["org.gimp.GIMP".into()],
        ui::store::Subject::flathub(&fh),
        None,
    );
    let open = named(&v, "Open");
    assert!(
        open.iter()
            .any(|w| w.is::<gtk::Button>() && w.has_css_class("get-primary")),
        "\"Open\" is the primary pill: {open:?}"
    );
    assert!(
        named(&v, "Installed").is_empty(),
        "no state pill next to Open"
    );
}

/// INT-1 sheet 2: the shortcut sheet scrolled its columns at 640 px and the last row showed half
/// ("Actual Size"). Now no row is ever cut: on a 1080-high screen the sheet is as tall as its rows,
/// on a 768-high one whole sections go on pages; every row is on a page, each page fits.
/// Runs with the kit stylesheet on (the rows' real heights).
pub(super) fn shortcut_sheet_rows_are_whole() {
    use ui::shortcut_sheet as sheet;
    let list =
        shell::shortcut_sheet::sections(&wol_core::shortcuts::load_defaults(|_| None), false);
    let want: usize = list.iter().map(|s| s.rows.len()).sum();
    for screen in [1080, sheet::SMALL_SCREEN] {
        let max = sheet::max_height(screen);
        let g = sheet::content(&list, max);
        let win = gtk::Window::builder().decorated(false).build();
        win.add_css_class("preview");
        win.set_child(Some(&g));
        win.present();
        pump(200);
        let mut stacks: Vec<gtk::Stack> = Vec::new();
        find_type(g.upcast_ref(), &mut stacks);
        let pages: Vec<gtk::Widget> = match stacks.first() {
            Some(st) => {
                let mut v = Vec::new();
                let mut c = st.first_child();
                while let Some(p) = c {
                    v.push(p.clone());
                    c = p.next_sibling();
                }
                v
            }
            None => vec![g.clone().upcast()],
        };
        let mut seen = 0;
        for (pi, p) in pages.iter().enumerate() {
            if let Some(st) = stacks.first() {
                st.set_visible_child(p);
                // Laid out on the next frame: wait for it (Broadway's clock can be slow).
                for _ in 0..40 {
                    pump(50);
                    if p.height() > 0 && p.is_drawable() {
                        break;
                    }
                }
            }
            let mut secs = Vec::new();
            find_class(p, "shortcut-sheet-section", &mut secs);
            for s in &secs {
                let mut r = s.first_child().and_then(|t| t.next_sibling());
                while let Some(row) = r {
                    seen += 1;
                    let rh = row.height();
                    assert!(rh > 0, "{screen}: a row with no height on page {pi}");
                    // Whole inside every ancestor up to the sheet: no viewport or clamp cuts it.
                    let mut a = row.parent();
                    while let Some(anc) = a {
                        let b = row.compute_bounds(&anc).expect("bounds");
                        assert!(
                            b.y() >= -0.5 && b.y() + b.height() <= anc.height() as f32 + 0.5,
                            "{screen}: row {}..{} cut by a {} {} high",
                            b.y(),
                            b.y() + b.height(),
                            anc.type_().name(),
                            anc.height()
                        );
                        if anc == *g.upcast_ref::<gtk::Widget>() {
                            break;
                        }
                        a = anc.parent();
                    }
                    r = row.next_sibling();
                }
            }
        }
        assert_eq!(seen, want, "{screen}: every row on a page, once");
        assert!(
            g.height() <= max,
            "{screen}: sheet {} high, the screen gives {max}",
            g.height()
        );
        if screen == 1080 {
            assert!(stacks.is_empty(), "1080: one page, the sheet grows");
        } else {
            assert!(
                pages.len() > 1,
                "768: the rows do not fit one page, so they go on pages"
            );
        }
        win.destroy();
    }
}

/// E2E-OPEN: a file dropped on a Dock tile opens with that tile's app (`ui::dock::dropped_on_tile`, what the
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
        ui::dock::dropped_on_tile(e, t, &launch, &files)
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
        !ui::dock::dropped_on_tile(&e4, &t4, &launch, &[remote]),
        "no local path for %F: refused"
    );
    assert!(
        !ui::dock::dropped_on_tile(&e4, &t4, &launch, &[]),
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
