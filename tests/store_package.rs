// SPDX-License-Identifier: GPL-2.0-or-later
//! App Store install page for a double-clicked `.wol` (ADR 0011): what it says for signed,
//! unsigned, damaged and already installed files, read through the real `verify`.
#[path = "golden_cases/wol_file.rs"]
mod wol_file;

use shell::ui::store::{
    PackageFile, ROW_REFUSED, ROW_UNSIGNED, ROW_WOLOS, TRUST_ROWS, capability_row, package_sheet,
};
use std::path::Path;

fn read(p: &Path, records: &Path) -> PackageFile {
    PackageFile::read_with(p.to_str().unwrap(), &[wol_file::trusted()], records)
}

#[test]
fn signed_unsigned_and_damaged_files() {
    let d = wol_file::dir("sheet");
    let none = d.join("no-records");
    let signed = wol_file::write(&d, "Shredder.wol", wol_file::SHREDDER, None, true);
    let s = package_sheet(&read(&signed, &none));
    assert_eq!(s.name, "File Shredder");
    assert_eq!(s.summary, "Permanently delete your files");
    assert_eq!(s.detail, "Version 3.1.0 · com.github.ADBeveridge.Raider");
    assert_eq!(s.trust, TRUST_ROWS[ROW_WOLOS]);
    assert_eq!(s.capabilities, [("folder-symbolic", "Your files")]);
    assert_eq!((s.install, s.button), (Some("signed"), "Install"));

    let unsigned = wol_file::write(&d, "Wike.wol", wol_file::WIKE, None, false);
    let s = package_sheet(&read(&unsigned, &none));
    assert_eq!(s.trust, TRUST_ROWS[ROW_UNSIGNED]);
    assert_eq!(s.install, Some("unsigned"));
    assert_eq!(s.capabilities.len(), 2);

    // Changed after signing, or signed by a key this machine does not trust: refused.
    let tampered = read(&wol_file::tampered(&signed), &none);
    let stranger = PackageFile::read_with(signed.to_str().unwrap(), &[[1; 32]], &none);
    for f in [tampered, stranger] {
        let s = package_sheet(&f);
        assert_eq!(s.trust, TRUST_ROWS[ROW_REFUSED]);
        assert_eq!(s.install, None);
        assert!(s.capabilities.is_empty() && s.summary.is_empty());
        assert!(!s.name.is_empty(), "file name stands in for the app name");
    }
}

#[test]
fn installed_versions_say_open_or_update() {
    let d = wol_file::dir("installed");
    let p = wol_file::write(&d, "Shredder.wol", wol_file::SHREDDER, None, true);
    let records = d.join("records");
    // wol-admind keeps one record per app: <records>/<id>/Info.json.
    let app = records.join("com.github.ADBeveridge.Raider");
    std::fs::create_dir_all(&app).unwrap();
    let record = app.join("Info.json");
    std::fs::write(&record, wol_file::SHREDDER).unwrap();
    assert_eq!(package_sheet(&read(&p, &records)).button, "Open");
    std::fs::write(&record, wol_file::SHREDDER.replace("3.1.0", "3.0.2")).unwrap();
    assert_eq!(package_sheet(&read(&p, &records)).button, "Update");
}

#[test]
fn the_files_icon_is_read() {
    let d = wol_file::dir("icon");
    let p = wol_file::write(
        &d,
        "Shredder.wol",
        wol_file::SHREDDER,
        Some(b"\x89PNG icon"),
        true,
    );
    assert_eq!(read(&p, &d).icon.as_deref(), Some(&b"\x89PNG icon"[..]));
}

#[test]
fn every_capability_has_a_row_and_the_copy_is_clean() {
    for (id, label) in shared::package::CAPABILITIES {
        let (icon, l) = capability_row(id).unwrap_or_else(|| panic!("no row for {id}"));
        assert!(icon.ends_with("-symbolic"));
        assert_eq!(l, label);
        clean(&format!("{l}."));
    }
    assert_eq!(capability_row("root"), None);
    for (_, title, sentence) in TRUST_ROWS {
        clean(&format!("{title}."));
        clean(sentence);
    }
    for label in [
        "Checking the app…",
        "Nothing beyond its own windows",
        "What It Uses",
    ] {
        clean(&format!("{label}."));
    }
}

/// Labels are linted as sentences (with a full stop); sentences as they are.
fn clean(s: &str) {
    assert!(
        shell::copy::lint(s).is_empty(),
        "{s}: {:?}",
        shell::copy::lint(s)
    );
}

/// ADR 0046: every certificate verdict has its row; withdrawn and invalid can't be installed.
#[test]
fn each_certificate_verdict_maps_to_its_row() {
    use shell::ui::store::{ROW_VERIFIED, ROW_WITHDRAWN, trust_row};
    use wol_core::woltrust::Trust as T;
    assert_eq!(trust_row(T::Wolos), (TRUST_ROWS[ROW_WOLOS], Some("signed")));
    assert_eq!(
        trust_row(T::Verified),
        (TRUST_ROWS[ROW_VERIFIED], Some("signed"))
    );
    assert_eq!(
        trust_row(T::Community),
        (TRUST_ROWS[ROW_UNSIGNED], Some("unsigned"))
    );
    assert_eq!(trust_row(T::Withdrawn), (TRUST_ROWS[ROW_WITHDRAWN], None));
    assert_eq!(trust_row(T::Invalid), (TRUST_ROWS[ROW_REFUSED], None));
}

/// ADR 0046 origin: the verified row names the maker and the repository.
#[test]
fn the_made_by_line_names_the_person_and_the_repository() {
    let line = shell::copy::trust_made_by("Ana Lima", "ana", Some("github.com/ana/notes"), "1.2");
    assert_eq!(
        line,
        "Made by Ana Lima (ana on GitHub), from github.com/ana/notes, version 1.2."
    );
    clean(&line);
    clean(&shell::copy::trust_made_by("Ana Lima", "ana", None, "1.2"));
}
