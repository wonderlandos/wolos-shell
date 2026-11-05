// SPDX-License-Identifier: GPL-2.0-or-later
//! System Settings goldens beyond one render per page (those come from `ui::settings::PAGES` and
//! `SUBPAGES` in golden.rs): search over every pane and row, wide and narrow, and the Modules pane
//! when its list cannot be read and when modules-install gave up on a module and a media format,
//! and Displays with two displays. [`checks`]: what the panes do after a change that failed or
//! succeeded (audit5 S7-5, S7-6, S7-10 to S7-13).
use super::{Case, find_type, pump};
use gtk::prelude::*;
use shell::kit::widgets as w;
use shell::status::{Audio, Battery, Network};
use shell::system::{Action, ActionError, System};
use shell::ui;
use std::path::PathBuf;
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
                        Arc::new(shell::system::Fake),
                        &ui::settings::Host::demo(),
                        query,
                    );
                    a.root.clone().upcast()
                }),
            });
        }
    }
    // QA-SEARCH-1: the Search bar's result for 'privacy settings' (and a launch of `wol-settings
    // privacy#screen-recording`) opens Settings on the Privacy pane with the Screen Recording
    // section lit, not on the first pane.
    for (tag, size) in [("wide", w::APP_SIZE), ("narrow", w::APP_NARROW)] {
        v.push(Case {
            name: format!("settings-opened-from-search-privacy-{tag}"),
            size,
            class: "preview-app",
            wallpaper: false,
            build: Box::new(|| {
                let a = ui::settings::content(
                    Arc::new(shell::system::Fake),
                    &ui::settings::Host::demo(),
                );
                let args = ["wol-settings", "privacy#screen-recording"].map(String::from);
                let target = ui::settings::requested(&args).expect("a page");
                ui::settings::open_target(&a, &target);
                pump(1200);
                a.root.clone().upcast()
            }),
        });
    }
    // S7-12: a list `wol` could not give is an error, not "no modules".
    v.push(Case {
        name: "settings-modules-error-wide".into(),
        size: w::APP_SIZE,
        class: "preview-app",
        wallpaper: false,
        build: Box::new(|| {
            let a = ui::settings::content(Arc::new(shell::system::Fake), &edge_host().0);
            a.open("Modules");
            pump(50);
            a.root.clone().upcast()
        }),
    });
    // ADR 0023: one copy of WolOS has no restore point, and Software Update says so.
    v.push(Case {
        name: "settings-softwareupdate-onecopy-wide".into(),
        size: w::APP_SIZE,
        class: "preview-app",
        wallpaper: false,
        build: Box::new(|| {
            let mut h = ui::settings::Host::demo();
            h.machine = Arc::new(ui::settings::DemoEdge {
                one_copy: true,
                ..Default::default()
            });
            let a = ui::settings::content(Arc::new(shell::system::Fake), &h);
            a.open("Software Update");
            pump(300);
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
            let a = ui::settings::content(Arc::new(shell::system::Fake), &gave_up_host());
            a.open("Modules");
            pump(50);
            a.root.clone().upcast()
        }),
    });
    // QA-ACC-11: a Japanese account's Keyboard page offers the input methods (the `ime` module).
    v.push(Case {
        name: "settings-keyboard-japanese-wide".into(),
        size: w::APP_SIZE,
        class: "preview-app",
        wallpaper: false,
        build: Box::new(|| {
            let a = in_japanese(|| {
                let a = ui::settings::content(Arc::new(shell::system::Fake), &ime_host());
                a.open("Keyboard");
                pump(300);
                a
            });
            a.root.clone().upcast()
        }),
    });
    // Two displays: one section each (the built-in one first, it is the main display), one Night
    // Shift switch for both.
    for (tag, size) in [("wide", w::APP_SIZE), ("narrow", w::APP_NARROW)] {
        v.push(Case {
            name: format!("settings-displays-two-{tag}"),
            size,
            class: "preview-app",
            wallpaper: false,
            build: Box::new(|| {
                let a = ui::settings::content(Arc::new(TwoDisplays), &ui::settings::Host::demo());
                a.open("Displays");
                a.root.clone().upcast()
            }),
        });
    }
    // NET-3: the whole Network pane (a tall window): Airplane Mode, each link with IP settings,
    // the VPN section with Add VPN, Proxy and DNS over TLS, the firewall.
    v.push(Case {
        name: "settings-network-all-wide".into(),
        size: (w::APP_SIZE.0, 1280),
        class: "preview-app",
        wallpaper: false,
        build: Box::new(|| {
            let mut h = ui::settings::Host::demo();
            h.machine = Arc::new(ui::settings::DemoEdge {
                airplane: Some(false),
                ..Default::default()
            });
            let a = ui::settings::content(std::sync::Arc::new(shell::system::Fake), &h);
            a.open("Network");
            pump(300);
            a.root.clone().upcast()
        }),
    });
    // General with every shell installed and pwsh chosen for new terminal windows.
    for (tag, size) in [("wide", w::APP_SIZE), ("narrow", w::APP_NARROW)] {
        v.push(Case {
            name: format!("settings-general-shells-{tag}"),
            size,
            class: "preview-app",
            wallpaper: false,
            build: Box::new(|| {
                let f = config_file(wol_core::terminal_shell::FILE);
                std::fs::create_dir_all(f.parent().expect("dir")).expect("config dir");
                std::fs::write(&f, "pwsh\n").expect("terminal shell");
                let a = ui::settings::content(Arc::new(shell::system::Fake), &all_shells_host());
                a.open("General");
                pump(50);
                let p = a.content.visible_child().expect("a page shows");
                let d: gtk::DropDown = on_row(&p, "Shell in the terminal");
                // Its place in the menu, not a number: a shell added to the table moves it (rsh).
                let at = wol_core::terminal_shell::ids().position(|i| i == "pwsh");
                assert_eq!(Some(d.selected() as usize), at, "pwsh is chosen");
                let _ = std::fs::remove_file(&f);
                a.root.clone().upcast()
            }),
        });
    }
    v
}

