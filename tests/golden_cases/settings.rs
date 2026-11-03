// SPDX-License-Identifier: GPL-2.0-or-later
//! System Settings goldens beyond one render per page (those come from `ui::settings::PAGES` and
//! `SUBPAGES` in golden.rs): search over every pane and row, wide and narrow, and the Modules pane
//! when its list cannot be read and when modules-install gave up on a module and a media format. [`checks`]: what the panes do after a change that failed or
//! succeeded (audit5 S7-5, S7-6, S7-10 to S7-13).
use super::{Case, find_type, pump};
use gtk::prelude::*;
use shell::kit::widgets as w;
use shell::status::{Audio, Battery, Network};
use shell::system::{Action, ActionError, System};
use shell::ui;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

pub fn cases() -> Vec<Case> {
    let mut v = Vec::new();
    for (tag, size) in [("wide", w::APP_SIZE), ("narrow", w::APP_NARROW)] {
        for (name, query) in [("search-rows", "volume"), ("search-none", "zzz")] {
            v.push(Case {
                name: format!("settings-{name}-{tag}"),
                size,
                class: "preview-app",
                wallpaper: false,
                build: Box::new(move || {
                    let a = ui::settings::content_searching(
                        Rc::new(shell::system::Fake),
                        &ui::settings::Host::demo(),
                        query,
                    );
                    a.root.clone().upcast()
                }),
            });
        }
    }
    // S7-12: a list `wol` could not give is an error, not "no modules".
    v.push(Case {
        name: "settings-modules-error-wide".into(),
        size: w::APP_SIZE,
        class: "preview-app",
        wallpaper: false,
        build: Box::new(|| {
            let a = ui::settings::content(Rc::new(shell::system::Fake), &edge_host().0);
            a.open("Modules");
            pump(50);
            a.root.clone().upcast()
        }),
    });
    // What modules-install gave up on: a "Could Not Be Added" section with Retry on each.
    v.push(Case {
        name: "settings-modules-gave-up-wide".into(),
        size: w::APP_SIZE,
        class: "preview-app",
        wallpaper: false,
        build: Box::new(|| {
            let a = ui::settings::content(Rc::new(shell::system::Fake), &gave_up_host());
            a.open("Modules");
            pump(50);
            a.root.clone().upcast()
        }),
    });
    v
}

/// Every check below, run once before the goldens.
pub fn checks() {
    software_update_switch_flips_back_when_the_change_fails();
    appearance_puts_the_old_look_back_when_the_save_fails();
    effects_controls_show_the_saved_look_after_a_failed_save();
    restore_defaults_hides_every_reset_button();
    a_failed_shortcut_save_shows_the_saved_shortcuts();
    modules_run_one_job_at_a_time();
    modules_offer_retry_for_what_could_not_be_added();
    startup_security_finish_setup_runs_once();
    pointer_sliders_show_the_saved_value_after_a_failed_save();
    language_menu_shows_the_saved_language_after_a_failed_save();
    login_items_show_the_saved_list_after_a_failed_save();
}

/// [`shell::system::Fake`], except that every action succeeds (Settings saves through actions).
struct Accepts;

impl System for Accepts {
    fn battery(&self) -> Option<Battery> {
        shell::system::Fake.battery()
    }
    fn audio(&self) -> Audio {
        shell::system::Fake.audio()
    }
    fn network(&self) -> Network {
        shell::system::Fake.network()
    }
    fn brightness(&self) -> Option<u8> {
        shell::system::Fake.brightness()
    }
    fn perform(&self, _a: &Action) -> Result<(), ActionError> {
        Ok(())
    }
}

/// The demo host on [`ui::settings::DemoEdge`], and that machine (to queue the key).
fn edge_host() -> (ui::settings::Host, Arc<ui::settings::DemoEdge>) {
    let m = Arc::new(ui::settings::DemoEdge::default());
    let mut h = ui::settings::Host::demo();
    h.machine = m.clone();
    (h, m)
}

/// The demo host on a [`ui::settings::DemoEdge`] whose modules-install gave up on Craft Suite and
/// one media format.
fn gave_up_host() -> ui::settings::Host {
    let mut h = ui::settings::Host::demo();
    h.machine = Arc::new(ui::settings::DemoEdge {
        gave_up: true,
        ..Default::default()
    });
    h
}

/// The page `name` of a Settings window on `sys`, built and loaded.
fn page(sys: Rc<dyn System>, host: &ui::settings::Host, name: &str) -> (w::AppWindow, gtk::Widget) {
    let a = ui::settings::content(sys, host);
    a.open(name);
    pump(300);
    let p = a.content.visible_child().expect("a page shows");
    (a, p)
}

