//! Layout check over live GTK allocations (rules: `crate::layout`). `WOL_LAYOUT_CHECK=exit` exits 1 on issues.
use crate::layout::{self, Node, Rect};
use gtk::{glib, prelude::*};

fn name(w: &gtk::Widget) -> String {
    let mut s = w.type_().name().trim_start_matches("Gtk").to_string();
    for c in w.css_classes() {
        s.push('.');
        s.push_str(&c);
    }
    if let Some(l) = w.downcast_ref::<gtk::Label>() {
        s.push_str(&format!(" {:?}", l.text().as_str()));
    }
    s
}

/// Measured tree rooted at `w`; coordinates relative to `root`.
pub fn tree(w: &gtk::Widget, root: &gtk::Widget, parent: &str) -> Node {
    let path = if parent.is_empty() {
        name(w)
    } else {
        format!("{parent} › {}", name(w))
    };
    let rect = w
        .compute_bounds(root)
        .map(|b| Rect {
            x: b.x().round() as i32,
            y: b.y().round() as i32,
            w: b.width().round() as i32,
            h: b.height().round() as i32,
        })
        .unwrap_or_default();
    let row = w
        .downcast_ref::<gtk::Box>()
        .is_some_and(|b| b.orientation() == gtk::Orientation::Horizontal)
        && !w.has_css_class("no-row-check");
    let clips = w.is::<gtk::ScrolledWindow>()
        || w.is::<gtk::Viewport>()
        || w.is::<gtk::Revealer>()
        || w.is::<gtk::Stack>()
        || w.is::<gtk::Popover>();
    let clipped_text = w
        .downcast_ref::<gtk::Label>()
        .is_some_and(|l| l.layout().is_ellipsized() && !l.has_css_class("may-ellipsize"));
    let mut children = Vec::new();
    let mut c = w.first_child();
    while let Some(ch) = c {
        if ch.is_visible() && ch.is_child_visible() && !ch.is::<gtk::Popover>() && ch.width() > 0 {
            children.push(tree(&ch, root, &path));
        }
        c = ch.next_sibling();
    }
    Node {
        path,
        rect,
        row,
        clips,
        clipped_text,
        children,
    }
}

pub fn issues(win: &gtk::Window) -> Vec<layout::Issue> {
    let root: &gtk::Widget = win.upcast_ref();
    let Some(child) = win.child() else {
        return Vec::new();
    };
    let frame = Rect {
        x: 0,
        y: 0,
        w: win.width(),
        h: win.height(),
    };
    layout::find_in(frame, &tree(&child, root, ""))
}

/// After the first layout: log issues; exit 1 when `WOL_LAYOUT_CHECK=exit`.
pub fn on_map(win: &gtk::Window, name: &'static str) {
    win.connect_map(move |w| {
        let w = w.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
            let found = issues(&w);
            for i in &found {
                eprintln!("wol layout: {name}: {}", i.message);
            }
            if !found.is_empty() && std::env::var("WOL_LAYOUT_CHECK").as_deref() == Ok("exit") {
                std::process::exit(1);
            }
        });
    });
}
