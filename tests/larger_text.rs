// SPDX-License-Identifier: GPL-2.0-or-later
//! Settings > Accessibility > Larger Text is the desktop's text scale (gsettings
//! `org.gnome.desktop.interface text-scaling-factor`, QA-ACC-5): on sets 1.25, off sets 1, and the
//! switch reads the state back from the same key, so a scale set by another tool shows as on.
//! Runs on an in-memory gsettings backend; skipped (loudly) where the schema is not installed.

use gtk::gio::prelude::*;

#[test]
fn the_switch_sets_and_reads_the_text_scaling_factor() {
    // Before anything touches gio: no dconf, no session bus needed.
    unsafe { std::env::set_var("GSETTINGS_BACKEND", "memory") };
    let schema = gtk::gio::SettingsSchemaSource::default()
        .and_then(|s| s.lookup("org.gnome.desktop.interface", true));
    let Some(schema) = schema else {
        panic!("gsettings-desktop-schemas is not installed: the golden image carries it");
    };
    assert!(schema.has_key("text-scaling-factor"));
    use shell::kit::style::{LARGER_TEXT, larger_text_on, set_larger_text, text_scale};
    assert_eq!(text_scale(), 1.0, "normal until asked");
    assert!(!larger_text_on());
    assert!(set_larger_text(true).is_ok());
    assert_eq!(text_scale(), LARGER_TEXT);
    assert_eq!(LARGER_TEXT, 1.25);
    assert!(larger_text_on());
    // The key itself, as GTK and Qt apps read it.
    let s = gtk::gio::Settings::new("org.gnome.desktop.interface");
    assert_eq!(s.double("text-scaling-factor"), 1.25);
    assert!(set_larger_text(false).is_ok());
    assert_eq!(s.double("text-scaling-factor"), 1.0);
    assert!(!larger_text_on());
    // Another tool set 1.5: the switch shows on, and turning it off puts 1 back.
    s.set_double("text-scaling-factor", 1.5).unwrap();
    assert_eq!(text_scale(), 1.5);
    assert!(larger_text_on());
    // Smaller than normal is not offered: the shell never shrinks below the kit's sizes.
    s.set_double("text-scaling-factor", 0.5).unwrap();
    assert_eq!(text_scale(), 1.0);
}
