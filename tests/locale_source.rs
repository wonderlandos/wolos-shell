// SPDX-License-Identifier: GPL-2.0-or-later
//! The shell's text unit (ADR 0045): `locale/shell/en.tsv` is what `wol-i18n extract` makes of the
//! copy modules, every translation file compiles, and the pseudo-locale lengthens every string.
use shared::i18n::{Lang, compile, files};
use std::path::Path;

fn unit() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("locale/shell")
}

#[test]
fn en_tsv_is_what_the_copy_modules_say() {
    let d = unit();
    let want = files::extracted_text(&d, &files::extract_dir(&d).expect("extract"));
    let have = std::fs::read_to_string(d.join("en.tsv")).expect("en.tsv");
    assert!(
        want == have,
        "locale/shell/en.tsv is out of date: run `cargo run -p shared --bin wol-i18n -- extract crates/shell/locale/shell`"
    );
}

#[test]
fn every_language_file_compiles_and_the_pseudo_locale_is_longer() {
    let d = unit();
    let tables = files::compile_dir(&d).expect("compile");
    assert!(tables.iter().any(|t| t.0 == Lang::En));
    let unit = files::load_unit(&d).expect("unit");
    for row in unit.rows() {
        let xx = compile::pseudo(&row.source);
        assert!(
            compile::width(&xx) > compile::width(&row.source),
            "{}",
            row.key
        );
    }
}

#[test]
fn every_sentence_in_the_module_is_a_key_of_the_unit() {
    let d = unit();
    let names = files::load_unit(&d).expect("unit").names();
    assert!(names.contains(&"settings.sound_louder".to_string()));
    assert!(names.len() >= 34, "{} keys", names.len());
}
