//! Shell surfaces per display (plan in `crate::displays`): a wallpaper and a menu bar on every
//! display, the Dock and the desktop widgets on the main one. Follows the display list live, so
//! plugging a display in dresses it and unplugging one moves the Dock and the desktop over.
use crate::displays::{self as plan, Connector};
use crate::kit::surface;
use gtk::{gdk, prelude::*};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// What one display holds besides the main-display surfaces.
struct Dressed {
    wallpaper: gtk::ApplicationWindow,
    /// None on the main display: the primary bar is there.
    bar: Option<gtk::ApplicationWindow>,
}

pub struct Displays {
    app: gtk::Application,
    sys: Rc<dyn crate::system::System>,
    /// On the main display; move with it.
    primary_bar: gtk::ApplicationWindow,
    primary: Rc<super::menubar::Bar>,
    main_only: Vec<gtk::Window>,
    shown: RefCell<Vec<Connector>>,
    /// Runs on every wallpaper map (startup timing marks the first).
    on_wallpaper: Rc<dyn Fn()>,
    dressed: RefCell<HashMap<Connector, Dressed>>,
}

/// The displays GTK knows now, in Hyprland's order, by connector.
pub fn monitors() -> Vec<(Connector, gdk::Monitor)> {
    let Some(d) = gdk::Display::default() else {
        return vec![];
    };
    let list = d.monitors();
    (0..list.n_items())
        .filter_map(|i| list.item(i).and_downcast::<gdk::Monitor>())
        .filter_map(|m| Some((m.connector()?.to_string(), m)))
        .collect()
}

impl Displays {
    /// Starts following the display list. `primary_bar`/`main_only` were built already (first
    /// frame); they are pinned to the main display here.
    pub fn start(
        app: &gtk::Application,
        sys: Rc<dyn crate::system::System>,
        primary_bar: gtk::ApplicationWindow,
        primary: Rc<super::menubar::Bar>,
        main_only: Vec<gtk::Window>,
        on_wallpaper: Box<dyn Fn()>,
    ) -> Rc<Displays> {
        let me = Rc::new(Displays {
            app: app.clone(),
            sys,
            primary_bar,
            primary,
            main_only,
            shown: Default::default(),
            on_wallpaper: Rc::from(on_wallpaper),
            dressed: Default::default(),
        });
        if let Some(d) = gdk::Display::default() {
            let weak = Rc::downgrade(&me);
            d.monitors().connect_items_changed(move |_, _, _, _| {
                if let Some(me) = weak.upgrade() {
                    me.sync();
                }
            });
        }
        me.sync();
        me
    }

    fn sync(&self) {
        let now = monitors();
        let names: Vec<Connector> = now.iter().map(|m| m.0.clone()).collect();
        let change = plan::change(&self.shown.borrow(), &names);
        let find = |c: &str| now.iter().find(|m| m.0 == c).map(|m| m.1.clone());
        for c in &change.removed {
            if let Some(d) = self.dressed.borrow_mut().remove(c) {
                d.wallpaper.destroy();
                if let Some(b) = d.bar {
                    b.destroy();
                }
            }
        }
        let main = plan::main_of(&names).cloned();
        if let Some(m) = &change.main {
            let mon = find(m);
            // The new main display had its own bar: the primary bar takes its place.
            if let Some(b) = self
                .dressed
                .borrow_mut()
                .get_mut(m)
                .and_then(|d| d.bar.take())
            {
                b.destroy();
            }
            surface::set_monitor(&self.primary_bar, mon.as_ref());
            self.primary.display.replace(m.clone());
            for w in
                std::iter::once(self.primary_bar.upcast_ref::<gtk::Window>()).chain(&self.main_only)
            {
                surface::set_monitor(w, mon.as_ref());
                if w.is_visible() || !surface::layered() {
                    w.present();
                }
            }
        }
        for c in &change.added {
            let Some(mon) = find(c) else { continue };
            let wallpaper = super::panels::wallpaper(&self.app, &mon);
            let f = self.on_wallpaper.clone();
            wallpaper.connect_map(move |_| f());
            wallpaper.present();
            let bar = (Some(c) != main.as_ref()).then(|| {
                let (w, _) = super::menubar::build(&self.app, &*self.sys, Some(&mon), false);
                w.present();
                w
            });
            self.dressed
                .borrow_mut()
                .insert(c.clone(), Dressed { wallpaper, bar });
        }
        // A display that stopped being the main one needs its own bar now.
        for (c, d) in self.dressed.borrow_mut().iter_mut() {
            if Some(c) != main.as_ref()
                && d.bar.is_none()
                && let Some(mon) = find(c)
            {
                let (w, _) = super::menubar::build(&self.app, &*self.sys, Some(&mon), false);
                w.present();
                d.bar = Some(w);
            }
        }
        self.shown.replace(names);
    }
}
