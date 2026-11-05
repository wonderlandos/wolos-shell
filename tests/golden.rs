// SPDX-License-Identifier: GPL-2.0-or-later
//! Golden screenshots of every surface, light and dark, with the layout check on.
//! Runs under a private `gtk4-broadwayd` (headless). `WOL_GOLDEN_UPDATE=1` rewrites goldens.
//! Every case's button texts are kept too, in `tests/golden/button-labels.tsv`: a changed label
//! fails even when the pixels stay inside the tolerance. `WOL_GOLDEN_UPDATE=1` rewrites the lines of
//! the cases it ran; `WOL_GOLDEN_LABELS=update` rewrites only those lines, no PNG.
//! Renders land in `$CARGO_TARGET_TMPDIR/golden/`.
use gtk::{gdk, glib, prelude::*};
use shell::kit::{self, widgets as w};
use shell::theme::{self, Variant};
use shell::ui;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

#[path = "golden_cases/app_view.rs"]
mod app_view_cases;
#[path = "golden_cases/controlcenter.rs"]
mod controlcenter_cases;
#[path = "golden_cases/displays.rs"]
mod displays_cases;
#[path = "golden_cases/focus.rs"]
mod focus_cases;
#[path = "golden_cases/font.rs"]
mod font_cases;
#[path = "golden_cases/form_kit.rs"]
mod form_kit;
#[path = "golden_cases/installer.rs"]
mod installer_cases;
#[path = "golden_cases/locale.rs"]
mod locale_cases;
#[path = "golden_cases/maker.rs"]
mod maker_cases;
#[path = "golden_cases/menubar.rs"]
mod menubar_cases;
#[path = "golden_cases/monitor.rs"]
mod monitor_cases;
#[path = "golden_cases/motion.rs"]
mod motion_cases;
#[path = "golden_cases/offers.rs"]
mod offer_cases;
#[path = "golden_cases/store_package.rs"]
mod package_cases;
#[path = "golden_cases/plugin_host.rs"]
mod plugin_host_cases;
#[path = "golden_cases/regressions.rs"]
mod regressions;
#[path = "golden_cases/settings.rs"]
mod settings_cases;
#[path = "golden_cases/setup.rs"]
mod setup_cases;
#[path = "golden_cases/theme_picker.rs"]
mod theme_picker_cases;
#[path = "golden_cases/tray.rs"]
mod tray_cases;
#[path = "golden_cases/widgets.rs"]
mod widget_cases;
#[path = "golden_cases/wol_file.rs"]
mod wol_file;

const WALL: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../design/wallpapers/wonderland-dusk.svg"
);
/// Max share of pixels allowed to differ (any channel > 24) before a golden fails.
const TOLERANCE: f64 = 0.005;

struct Case {
    name: String,
    size: (i32, i32),
    class: &'static str,
    wallpaper: bool,
    build: Box<dyn Fn() -> gtk::Widget>,
}

fn analyze_full() -> wol_core::malware::Full {
    wol_core::malware::Full {
        on: true,
        version: 28170,
        signatures: 3_628_245,
        updated: 1,
        ..Default::default()
    }
}

/// dedup A9: Setup's language list moves the keyboard every time (the page following a language
/// is not the person's pick, as in the installer). It used to stop after the first language,
/// because the list's own move counted as a pick. Ends on the language it started with.
fn keyboard_follows_languages(s: &ui::setup::Setup) {
    use shell::setup::{KEYBOARDS, LANGUAGES};
    let kb = |i: usize| {
        KEYBOARDS
            .iter()
            .position(|k| k.0 == LANGUAGES[i].keymap)
            .unwrap_or(0)
    };
    let (start, _) = s.language_and_keyboard();
    for i in [2, 5, start] {
        s.pick_language(i);
        assert_eq!(s.language_and_keyboard(), (i, kb(i)), "after language {i}");
    }
}

/// Files' "Check for Malware" window in one state.
fn analyze_case(tag: &str, view: ui::analyze::View) -> Case {
    Case {
        name: format!("analyze-{tag}"),
        size: (400, 1),
        class: "preview",
        wallpaper: true,
        build: Box::new(move || ui::analyze::content(&view).0),
    }
}

