// SPDX-License-Identifier: GPL-2.0-or-later
//! Processes: every process as a tree (children under their parent, busiest first, foldable),
//! or the matches of the toolbar filter as a list; End Process asks first. A list view: only
//! the rows on screen are widgets, however many processes there are.
use super::model::{self, Model, TreeRow};
use crate::copy::monitor as copy;
use crate::kit::widgets as w;
use gtk::{gio, glib, prelude::*};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Width of the number columns, in characters.
const PID_CHARS: i32 = 7;
const CPU_CHARS: i32 = 5;
const MEM_CHARS: i32 = 8;

/// Fold or unfold one process (the model keeps the set).
pub type Fold = Rc<dyn Fn(i32)>;

pub struct Page {
    pub root: gtk::Box,
    store: gio::ListStore,
    selection: gtk::SingleSelection,
    list: gtk::Box,
    empty: gtk::Box,
    /// The selected process, kept by pid across the second-by-second refresh.
    chosen: Cell<Option<i32>>,
    refreshing: Cell<bool>,
    rows: RefCell<Vec<TreeRow>>,
    /// Runs with the selection after a change (the toolbar's End Process follows it).
    pub on_select: RefCell<Option<Box<dyn Fn(Option<&TreeRow>)>>>,
}

fn number(chars: i32) -> gtk::Label {
    let l = w::label("", "secondary");
    l.set_width_chars(chars);
    l.set_xalign(1.0);
    l
}

/// One row's widgets, made once per visible row and rebound as it scrolls.
fn setup(fold: Fold) -> impl Fn(&gtk::ListItem) {
    move |item: &gtk::ListItem| {
        let r = w::hbox(w::S2);
        r.add_css_class("process-row");
        let fold = fold.clone();
        let twist = w::icon_button("pan-end-symbolic", copy::FOLD, || {});
        twist.add_css_class("process-twist");
        {
            let item = item.downgrade();
            twist.connect_clicked(move |_| {
                let pid = item
                    .upgrade()
                    .and_then(|i| i.item())
                    .and_downcast::<glib::BoxedAnyObject>()
                    .map(|o| o.borrow::<TreeRow>().proc.pid);
                if let Some(p) = pid {
                    fold(p);
                }
            });
        }
        r.append(&twist);
        let name = w::label("", "");
        name.set_hexpand(true);
        name.set_xalign(0.0);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        name.add_css_class("may-ellipsize");
        name.set_max_width_chars(1);
        r.append(&name);
        for chars in [PID_CHARS, CPU_CHARS, MEM_CHARS] {
            r.append(&number(chars));
        }
        item.set_child(Some(&r));
    }
}

fn bind(item: &gtk::ListItem) {
    let (Some(r), Some(o)) = (
        item.child().and_downcast::<gtk::Box>(),
        item.item().and_downcast::<glib::BoxedAnyObject>(),
    ) else {
        return;
    };
    let row = o.borrow::<TreeRow>();
    let mut c = r.first_child();
    let mut i = 0;
    while let Some(ch) = c {
        if let Some(b) = ch.downcast_ref::<gtk::Button>() {
            // The fold button keeps its place on a leaf (the names stay in line), unseen.
            b.set_opacity(if row.children > 0 { 1.0 } else { 0.0 });
            b.set_sensitive(row.children > 0);
            b.set_margin_start(w::S4 * i32::try_from(row.depth).unwrap_or(0));
            if let Some(img) = b.child().and_downcast::<gtk::Image>() {
                img.set_icon_name(Some(if row.folded {
                    "pan-end-symbolic"
                } else {
                    "pan-down-symbolic"
                }));
            }
        } else if let Some(l) = ch.downcast_ref::<gtk::Label>() {
            let p = &row.proc;
            l.set_text(&match i {
                1 => p.name.clone(),
                2 => p.pid.to_string(),
                3 => model::percent(f64::from(p.cpu)),
                _ => model::bytes(p.rss),
            });
        }
        i += 1;
        c = ch.next_sibling();
    }
}