fn all<T: IsA<gtk::Widget>>(w: &gtk::Widget) -> Vec<T> {
    let mut v = Vec::new();
    find_type(w, &mut v);
    v
}

/// The control of type `T` on the row titled `title`.
fn on_row<T: IsA<gtk::Widget>>(page: &gtk::Widget, title: &str) -> T {
    for l in all::<gtk::Label>(page)
        .iter()
        .filter(|l| l.label() == title)
    {
        let mut up = l.parent();
        for _ in 0..4 {
            let Some(p) = up else { break };
            if let Some(t) = all::<T>(&p).into_iter().next() {
                return t;
            }
            up = p.parent();
        }
    }
    panic!("no row {title:?} with that control")
}

fn button(page: &gtk::Widget, label: &str) -> gtk::Button {
    all::<gtk::Button>(page)
        .into_iter()
        .find(|b| {
            b.label().as_deref() == Some(label)
                || all::<gtk::Label>(b.upcast_ref())
                    .iter()
                    .any(|l| l.label() == label)
        })
        .unwrap_or_else(|| panic!("no {label:?} button"))
}

/// `w` and every parent up to `page` are visible (the window is not mapped).
fn shown(w: &gtk::Widget, page: &gtk::Widget) -> bool {
    let mut at = Some(w.clone());
    while let Some(x) = at {
        if !x.is_visible() {
            return false;
        }
        if x == *page {
            return true;
        }
        at = x.parent();
    }
    true
}

fn says(page: &gtk::Widget, text: &str) -> bool {
    all::<gtk::Label>(page)
        .iter()
        .any(|l| l.label() == text && shown(l.upcast_ref(), page))
}

fn config_file(rel: &str) -> PathBuf {
    PathBuf::from(std::env::var("XDG_CONFIG_HOME").expect("golden config home")).join(rel)
}

/// S7-5: wol-admind refusing (here: no such action) leaves "Download new updates" on, as it is.
fn software_update_switch_flips_back_when_the_change_fails() {
    let (_a, p) = page(
        Rc::new(shell::system::Fake),
        &ui::settings::Host::demo(),
        "Software Update",
    );
    let dl: gtk::Switch = on_row(&p, "Download new updates");
    let main: gtk::Switch = on_row(&p, "Automatic updates");
    assert!(dl.is_active() && main.is_active(), "the demo has both on");
    dl.set_active(false);
    assert!(!dl.is_sensitive(), "the switch waits for its change");
    pump(100);
    assert!(
        dl.is_active() && dl.state() && dl.is_sensitive(),
        "a refused change must flip the switch back on"
    );
    assert!(main.is_active(), "the main switch follows what is on");
    assert!(says(&p, ActionError::Unavailable.message()), "and says why");
}

/// S7-6: a theme that cannot be saved leaves the old tile checked and says so.
fn appearance_puts_the_old_look_back_when_the_save_fails() {
    // A folder where the theme file goes: the save fails.
    let blocked = config_file(shared::theme::ACTIVE_FILE);
    std::fs::create_dir_all(&blocked).expect("block the theme file");
    let (_a, p) = page(
        Rc::new(shell::system::Fake),
        &ui::settings::Host::demo(),
        "Appearance",
    );
    let tile = |name: &str| {
        all::<gtk::ToggleButton>(&p)
            .into_iter()
            .filter(|t| t.has_css_class("preview-tile"))
            .find(|t| {
                all::<gtk::Label>(t.upcast_ref())
                    .iter()
                    .any(|l| l.label() == name)
            })
            .unwrap_or_else(|| panic!("no {name} tile"))
    };
    let checked = || -> Vec<bool> {
        ["Light", "Dark", "Auto"]
            .iter()
            .map(|n| tile(n).is_active())
            .collect()
    };
    let was = checked();
    let other = if tile("Dark").is_active() {
        "Light"
    } else {
        "Dark"
    };
    tile(other).set_active(true);
    pump(20);
    let now = checked();
    let _ = std::fs::remove_dir_all(&blocked);
    assert_eq!(now, was, "a failed save must check the old tile again");
    assert!(
        all::<gtk::Label>(&p).iter().any(|l| {
            shown(l.upcast_ref(), &p) && l.label().starts_with("This change couldn’t be saved")
        }),
        "and say so"
    );
}