fn cases() -> Vec<Case> {
    let mut v: Vec<Case> = vec![
        Case {
            name: "menubar".into(),
            size: (900, 28),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let b = ui::menubar::content(&shell::system::Fake);
                b.set_app("Files".into(), "wol-files".into());
                b.root.clone().upcast()
            }),
        },
        Case {
            // A display without the focus: its menu bar dimmed.
            name: "menubar-inactive".into(),
            size: (900, 28),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let b = ui::menubar::content(&shell::system::Fake);
                b.set_app("Files".into(), "wol-files".into());
                b.set_active(false);
                b.root.clone().upcast()
            }),
        },
        Case {
            // No focused window: no app name and no app/Window menus (no resident Finder).
            name: "menubar-desktop".into(),
            size: (900, 28),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let b = ui::menubar::content(&shell::system::Fake);
                b.set_app(String::new(), String::new());
                b.root.clone().upcast()
            }),
        },
        Case {
            name: "about-app".into(),
            size: (320, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                ui::panels::about_card("Text Editor", "wolos-files", "org.gnome.TextEditor")
                    .upcast()
            }),
        },
        Case {
            name: "dock".into(),
            size: (975, 120),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let running = ["foot".to_string(), "org.gnome.nautilus".into()]
                    .into_iter()
                    .collect();
                ui::dock::content(&running).root.clone().upcast()
            }),
        },
        Case {
            // QA-MANUAL-5/43 (manual webapps.md): a web app kept in the Dock sits after the
            // pinned apps with its own icon file, and its window lights its own dot.
            name: "dock-kept-webapp".into(),
            size: (1043, 120),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let app = wol_core::webapps::WebApp::new(
                    "Mail",
                    "https://mail.example.com",
                    wol_core::webapps::Profile::Own,
                )
                .unwrap();
                let text =
                    wol_core::webapps::desktop_entry(&app, std::path::Path::new("/h/.local/share"));
                let mut e = shell::desktop::parse("wolos-webapp-mail.desktop", &text).unwrap();
                e.icon = concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../design/icons/store/evolution.svg"
                )
                .into();
                let items = shell::apps::keep_in_dock(
                    &shared::layout::Layout::default().dock_items,
                    shell::apps::PINS,
                    "wolos-webapp-mail",
                )
                .unwrap();
                let running = [e.wm_class.to_lowercase()].into_iter().collect();
                let d = ui::dock::content_with(&running, &items, shared::layout::Edge::Bottom);
                d.set_kept(vec![shell::apps::entry_pin(&e)]);
                d.refill();
                d.set_running(&running);
                assert_eq!(d.pins().last().map(|p| p.name), Some("Mail"));
                d.root.clone().upcast()
            }),
        },
        Case {
            // manual apps.md:60: an app running without a Dock icon (Geany) shows after the
            // pinned apps with its running dot while it runs.
            name: "dock-running-unpinned".into(),
            size: (1043, 120),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let calc = shell::desktop::Entry {
                    id: "geany.desktop".into(),
                    name: "Geany".into(),
                    exec: vec!["geany".into()],
                    icon: concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/../../design/icons/store/geany.svg"
                    )
                    .into(),
                    wm_class: String::new(),
                };
                let running = ["foot".to_string(), "geany".into()].into_iter().collect();
                let d = ui::dock::content(&Default::default());
                d.set_installed(vec![calc]);
                d.set_running(&running);
                assert_eq!(d.pins().last().map(|p| p.name), Some("Geany"));
                d.root.clone().upcast()
            }),
        },
        Case {
            // A helper (wol mcp) asks before a change: Deny, Always Allow, Allow Once.
            name: "approval".into(),
            size: (400, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                ui::approval::content(
                    Arc::new(shell::system::Fake),
                    shared::shell::Approval {
                        helper: "Claude Desktop".into(),
                        tool: "appearance set".into(),
                        request: shared::shell::ApprovalRequest::Change {
                            capability: "appearance".into(),
                            what: "switch to Dark appearance".into(),
                        },
                    },
                )
            }),
        },
        Case {
            // Any program using `wol` asks to install a repo package: the sheet leads with the
            // change and the package name, not with "wol" (audit P6-4).
            name: "approval-via-wol".into(),
            size: (400, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                ui::approval::content(
                    Arc::new(shell::system::Fake),
                    shared::shell::Approval {
                        helper: shared::portal::HELPER_GATE.into(),
                        tool: "wol: packages-install".into(),
                        request: shared::shell::ApprovalRequest::Packages {
                            verb: "install".into(),
                            names: vec!["gimp".into()],
                        },
                    },
                )
            }),
        },
        Case {
            // A Flatpak app asks for its permissions while it installs (wol-flatpak-consent).
            name: "flatpak-consent".into(),
            size: (400, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let perm = |s| wol_core::flatpak::Perm::parse(s).unwrap();
                ui::flatpak_consent::content(&ui::flatpak_consent::Info {
                    name: "GIMP".into(),
                    icon: "org.gimp.GIMP".into(),
                    update: false,
                    perms: vec![
                        perm("filesystems=home"),
                        perm("shared=network"),
                        perm("devices=dri"),
                        perm("sockets=x11"),
                    ],
                })
                .0
            }),
        },
        analyze_case("off", ui::analyze::View::NotReady(Default::default())),
        analyze_case(
            "waiting",
            ui::analyze::View::NotReady(wol_core::malware::Full {
                on: true,
                updating: true,
                ..Default::default()
            }),
        ),
        analyze_case(
            "checking",
            ui::analyze::View::Checking {
                shown: "“setup.exe”".into(),
            },
        ),
        analyze_case(
            "clean",
            ui::analyze::View::Done {
                analysis: wol_core::malware::Analysis::Clean,
                shown: "“setup.exe”".into(),
                full: analyze_full(),
            },
        ),
        analyze_case(
            "found",
            ui::analyze::View::Done {
                analysis: wol_core::malware::Analysis::Found(vec![wol_core::malware::Hit {
                    name: "Eicar-Test-Signature".into(),
                    path: "/home/julia/Downloads/eicar.com".into(),
                }]),
                shown: "“eicar.com”".into(),
                full: analyze_full(),
            },
        ),
        analyze_case(
            "incomplete",
            ui::analyze::View::Done {
                analysis: wol_core::malware::Analysis::Incomplete(vec!["clamav: timed out".into()]),
                shown: "“setup.exe”".into(),
                full: analyze_full(),
            },
        ),
        Case {
            // An administrator change from a helper: the password, every time.
            name: "approval-admin".into(),
            size: (400, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                ui::approval::content(
                    Arc::new(shell::system::Fake),
                    shared::shell::Approval {
                        helper: "Claude Desktop".into(),
                        tool: "wifi join".into(),
                        request: shared::shell::ApprovalRequest::WifiJoin {
                            ssid: "Wonderland".into(),
                            password: Some("hunter22".into()),
                        },
                    },
                )
            }),
        },
        Case {
            // Caps Lock on while the admin password is typed: the note under the field.
            name: "approval-admin-caps-lock".into(),
            size: (400, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let root = ui::approval::content(
                    Arc::new(shell::system::Fake),
                    shared::shell::Approval {
                        helper: "Claude Desktop".into(),
                        tool: "wifi join".into(),
                        request: shared::shell::ApprovalRequest::WifiJoin {
                            ssid: "Wonderland".into(),
                            password: Some("hunter22".into()),
                        },
                    },
                );
                // Hidden notes are not drawable, so walk every widget.
                fn caps(w: &gtk::Widget, n: &mut usize) {
                    if w.has_css_class("caps-lock") {
                        w.set_visible(true);
                        *n += 1;
                    }
                    let mut c = w.first_child();
                    while let Some(ch) = c {
                        caps(&ch, n);
                        c = ch.next_sibling();
                    }
                }
                let mut n = 0;
                caps(&root, &mut n);
                assert_eq!(n, 1, "one Caps Lock note");
                root
            }),
        },
        Case {
            name: "controlcenter".into(),
            size: (368, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| ui::control_center::content(Arc::new(shell::system::Fake))),
        },
        Case {
            name: "searchbar".into(),
            size: (728, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let items = Rc::new(std::cell::RefCell::new(ui::searchbar::index(vec![])));
                ui::searchbar::content(items, Default::default(), "te").0
            }),
        },
        Case {
            name: "searchbar-files".into(),
            size: (728, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let items = Rc::new(std::cell::RefCell::new(ui::searchbar::index(vec![])));
                let names = shell::files::Index::parse(
                    "Documents/\nDocuments/Taxes 2026.pdf\nDocuments/Tax Receipts/\n\
                     Pictures/tax receipt.jpeg\nDownloads/syntax-cheatsheet.md\n",
                    std::path::Path::new("/home/alice"),
                );
                ui::searchbar::content(items, Rc::new(std::cell::RefCell::new(names)), "tax").0
            }),
        },
        Case {
            // Arithmetic answers itself; Return copies the answer.
            name: "searchbar-calc".into(),
            size: (728, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let items = Rc::new(std::cell::RefCell::new(ui::searchbar::index(vec![])));
                ui::searchbar::content(items, Default::default(), "(1200 * 12) / 7").0
            }),
        },
        Case {
            name: "searchbar-empty".into(),
            size: (728, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let items = Rc::new(std::cell::RefCell::new(ui::searchbar::index(vec![])));
                ui::searchbar::content(items, Default::default(), "qwzx kettle").0
            }),
        },
        Case {
            name: "lock".into(),
            size: (900, 600),
            class: "lock-window",
            wallpaper: false,
            build: Box::new(|| ui::lock::lock_screen(&w::UserPanel::new("Alice Liddell"))),
        },
        Case {
            name: "lock-rejected".into(),
            size: (900, 600),
            class: "lock-window",
            wallpaper: false,
            build: Box::new(|| {
                let p = w::UserPanel::new("Alice Liddell");
                p.reject(shell::login::Verdict::Denied.message());
                ui::lock::lock_screen(&p)
            }),
        },
        Case {
            name: "lock-checking".into(),
            size: (900, 600),
            class: "lock-window",
            wallpaper: false,
            build: Box::new(|| {
                let p = w::UserPanel::new("Alice Liddell");
                p.field.set_text("curiouser");
                p.set_busy(true);
                ui::lock::lock_screen(&p)
            }),
        },
        Case {
            name: "lock-caps-lock".into(),
            size: (900, 600),
            class: "lock-window",
            wallpaper: false,
            build: Box::new(|| {
                let p = w::UserPanel::new("Alice Liddell");
                p.field.set_text("CURIOUSER");
                p.field.set_position(-1); // typed; focus selects it (selection contrast checked)
                p.set_caps_lock(true);
                ui::lock::lock_screen(&p)
            }),
        },
        Case {
            name: "lock-startup".into(),
            size: (900, 600),
            class: "lock-window",
            wallpaper: false,
            build: Box::new(|| ui::lock::startup_lock_screen(&w::UserPanel::new("Alice Liddell"))),
        },
        Case {
            name: "login".into(),
            size: (900, 600),
            class: "lock-window",
            wallpaper: false,
            build: Box::new(|| {
                ui::lock::Login::new(&[
                    ("alice".into(), "alice".into()),
                    ("dinah".into(), "dinah".into()),
                    ("hatter".into(), "Mad Hatter".into()),
                ])
                .root
                .clone()
            }),
        },
        Case {
            name: "login-password".into(),
            size: (900, 600),
            class: "lock-window",
            wallpaper: false,
            build: Box::new(|| {
                let l = ui::lock::Login::new(&[
                    ("alice".into(), "alice".into()),
                    ("dinah".into(), "dinah".into()),
                ]);
                l.pick("dinah", "dinah");
                l.root.clone()
            }),
        },
        Case {
            name: "switcher".into(),
            size: (900, 200),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let c = |a: &str, class: &str, f: i64| shell::hypr::Client {
                    focus: f,
                    ..shell::hypr::Client::new(a, class, "")
                };
                let apps = shell::switcher::apps(
                    &[
                        c("0x1", "foot", 0),
                        c("0x2", "org.gnome.Nautilus", 1),
                        c("0x3", "helium", 2),
                        c("0x4", "foot", 3),
                        c("0x5", "org.wolos.settings", 4),
                        c("0x6", "org.gnome.Geary", 5),
                    ],
                    &[],
                );
                let s = ui::switcher::Strip::new(&apps, 1);
                s.root.set_halign(gtk::Align::Center);
                s.root.set_valign(gtk::Align::Center);
                s.root.clone().upcast()
            }),
        },
        Case {
            // ⇧⌘/: every shortcut shipped by default, in columns, on a 1080-high screen: the sheet
            // is as tall as its rows (it used to scroll at 640 and cut "Actual Size" in half).
            name: "shortcut-sheet".into(),
            size: (ui::shortcut_sheet::WIDTH + 2 * kit::widgets::S6, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let list = wol_core::shortcuts::load_defaults(|_| None);
                let c = ui::shortcut_sheet::content(
                    &shell::shortcut_sheet::sections(&list, false),
                    ui::shortcut_sheet::max_height(1080),
                );
                c.set_halign(gtk::Align::Center);
                c.set_valign(gtk::Align::Center);
                c.upcast()
            }),
        },
        Case {
            // The same sheet on a 768-high screen: whole sections on pages, the pager by the footnote.
            name: "shortcut-sheet-small-screen".into(),
            size: (ui::shortcut_sheet::WIDTH + 2 * kit::widgets::S6, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let list = wol_core::shortcuts::load_defaults(|_| None);
                let c = ui::shortcut_sheet::content(
                    &shell::shortcut_sheet::sections(&list, false),
                    ui::shortcut_sheet::max_height(ui::shortcut_sheet::SMALL_SCREEN),
                );
                c.set_halign(gtk::Align::Center);
                c.set_valign(gtk::Align::Center);
                c.upcast()
            }),
        },
        Case {
            // Files dropped on the desktop, newest first; dragged out to any app.
            name: "drop-shelf".into(),
            size: (ui::drop_shelf::WIDTH + 2 * kit::widgets::S6, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let items: Vec<std::path::PathBuf> = [
                    "/home/ana/Documents/Quarterly Report.pdf",
                    "/home/ana/Pictures/Beach at sunset.jpg",
                    "/home/ana/Downloads/project-assets.zip",
                    "/home/ana/notes.txt",
                ]
                .iter()
                .map(Into::into)
                .collect();
                let c = ui::drop_shelf::content(&items, false);
                c.set_halign(gtk::Align::Center);
                c.set_valign(gtk::Align::Center);
                c.upcast()
            }),
        },
        Case {
            // ⌥⌘Esc: open apps, most recent first, the frontmost chosen.
            name: "force-quit".into(),
            size: (420, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let c = |a: &str, class: &str, f: i64| shell::hypr::Client {
                    focus: f,
                    ..shell::hypr::Client::new(a, class, "")
                };
                let apps = shell::switcher::apps(
                    &[
                        c("0x1", "org.gnome.Nautilus", 0),
                        c("0x2", "helium", 1),
                        c("0x3", "foot", 2),
                        c("0x4", "org.gnome.Geary", 3),
                    ],
                    &[],
                );
                let p = ui::force_quit::Picker::new(|_, _| {});
                p.set_apps(&apps);
                p.root.clone().upcast()
            }),
        },
        Case {
            name: "force-quit-empty".into(),
            size: (420, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let p = ui::force_quit::Picker::new(|_, _| {});
                p.set_apps(&[]);
                p.root.clone().upcast()
            }),
        },
        Case {
            name: "overview".into(),
            size: (1280, 800),
            class: "lock-window",
            wallpaper: false,
            build: Box::new(|| {
                let c = |a: &str, class: &str, title: &str, ws: i64, size: [i32; 2]| {
                    let mut c = shell::hypr::Client::new(a, class, title);
                    c.workspace.id = ws;
                    c.size = size;
                    c
                };
                let clients = vec![
                    c("0x1", "foot", "~/src/wolos — fish", 1, [900, 560]),
                    c("0x2", "org.gnome.Nautilus", "Documents", 1, [1100, 700]),
                    c("0x3", "helium", "WolOS — The friendly BSD", 1, [1280, 760]),
                    c("0x4", "org.gnome.Geary", "Inbox", 1, [800, 600]),
                    c("0x5", "wol-music", "Music", 2, [800, 600]),
                    c("0x6", "wol-notes", "Notes", 3, [800, 600]),
                ];
                let desktops = shell::overview::desktops(&clients, 1);
                let here = clients
                    .iter()
                    .filter(|c| c.workspace.id == 1)
                    .map(|client| ui::overview::Window {
                        client,
                        thumb: None,
                    })
                    .collect();
                ui::overview::content((1280, 800), &desktops, 1, here, |_| {}).0
            }),
        },
        Case {
            name: "capture-bar".into(),
            size: (900, 140),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let b = ui::capture::Toolbar::new(
                    shell::capture::Mode::Selection,
                    shell::capture::Options::default(),
                );
                b.root.set_halign(gtk::Align::Center);
                b.root.set_valign(gtk::Align::End);
                b.root.clone().upcast()
            }),
        },
        Case {
            name: "capture-thumb".into(),
            size: (480, 240),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let shot = Path::new(WALL);
                let t = gdk::Texture::from_filename(shot).expect("wallpaper");
                let th = ui::capture::thumbnail(&t, shot);
                let b = w::vbox(0);
                b.set_vexpand(true);
                th.set_vexpand(true);
                b.append(&th);
                b.upcast()
            }),
        },
        // Markup (ADR 0031 W5): pens, Undo, Copy, Save over the picture, two strokes drawn.
        Case {
            name: "capture-markup".into(),
            size: (900, 600),
            class: "preview",
            wallpaper: false,
            build: Box::new(|| {
                let t = gdk::Texture::from_filename(Path::new(WALL)).expect("wallpaper");
                let v = ui::capture::MarkupView::new(&t).expect("markup view");
                {
                    let mut m = v.markup.borrow_mut();
                    let (w0, h0) = m.size;
                    m.begin(shell::capture::Ink::Red, (w0 * 0.2, h0 * 0.3));
                    for i in 1..=20 {
                        let f = f64::from(i) / 20.0;
                        m.extend((w0 * (0.2 + 0.4 * f), h0 * (0.3 + 0.2 * (f * 6.0).sin())));
                    }
                    m.begin(shell::capture::Ink::Blue, (w0 * 0.7, h0 * 0.7));
                    m.extend((w0 * 0.85, h0 * 0.75));
                }
                v.root.clone().upcast()
            }),
        },
        Case {
            name: "banners".into(),
            size: (420, 360),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let n = |app: &str, summary: &str, body: &str| shell::ui::notify::Note {
                    id: 1,
                    app: app.into(),
                    icon: String::new(),
                    summary: summary.into(),
                    body: body.into(),
                    critical: false,
                    timeout_ms: -1,
                    image: String::new(),
                    actions: vec![],
                };
                let col = w::vbox(0);
                col.add_css_class("banners-preview");
                col.set_halign(gtk::Align::End);
                col.set_valign(gtk::Align::Start);
                col.append(&ui::notify::banner(
                    &n("Mail", "Dinah", "Tea at four? The Hatter insists it is always six o'clock, so bring a watch that agrees with him and some extra cups."),
                    1,
                ));
                col.append(&ui::notify::banner(
                    &n("Music", "Now Playing", "White Rabbit — Jefferson Airplane"),
                    3,
                ));
                col.append(&ui::notify::banner(
                    &n("Calendar", "Croquet with the Queen", ""),
                    1,
                ));
                col.upcast()
            }),
        },
        Case {
            name: "banner-screenshot".into(),
            size: (420, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                // What wol-screenshot sends: the folder in the title, the picture as thumbnail.
                let note = shell::ui::notify::Note {
                    id: 1,
                    app: "Screenshot".into(),
                    icon: "wolos-photos".into(),
                    summary: "Screenshot saved to Desktop".into(),
                    body: String::new(),
                    critical: false,
                    timeout_ms: -1,
                    image: concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/../../design/wallpapers/wonderland-day.svg"
                    )
                    .into(),
                    actions: vec![],
                };
                let col = w::vbox(0);
                col.add_css_class("banners-preview");
                col.append(&ui::notify::banner(&note, 1));
                col.upcast()
            }),
        },
        Case {
            name: "banner-action".into(),
            size: (420, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                // The NVIDIA driver finished installing online: one Restart button.
                let note = shell::ui::notify::Note {
                    id: 1,
                    app: "WolOS".into(),
                    icon: String::new(),
                    summary: "Graphics driver installed".into(),
                    body: "Restart to finish setting up your graphics card.".into(),
                    critical: false,
                    timeout_ms: -1,
                    image: String::new(),
                    actions: vec![("restart".into(), "Restart".into())],
                };
                let col = w::vbox(0);
                col.add_css_class("banners-preview");
                col.append(&ui::notify::banner(&note, 1));
                col.upcast()
            }),
        },
    ];
    for (sw, sh, tag) in [(1280, 800, "1280"), (1024, 640, "1024")] {
        for st in shell::setup::STEPS {
            v.push(Case {
                name: format!("setup-{}-{tag}", st.id()),
                size: (sw, sh),
                class: "lock-window",
                wallpaper: false,
                build: Box::new(move || {
                    let s = ui::setup::Setup::new(&shell::system::Fake);
                    if st == shell::setup::Step::Language {
                        keyboard_follows_languages(&s);
                    }
                    s.show(st);
                    s.root.clone()
                }),
            });
        }
    }
    // First sign-in Setup: every page, as the person sees it before the desktop.
    for (sw, sh, tag) in [(1280, 800, "1280"), (1024, 640, "1024")] {
        for p in shell::first_login::PAGES {
            v.push(Case {
                name: format!("firstlogin-{}-{tag}", p.id()),
                size: (sw, sh),
                class: "lock-window",
                wallpaper: false,
                build: Box::new(move || {
                    let s =
                        ui::first_login::FirstLogin::new(std::sync::Arc::new(shell::system::Fake));
                    s.show(p);
                    s.root.clone()
                }),
            });
        }
    }
    // An administrator turned error reports on: the password row asks once.
    v.push(Case {
        name: "firstlogin-privacy-password-1024".into(),
        size: (1024, 640),
        class: "lock-window",
        wallpaper: false,
        build: Box::new(|| {
            let facts = ui::first_login::Facts {
                choices: shell::first_login::Choices {
                    reports: Some(true),
                    ..Default::default()
                },
                ..Default::default()
            };
            let s =
                ui::first_login::FirstLogin::with(std::sync::Arc::new(shell::system::Fake), &facts);
            s.show(shell::first_login::Page::Privacy);
            s.ask_password();
            s.root.clone()
        }),
    });
    v.push(Case {
        name: "login-hint".into(),
        size: (900, 600),
        class: "lock-window",
        wallpaper: false,
        build: Box::new(|| {
            let l = ui::lock::Login::new(&[
                ("alice".into(), "alice".into()),
                ("dinah".into(), "dinah".into()),
            ]);
            l.pick("alice", "alice");
            l.panel.reject(shell::login::Verdict::Denied.message());
            let hint = shell::login::hint_message(3, Some("white rabbit"));
            l.panel.message.set_label(&hint.unwrap_or_default());
            l.root.clone()
        }),
    });
    for v2 in [(1280, 800, "1280"), (1024, 640, "1024")] {
        v.push(Case {
            name: format!("setup-account-filled-{}", v2.2),
            size: (v2.0, v2.1),
            class: "lock-window",
            wallpaper: false,
            build: Box::new(|| {
                use shell::setup::Field;
                let s = ui::setup::Setup::new(&shell::system::Fake);
                s.show(shell::setup::Step::Account);
                s.type_into(Field::FullName, "Alice Liddell");
                s.type_into(Field::Password, "curiouser");
                s.type_into(Field::Verify, "curious");
                s.next(); // Continue: the problem shows at the foot, the field is outlined
                s.root.clone()
            }),
        });
    }
    v.push(Case {
        name: "setup-account-mismatch-1024".into(),
        size: (1024, 640),
        class: "lock-window",
        wallpaper: false,
        build: Box::new(|| {
            use shell::setup::Field;
            let s = ui::setup::Setup::new(&shell::system::Fake);
            s.show(shell::setup::Step::Account);
            s.type_into(Field::FullName, "Alice Liddell");
            s.type_into(Field::Password, "curiouser");
            s.type_into(Field::Verify, "curix"); // shows while typing, before Continue
            s.root.clone()
        }),
    });
    // Caps Lock on while typing a password: the warning shows under the field.
    v.push(Case {
        name: "setup-account-caps-lock-1024".into(),
        size: (1024, 640),
        class: "lock-window",
        wallpaper: false,
        build: Box::new(|| {
            use shell::setup::Field;
            let s = ui::setup::Setup::new(&shell::system::Fake);
            s.show(shell::setup::Step::Account);
            s.type_into(Field::FullName, "Alice Liddell");
            s.type_into(Field::Password, "CURIOUSER");
            s.set_caps_lock(true);
            s.root.clone()
        }),
    });
    installer_cases::add(&mut v);
    for (sw, sh, tag) in [(900, 600, "wide"), (480, 360, "narrow")] {
        for p in ui::store::PAGES.into_iter().chain(["Search"]) {
            v.push(Case {
                name: format!("store-{}-{tag}", p.to_lowercase()),
                size: (sw, sh),
                class: "preview-app",
                wallpaper: false,
                build: Box::new(move || {
                    let s = ui::store::content(
                        Arc::new(shell::system::Fake),
                        ui::store::Env {
                            data_dirs: vec![],
                            pkg_search: false,
                            flatpaks: vec![],
                            installed: Some(Default::default()),
                        },
                    );
                    if p == "Search" {
                        s.search_now("photo");
                    } else {
                        s.aw.select(p);
                    }
                    s.aw.root.clone().upcast()
                }),
            });
        }
        // A kit got before says Added: its packages are in the (injected) installed set.
        v.push(Case {
            name: format!("store-kits-added-{tag}"),
            size: (sw, sh),
            class: "preview-app",
            wallpaper: false,
            build: Box::new(move || {
                let codecs = wol_core::store::kit("codecs-kit").expect("the codecs kit");
                let mut installed = wopkg::context::Installed::default();
                installed
                    .pacman
                    .extend(codecs.packages().iter().map(|p| p.to_string()));
                let s = ui::store::content(
                    Arc::new(shell::system::Fake),
                    ui::store::Env {
                        data_dirs: vec![],
                        pkg_search: false,
                        flatpaks: vec![],
                        installed: Some(installed),
                    },
                );
                s.aw.select("Kits");
                s.aw.root.clone().upcast()
            }),
        });
        v.push(Case {
            // Search with repo and Flathub hits: Flathub apps carry a badge, one tile per app.
            name: format!("store-search-flathub-{tag}"),
            size: (sw, sh),
            class: "preview-app",
            wallpaper: false,
            build: Box::new(move || {
                let s = ui::store::content(
                    Arc::new(shell::system::Fake),
                    ui::store::Env {
                        data_dirs: vec![],
                        pkg_search: false,
                        flatpaks: vec!["com.github.maoschanz.drawing".into()],
                        installed: Some(Default::default()),
                    },
                );
                let hit = |id: &str, name: &str, summary: &str| wol_core::flatpak::Hit {
                    id: id.into(),
                    name: name.into(),
                    summary: summary.into(),
                };
                s.search_with(
                    "paint",
                    vec![wol_core::store::PkgHit {
                        name: "mtpaint".into(),
                        summary: "Painting program for pixel art".into(),
                    }],
                    vec![
                        hit(
                            "com.github.maoschanz.drawing",
                            "Drawing",
                            "A simple drawing app",
                        ),
                        hit(
                            "com.github.PintaProject.Pinta",
                            "Pinta",
                            "Edit images and paint digitally",
                        ),
                    ],
                );
                s.aw.root.clone().upcast()
            }),
        });
        for kind in ["flathub", "native"] {
            v.push(Case {
                // Details page: icon, name, Get, screenshots, description, information, permissions.
                name: format!("store-details-{kind}-{tag}"),
                size: (sw, sh),
                class: "preview-app",
                wallpaper: false,
                build: Box::new(move || {
                    let s = ui::store::content(
                        Arc::new(shell::system::Fake),
                        ui::store::Env {
                            data_dirs: vec![],
                            pkg_search: false,
                            flatpaks: vec![],
                            installed: Some(Default::default()),
                        },
                    );
                    // Fixed art, never another golden: a golden made from goldens changes
                    // with every re-bake of those (an update run would not settle).
                    let shot = |n: &str| {
                        format!("{}/../../design/wallpapers/{n}", env!("CARGO_MANIFEST_DIR"))
                    };
                    let perm = |p: &str| wol_core::flatpak::Perm::parse(p).unwrap();
                    let (subject, loaded) = if kind == "flathub" {
                        (
                            ui::store::Subject::flathub(&wol_core::flatpak::Hit {
                                id: "com.github.PintaProject.Pinta".into(),
                                name: "Pinta".into(),
                                summary: "Edit images and paint digitally".into(),
                            }),
                            ui::store::Loaded {
                                details: Some(wol_core::appstream::Details {
                                    name: "Pinta".into(),
                                    summary: "Edit images and paint digitally".into(),
                                    developer: Some("Pinta Contributors".into()),
                                    description: vec![
                                        "Pinta is a free, open source program for drawing and image editing.".into(),
                                        "• Unlimited undo history".into(),
                                        "• Layers and adjustments".into(),
                                    ],
                                    screenshots: vec![
                                        shot("wonderland-day.svg"),
                                        shot("wonderland-night.svg"),
                                    ],
                                    ..Default::default()
                                }),
                                size: Some("41.2 MB".into()),
                                perms: vec![
                                    perm("sockets=x11"),
                                    perm("filesystems=xdg-pictures"),
                                    perm("devices=dri"),
                                ],
                                ..Default::default()
                            },
                        )
                    } else {
                        (
                            ui::store::Subject::catalog(wol_core::store::get("gimp").unwrap()),
                            ui::store::Loaded {
                                size: Some("154.93 MiB".into()),
                                ..Default::default()
                            },
                        )
                    };
                    s.show_details(subject, loaded);
                    s.aw.root.clone().upcast()
                }),
            });
        }
        // A community package: the risk first, no Get button, and what reading it found.
        v.push(Case {
            name: format!("store-aur-details-{tag}"),
            size: (sw, sh),
            class: "preview-app",
            wallpaper: false,
            build: Box::new(move || {
                let s = ui::store::content(
                    Arc::new(shell::system::Fake),
                    ui::store::Env {
                        data_dirs: vec![],
                        pkg_search: false,
                        flatpaks: vec![],
                        installed: Some(Default::default()),
                    },
                );
                let hit = wol_core::store::PkgHit {
                    name: "demo-patched-bin".into(),
                    summary: "A browser with extra patches".into(),
                };
                s.show_details(ui::store::Subject::aur(&hit), Default::default());
                s.aw.root.clone().upcast()
            }),
        });
        // One app from three places, installed from one: versions, pin and undo.
        v.push(Case {
            name: format!("store-versions-{tag}"),
            size: (sw, sh),
            class: "preview-app",
            wallpaper: false,
            build: Box::new(move || {
                use shell::packages::AppSource;
                let s = ui::store::content(
                    Arc::new(shell::system::Fake),
                    ui::store::Env {
                        data_dirs: vec![],
                        pkg_search: false,
                        flatpaks: vec![],
                        installed: Some(Default::default()),
                    },
                );
                let src = |origin: &str, kind: &str, version: &str, installed| AppSource {
                    origin: origin.into(),
                    kind: kind.into(),
                    id: "demo".into(),
                    version: version.into(),
                    installed,
                };
                let hit = wol_core::store::PkgHit {
                    name: "demo".into(),
                    summary: "An app from three places".into(),
                };
                s.show_details(
                    ui::store::Subject::pkg(&hit),
                    ui::store::Loaded {
                        sources: vec![
                            src("WolOS", "pacman", "3.1-1", true),
                            src("Arch", "pacman", "3.0-2", false),
                            src("Flathub", "flatpak", "3.1", false),
                        ],
                        installed: Some("3.0-1".into()),
                        undo: Some("7".into()),
                        ..Default::default()
                    },
                );
                s.aw.root.clone().upcast()
            }),
        });
        // The Community page apart from the catalogue, locked: the warning and the way in, no package.
        v.push(Case {
            name: format!("store-aur-tab-{tag}"),
            size: (sw, sh),
            class: "preview-app",
            wallpaper: false,
            build: Box::new(move || {
                let s = ui::store::content(
                    Arc::new(shell::system::Fake),
                    ui::store::Env {
                        data_dirs: vec![],
                        pkg_search: false,
                        flatpaks: vec![],
                        installed: Some(Default::default()),
                    },
                );
                s.aw.open("Community");
                s.aw.root.clone().upcast()
            }),
        });
        // After the password: its search and the legend under it.
        v.push(Case {
            name: format!("store-aur-open-{tag}"),
            size: (sw, sh),
            class: "preview-app",
            wallpaper: false,
            build: Box::new(move || {
                let s = ui::store::content(
                    Arc::new(shell::system::Fake),
                    ui::store::Env {
                        data_dirs: vec![],
                        pkg_search: false,
                        flatpaks: vec![],
                        installed: Some(Default::default()),
                    },
                );
                s.aur.show_open();
                s.aw.open("Community");
                s.aw.root.clone().upcast()
            }),
        });
        v.push(Case {
            // A section of the details page's scroller: as tall as it reads (natural height).
            name: format!("store-aur-audit-{tag}"),
            size: (sw, 1),
            class: "preview-app",
            wallpaper: false,
            build: Box::new(move || {
                let graph = format!("{}/tests/golden_cases/aur-graph.svg", env!("CARGO_MANIFEST_DIR"));
                let found = ui::store::audit_section(&shell::packages::Audit {
                    state: "Needs Review".into(),
                    flags: vec![shell::packages::AuditFlag {
                        title: "Runs a Download Unchecked".into(),
                        severity: "High".into(),
                        count: 2,
                    }],
                    findings: vec![(
                        "Runs a Download Unchecked".into(),
                        "curl downloads http://203.0.113.10/x at demo.install:2, then sh runs it at demo.install:2, and no checksum checks it first.".into(),
                    )],
                    graph: Some(graph),
                });
                found.add_css_class("background");
                found.upcast()
            }),
        });
        for p in ui::settings::PAGES
            .into_iter()
            .chain(ui::settings::SUBPAGES)
        {
            v.push(Case {
                name: format!(
                    "settings-{}-{tag}",
                    p.to_lowercase().replace([' ', '&'], "")
                ),
                size: (sw, sh),
                class: "preview-app",
                wallpaper: false,
                build: Box::new(move || {
                    let a = ui::settings::content(
                        Arc::new(shell::system::Fake),
                        &ui::settings::Host::demo(),
                    );
                    a.open(p);
                    a.root.clone().upcast()
                }),
            });
        }
    }
    // Audit2 A2-4: a narrow Settings window brings its sidebar back from the toolbar toggle.
    v.push(Case {
        name: "settings-sidebar-narrow".into(),
        size: w::APP_NARROW,
        class: "preview-app",
        wallpaper: false,
        build: Box::new(|| {
            let a =
                ui::settings::content(Arc::new(shell::system::Fake), &ui::settings::Host::demo());
            a.open("General");
            // After the narrow layout is applied on realize.
            let t = a.sidebar_toggle.clone();
            a.root.connect_map(move |_| t.emit_clicked());
            a.root.clone().upcast()
        }),
    });
    // Clipboard history (⇧⌘V): a pinned snippet first, then the recent copies, newest first.
    // The list scrolls, so the panel's minimum height is less than it shows: its natural height.
    v.push(Case {
        name: "clipboard".into(),
        size: (360, 280),
        class: "preview",
        wallpaper: true,
        build: Box::new(|| {
            let mut h =
                shell::clipboard::History::with_pins(vec!["alice@wonderland.example".into()]);
            for t in [
                "sudo pacman -Syu",
                "https://wolos.org/docs/install",
                "Meet at the Mad Hatter’s at four",
            ] {
                h.push(t);
            }
            ui::clipboard::Panel::new(h).root.upcast()
        }),
    });
    // Session menu and the Restart / Shut Down / Log Out question (60 s countdown).
    v.push(Case {
        // Natural width: the panel is as wide as its five actions (453 since the Inter type scale; 146 high on the 20 px body line).
        name: "session".into(),
        size: (453, 146),
        class: "preview",
        wallpaper: true,
        build: Box::new(|| ui::panels::SessionMenu::new().root.upcast()),
    });
    // Natural sizes: the question sets the width and the countdown line wraps under it.
    for (action, size) in [("shutdown", (432, 166)), ("logout", (314, 166))] {
        v.push(Case {
            name: format!("session-confirm-{action}"),
            size,
            class: "preview",
            wallpaper: true,
            build: Box::new(move || {
                let m = ui::panels::SessionMenu::new();
                m.preview(action);
                m.root.upcast()
            }),
        });
    }
    v.push(Case {
        // Dock Downloads stack: newest files first, then Open in Files.
        name: "dock-downloads-stack".into(),
        size: (420, 1),
        class: "preview",
        wallpaper: true,
        build: Box::new(|| {
            let i = |n: &str, dir: bool| wol_core::downloads::Item {
                name: n.into(),
                path: format!("/home/alice/Downloads/{n}"),
                modified: 0,
                dir,
            };
            let items = [
                i("Quarterly report.pdf", false),
                i("holiday-photos.zip", false),
                i("wallpaper.png", false),
                i("Album", true),
                i("setup-notes.txt", false),
                i("song.mp3", false),
            ];
            let p = w::glass_panel(w::S4);
            p.append(&ui::dock::stack_grid(
                &items,
                "/home/alice/Downloads",
                Rc::new(|| {}),
            ));
            p.upcast()
        }),
    });
    v.push(Case {
        // Store tiles while installing: the ring fills with the install's percentage.
        name: "store-tile-progress".into(),
        size: (440, 1),
        class: "preview-app",
        wallpaper: false,
        build: Box::new(|| {
            let b = w::vbox(w::S2);
            for p in [0u8, 42, 90] {
                let t = w::AppTile::new(
                    &w::store_icon("gimp", w::ICON_L),
                    "GIMP",
                    "Edit photos and make artwork",
                );
                t.set_state(w::TileState::Progress(p));
                b.append(&t.root);
            }
            b.upcast()
        }),
    });
    v.extend(controlcenter_cases::control_center_cases());
    v.extend(form_kit::cases());
    v.extend(audit4_shell_cases());
    v.extend(widget_cases::cases());
    v.extend(settings_cases::cases());
    v.extend(displays_cases::cases());
    v.extend(focus_cases::cases());
    v.extend(setup_cases::cases());
    v.extend(locale_cases::cases());
    v.extend(plugin_host_cases::cases());
    v.extend(package_cases::cases());
    v.extend(app_view_cases::cases());
    v.extend(offer_cases::cases());
    v.extend(controlcenter_cases::cases());
    v.extend(theme_picker_cases::cases());
    v.extend(maker_cases::cases());
    v.extend(tray_cases::cases());
    v.extend(menubar_cases::cases());
    v.extend(motion_cases::cases());
    v.extend(monitor_cases::cases());
    for (tag, (sw, _)) in [("wide", w::APP_SIZE), ("narrow", w::APP_NARROW)] {
        v.push(Case {
            name: format!("settings-malware-{tag}"),
            size: (sw, 1),
            class: "preview-app",
            wallpaper: false,
            build: Box::new(|| {
                let p = w::vbox(w::S4);
                p.add_css_class("page");
                p.add_css_class("app-window");
                p.append(&ui::settings::malware_section(&wol_core::malware::Status {
                    on: Some(true),
                    signatures_built: Some(1),
                    signatures_age_days: Some(0),
                    signatures_version: Some(1),
                    blocked: vec![wol_core::malware::Blocked {
                        path: "/home/julia/Downloads/setup.exe".into(),
                        name: "Win.Test.Example".into(),
                        sha256: String::new(),
                        when: 1,
                        uid: 1000,
                    }],
                    full: wol_core::malware::Full {
                        on: true,
                        version: 28170,
                        signatures: 3_628_245,
                        updated: 1,
                        ..Default::default()
                    },
                }));
                p.append(&ui::settings::malware_full_section(
                    &wol_core::malware::Full {
                        on: true,
                        version: 28170,
                        signatures: 3_628_245,
                        updated: 1,
                        ..Default::default()
                    },
                    |_, _| {},
                ));
                p.upcast()
            }),
        });
        v.push(Case {
            name: format!("settings-windows-tiling-{tag}"),
            size: (sw, 1),
            class: "preview-app",
            wallpaper: false,
            build: Box::new(|| {
                let p = w::vbox(w::S4);
                p.add_css_class("page");
                p.add_css_class("app-window");
                p.append(&ui::settings::windows_section(
                    Arc::new(shell::system::Fake),
                    wol_core::hyprprefs::DesktopPrefs {
                        tiling: true,
                        layout: wol_core::hyprprefs::Layout::Master,
                        ..Default::default()
                    },
                ));
                p.upcast()
            }),
        });
    }
    v
}

