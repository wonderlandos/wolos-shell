//! Keyboard: input source and key repeat (Hyprland fragment).
use super::*;

pub(super) fn page() -> gtk::Box {
    let p = w::page("Keyboard");
    let file = config(prefs::HYPR_DIR).join("keyboard.conf");
    let (layout, repeat) = prefs::parse_keyboard(read(&file).as_deref());
    let cur = Rc::new(std::cell::Cell::new((layout, repeat)));
    let save = {
        let cur = cur.clone();
        move || {
            let (l, r) = cur.get();
            if let Some(t) = prefs::keyboard_conf(prefs::LAYOUTS[l].0, r) {
                save_hypr("keyboard.conf", &t);
            }
        }
    };
    let save = Rc::new(save);
    let labels: Vec<&str> = prefs::LAYOUTS.iter().map(|l| l.1).collect();
    let repeats: Vec<&str> = prefs::REPEATS.iter().map(|r| r.0).collect();
    let (c1, s1, c2, s2) = (cur.clone(), save.clone(), cur.clone(), save.clone());
    p.append(&w::section(
        "",
        &[
            row(
                "Input source",
                &dropdown(&labels, layout, move |i| {
                    c1.set((i, c1.get().1));
                    s1();
                }),
            ),
            row(
                "Key repeat rate",
                &dropdown(&repeats, repeat, move |i| {
                    c2.set((c2.get().0, i));
                    s2();
                }),
            ),
        ],
    ));
    let field = w::text_field("Type here to test");
    field.set_hexpand(true);
    p.append(&w::section("", &[row("Try it", &field)]));
    p
}