impl Page {
    pub fn new(fold: Fold) -> Rc<Page> {
        let root = w::page(copy::PROCESSES);
        let frame = w::vbox(0);
        frame.add_css_class("process-frame");
        frame.set_vexpand(true);
        let head = w::hbox(w::S2);
        head.add_css_class("process-row");
        let name = w::label(copy::NAME, "secondary");
        name.set_hexpand(true);
        name.set_xalign(0.0);
        head.append(&name);
        for (t, chars) in [
            (copy::PID, PID_CHARS),
            (copy::CPU_SHORT, CPU_CHARS),
            (copy::MEMORY, MEM_CHARS),
        ] {
            let l = number(chars);
            l.set_text(t);
            head.append(&l);
        }
        frame.append(&head);
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let selection = gtk::SingleSelection::new(Some(store.clone()));
        selection.set_autoselect(false);
        selection.set_can_unselect(true);
        let factory = gtk::SignalListItemFactory::new();
        let setup = setup(fold);
        factory.connect_setup(move |_, o| {
            if let Some(i) = o.downcast_ref::<gtk::ListItem>() {
                setup(i);
            }
        });
        factory.connect_bind(|_, o| {
            if let Some(i) = o.downcast_ref::<gtk::ListItem>() {
                bind(i);
            }
        });
        let view = gtk::ListView::new(Some(selection.clone()), Some(factory));
        view.add_css_class("process-list");
        let list = gtk::ScrolledWindow::builder()
            .child(&view)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .min_content_height(crate::kit::graph::LIST_MIN)
            .build();
        frame.append(&list);
        root.append(&frame);
        let empty = w::empty_state(
            "edit-find-symbolic",
            copy::NO_MATCH.0,
            copy::NO_MATCH.1,
            None,
        );
        empty.set_visible(false);
        root.append(&empty);
        let p = Rc::new(Page {
            root,
            store,
            selection,
            list: frame,
            empty,
            chosen: Cell::new(None),
            refreshing: Cell::new(false),
            rows: RefCell::default(),
            on_select: RefCell::default(),
        });
        {
            let me = Rc::downgrade(&p);
            p.selection.connect_selected_notify(move |s| {
                let Some(p) = me.upgrade() else { return };
                if p.refreshing.get() {
                    return;
                }
                let i = s.selected();
                let rows = p.rows.borrow();
                let row = usize::try_from(i).ok().and_then(|i| rows.get(i));
                p.chosen.set(row.map(|r| r.proc.pid));
                if let Some(f) = p.on_select.borrow().as_ref() {
                    f(row);
                }
            });
        }
        p
    }

    pub fn selected(&self) -> Option<TreeRow> {
        let pid = self.chosen.get()?;
        self.rows
            .borrow()
            .iter()
            .find(|r| r.proc.pid == pid)
            .cloned()
    }

    pub fn update(&self, m: &Model) {
        let rows = m.proc_rows();
        let objs: Vec<glib::BoxedAnyObject> = rows
            .iter()
            .cloned()
            .map(glib::BoxedAnyObject::new)
            .collect();
        self.refreshing.set(true);
        self.store.splice(0, self.store.n_items(), &objs);
        let at = self
            .chosen
            .get()
            .and_then(|pid| rows.iter().position(|r| r.proc.pid == pid))
            .and_then(|i| u32::try_from(i).ok());
        self.selection
            .set_selected(at.unwrap_or(gtk::INVALID_LIST_POSITION));
        self.refreshing.set(false);
        if at.is_none()
            && self.chosen.take().is_some()
            && let Some(f) = self.on_select.borrow().as_ref()
        {
            f(None);
        }
        let none = rows.is_empty() && !m.filter.trim().is_empty();
        *self.rows.borrow_mut() = rows;
        self.list.set_visible(!none);
        self.empty.set_visible(none);
    }
}
