// SPDX-License-Identifier: GPL-2.0-or-later
//! The share sheet (E2E-SHARE, docs/modules/share.md): what Share opens for a file, a text or a
//! link, from a libwol app (`share.file`, `share.text`), from Files ("Share…"), from Atlas
//! (through the bridge) and from `wol-share`. A tile per app that can take the thing
//! ([`crate::share_targets`]), then Open With…, Save… and Copy; Escape or Cancel closes it.
//! Cancel has the focus when it opens; Tab and the arrow keys reach the tiles, Return picks the one with the focus.
use crate::copy;
use crate::kit::widgets as w;
use gtk::{glib, prelude::*};
use std::cell::RefCell;
use std::rc::Rc;

/// Tiles in a row.
const PER_ROW: i32 = 4;
/// Characters of an app's name under its icon before it is cut with an ellipsis.
const NAME_CHARS: i32 = 9;

/// An app that can take the thing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tile {
    pub name: String,
    pub icon: String,
}

/// Everything the sheet shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    /// The file's name, or the title of the text.
    pub title: String,
    /// The sentence under it.
    pub detail: String,
    pub tiles: Vec<Tile>,
    /// Open With… is for files only.
    pub open_with: bool,
    /// "Save a Copy…" for a file, "Save…" for text.
    pub save: String,
    /// Save and Copy beside Cancel: false for the Open With chooser, which only picks an app.
    pub actions: bool,
}

impl View {
    /// The sheet for a file called `name`.
    pub fn file(name: &str, tiles: Vec<Tile>) -> Self {
        Self::of(
            name,
            copy::FILE_SHEET_DETAIL,
            tiles,
            true,
            "Save a Copy\u{2026}",
        )
    }

    /// The sheet for a text or a link; `preview` is its start.
    pub fn text(title: &str, preview: &str, tiles: Vec<Tile>) -> Self {
        let detail = if preview.is_empty() {
            copy::SHARE_TEXT_DETAIL
        } else {
            preview
        };
        Self::of(title, detail, tiles, false, "Save\u{2026}")
    }

    /// The Open With chooser for a file called `name`: the apps that open it, Cancel.
    pub fn open(name: &str, tiles: Vec<Tile>) -> Self {
        let mut v = Self::of(name, copy::OPEN_WITH_DETAIL, tiles, false, "");
        v.title = format!("Open \u{201c}{name}\u{201d} With");
        v.actions = false;
        v
    }

    fn of(title: &str, detail: &str, tiles: Vec<Tile>, open_with: bool, save: &str) -> Self {
        let detail = if tiles.is_empty() {
            copy::SHARE_NO_APPS
        } else {
            detail
        };
        View {
            title: title.to_owned(),
            detail: detail.to_owned(),
            tiles,
            open_with,
            save: save.to_owned(),
            actions: true,
        }
    }
}

/// What the person chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// The tile at this index.
    Target(usize),
    Copy,
    Save,
    OpenWith,
    Cancel,
}

fn tile(t: &Tile, picked: impl Fn() + 'static) -> gtk::Button {
    let b = gtk::Button::new();
    b.add_css_class("square-tile");
    let col = w::vbox(w::S1);
    col.append(&w::app_icon(&t.icon, w::ICON_L));
    let name = w::centered(&t.name, "");
    name.set_max_width_chars(NAME_CHARS);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    col.append(&name);
    b.set_child(Some(&col));
    b.set_tooltip_text(Some(&t.name));
    b.connect_clicked(move |_| picked());
    b
}

