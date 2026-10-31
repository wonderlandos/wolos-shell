// SPDX-License-Identifier: GPL-2.0-or-later
//! wol-analyze: what Files' "Analyze with ClamAV" opens. Checks through `wol-xguard scan`. With
//! the signatures off it checks nothing and offers Settings: only the switch there turns them on.
use crate::copy;
use crate::kit::{self, widgets as w};
use gtk::{gio, glib, prelude::*};
use wol_core::malware::{self, Analysis, Full};

/// Where Settings keeps the switch.
const SETTINGS: [&str; 2] = ["wol-settings", "Privacy & Security"];
/// Findings listed in the window; the sentence above them still covers all of them.
const MAX_ROWS: usize = 6;

pub enum View {
    /// Waiting for `wol-xguard scan`.
    Checking { shown: String },
    Done {
        analysis: Analysis,
        shown: String,
        full: Full,
    },
    /// Nothing was checked: the signatures are off or not here yet.
    NotReady(Full),
}

/// How the selection is named in a sentence.
pub fn shown(paths: &[String]) -> String {
    match paths {
        [one] => format!("“{}”", malware::file_name(one)),
        many => format!("{} files", many.len()),
    }
}

fn head(icon: &str, title: &str, body: &str) -> gtk::Box {
    let head = w::hbox(w::S3);
    let image = gtk::Image::from_icon_name(icon);
    image.set_pixel_size(w::ICON_L);
    head.append(&image);
    let t = w::vbox(0);
    t.set_hexpand(true);
    t.set_valign(gtk::Align::Center);
    let tl = w::label(title, "title");
    tl.set_wrap(true);
    t.append(&tl);
    let why = w::label(body, "secondary");
    why.set_wrap(true);
    t.append(&why);
    head.append(&t);
    head
}

/// (root, Open Settings when offered, the button that closes the window).
pub fn content(view: &View) -> (gtk::Widget, Option<gtk::Button>, gtk::Button) {
    let root = w::glass_panel(0);
    root.add_css_class("approval");
    root.set_width_request(w::CONTROL_CENTER_WIDTH + w::S8);
    let b = w::vbox(w::S3);
    let row = w::hbox(w::S2);
    row.append(&w::spacer());
    let mut settings = None;
    let close = match view {
        View::Checking { shown } => {
            let (title, body) = copy::analyze_checking(shown);
            let top = head("security-high-symbolic", &title, &body);
            top.append(&w::spinner());
            b.append(&top);
            w::button("Cancel", w::Kind::Default, || {})
        }
        View::Done {
            analysis,
            shown,
            full,
        } => {
            let (title, body) = copy::analyze_result(analysis, shown, full);
            let icon = match analysis {
                Analysis::Clean => "security-high-symbolic",
                Analysis::Found(_) | Analysis::Incomplete(_) => "dialog-warning-symbolic",
            };
            b.append(&head(icon, &title, &body));
            let rows: Vec<gtk::Widget> = match analysis {
                Analysis::Clean => Vec::new(),
                Analysis::Found(hits) => hits
                    .iter()
                    .take(MAX_ROWS)
                    .map(|h| {
                        w::list_row("", malware::file_name(&h.path), &h.name, None)
                            .upcast::<gtk::Widget>()
                    })
                    .collect(),
                Analysis::Incomplete(why) => why
                    .iter()
                    .take(MAX_ROWS)
                    .map(|s| w::list_row("", s, "", None).upcast::<gtk::Widget>())
                    .collect(),
            };
            if !rows.is_empty() {
                b.append(&w::section("", &rows));
            }
            w::button("Close", w::Kind::Default, || {})
        }
        View::NotReady(full) => {
            let (title, body) = copy::analyze_offer(full);
            b.append(&head("dialog-information-symbolic", &title, &body));
            settings = Some(w::button("Open Settings", w::Kind::Primary, || {}));
            w::button("Cancel", w::Kind::Default, || {})
        }
    };
    row.append(&close);
    if let Some(s) = &settings {
        row.append(s);
    }
    b.append(&row);
    let sheet = w::sheet(&b);
    sheet.set_reveal_child(true);
    root.append(&sheet);
    (root.upcast(), settings, close)
}

fn show(win: &gtk::ApplicationWindow, view: &View) {
    let (root, settings, close) = content(view);
    win.set_child(Some(&root));
    let wn = win.clone();
    close.connect_clicked(move |_| wn.close());
    if let Some(s) = settings {
        let wn = win.clone();
        s.connect_clicked(move |_| {
            kit::spawn(&SETTINGS.map(str::to_owned));
            wn.close();
        });
    }
}

fn scan_output(argv: &[String]) -> String {
    std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

fn full_now() -> Full {
    let t = std::time::Duration::from_secs(5);
    malware::full(
        &|argv: &[&str]| wol_core::system::run_with_timeout(argv, t),
        &|p: &str| std::fs::read_to_string(p).ok(),
    )
}

/// Absolute paths only: anything else could read as an option of `wol-xguard scan`.
pub fn paths(args: impl Iterator<Item = String>) -> Vec<String> {
    args.filter(|a| a.starts_with('/')).collect()
}

pub fn run() -> glib::ExitCode {
    let files = paths(std::env::args().skip(1));
    if files.is_empty() {
        eprintln!("wol-analyze: no file to check");
        return glib::ExitCode::FAILURE;
    }
    let app = gtk::Application::builder()
        .application_id("org.wolos.analyze")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| {
        kit::style::install("");
        let win = gtk::ApplicationWindow::builder()
            .application(app)
            .title(copy::ANALYZE_TITLE)
            .decorated(false)
            .resizable(false)
            .build();
        let keys = gtk::EventControllerKey::new();
        let wn = win.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                wn.close();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        win.add_controller(keys);
        let full = full_now();
        if !full.ready() {
            show(&win, &View::NotReady(full));
            win.present();
            return;
        }
        let name = shown(&files);
        show(
            &win,
            &View::Checking {
                shown: name.clone(),
            },
        );
        win.present();
        let (wn, argv, asked) = (win.clone(), malware::analyze_argv(&files), files.len());
        glib::spawn_future_local(async move {
            let out = gio::spawn_blocking(move || scan_output(&argv))
                .await
                .unwrap_or_default();
            let analysis = malware::parse_analysis(&out, asked);
            show(
                &wn,
                &View::Done {
                    analysis,
                    shown: name,
                    full,
                },
            );
        });
    });
    app.run_with_args::<&str>(&[]);
    glib::ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selection_is_named_and_only_absolute_paths_pass() {
        assert_eq!(shown(&["/a/setup.exe".into()]), "“setup.exe”");
        assert_eq!(shown(&["/a".into(), "/b".into()]), "2 files");
        let given = ["--rules", "/tmp/x", "-", "/home/a"].map(str::to_owned);
        assert_eq!(paths(given.into_iter()), ["/tmp/x", "/home/a"]);
    }
}
