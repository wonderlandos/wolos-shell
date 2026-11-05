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
    assert!(names.contains(&"installer.choose_disk".to_string()));
    assert!(names.contains(&"setup.choose_a_password".to_string()));
    assert!(names.len() >= 300, "{} keys", names.len());
}

/// L10N-2: the seven official languages ship whole: every key of en.tsv is translated in each,
/// none stale, none with an error (`wol-i18n status` at 100 %).
#[test]
fn every_official_language_is_complete() {
    let d = unit();
    let unit = files::load_unit(&d).expect("unit");
    for l in Lang::SHIPPED.into_iter().skip(1) {
        let rows = files::load_lang(&d, l).expect("rows");
        let c = compile::compile(&unit, l, &rows).expect("compiles");
        assert!(c.ok(), "{l:?}: {:?}", c.problems);
        assert_eq!(
            (c.translated, c.stale),
            (c.rows, 0),
            "{}: {} of {} translated, {} stale",
            l.tag(),
            c.translated,
            c.rows,
            c.stale
        );
    }
}

/// QA-A7-10 / QA-A7-1: the language is English until a process calls `i18n::init()`, so each
/// binary that shows text in the person's language starts it before its UI: the lock screen, the
/// login window (their `tr` calls read English on every computer), and the first sign-in Setup,
/// which is `wol-setup --first-login` (`ui::first_login::run`).
#[test]
fn every_process_that_shows_catalog_text_chooses_its_language() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for (file, ui_call) in [
        ("bin/wol-lock.rs", "run_lock("),
        ("bin/wol-loginwindow.rs", "run_login("),
        ("bin/wol-shell.rs", "run_shell("),
        ("bin/wol-settings.rs", "settings::run("),
    ] {
        let text = std::fs::read_to_string(src.join(file)).expect(file);
        let init = text.find("i18n::init()").unwrap_or(usize::MAX);
        let ui = text.find(ui_call).expect(ui_call);
        assert!(init < ui, "{file}: i18n::init() before {ui_call}");
    }
    let text = std::fs::read_to_string(src.join("ui/first_login.rs")).expect("first_login");
    let run = text.find("pub fn run(again: bool)").expect("run");
    let (init, window) = (
        text[run..].find("i18n::init()"),
        text[run..].find("a::run("),
    );
    assert!(
        init.is_some() && init < window,
        "ui/first_login.rs: run() chooses the language before the window"
    );
}