/// (the sheet's root, the widget that takes the focus first: Cancel). `pick` gets the choice.
pub fn content(view: &View, pick: Rc<dyn Fn(Choice)>) -> (gtk::Widget, gtk::Widget) {
    let root = w::glass_panel(0);
    root.add_css_class("approval");
    // Four tiles in a row, a little wider than the Check for Malware window.
    root.set_width_request(w::CONTROL_CENTER_WIDTH + w::S4 + w::S10 + w::S4);
    let b = w::vbox(w::S3);
    let head = w::vbox(w::S1);
    let title = w::label(&view.title, "title");
    title.set_wrap(true);
    head.append(&title);
    let why = w::label(&view.detail, "secondary");
    why.set_wrap(true);
    head.append(&why);
    b.append(&head);

    if !view.tiles.is_empty() {
        let grid = gtk::Grid::new();
        grid.set_column_homogeneous(true);
        let gap = u32::try_from(w::S2).unwrap_or_default();
        grid.set_column_spacing(gap);
        grid.set_row_spacing(gap);
        for ((i, t), n) in view.tiles.iter().enumerate().zip(0i32..) {
            let p = pick.clone();
            let btn = tile(t, move || p(Choice::Target(i)));
            grid.attach(&btn, n % PER_ROW, n / PER_ROW, 1, 1);
        }
        b.append(&grid);
    }

    let row = w::hbox(w::S2);
    let p = pick.clone();
    let cancel = w::button("Cancel", w::Kind::Default, move || p(Choice::Cancel));
    row.append(&cancel);
    row.append(&w::spacer());
    if view.open_with {
        let p = pick.clone();
        row.append(&w::button(
            "Open With\u{2026}",
            w::Kind::Default,
            move || p(Choice::OpenWith),
        ));
    }
    if view.actions {
        let p = pick.clone();
        row.append(&w::button(&view.save, w::Kind::Default, move || {
            p(Choice::Save)
        }));
        let p = pick.clone();
        row.append(&w::button("Copy", w::Kind::Primary, move || {
            p(Choice::Copy)
        }));
    }
    b.append(&row);

    let sheet = w::sheet(&b);
    sheet.set_reveal_child(true);
    root.append(&sheet);
    // The sheet never starts on a tile: a stray Return must not send the file anywhere. Cancel has
    // the keyboard; the arrows and Tab reach the tiles.
    (root.upcast(), cancel.upcast())
}

/// Show the sheet in its own window; `done` gets the choice once (closing the window is Cancel).
/// On the GTK main thread.
pub fn open(view: &View, done: impl FnOnce(Choice) + 'static) {
    let done = Rc::new(RefCell::new(Some(done)));
    let win = gtk::Window::builder()
        .title(&view.title)
        .modal(true)
        .decorated(false)
        .resizable(false)
        .build();
    if let Some(app) = gtk::gio::Application::default().and_downcast::<gtk::Application>() {
        win.set_application(Some(&app));
    }
    let finish = {
        let (win, done) = (win.clone(), done.clone());
        move |c: Choice| {
            if let Some(f) = done.borrow_mut().take() {
                f(c);
            }
            win.close();
        }
    };
    let finish: Rc<dyn Fn(Choice)> = Rc::new(finish);
    let (root, first) = content(view, finish.clone());
    win.set_child(Some(&root));
    // Closing it any other way is a Cancel as well.
    win.connect_close_request(move |_| {
        if let Some(f) = done.borrow_mut().take() {
            f(Choice::Cancel);
        }
        glib::Propagation::Proceed
    });
    crate::kit::surface::on_escape(&win, move || finish(Choice::Cancel));
    win.present();
    first.grab_focus();
}

/// What `wol-share` was asked to share.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Given {
    /// A file, by its full path.
    File(String),
    /// Text from standard input.
    Stdin,
    /// A web address.
    Link(String),
}

/// `wol-share FILE | -`: a path (made full), `-`, or a web address. None for anything else
/// (no argument, more than one, an option).
pub fn given(args: &[String], cwd: &std::path::Path) -> Option<Given> {
    let [one] = args else { return None };
    if one == "-" {
        return Some(Given::Stdin);
    }
    if crate::share_targets::is_url(one) {
        return Some(Given::Link(one.clone()));
    }
    if one.starts_with('-') || one.is_empty() {
        return None;
    }
    let p = cwd.join(one);
    Some(Given::File(p.to_string_lossy().into_owned()))
}