fn find_type<T: IsA<gtk::Widget>>(w: &gtk::Widget, out: &mut Vec<T>) {
    if let Some(t) = w.downcast_ref::<T>() {
        out.push(t.clone());
    }
    let mut c = w.first_child();
    while let Some(ch) = c {
        find_type(&ch, out);
        c = ch.next_sibling();
    }
}

fn pump(ms: u64) {
    let until = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    let ctx = glib::MainContext::default();
    while std::time::Instant::now() < until {
        while ctx.iteration(false) {}
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Let focus and any focus-ring transition finish: an idle round, then two frame-clock frames.
fn settle(win: &gtk::Window) {
    while glib::MainContext::default().iteration(false) {}
    if let Some(fc) = win.frame_clock() {
        let start = fc.frame_counter();
        let until = std::time::Instant::now() + std::time::Duration::from_millis(500);
        while fc.frame_counter() < start + 2 && std::time::Instant::now() < until {
            win.queue_draw();
            pump(16);
        }
    }
}

fn render(win: &gtk::Window) -> gdk::Texture {
    settle(win);
    let (wd, ht) = (win.width(), win.height());
    let p = gtk::WidgetPaintable::new(Some(win));
    let mut node = None;
    for _ in 0..20 {
        let snap = gtk::Snapshot::new();
        p.snapshot(&snap, wd as f64, ht as f64);
        node = snap.to_node();
        if node.is_some() {
            break;
        }
        win.queue_draw();
        pump(100);
    }
    let node =
        node.unwrap_or_else(|| panic!("render node: window {wd}x{ht} mapped={}", win.is_mapped()));
    let r = gtk::gsk::CairoRenderer::new();
    r.realize(None::<&gdk::Surface>).expect("cairo renderer");
    let t = r.render_texture(
        node,
        Some(&gtk::graphene::Rect::new(0.0, 0.0, wd as f32, ht as f32)),
    );
    r.unrealize();
    t
}

/// Text that must meet WCAG AA (4.5:1) against what is behind it, measured from the render.
fn contrast_classes(case: &str) -> &'static [&'static str] {
    if case.starts_with("lock")
        || case.starts_with("login")
        || case.starts_with("setup")
        || case.starts_with("firstlogin")
    {
        &[
            "lock-field",
            "lock-message",
            "lock-name",
            "lock-caption",
            "lock-date",
        ]
    } else if case.starts_with("controlcenter")
        || case.starts_with("kit-")
        || case.starts_with("approval")
    {
        &["secondary", "title"]
    } else if case.starts_with("wolos-menu") {
        // A menu drawn out of its popover still sits on the popover's glass: its rows read in
        // dark as in light (the dark ones were light text on the light wallpaper).
        &["menu-item"]
    } else {
        &[]
    }
}

/// An icon-only button says what it does on hover: a visible Button or MenuButton showing an
/// image and no text needs a tooltip.
fn untitled_icon_buttons(w: &gtk::Widget) -> Vec<String> {
    fn has<T: IsA<gtk::Widget>>(w: &gtk::Widget) -> bool {
        let mut c = w.first_child();
        while let Some(ch) = c {
            if ch.is_drawable() && (ch.is::<T>() || has::<T>(&ch)) {
                return true;
            }
            c = ch.next_sibling();
        }
        false
    }
    let mut out = vec![];
    let button = w.is::<gtk::Button>() || w.is::<gtk::MenuButton>();
    if button
        && w.is_drawable()
        && w.tooltip_text().is_none()
        && has::<gtk::Image>(w)
        && !has::<gtk::Label>(w)
        // A MenuButton's inner button carries the tooltip on the MenuButton.
        && !w.parent().is_some_and(|p| p.is::<gtk::MenuButton>())
    {
        let icon = {
            let mut v = vec![];
            find_type::<gtk::Image>(w, &mut v);
            v.first()
                .and_then(|i| i.icon_name())
                .map(|n| n.to_string())
                .unwrap_or_default()
        };
        out.push(format!(
            "icon-only button without a tooltip ({icon}, .{})",
            w.css_classes().join(".")
        ));
    }
    let mut c = w.first_child();
    while let Some(ch) = c {
        out.extend(untitled_icon_buttons(&ch));
        c = ch.next_sibling();
    }
    out
}

/// One icon source (audit A6-ux 0.2): every themed icon a golden draws comes from the repo's
/// `design/` tree (the WolOS theme kit/style.rs lays out from it) or from GTK's own built-in
/// resources, never from an icon theme of the machine that renders. A host Adwaita or hicolor
/// icon would make the golden depend on what that machine has installed.
fn host_icons(w: &gtk::Widget) -> Vec<String> {
    let design = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../design"))
        .canonicalize()
        .expect("the design/ tree");
    let theme = gtk::IconTheme::for_display(&w.display());
    let mut images = vec![];
    find_type::<gtk::Image>(w, &mut images);
    let mut out = vec![];
    for i in images.iter().filter(|i| i.is_drawable()) {
        let size = if i.pixel_size() > 0 {
            i.pixel_size()
        } else {
            16
        };
        let (scale, dir, flags) = (
            i.scale_factor(),
            i.direction(),
            gtk::IconLookupFlags::empty(),
        );
        // The lookup GtkImage itself makes: a name as is, a GIcon with its fallback names.
        let (name, found) = if let Some(name) = i.icon_name() {
            let found = theme.lookup_icon(&name, &[], size, scale, dir, flags);
            (name.to_string(), found)
        } else if let Some(g) = i.gicon() {
            let found = theme.lookup_by_gicon(&g, size, scale, dir, flags);
            (
                gtk::gio::prelude::IconExt::to_string(&g)
                    .map(|s| s.to_string())
                    .unwrap_or_default(),
                found,
            )
        } else {
            continue;
        };
        let Some(file) = found.file() else {
            out.push(format!("icon {name} has no file"));
            continue;
        };
        if file.uri_scheme().as_deref() == Some("resource") {
            continue;
        }
        let path = file.path().and_then(|p| p.canonicalize().ok());
        if !path.as_deref().is_some_and(|p| p.starts_with(&design)) {
            out.push(format!(
                "icon {name} comes from outside design/: {}",
                file.uri()
            ));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The golden renderer sees the shipped icon set and nothing else. kit/style.rs lays the WolOS
/// theme out from design/ and leaves the machine's own icon directories (/usr/share/icons,
/// ~/.local/share/icons, flatpak exports) on the search path, and that theme inherits Adwaita and
/// hicolor: an app icon such as org.gimp.GIMP then resolves on a machine that has the app and
/// falls back on one that has not, and the golden follows the machine. Keep only the directories
/// that are the design/ tree or the theme root made from it; GTK's built-in resource icons stay.
fn pin_icon_search_path() {
    let design = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../design"))
        .canonicalize()
        .expect("the design/ tree");
    let temp = std::env::temp_dir()
        .canonicalize()
        .expect("the temp directory");
    let theme = gtk::IconTheme::for_display(&gdk::Display::default().unwrap());
    let own: Vec<PathBuf> = theme
        .search_path()
        .into_iter()
        .filter(|p| {
            let p = p.canonicalize().unwrap_or_else(|_| p.clone());
            p.starts_with(&design) || p.starts_with(&temp)
        })
        .collect();
    assert!(
        own.iter().any(|p| p.join("WolOS/index.theme").is_file()),
        "no WolOS theme made from design/ on the icon search path: {:?}",
        theme.search_path()
    );
    let own: Vec<&Path> = own.iter().map(PathBuf::as_path).collect();
    theme.set_search_path(&own);
}

fn find_class(w: &gtk::Widget, class: &str, out: &mut Vec<gtk::Widget>) {
    if w.is_drawable() && w.has_css_class(class) {
        out.push(w.clone());
    }
    let mut c = w.first_child();
    while let Some(ch) = c {
        find_class(&ch, class, out);
        c = ch.next_sibling();
    }
}

fn luminance(p: &[u8]) -> f64 {
    let ch = |v: u8| {
        let c = v as f64 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch(p[0]) + 0.7152 * ch(p[1]) + 0.0722 * ch(p[2])
}

/// Contrast between the widget's background (median pixel) and its most extreme pixel (glyph core).
fn contrast(px: &(i32, i32, Vec<u8>), r: &gtk::graphene::Rect) -> f64 {
    let (w, h, ref data) = *px;
    let (x0, y0) = (r.x().max(0.0) as i32, r.y().max(0.0) as i32);
    let (x1, y1) = (
        ((r.x() + r.width()) as i32).min(w),
        ((r.y() + r.height()) as i32).min(h),
    );
    let mut l: Vec<f64> = (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (y * w + x) as usize * 4))
        .map(|i| luminance(&data[i..i + 3]))
        .collect();
    if l.is_empty() {
        return 21.0;
    }
    l.sort_by(f64::total_cmp);
    let (lo, mid, hi) = (l[0], l[l.len() / 2], l[l.len() - 1]);
    ((hi + 0.05) / (mid + 0.05)).max((mid + 0.05) / (lo + 0.05))
}

fn contrast_issues(win: &gtk::Window, tex: &gdk::Texture, classes: &[&str]) -> Vec<String> {
    let px = pixels(tex);
    let mut out = Vec::new();
    for class in classes {
        let mut found = Vec::new();
        find_class(win.upcast_ref(), class, &mut found);
        for wd in found {
            if wd
                .downcast_ref::<gtk::Label>()
                .is_some_and(|l| l.label().is_empty())
            {
                continue;
            }
            let Some(r) = wd.compute_bounds(win) else {
                continue;
            };
            let c = contrast(&px, &r);
            if c < 4.5 {
                out.push(format!("{class}: contrast {c:.2}:1 < 4.5:1"));
            }
        }
    }
    out
}

/// True when the outer ring of the render carries colour: the dusk wallpaper is pink and violet,
/// a window without it is flat grey or white. Cases on the wallpaper must show it.
fn shows_wallpaper(t: &gdk::Texture) -> bool {
    let (w, h, px) = pixels(t);
    let edge = |v: i32, n: i32| v < 4 || v >= n - 4;
    (0..h).any(|y| {
        (0..w).any(|x| {
            if !(edge(x, w) || edge(y, h)) {
                return false;
            }
            let p = &px[((y * w + x) * 4) as usize..][..3];
            p.iter().max().unwrap() - p.iter().min().unwrap() > 20
        })
    })
}

/// Waits until the wallpaper is on screen instead of trusting a fixed delay: the stylesheet's
/// image loads off the main loop and a starved CPU can take many seconds to deliver it. Gives up
/// after 90 s.
fn wait_for_wallpaper(win: &gtk::Window) -> bool {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(90);
    while !shows_wallpaper(&render(win)) {
        if std::time::Instant::now() > until {
            return false;
        }
        pump(250);
    }
    true
}

fn pixels(t: &gdk::Texture) -> (i32, i32, Vec<u8>) {
    let mut d = gdk::TextureDownloader::new(t);
    d.set_format(gdk::MemoryFormat::R8g8b8a8);
    let (bytes, stride) = d.download_bytes();
    let (w, h) = (t.width(), t.height());
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h as usize {
        out.extend_from_slice(&bytes[y * stride..y * stride + w as usize * 4]);
    }
    (w, h, out)
}

/// Share of differing pixels, or 1.0 on size mismatch.
fn diff(a: &gdk::Texture, b: &gdk::Texture) -> f64 {
    let ((aw, ah, pa), (bw, bh, pb)) = (pixels(a), pixels(b));
    if (aw, ah) != (bw, bh) {
        return 1.0;
    }
    let bad = pa
        .chunks(4)
        .zip(pb.chunks(4))
        .filter(|(x, y)| x.iter().zip(*y).any(|(p, q)| p.abs_diff(*q) > 24))
        .count();
    bad as f64 / (aw * ah) as f64
}

thread_local! {
    static WARNINGS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn main() {
    // Several golden runs share a build machine (one per checkout): start at a pid-based display
    // and take the first whose port and socket nobody holds, or broadwayd cannot bind and
    // gtk::init fails.
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let n = (0..200)
        .map(|i| 40 + (std::process::id() + i) % 200)
        .find(|n| {
            !Path::new(&runtime)
                .join(format!("broadway{}.socket", n + 1))
                .exists()
                && std::net::TcpListener::bind(("0.0.0.0", (18000 + n) as u16)).is_ok()
        })
        .expect("a free Broadway display (ports 18040-18239)");
    let display = format!(":{n}");
    let port = (18000 + n).to_string();
    /// Kills the private broadwayd on every exit path (panics included).
    struct Kill(std::process::Child);
    impl Drop for Kill {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let broadway = Kill(
        std::process::Command::new("gtk4-broadwayd")
            .args([display.as_str(), "--port", port.as_str()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("gtk4-broadwayd (GTK 4 package) is required"),
    );
    let sock = Path::new(&runtime).join(format!("broadway{}.socket", n + 1));
    // gtk::init races broadwayd start-up: wait (up to 5 s) until its socket accepts a connection.
    for _ in 0..100 {
        if std::os::unix::net::UnixStream::connect(&sock).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let cfg = std::env::temp_dir().join(format!("wol-golden-{}", std::process::id()));
    unsafe {
        std::env::set_var("GDK_BACKEND", "broadway");
        std::env::set_var("BROADWAY_DISPLAY", &display);
        std::env::set_var("WOL_FAKE_NOW", "1");
        std::env::set_var("XDG_CONFIG_HOME", &cfg);
        std::env::set_var("GTK_THEME", "Adwaita");
        // The text scale (Settings > Accessibility > Larger text) is a gsettings key; the checks
        // set it and read it back on a backend of their own, never the host's.
        std::env::set_var("GSETTINGS_BACKEND", "memory");
    }
    // Any GLib/GTK warning or critical fails the case that produced it (style bugs, bad sizes).
    {
        glib::log_set_writer_func(|level, fields| {
            if matches!(
                level,
                glib::LogLevel::Warning | glib::LogLevel::Critical | glib::LogLevel::Error
            ) {
                let msg = fields
                    .iter()
                    .find(|f| f.key() == "MESSAGE")
                    .and_then(|f| f.value_str().map(str::to_owned))
                    .unwrap_or_default();
                // Known noise, not style bugs: GTK 4.24's deprecation notice for the
                // `gtk-application-prefer-dark-theme` set once per variant (kit/style.rs),
                // Broadway's frame-timing warning that lands on a random case, and the layer-shell
                // library (1.3, `--features layer-shell`): its `gtk_layer_is_supported()` asserts
                // a Wayland display before answering no, so every `is_supported()` in
                // kit/surface.rs logs it under Broadway (first from the dock regression's
                // `set_keyboard`); the surface stays a dev toplevel, which is what a golden draws.
                const NOISE: [&str; 3] = [
                    "Use GtkSettings:gtk-interface-color-scheme instead",
                    "Duplicate call to gdk_frame_timings_submitted",
                    "init_and_get_layer_shell_global: assertion 'GDK_IS_WAYLAND_DISPLAY(gdk_display)' failed",
                ];
                if !NOISE.iter().any(|n| msg.contains(n)) {
                    WARNINGS.with_borrow_mut(|v| v.push(format!("{level:?}: {msg}")));
                }
            }
            glib::log_writer_default(level, fields)
        });
    }
    gtk::init().expect("gtk init");
    // goldens must not depend on the caret blink phase
    if let Some(s) = gtk::Settings::default() {
        s.set_gtk_cursor_blink(false);
    }
    // Product symbolic icons (installed into the WolOS theme on the image).
    gtk::IconTheme::for_display(&gdk::Display::default().unwrap()).add_search_path(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../design/icons/symbolic"
    ));
    locale_cases::pseudo_locale_leaves_no_english();
    locale_cases::picking_a_language_relabels_setup();
    setup_cases::account_tab_draws_one_ring();
    setup_cases::welcome_tab_rings_continue();
    setup_cases::validation_survives_back_forward();
    setup_cases::row_buttons_ring_inside_the_row();
    setup_cases::empty_desktop_renders_a_node();
    setup_cases::drop_shelf_insets_are_even();
    regressions::effects_page_answers_after_it_is_built();
    settings_cases::checks();
    settings_cases::larger_text_is_a_switch_and_grows_the_shell_text();
    displays_cases::checks();
    regressions::a_stalled_page_fade_settles();
    regressions::a_narrow_window_can_bring_the_sidebar_back();
    regressions::displays_coming_and_going_leave_nothing_behind();
    regressions::a_toggle_waits_for_its_change_and_reverts_when_it_fails();
    regressions::return_belongs_to_the_button_with_the_keyboard_ring();
    regressions::the_dock_takes_a_new_layout();
    motion_cases::reduce_motion_snaps();
    regressions::an_app_tile_keeps_one_source_badge();
    regressions::installed_is_a_state_and_open_the_action();
    focus_cases::sidebar_arrows_go_from_group_to_group();
    focus_cases::only_the_focused_control_has_the_ring();
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("golden");
    std::fs::create_dir_all(&out).unwrap();
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let update = std::env::var_os("WOL_GOLDEN_UPDATE").is_some();
    let update_labels = update || std::env::var("WOL_GOLDEN_LABELS").as_deref() == Ok("update");
    let labels_file = golden.join(LABELS_FILE);
    let mut labels = read_labels(&labels_file);
    let only = std::env::var("WOL_GOLDEN_ONLY").ok();
    let mut failures = Vec::new();
    // What the checks above logged is theirs: left in WARNINGS it would fail the first case of
    // the loop (menubar-light, or whichever WOL_GOLDEN_ONLY picks) for a reason it never had.
    let setup = WARNINGS.with_borrow_mut(std::mem::take);
    for i in &setup {
        eprintln!("setup: {i}");
    }
    if !setup.is_empty() {
        failures.push("setup (the checks before the cases)".to_string());
    }
    for variant in [Variant::Light, Variant::Dark] {
        let t = theme::resolve(
            Some(if variant == Variant::Light {
                "wol-light"
            } else {
                "wol-dark"
            }),
            false,
        );
        let extra = format!(
            "window.preview {{ background: url(\"file://{WALL}\") center / cover; }} window.preview-app {{ background: none; }}"
        );
        kit::style::apply(&t, &extra);
        if variant == Variant::Light {
            pin_icon_search_path();
            // Once, with the kit stylesheet on: a golden in the fallback type locks the wrong glyphs.
            font_cases::ui_family_is_inter();
            regressions::shortcut_sheet_rows_are_whole();
        }
        for c in cases() {
            let name = format!(
                "{}-{}",
                c.name,
                if variant == Variant::Light {
                    "light"
                } else {
                    "dark"
                }
            );
            if only.as_deref().is_some_and(|o| !name.contains(o)) {
                continue;
            }
            let win = gtk::Window::builder()
                .decorated(false)
                .default_width(c.size.0)
                .default_height(c.size.1)
                .build();
            win.add_css_class(c.class);
            if !c.wallpaper && c.class == "preview-app" {
                win.add_css_class("app");
            }
            // Every case starts in English; a locale case sets its own language.
            shell::i18n::set_lang(shared::i18n::Lang::En);
            let child = (c.build)();
            // Broadway clamps windows to its 1024x768 screen; a sized frame holds the declared size.
            let frame = gtk::Overlay::new();
            frame.set_child(Some(&child));
            if c.size.1 > 1 {
                frame.set_size_request(c.size.0, c.size.1);
            }
            win.set_child(Some(&frame));
            win.present();
            pump(900);
            let wallpaper_waited = !c.wallpaper || wait_for_wallpaper(&win);
            fn count(n: &shell::layout::Node) -> usize {
                1 + n.children.iter().map(count).sum::<usize>()
            }
            let nodes = win
                .child()
                .map(|c| count(&kit::check::tree(&c, win.upcast_ref(), "")))
                .unwrap_or(0);
            assert!(nodes > 3, "{name}: layout tree has {nodes} nodes");
            let mut issues: Vec<String> = kit::check::issues(&win)
                .into_iter()
                .map(|i| i.message)
                .collect();
            if c.size.1 > 1 && (win.width() > c.size.0 || win.height() > c.size.1) {
                issues.push(format!(
                    "content needs {}x{}, surface is {}x{}",
                    win.width(),
                    win.height(),
                    c.size.0,
                    c.size.1
                ));
            }
            let tex = render(&win);
            // A golden must be the size it claims (height 1 = natural height).
            if (tex.width(), tex.height()) != c.size && (c.size.1 > 1 || tex.width() != c.size.0) {
                issues.push(format!(
                    "rendered {}x{}, declared {}x{}",
                    tex.width(),
                    tex.height(),
                    c.size.0,
                    c.size.1
                ));
            }
            if !wallpaper_waited {
                issues.push("wallpaper not drawn after 90 s".into());
            }
            issues.extend(WARNINGS.with_borrow_mut(std::mem::take));
            issues.extend(contrast_issues(&win, &tex, contrast_classes(&c.name)));
            issues.extend(untitled_icon_buttons(win.upcast_ref()));
            issues.extend(host_icons(win.upcast_ref()));
            let said = button_labels(win.upcast_ref()).join(" | ");
            if update_labels {
                labels.insert(name.clone(), said);
            } else {
                match labels.get(&name) {
                    Some(was) if *was == said => {}
                    Some(was) => issues.push(format!(
                        "button labels changed: golden [{was}], now [{said}]"
                    )),
                    None => issues.push(format!(
                        "no button labels in {LABELS_FILE} (run with WOL_GOLDEN_LABELS=update)"
                    )),
                }
            }
            let png = out.join(format!("{name}.png"));
            tex.save_to_png(&png).unwrap();
            let gold = golden.join(format!("{name}.png"));
            if update {
                std::fs::copy(&png, &gold).unwrap();
            } else if let Ok(g) = gdk::Texture::from_filename(&gold) {
                if c.wallpaper && !shows_wallpaper(&g) {
                    issues.push("golden was captured without the wallpaper".into());
                }
                let d = diff(&tex, &g);
                if d > TOLERANCE {
                    issues.push(format!(
                        "differs from golden by {:.2}% of pixels",
                        d * 100.0
                    ));
                }
            } else {
                issues.push("no golden (run with WOL_GOLDEN_UPDATE=1)".into());
            }
            for i in &issues {
                eprintln!("{name}: {i}");
            }
            if !issues.is_empty() {
                failures.push(name.clone());
            }
            println!(
                "{name}: {} ({nodes} widgets checked)",
                if issues.is_empty() { "ok" } else { "FAIL" }
            );
            win.destroy();
        }
    }
    if update_labels {
        write_labels(&labels_file, &labels);
    }
    drop(broadway);
    let _ = std::fs::remove_dir_all(cfg);
    println!("renders: {}", out.display());
    if !failures.is_empty() {
        eprintln!("golden failures: {failures:?}");
        std::process::exit(1);
    }
}

/// The button-label manifest next to the PNGs: one `case<TAB>labels` line per case, sorted.
const LABELS_FILE: &str = "button-labels.tsv";

fn read_labels(p: &Path) -> std::collections::BTreeMap<String, String> {
    std::fs::read_to_string(p)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn write_labels(p: &Path, m: &std::collections::BTreeMap<String, String>) {
    let mut s = String::new();
    for (k, v) in m {
        s.push_str(&format!("{k}\t{v}\n"));
    }
    std::fs::write(p, s).unwrap();
}

/// The text of every drawn button (its label, or its drawn labels joined), in tree order. A state
/// that became a button (or a button that became a state) changes this list, not only pixels.
fn button_labels(w: &gtk::Widget) -> Vec<String> {
    fn texts(w: &gtk::Widget, out: &mut Vec<String>) {
        if !w.is_drawable() {
            return;
        }
        if let Some(l) = w.downcast_ref::<gtk::Label>() {
            let t = l.label();
            if !t.is_empty() {
                out.push(t.to_string());
            }
        }
        let mut c = w.first_child();
        while let Some(ch) = c {
            texts(&ch, out);
            c = ch.next_sibling();
        }
    }
    let mut out = Vec::new();
    if w.is_drawable() && w.is::<gtk::Button>() {
        let mut t = Vec::new();
        texts(w, &mut t);
        if !t.is_empty() {
            out.push(t.join(" ").replace(['\t', '\n'], " "));
        }
        return out;
    }
    let mut c = w.first_child();
    while let Some(ch) = c {
        out.extend(button_labels(&ch));
        c = ch.next_sibling();
    }
    out
}

/// Audit4 S3-7: the login window with no account that can sign in says so.
fn audit4_shell_cases() -> Vec<Case> {
    vec![Case {
        name: "login-empty".into(),
        size: (900, 600),
        class: "lock-window",
        wallpaper: false,
        build: Box::new(|| ui::lock::Login::new(&[]).root.clone()),
    }]
}
