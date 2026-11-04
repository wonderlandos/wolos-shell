// SPDX-License-Identifier: GPL-2.0-or-later
//! A centred column: one child, at most `max` px wide (`wide` px once the room reaches `wide_from`), the rest of the
//! room left and right of it. Settings panes use it so a row never runs longer than one door-light ruler (576 px of
//! content) on a wide window (brand-screens section 6); the Store widens to 840 for a third tile (section 7).
use gtk::prelude::{IsA, WidgetExt};
use gtk::subclass::prelude::{
    ObjectImpl, ObjectSubclass, ObjectSubclassExt, ObjectSubclassIsExt, WidgetImpl,
};
use gtk::{self, gdk, glib};

/// Width of a Settings pane column including its 24 px page padding: 576 of content.
pub const PANE_COLUMN: i32 = 624;
/// The Store column on a wide window: (room needed, width then): 840 of content once the pane is 888 wide.
pub const STORE_WIDE: (i32, i32) = (888, 888);

mod imp {
    use super::{ObjectImpl, ObjectSubclass, ObjectSubclassExt, WidgetExt, WidgetImpl, gdk, glib};
    use std::cell::Cell;

    #[derive(Default)]
    pub struct Column {
        pub max: Cell<i32>,
        /// (room needed, width then); (0, 0) for none.
        pub wide: Cell<(i32, i32)>,
    }

    impl Column {
        /// The width the column takes in `room` px.
        pub fn width_in(&self, room: i32) -> i32 {
            let (from, to) = self.wide.get();
            let max = if from > 0 && room >= from {
                to
            } else {
                self.max.get()
            };
            room.min(max)
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Column {
        const NAME: &'static str = "WolColumn";
        type Type = super::Column;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Column {
        fn dispose(&self) {
            while let Some(c) = self.obj().first_child() {
                c.unparent();
            }
        }
    }

    impl WidgetImpl for Column {
        // Wrapped text makes the height depend on the width: GTK asks for the height at the width it gives.
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let Some(child) = self.obj().first_child() else {
                return (0, 0, -1, -1);
            };
            let max = self.max.get().max(self.wide.get().1);
            if orientation == gtk::Orientation::Horizontal {
                let (min, nat, _, _) = child.measure(orientation, -1);
                (min, nat.min(max).max(min), -1, -1)
            } else {
                let w = if for_size < 0 {
                    max
                } else {
                    self.width_in(for_size)
                };
                let (min, nat, _, _) = child.measure(orientation, w);
                (min, nat, -1, -1)
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            let Some(child) = self.obj().first_child() else {
                return;
            };
            let w = self.width_in(width);
            child.size_allocate(
                &gdk::Rectangle::new((width - w) / 2, 0, w, height),
                baseline,
            );
        }
    }
}

glib::wrapper! {
    pub struct Column(ObjectSubclass<imp::Column>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

/// `child` in a column at most `max` px wide, centred in the room it gets; `wide` = (room needed, width then) lets a
/// wide window take a wider column.
pub fn column(child: &impl IsA<gtk::Widget>, max: i32, wide: (i32, i32)) -> Column {
    let c: Column = glib::Object::new();
    c.imp().max.set(max);
    c.imp().wide.set(wide);
    c.set_hexpand(true);
    c.set_vexpand(child.vexpands());
    child.set_parent(&c);
    c
}