/// `wol-share`: the share sheet for one thing (`args`: what the command line gave after the
/// program name; `cwd` makes a relative path full). Ok is what the person did, one word
/// (`copied`, `saved`, `opened` or `cancelled`); Err is the sentence to tell them (or the usage
/// line, which no person reads on screen). Files' "Share…" starts it.
pub fn run(args: &[String], cwd: &std::path::Path) -> Result<String, String> {
    use shared::portal::Ask;
    let Some(what) = given(args, cwd) else {
        return Err("usage: wol-share FILE | - | ADDRESS".to_owned());
    };
    let app = gtk::Application::builder()
        .application_id("org.wolos.share")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let result: Rc<RefCell<Result<String, String>>> =
        Rc::new(RefCell::new(Ok("cancelled".to_owned())));
    let out = result.clone();
    app.connect_activate(move |app| {
        crate::kit::style::install("");
        // The sheet is a window of its own; the application stays until the answer is in.
        let hold = app.hold();
        let (app, out, what) = (app.clone(), out.clone(), what.clone());
        glib::spawn_future_local(async move {
            let ask = gtk::gio::spawn_blocking(move || {
                use std::io::Read;
                match what {
                    Given::File(path) => Ok(Ask::ShareFile { path }),
                    Given::Link(url) => Ok(Ask::Share {
                        title: url.clone(),
                        text: url,
                    }),
                    Given::Stdin => {
                        let mut bytes = Vec::new();
                        let read = std::io::stdin()
                            .take(crate::share_targets::stdin_limit())
                            .read_to_end(&mut bytes);
                        match read {
                            Ok(_) => crate::share_targets::stdin_text(bytes).and_then(|text| {
                                if text.trim().is_empty() {
                                    return Err("nothing to share on standard input".to_owned());
                                }
                                Ok(Ask::Share {
                                    title: "Text".to_owned(),
                                    text,
                                })
                            }),
                            Err(_) => Err("nothing to share on standard input".to_owned()),
                        }
                    }
                }
            })
            .await
            .unwrap_or_else(|_| Err("nothing could be read".to_owned()));
            let done = match ask {
                Ok(ask) => gtk::gio::spawn_blocking(move || {
                    crate::app_session::answer(&crate::app_session::Desktop, ask)
                })
                .await
                .unwrap_or_else(|_| Err(copy::SHARE_APP_NOT_STARTED.to_owned())),
                Err(e) => Err(e),
            };
            *out.borrow_mut() = done.map(|v| {
                v.as_ref()
                    .and_then(|v| v.get("done"))
                    .and_then(|d| d.as_str())
                    .unwrap_or("cancelled")
                    .to_owned()
            });
            drop(hold);
            app.quit();
        });
    });
    app.run_with_args::<&str>(&[]);
    result.replace(Ok(String::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn wol_share_takes_one_file_dash_or_address() {
        let cwd = Path::new("/home/ana");
        assert_eq!(
            given(&args(&["/tmp/a b.txt"]), cwd),
            Some(Given::File("/tmp/a b.txt".into()))
        );
        assert_eq!(
            given(&args(&["notes/a.txt"]), cwd),
            Some(Given::File("/home/ana/notes/a.txt".into()))
        );
        assert_eq!(given(&args(&["-"]), cwd), Some(Given::Stdin));
        assert_eq!(
            given(&args(&["https://e.org/x"]), cwd),
            Some(Given::Link("https://e.org/x".into()))
        );
        for bad in [args(&[]), args(&["--help"]), args(&["a", "b"]), args(&[""])] {
            assert_eq!(given(&bad, cwd), None, "{bad:?}");
        }
    }

    #[test]
    fn the_open_with_chooser_only_picks_an_app() {
        let tile = |n: &str| Tile {
            name: n.into(),
            icon: "i".into(),
        };
        let v = View::open("a.pdf", vec![tile("Papers"), tile("Atlas")]);
        assert_eq!(v.title, "Open \u{201c}a.pdf\u{201d} With");
        assert_eq!(v.detail, copy::OPEN_WITH_DETAIL);
        assert!(!v.actions && !v.open_with);
        assert!(View::file("a.pdf", vec![tile("x")]).actions);
        assert!(View::text("t", "", vec![]).actions);
    }

    #[test]
    fn a_sheet_without_apps_says_so_and_keeps_copy_and_save() {
        let none = View::file("a.pdf", Vec::new());
        assert_eq!(none.detail, copy::SHARE_NO_APPS);
        assert!(none.open_with && none.tiles.is_empty());
        assert!(copy::lint(&none.detail).is_empty());
        let some = View::file(
            "a.pdf",
            vec![Tile {
                name: "Notes".into(),
                icon: "n".into(),
            }],
        );
        assert_eq!(some.detail, copy::FILE_SHEET_DETAIL);
        let text = View::text("Idea", "", Vec::new());
        assert!(!text.open_with);
        assert_eq!(text.save, "Save\u{2026}");
        assert_eq!(
            View::text(
                "Idea",
                "start of it",
                vec![Tile {
                    name: "N".into(),
                    icon: "n".into()
                }]
            )
            .detail,
            "start of it"
        );
    }
}
