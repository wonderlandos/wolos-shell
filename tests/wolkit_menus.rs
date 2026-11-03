// SPDX-License-Identifier: GPL-2.0-or-later
//! The bar and a Wolkit app agree on the menus' object: the bar calls `Get` and `Activate` on the
//! interface and path the app publishes (`crates/wolkit/src/window/native/menus.rs`), follows its
//! `Changed` signal, never starts a service for it, and a glass window is the app's window.

const BAR: &str = include_str!("../src/ui/menubar.rs");
const APPMENU: &str = include_str!("../src/appmenu.rs");
const HYPR: &str = include_str!("../src/hypr.rs");
const DOCK: &str = include_str!("../src/ui/dock.rs");
const APP: &str = include_str!("../../wolkit/src/window/native/menus.rs");

#[test]
fn the_bar_speaks_the_apps_interface() {
    assert!(APPMENU.contains("pub const WOLKIT_MENUS: &str = \"org.wolos.Menus\";"));
    assert!(APP.contains("#[zbus::interface(name = \"org.wolos.Menus\")]"));
    for m in ["\"Get\"", "\"Activate\"", "\"Changed\""] {
        assert!(BAR.contains(m), "the bar never uses {m}");
    }
    for f in ["fn get(", "fn activate(", "async fn changed("] {
        assert!(APP.contains(f), "the app does not publish {f}");
    }
    assert!(
        BAR.matches("gio::DBusCallFlags::NO_AUTO_START").count() >= 2,
        "asking an app for its menus must never start it"
    );
}

#[test]
fn the_apps_menus_sit_before_window() {
    assert!(BAR.contains("BarItem::AppMenus => self.app_menus.clone().upcast()"));
    assert!(BAR.contains("app_menus.append(&window_menu)"));
    assert!(BAR.contains("self.host.insert_child_after(&b, last.as_ref())"));
}

#[test]
fn a_glass_window_is_its_apps_window_everywhere_the_class_is_read() {
    assert_eq!(HYPR.matches("crate::appmenu::app_id(").count(), 4);
    assert!(
        DOCK.contains("(\\\\.glass)?$"),
        "the Dock's focus target accepts the suffix"
    );
}
