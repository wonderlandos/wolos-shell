//! Settings switches that save a preference wait for the save and flip back, with the reason under
//! the pane, when it fails (`form::pending_toggle_row`). A plain `form::toggle_row` there would show
//! a change that never happened.
use std::path::Path;

#[test]
fn settings_preference_switches_revert_on_failure() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui/settings");
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_none_or(|x| x != "rs") {
            continue;
        }
        let src = std::fs::read_to_string(&p).unwrap();
        let code = src.split("#[cfg(test)]").next().unwrap();
        assert!(
            !code.contains("form::toggle_row("),
            "{}: use form::pending_toggle_row for a switch that saves",
            p.display()
        );
        // A bare switch whose handler writes a file can't show the write failed.
        for part in code.split("w::toggle(").skip(1) {
            let body = &part[..part.find("});").unwrap_or(part.len())];
            for save in ["save_shell(", "focus::write(", "change("] {
                assert!(
                    !body.contains(save),
                    "{}: a switch that calls {save} must be w::pending_toggle",
                    p.display()
                );
            }
        }
    }
}
