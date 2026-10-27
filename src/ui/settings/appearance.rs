//! Appearance: light/dark/auto and accent colour (theme contract).
use super::*;

// ---- Appearance ----

pub(super) fn page() -> gtk::Box {
    let p = w::page("Appearance");
    let (mode, acc) = accent::split(kit::style::active_id().as_deref());
    let state = Rc::new(RefCell::new((mode, acc.to_string())));
    let light = crate::theme::resolve(Some("wol-light"), false);
    let dark = crate::theme::resolve(Some("wol-dark"), true);
    let tiles = w::wrap_row(w::S3, 3);
    let choices: [(Option<Variant>, &str, Vec<w::Palette>); 3] = [
        (Some(Variant::Light), "Light", vec![w::Palette::of(&light)]),
        (Some(Variant::Dark), "Dark", vec![w::Palette::of(&dark)]),
        (
            None,
            "Auto",
            vec![w::Palette::of(&light), w::Palette::of(&dark)],
        ),
    ];
    let mut first: Option<gtk::ToggleButton> = None;
    let apply = |s: &(Option<Variant>, String)| {
        let _ = match accent::theme_id(s.0, &s.1) {
            Some(id) => kit::style::set_active(&id),
            None => std::fs::remove_file(crate::theme::active_path()),
        };
    };
    for (v, title, slices) in choices {
        let t = w::theme_preview(slices, title, v == mode);
        if let Some(f) = &first {
            t.set_group(Some(f));
        } else {
            first = Some(t.clone());
        }
        let st = state.clone();
        t.connect_toggled(move |t| {
            if t.is_active() {
                st.borrow_mut().0 = v;
                apply(&st.borrow());
            }
        });
        tiles.append(&t);
    }
    p.append(&w::section("", &[tiles.clone().upcast()]));
    if let Some(card) = tiles.parent() {
        card.add_css_class("preview-card");
    }

    let swatches = w::wrap_row(w::S2, 8);
    let variant = if mode == Some(Variant::Dark) {
        Variant::Dark
    } else {
        Variant::Light
    };
    let mut first: Option<gtk::ToggleButton> = None;
    for (id, name, ..) in accent::ACCENTS {
        let hex = accent::color(variant, id).unwrap_or_default();
        let c = Color::parse(hex).unwrap_or(Color::WHITE);
        let dot = Color::parse(accent::text_on(hex)).unwrap_or(Color::WHITE);
        let s = w::accent_swatch(c, dot, name, id == acc);
        if let Some(f) = &first {
            s.set_group(Some(f));
        } else {
            first = Some(s.clone());
        }
        let st = state.clone();
        s.connect_toggled(move |s| {
            if s.is_active() {
                st.borrow_mut().1 = id.to_string();
                apply(&st.borrow());
            }
        });
        swatches.append(&s);
    }
    p.append(&w::section("", &[row("Accent color", &swatches)]));
    p
}
