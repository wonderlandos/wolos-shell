// SPDX-License-Identifier: GPL-2.0-or-later
//! The goldens must be made in the type the product ships. kit.css names Inter first; an image
//! without it silently falls back to Cantarell and every golden locks the wrong glyphs and widths
//! (audit A6-ux, 0.1). Call `ui_family_is_inter()` once the kit stylesheet is applied.
use gtk::prelude::*;

/// The family Pango really resolves for the kit's UI text (body, bold title, secondary).
pub fn resolved_families() -> Vec<String> {
    // The family is set on `window` in kit.css and inherited: the label must sit in one.
    let win = gtk::Window::new();
    let label = gtk::Label::new(Some("Hamburgefonstiv 0123"));
    win.set_child(Some(&label));
    let ctx = label.pango_context();
    let desc = ctx.font_description().expect("the label has a font");
    let font = ctx.load_font(&desc).expect("Pango resolves the UI font");
    vec![
        font.describe()
            .family()
            .map(|f| f.to_string())
            .unwrap_or_default(),
    ]
}

/// Panics, naming the fallback, unless the UI text resolves to Inter.
pub fn ui_family_is_inter() {
    let got = resolved_families();
    assert!(
        got.iter().all(|f| f == "Inter"),
        "the golden image must resolve the UI font to Inter (add inter-font to tests/golden-env/Dockerfile); got {got:?}"
    );
}
