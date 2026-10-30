//! Every Kit button looks pressed while held, shows a focus ring from the keyboard and looks
//! disabled when it can't act (kit.css).
const CSS: &str = include_str!("../src/kit/kit.css");

fn hover_classes() -> Vec<String> {
    let mut v: Vec<String> = CSS
        .match_indices(":hover")
        .filter_map(|(i, _)| {
            let head = &CSS[..i];
            let start = head.rfind("button.")?;
            let cls = &head[start + "button.".len()..];
            (!cls.is_empty() && cls.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
                .then(|| cls.to_string())
        })
        .collect();
    v.sort();
    v.dedup();
    v
}

#[test]
fn hovered_buttons_also_look_pressed() {
    let classes = hover_classes();
    assert!(classes.len() > 10, "{classes:?}");
    let missing: Vec<_> = classes
        .iter()
        .filter(|c| !CSS.contains(&format!("button.{c}:active")))
        .collect();
    assert!(missing.is_empty(), "no pressed look: {missing:?}");
}

#[test]
fn push_buttons_hover_press_and_disable() {
    for s in ["button.btn:hover", "button.btn:active", "button.btn:disabled"] {
        assert!(CSS.contains(s), "{s}");
    }
}

#[test]
fn every_button_has_a_keyboard_focus_ring() {
    assert!(
        CSS.contains("\nbutton:focus-visible {"),
        "a global button focus ring"
    );
}