/// S7-10: a slider whose save failed shows the saved value again (Desk read the file back).
fn effects_controls_show_the_saved_look_after_a_failed_save() {
    let (_a, p) = page(
        Rc::new(shell::system::Fake),
        &ui::settings::Host::demo(),
        "Effects & Looks",
    );
    let first = || {
        all::<gtk::Scale>(&p)
            .into_iter()
            .find(|s| s.is_sensitive())
            .expect("a slider")
    };
    let s = first();
    let saved = s.value();
    let adj = s.adjustment();
    let moved = if saved + adj.step_increment() <= adj.upper() {
        saved + adj.step_increment()
    } else {
        saved - adj.step_increment()
    };
    s.set_value(moved);
    pump(400);
    assert_eq!(
        first().value(),
        saved,
        "a slider must show the saved value after its save failed"
    );
}

/// A shortcut id that ships with a key, changed to off in the saved shortcuts file.
fn one_changed_shortcut() -> PathBuf {
    let defaults = wol_core::shortcuts::load_defaults(|p| std::fs::read_to_string(p).ok());
    let s = defaults
        .iter()
        .find(|s| s.editable && s.default.is_some())
        .expect("a shipped shortcut");
    let mut o = wol_core::shortcuts::Overrides::default();
    o.changed.insert(s.id.clone(), None);
    let f = config_file(wol_core::shortcuts::PREFS_FILE);
    std::fs::create_dir_all(f.parent().expect("dir")).expect("config dir");
    std::fs::write(&f, o.to_json()).expect("shortcuts file");
    f
}

fn visible_resets(p: &gtk::Widget) -> usize {
    all::<gtk::Button>(p)
        .iter()
        .filter(|b| {
            b.tooltip_text().as_deref() == Some("Reset to the default") && shown(b.upcast_ref(), p)
        })
        .count()
}

/// S7-11: Restore Defaults leaves no per-row Reset button behind.
fn restore_defaults_hides_every_reset_button() {
    let f = one_changed_shortcut();
    let (_a, p) = page(
        Rc::new(Accepts),
        &ui::settings::Host::demo(),
        "Keyboard Shortcuts",
    );
    let before = visible_resets(&p);
    button(&p, "Restore Defaults").emit_clicked();
    pump(50);
    let after = visible_resets(&p);
    let _ = std::fs::remove_file(&f);
    assert_eq!(before, 1, "the changed shortcut offers Reset");
    assert_eq!(after, 0, "Restore Defaults must hide every Reset button");
}

/// S7-10: a shortcut change that could not be saved: the rows show the saved shortcuts again.
fn a_failed_shortcut_save_shows_the_saved_shortcuts() {
    let f = one_changed_shortcut();
    let (_a, p) = page(
        Rc::new(shell::system::Fake),
        &ui::settings::Host::demo(),
        "Keyboard Shortcuts",
    );
    button(&p, "Restore Defaults").emit_clicked();
    pump(50);
    let after = visible_resets(&p);
    let _ = std::fs::remove_file(&f);
    assert_eq!(
        after, 1,
        "the saved change is still there: its Reset shows again"
    );
}

/// S7-12: Install waits (spinner, off) until its run ended; a second click starts nothing.
fn modules_run_one_job_at_a_time() {
    let (_a, p) = page(
        Rc::new(shell::system::Fake),
        &ui::settings::Host::demo(),
        "Modules",
    );
    let install = button(&p, "Install");
    install.emit_clicked();
    assert!(!install.is_sensitive(), "Install waits for its run");
    let spinning = all::<gtk::Spinner>(&p)
        .iter()
        .any(|s| s.is_visible() && s.is_spinning());
    assert!(spinning, "a spinner shows while it runs");
    pump(300);
    // The run failed (Fake): the page reloaded and Install is offered again.
    assert!(button(&p, "Install").is_sensitive());
}

/// What modules-install gave up on shows under "Could Not Be Added" with Retry (not also under
/// Available); Retry is one job at a time, as Install is.
fn modules_offer_retry_for_what_could_not_be_added() {
    let (_a, p) = page(Rc::new(shell::system::Fake), &gave_up_host(), "Modules");
    assert!(says(&p, "Could Not Be Added"), "the section shows");
    assert!(says(&p, "Craft Suite") && says(&p, "gst-libav"));
    assert!(says(&p, shell::copy::settings::MODULES_GAVE_UP));
    assert!(says(&p, shell::copy::settings::MODULES_GAVE_UP_MEDIA));
    let labelled = |label: &str| -> Vec<gtk::Button> {
        all::<gtk::Button>(&p)
            .into_iter()
            .filter(|b| {
                b.label().as_deref() == Some(label)
                    || all::<gtk::Label>(b.upcast_ref())
                        .iter()
                        .any(|l| l.label() == label)
            })
            .collect()
    };
    let retries = || labelled("Retry");
    assert_eq!(retries().len(), 2, "one Retry per item");
    let installs = labelled("Install").len();
    // Demo: Craft Suite and Console are not installed; Craft Suite moved to the new section.
    assert_eq!(installs, 1, "a given-up module is not offered twice");
    let r = retries()[0].clone();
    r.emit_clicked();
    assert!(!r.is_sensitive(), "Retry waits for its run");
    r.emit_clicked();
    pump(300);
    // The run failed (Fake): the page reloaded and both are offered again.
    assert_eq!(retries().len(), 2);
    assert!(retries().iter().all(|b| b.is_sensitive()));
}

