//! Every password field (Setup, login window, lock screen, admin sheet, Control Center) has the
//! eye button that shows what was typed: a hidden typo otherwise reads as "Incorrect password".
use std::path::Path;

fn sources(dir: &Path, out: &mut Vec<(String, String)>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push((
                p.display().to_string(),
                std::fs::read_to_string(&p).unwrap(),
            ));
        }
    }
}

#[test]
fn every_password_field_can_show_the_password() {
    let mut files = vec![];
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    let mut fields = 0;
    for (path, src) in &files {
        assert!(
            !src.contains("PasswordEntry::new()"),
            "{path}: use the builder with show_peek_icon"
        );
        for part in src.split("PasswordEntry::builder()").skip(1) {
            fields += 1;
            let chain = &part[..part.find(".build()").unwrap()];
            assert!(
                chain.contains(".show_peek_icon(true)"),
                "{path}: password field without the eye button"
            );
        }
    }
    assert!(fields >= 2, "found {fields} password fields");
}
