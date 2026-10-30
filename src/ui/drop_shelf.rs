//! The drop shelf: files dropped on the desktop gather on a glass shelf at the right edge, and
//! are dragged from there into any app (one row, or all of them at once). Kit parts only.
use crate::drop_shelf::{Shelf, count, name};
use crate::kit::widgets as w;
use gtk::{gdk, gio, prelude::*};
use std::cell::RefCell;
use std::path::{Path, PathBuf};

/// Shelf width.
pub const WIDTH: i32 = 280;
/// Taller than this, the list scrolls.
pub const MAX_HEIGHT: i32 = 420;

thread_local! {
    static SHELF: RefCell<Shelf> = RefCell::default();
    static WIN: RefCell<Option<(gtk::Window, gtk::Box)>> = const { RefCell::new(None) };
}

/// A drag that hands `paths` to whatever they are dropped on.
fn drag_out(widget: &impl IsA<gtk::Widget>, paths: Vec<PathBuf>) {
    let src = gtk::DragSource::new();
    src.set_actions(gdk::DragAction::COPY);
    src.connect_prepare(move |_, _, _| {
        let files: Vec<gio::File> = paths.iter().map(gio::File::for_path).collect();
        Some(gdk::ContentProvider::for_value(
            &gdk::FileList::from_array(&files).to_value(),
        ))
    });
    widget.add_controller(src);
}

fn file_icon(p: &std::path::Path) -> gtk::Image {
    let (ct, _) = gio::content_type_guess(Some(p), None);
    let img = gtk::Image::from_gicon(&gio::content_type_get_icon(&ct));
    img.set_pixel_size(w::ICON_M);
    img
}

fn row(p: &Path, on_remove: Option<std::rc::Rc<dyn Fn(PathBuf)>>) -> gtk::Box {
    let r = w::hbox(w::S2);
    r.add_css_class("drop-shelf-row");
    r.append(&file_icon(p));
    let n = w::label(&name(p), "");
    n.set_xalign(0.0);
    n.set_hexpand(true);
    n.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    n.set_tooltip_text(Some(&p.display().to_string()));
    r.append(&n);
    if let Some(f) = on_remove {
        let p2 = p.to_path_buf();
        let x = w::icon_button("window-close-symbolic", "Remove from Shelf", move || {
            f(p2.clone())
        });
        x.set_valign(gtk::Align::Center);
        r.append(&x);
    }
    drag_out(&r, vec![p.to_path_buf()]);
    r
}

/// The shelf's content for `items`. `live` false: a preview (goldens) with no buttons wired.
pub fn content(items: &[PathBuf], live: bool) -> gtk::Box {
    let g = w::glass_panel(w::S3);
    g.add_css_class("drop-shelf");
    g.set_size_request(WIDTH, -1);
    let head = w::hbox(w::S2);
    let t = w::label("Shelf", "title");
    t.set_xalign(0.0);
    t.set_hexpand(true);
    head.append(&t);
    let all = w::label(&count(items.len()), "dim");
    all.set_valign(gtk::Align::Center);
    all.set_tooltip_text(Some("Drag to take everything"));
    drag_out(&all, items.to_vec());
    head.append(&all);
    g.append(&head);
    let remove: Option<std::rc::Rc<dyn Fn(PathBuf)>> =
        live.then(|| std::rc::Rc::new(|p: PathBuf| change(|s| s.remove(&p).then_some(()))) as _);
    let list = w::vbox(w::S1);
    if items.is_empty() {
        let e = w::footnote("Drop files on the desktop to keep them here.");
        e.set_xalign(0.0);
        list.append(&e);
    }
    for p in items.iter().rev() {
        list.append(&row(p, remove.clone()));
    }
    let (_, natural, _, _) = list.measure(gtk::Orientation::Vertical, WIDTH - 2 * w::S4);
    let scroll = gtk::ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(natural.min(MAX_HEIGHT))
        .max_content_height(MAX_HEIGHT)
        .build();
    g.append(&scroll);
    if live {
        let foot = w::hbox(w::S2);
        foot.set_halign(gtk::Align::End);
        foot.append(&w::button("Clear", w::Kind::Default, || {
            change(|s| {
                s.clear();
                Some(())
            });
            hide();
        }));
        g.append(&foot);
    }
    g
}

fn refresh() {
    let items = SHELF.with_borrow(|s| s.items().to_vec());
    WIN.with_borrow(|w| {
        if let Some((_, holder)) = w {
            while let Some(c) = holder.first_child() {
                holder.remove(&c);
            }
            holder.append(&content(&items, true));
        }
    });
}

fn hide() {
    WIN.with_borrow(|w| {
        if let Some((win, _)) = w {
            win.set_visible(false);
        }
    });
}

/// Applies `f`; when it changed something, the shelf redraws (and hides once empty).
fn change(f: impl FnOnce(&mut Shelf) -> Option<()>) {
    if SHELF.with_borrow_mut(f).is_some() {
        refresh();
        if SHELF.with_borrow(Shelf::is_empty) {
            hide();
        }
    }
}

/// Files dropped on the desktop: onto the shelf, which comes up.
pub fn add(paths: Vec<PathBuf>) {
    if paths.is_empty() {
        return;
    }
    SHELF.with_borrow_mut(|s| s.add(paths));
    refresh();
    WIN.with_borrow(|w| {
        if let Some((win, _)) = w {
            win.present();
        }
    });
}

/// Lets `widget` (the desktop) take files dropped on it.
pub fn accept_drops(widget: &impl IsA<gtk::Widget>) {
    let t = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    t.connect_drop(|_, v, _, _| {
        let Ok(list) = v.get::<gdk::FileList>() else {
            return false;
        };
        add(list.files().iter().filter_map(|f| f.path()).collect());
        true
    });
    widget.add_controller(t);
}

/// Resident panel at the right edge; stays up while you drag from it, Esc hides it.
pub fn build(app: &gtk::Application) -> gtk::Window {
    let holder = w::vbox(0);
    let win = super::panels::panel(
        app,
        "drop-shelf-window",
        "wol-panel-shelf",
        [false, false, false, true],
        &holder,
    );
    WIN.with_borrow_mut(|w| *w = Some((win.clone(), holder)));
    refresh();
    win
}