/// The slider on `page` titled `title`, after its save failed: back at the saved value.
fn slider_shows_saved_value_after_failed_save(page_name: &str, title: &str) {
    let (_a, p) = page(
        Rc::new(shell::system::Fake),
        &ui::settings::Host::demo(),
        page_name,
    );
    let s: gtk::Scale = on_row(&p, title);
    let saved = s.value();
    let adj = s.adjustment();
    let moved = if saved + adj.step_increment() <= adj.upper() {
        saved + adj.step_increment()
    } else {
        saved - adj.step_increment()
    };
    s.set_value(moved);
    pump(400);
    assert!(
        (s.value() - saved).abs() < 1e-6,
        "{page_name} > {title}: the slider must show the saved value after its save failed ({} != {saved})",
        s.value()
    );
}

/// Desk::set_or in Accessibility and Trackpad & Mouse: a failed save puts the slider back.
fn pointer_sliders_show_the_saved_value_after_a_failed_save() {
    slider_shows_saved_value_after_failed_save("Accessibility", "Pointer size");
    slider_shows_saved_value_after_failed_save("Trackpad & Mouse", "Tracking speed");
}

/// Desk::set_or in Language & Region: a failed save selects the saved language again and does
/// not promise it for the next login.
fn language_menu_shows_the_saved_language_after_a_failed_save() {
    let (_a, p) = page(
        Rc::new(shell::system::Fake),
        &ui::settings::Host::demo(),
        "Language & Region",
    );
    let d: gtk::DropDown = on_row(&p, "Preferred language");
    let saved = d.selected();
    d.set_selected(if saved == 1 { 2 } else { 1 });
    pump(400);
    assert_eq!(d.selected(), saved, "the saved language is selected again");
    assert!(
        !says(&p, "Apps use the new language the next time you log in."),
        "no next-login note for a language that was not saved"
    );
}

/// Desk::set_or in Login Items: a removal that could not be saved shows the item again.
fn login_items_show_the_saved_list_after_a_failed_save() {
    let f = config_file(wol_core::hyprprefs::PREFS_FILE);
    std::fs::create_dir_all(f.parent().expect("dir")).expect("config dir");
    let had = std::fs::read_to_string(&f).ok();
    let mut prefs = wol_core::hyprprefs::DesktopPrefs::parse(had.as_deref());
    prefs.login_items = vec!["k8-test-app --start".into()];
    std::fs::write(&f, prefs.to_json()).expect("desktop prefs");
    let (_a, p) = page(
        Rc::new(shell::system::Fake),
        &ui::settings::Host::demo(),
        "Login Items",
    );
    let shows = |p: &gtk::Widget| says(p, "k8-test-app --start");
    let before = shows(&p);
    let remove = all::<gtk::Button>(&p)
        .into_iter()
        .find(|b| b.tooltip_text().as_deref() == Some("Remove"))
        .expect("a Remove button");
    remove.emit_clicked();
    let gone = !shows(&p);
    pump(400);
    let back = shows(&p);
    match had {
        Some(t) => std::fs::write(&f, t).expect("put the prefs back"),
        None => {
            let _ = std::fs::remove_file(&f);
        }
    }
    assert!(before && gone, "the item shows, then goes on Remove");
    assert!(
        back,
        "a removal that could not be saved shows the item again"
    );
}

/// S7-13: after Finish Setup queued the key the button stays off and the footnote shows.
fn startup_security_finish_setup_runs_once() {
    let (host, m) = edge_host();
    let (_a, p) = page(Rc::new(Accepts), &host, "Startup Security");
    let queued = shell::copy::settings::STARTUP_KEY_QUEUED;
    assert!(!says(&p, queued), "nothing queued yet");
    let finish = button(&p, "Finish Setup");
    // What boot-enroll leaves behind once it queued the key.
    m.queued.store(true, std::sync::atomic::Ordering::Relaxed);
    finish.emit_clicked();
    pump(200);
    assert!(
        !finish.is_sensitive(),
        "a second click must not enroll again"
    );
    assert!(
        says(&p, queued),
        "the key-queued footnote shows after the action"
    );
}