/// The demo host with every shell of `wol_core::terminal_shell::SHELLS` installed.
fn all_shells_host() -> ui::settings::Host {
    let mut h = ui::settings::Host::demo();
    h.terminal_shells = wol_core::terminal_shell::ids().collect();
    h
}

/// [`shell::system::Fake`] with an external display next to the built-in one.
struct TwoDisplays;

impl System for TwoDisplays {
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
    fn night_light(&self) -> Option<bool> {
        shell::system::Fake.night_light()
    }
    fn monitors(&self) -> Vec<shell::system::Monitor> {
        let mut v = shell::system::Fake.monitors();
        v.push(shell::system::Monitor {
            name: "DP-1".into(),
            description: "External Display".into(),
            width: 3840,
            height: 2160,
            refresh: 60.0,
            scale: 1.5,
            modes: vec![
                "3840x2160@60.00Hz".into(),
                "3840x2160@30.00Hz".into(),
                "2560x1440@60.00Hz".into(),
                "1920x1080@60.00Hz".into(),
            ],
            mirror_of: None,
        });
        v
    }
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
    scroll_speed_is_lines_per_notch_from_one_to_ten();
    language_menu_shows_the_saved_language_after_a_failed_save();
    login_items_show_the_saved_list_after_a_failed_save();
    terminal_shell_choice_is_saved_or_put_back();
    keyboard_offers_input_methods_only_for_languages_that_need_them();
    default_apps_write_the_choice_and_keep_the_file();
    the_keyboard_returns_to_the_switch_after_the_password();
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

/// The demo host whose module list has the input methods, not installed.
fn ime_host() -> ui::settings::Host {
    let mut h = ui::settings::Host::demo();
    h.machine = Arc::new(ui::settings::DemoEdge {
        ime: true,
        ..Default::default()
    });
    h
}

/// `build` with the account's language set to Japanese in the desktop prefs; the prefs go back.
fn in_japanese<T>(build: impl FnOnce() -> T) -> T {
    let f = config_file(wol_core::hyprprefs::PREFS_FILE);
    std::fs::create_dir_all(f.parent().expect("dir")).expect("config dir");
    let had = std::fs::read_to_string(&f).ok();
    let mut prefs = wol_core::hyprprefs::DesktopPrefs::parse(had.as_deref());
    prefs.language = Some("ja_JP.UTF-8".into());
    std::fs::write(&f, prefs.to_json()).expect("desktop prefs");
    let out = build();
    match had {
        Some(t) => std::fs::write(&f, t).expect("put the prefs back"),
        None => {
            let _ = std::fs::remove_file(&f);
        }
    }
    out
}

/// The page `name` of a Settings window on `sys`, built and loaded.
fn page(
    sys: Arc<dyn System>,
    host: &ui::settings::Host,
    name: &str,
) -> (w::AppWindow, gtk::Widget) {
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
        Arc::new(shell::system::Fake),
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
        Arc::new(shell::system::Fake),
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
        Arc::new(shell::system::Fake),
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
        Arc::new(Accepts),
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
        Arc::new(shell::system::Fake),
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
        Arc::new(shell::system::Fake),
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
    // QA-VIS2-41: the running row keeps row height (the spinner used to vexpand the row and push
    // the list off the page).
    let row = install.parent().expect("the job's line");
    assert!(
        !row.compute_expand(gtk::Orientation::Vertical),
        "the running row asks for no free height"
    );
    pump(300);
    // The run failed (Fake): the page reloaded and Install is offered again.
    assert!(button(&p, "Install").is_sensitive());
}

/// What modules-install gave up on shows under "Could Not Be Added" with Retry (not also under
/// Available); Retry is one job at a time, as Install is.
fn modules_offer_retry_for_what_could_not_be_added() {
    let (_a, p) = page(Arc::new(shell::system::Fake), &gave_up_host(), "Modules");
    assert!(says(&p, "Could Not Be Added"), "the section shows");
    // A media format package reads as the formats it brings, not its package name.
    assert!(says(&p, "Craft Suite") && says(&p, "AAC, H.264, HEVC (H.265), AC3 (Dolby Digital)"));
    assert!(!says(&p, "gst-libav"));
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
        Arc::new(shell::system::Fake),
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
    slider_shows_saved_value_after_failed_save("Trackpad & Mouse", "Scroll speed");
}

/// [`Accepts`], and the wheel speed (lines per notch) of the last desktop prefs it was asked to save.
#[derive(Default)]
struct WheelSpy(Arc<std::sync::atomic::AtomicU32>);

impl System for WheelSpy {
    fn battery(&self) -> Option<Battery> {
        Accepts.battery()
    }
    fn audio(&self) -> Audio {
        Accepts.audio()
    }
    fn network(&self) -> Network {
        Accepts.network()
    }
    fn brightness(&self) -> Option<u8> {
        Accepts.brightness()
    }
    fn perform(&self, a: &Action) -> Result<(), ActionError> {
        if let Action::Desktop(p) = a {
            self.0
                .store(p.scroll_lines, std::sync::atomic::Ordering::SeqCst);
        }
        Accepts.perform(a)
    }
}

/// Trackpad & Mouse > Scroll speed: lines per wheel notch, 1 to 10 in whole steps, 3 as shipped
/// (the compositor's wheel factor 1.0), in the Mouse section before Natural scrolling; moving it
/// saves the desktop prefs with that many lines.
fn scroll_speed_is_lines_per_notch_from_one_to_ten() {
    let spy = WheelSpy::default();
    let saved = spy.0.clone();
    let (_a, p) = page(
        std::sync::Arc::new(spy),
        &ui::settings::Host::demo(),
        "Trackpad & Mouse",
    );
    let s: gtk::Scale = on_row(&p, "Scroll speed");
    let adj = s.adjustment();
    assert_eq!(
        (adj.lower(), adj.upper(), adj.step_increment(), s.value()),
        (1.0, 10.0, 1.0, 3.0),
        "1 to 10 lines, 3 as shipped"
    );
    assert!(
        says(&p, "1 line") && says(&p, "10 lines"),
        "the ends say what the numbers are"
    );
    let labels: Vec<String> = all::<gtk::Label>(&p)
        .iter()
        .map(|l| l.label().to_string())
        .collect();
    let at = |t: &str| labels.iter().position(|l| l == t).unwrap_or(usize::MAX);
    assert!(
        at("Mouse") < at("Scroll speed")
            && at("Scroll speed") < at("Content follows the wheel like a trackpad."),
        "Scroll speed is a Mouse row, before Natural scrolling: {labels:?}"
    );
    s.set_value(7.0);
    pump(500);
    assert_eq!(
        saved.load(std::sync::atomic::Ordering::SeqCst),
        7,
        "7 lines per notch reach the saved desktop prefs"
    );
    assert_eq!(s.value(), 7.0, "a saved change stays where it was put");
}

/// Desk::set_or in Language & Region: a failed save selects the saved language again and does
/// not promise it for the next login.
fn language_menu_shows_the_saved_language_after_a_failed_save() {
    let (_a, p) = page(
        Arc::new(shell::system::Fake),
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
        Arc::new(shell::system::Fake),
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
    let (_a, p) = page(Arc::new(Accepts), &host, "Startup Security");
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

/// General > Shell in the terminal: only the installed shells are offered, a choice is written to
/// `wolos/terminal-shell`, and one that cannot be saved puts the saved shell back with a note.
fn terminal_shell_choice_is_saved_or_put_back() {
    use wol_core::terminal_shell as ts;
    let f = config_file(ts::FILE);
    std::fs::create_dir_all(f.parent().expect("dir")).expect("config dir");
    let had = std::fs::read_to_string(&f).ok();
    let _ = std::fs::remove_file(&f);
    let mut host = ui::settings::Host::demo();
    host.terminal_shells = vec!["zsh", "bash", "pwsh"];
    let (_a, p) = page(Arc::new(shell::system::Fake), &host, "General");
    let d: gtk::DropDown = on_row(&p, "Shell in the terminal");
    let labels: Vec<String> = (0..d.model().expect("model").n_items())
        .map(|i| {
            d.model()
                .and_then(|m| m.item(i))
                .and_then(|o| o.downcast::<gtk::StringObject>().ok())
                .map(|s| s.string().to_string())
                .unwrap_or_default()
        })
        .collect();
    assert_eq!(labels, ["zsh", "bash", "pwsh"], "installed shells only");
    assert_eq!(d.selected(), 0, "no file: zsh");
    d.set_selected(2);
    pump(100);
    let saved = std::fs::read_to_string(&f).ok();
    // A directory where the file goes: the next save cannot happen.
    let _ = std::fs::remove_file(&f);
    std::fs::create_dir_all(&f).expect("block the file");
    d.set_selected(1);
    pump(100);
    let back = d.selected();
    let noted = says(&p, shell::copy::settings::GENERAL_TERMINAL_SHELL_NOT_SAVED);
    let _ = std::fs::remove_dir_all(&f);
    if let Some(t) = had {
        std::fs::write(&f, t).expect("put the choice back");
    }
    assert_eq!(saved.as_deref(), Some("pwsh\n"), "the choice is saved");
    assert_eq!(
        back, 2,
        "a choice that could not be saved shows the saved shell again"
    );
    assert!(noted, "and says so");
}

/// QA-VIS2-34: the right password on the Authenticate sheet turns Remote Login on, the sheet's
/// line is cleared, and the keyboard is back on the switch (it jumped to the sidebar's search
/// when the focused password field collapsed with the sheet).
fn the_keyboard_returns_to_the_switch_after_the_password() {
    let a = ui::settings::content(
        std::sync::Arc::new(super::regressions::AsksFirst { then: None }),
        &ui::settings::Host::demo(),
    );
    let win = gtk::Window::builder()
        .default_width(w::APP_SIZE.0)
        .default_height(w::APP_SIZE.1)
        .child(&a.root)
        .build();
    win.present();
    a.open("Sharing");
    pump(300);
    let p = a.content.visible_child().expect("a page shows");
    let s: gtk::Switch = on_row(&p, "Remote Login");
    s.grab_focus();
    s.set_active(true);
    pump(100);
    let root: gtk::Widget = a.root.clone().upcast();
    let field = all::<gtk::PasswordEntry>(&root)
        .into_iter()
        .find(|f| {
            f.ancestor(gtk::Revealer::static_type())
                .and_downcast::<gtk::Revealer>()
                .is_some_and(|r| r.reveals_child())
        })
        .expect("the Authenticate sheet is up");
    field.set_text("secret");
    field.emit_by_name::<()>("activate", &[]);
    pump(300);
    assert!(s.is_active() && s.state(), "Remote Login is on");
    let focus = gtk::prelude::GtkWindowExt::focus(&win);
    assert_eq!(
        focus.as_ref(),
        Some(s.upcast_ref::<gtk::Widget>()),
        "the keyboard is on the switch, not {:?}",
        focus.as_ref().map(|f| f.type_().name())
    );
    win.destroy();
}

/// Records every action it is asked to perform (and accepts it).
struct Records(std::sync::Mutex<Vec<String>>);

impl System for Records {
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
    fn perform(&self, a: &Action) -> Result<(), ActionError> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(format!("{a:?}"));
        Ok(())
    }
}

/// QA-ACC-5, Settings > Accessibility > Larger text: the switch asks for `Action::LargerText`, on
/// and off; and the text scale (gsettings `text-scaling-factor`) makes the shell's own text taller
/// at once through the style sheet that `kit::style` loads. Leaves the scale at 1. Run after the
/// checks that need the plain GTK look: it applies the kit sheet.
pub fn larger_text_is_a_switch_and_grows_the_shell_text() {
    use shell::kit::style;
    let sys = Arc::new(Records(Default::default()));
    let (_a, p) = page(sys.clone(), &ui::settings::Host::demo(), "Accessibility");
    let sw: gtk::Switch = on_row(&p, "Larger text");
    assert!(!sw.is_active(), "normal text until asked");
    sw.set_active(true);
    pump(200);
    sw.set_active(false);
    pump(200);
    let got = sys.0.lock().unwrap().clone();
    assert_eq!(got, ["LargerText(true)", "LargerText(false)"], "{got:?}");

    // QA-ACC-9: without a compositor there is no Zoom switch, and the row says why; with one, the switch.
    assert!(
        says(&p, shell::copy::settings::ZOOM_UNAVAILABLE),
        "Zoom row names why it is off"
    );
    let (_z, with_zoom) = page(
        Arc::new(shell::system::Fake),
        &ui::settings::Host::demo(),
        "Accessibility",
    );
    let zoom: gtk::Switch = on_row(&with_zoom, "Zoom");
    assert!(!says(&with_zoom, shell::copy::settings::ZOOM_UNAVAILABLE) && !zoom.is_active());

    // Give me more time (QA-ACC-6): the menu asks for the multiplier and no other.
    let menu: gtk::DropDown = on_row(&p, "Give me more time");
    assert_eq!(menu.selected(), 0, "normal until asked");
    menu.set_selected(2);
    pump(200);
    menu.set_selected(3);
    pump(200);
    let got = sys.0.lock().unwrap().clone();
    assert_eq!(&got[2..], ["Timeouts(Five)", "Timeouts(Never)"], "{got:?}");

    let t = shell::theme::resolve(Some("wol-light"), false);
    // The kit sets its type on `window`, so the page is measured inside one (not shown: a shown
    // window wakes the menu bar's bus watch, which this part of the run has no session bus for).
    let natural = || {
        let (a, p) = page(
            Arc::new(Accepts),
            &ui::settings::Host::demo(),
            "Accessibility",
        );
        let win = gtk::Window::builder().decorated(false).build();
        win.set_child(Some(&a.root));
        let l = all::<gtk::Label>(&p)
            .into_iter()
            .find(|l| l.label() == "Larger text")
            .expect("the row's title");
        let h = l.measure(gtk::Orientation::Vertical, -1).1;
        win.set_child(None::<&gtk::Widget>);
        win.destroy();
        h
    };
    assert!(style::set_larger_text(false).is_ok());
    style::apply(&t, "");
    let normal = natural();
    assert!(style::set_larger_text(true).is_ok());
    style::apply(&t, "");
    let larger = natural();
    assert!(style::set_larger_text(false).is_ok());
    style::apply(&t, "");
    assert!(
        f64::from(larger) >= f64::from(normal) * 1.2,
        "the row title is {normal} px tall at normal size and {larger} px at 1.25"
    );
}

/// QA-ACC-11: Keyboard > Input Methods shows the `ime` module (Install) for a Japanese account and
/// nothing for an English one; Install asks wol-admind for the module's meta package.
fn keyboard_offers_input_methods_only_for_languages_that_need_them() {
    let sys = Arc::new(Records(Default::default()));
    let (_a, p) = in_japanese(|| page(sys.clone(), &ime_host(), "Keyboard"));
    pump(300);
    assert!(says(&p, "Input Methods"), "the section shows for Japanese");
    assert!(says(&p, "Input methods"), "the module's row");
    assert!(says(&p, shell::copy::settings::INPUT_METHODS_ABOUT));
    button(&p, "Install").emit_clicked();
    pump(300);
    let got = sys.0.lock().unwrap().clone();
    assert!(
        got.iter().any(|a| a == "PkgInstall(\"wolos-module-ime\")"),
        "{got:?}"
    );
    // English: no section (the list is not even asked for).
    let (_b, q) = page(Arc::new(Accepts), &ime_host(), "Keyboard");
    pump(300);
    assert!(!says(&q, "Input Methods"), "no input methods for English");
}

/// E2E-OPEN: Default Apps lists the installed apps of a kind, selects the one the person's
/// `mimeapps.list` names, and a pick writes that one choice (one journal entry) without losing a
/// comment, a blank line or another group of the file; a kind no app serves is greyed.
fn default_apps_write_the_choice_and_keep_the_file() {
    let root = std::env::temp_dir().join(format!("wol-default-apps-{}", std::process::id()));
    let apps = root.join("applications");
    std::fs::create_dir_all(&apps).expect("applications dir");
    let entry = |name: &str, mimes: &str| {
        format!("[Desktop Entry]\nType=Application\nName={name}\nExec=v %F\nMimeType={mimes}\n")
    };
    std::fs::write(
        apps.join("a.desktop"),
        entry("Alpha", "image/png;image/jpeg;"),
    )
    .unwrap();
    std::fs::write(apps.join("b.desktop"), entry("Bravo", "image/png;")).unwrap();
    let mimeapps = config_file("mimeapps.list");
    std::fs::create_dir_all(mimeapps.parent().expect("dir")).expect("config dir");
    let had = std::fs::read_to_string(&mimeapps).ok();
    std::fs::write(
        &mimeapps,
        "# mine\n[Default Applications]\nimage/png=a.desktop;\n\n[Other]\nk=v\n",
    )
    .unwrap();
    let old = (
        std::env::var_os("XDG_DATA_HOME"),
        std::env::var_os("XDG_DATA_DIRS"),
    );
    unsafe {
        std::env::set_var("XDG_DATA_HOME", &root);
        std::env::set_var("XDG_DATA_DIRS", &root);
    }
    let (_a, p) = page(
        std::sync::Arc::new(shell::system::Fake),
        &ui::settings::Host::demo(),
        "Default Apps",
    );
    pump(400);
    let images: gtk::DropDown = on_row(&p, "Images");
    let web: gtk::DropDown = on_row(&p, "Web browser");
    let (selected, greyed) = (images.selected(), !web.is_sensitive());
    images.set_selected(1);
    pump(600);
    let saved = std::fs::read_to_string(&mimeapps).unwrap_or_default();
    let journal = wol_core::journal::Journal::for_config(&config_file("")).entries();
    unsafe {
        match old.0 {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        match old.1 {
            Some(v) => std::env::set_var("XDG_DATA_DIRS", v),
            None => std::env::remove_var("XDG_DATA_DIRS"),
        }
    }
    match had {
        Some(t) => std::fs::write(&mimeapps, t).expect("put the file back"),
        None => {
            let _ = std::fs::remove_file(&mimeapps);
        }
    }
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(selected, 0, "Alpha is the current Images app");
    assert!(greyed, "no installed app serves a web browser row here");
    assert_eq!(
        saved,
        "# mine\n[Default Applications]\nimage/png=b.desktop;a.desktop;\n\n[Other]\nk=v\n\
         \n[Added Associations]\nimage/png=b.desktop;\n",
        "one line changed; the comment, blank line and [Other] stay; Bravo opens png only"
    );
    assert!(
        journal.iter().any(|e| e.what == "default apps"),
        "the change is one journal entry (Restore can undo it)"
    );
}
