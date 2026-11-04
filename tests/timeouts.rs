// SPDX-License-Identifier: GPL-2.0-or-later
//! Settings > Accessibility > Give me more time (QA-ACC-6): the choice is a file every shell
//! process follows, and every timer that a person has to beat asks for it when it starts.
use shared::patience::Patience;
use std::path::Path;

#[test]
fn the_choice_is_saved_read_back_and_removed_for_normal() {
    let d = std::env::temp_dir().join(format!("wol-timeouts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    unsafe { std::env::set_var("XDG_CONFIG_HOME", &d) };
    let file = d.join("wol/timeouts");
    assert_eq!(
        shell::kit::patience::current(),
        Patience::Normal,
        "no file: normal"
    );
    for (p, text) in [
        (Patience::Five, "5\n"),
        (Patience::Never, "never\n"),
        (Patience::Double, "2\n"),
    ] {
        shell::kit::patience::save(p).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
        assert_eq!(shell::kit::patience::current(), p);
        // `wol` and wold read the same file the same way.
        assert_eq!(Patience::read(&d), p);
    }
    shell::kit::patience::save(Patience::Normal).unwrap();
    assert!(!file.exists(), "normal is no file");
    shell::kit::patience::save(Patience::Normal).unwrap();
    assert_eq!(shell::kit::patience::current(), Patience::Normal);
    let _ = std::fs::remove_dir_all(&d);
}

/// Every wait the shell puts on a person goes through `kit::patience`. A new bare timer here
/// would be a sheet or a banner that "Give me more time" does not reach.
#[test]
fn timed_sheets_banners_and_countdowns_ask_for_the_setting() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for (file, bare) in [
        (
            "ui/approval.rs",
            "timeout_add_seconds_local_once(APPROVAL_TIMEOUT_SECS",
        ),
        (
            "kit/toast.rs",
            "timeout_add_local_once(std::time::Duration::from_millis(TOAST_MS)",
        ),
        ("ui/capture.rs", "timeout_add_seconds_local_once(THUMB_SECS"),
        ("ui/notify.rs", "timeout_add_local_once(offers::CLEAR_AFTER"),
        ("displays.rs", "left: REVERT_SECS"),
    ] {
        let text = std::fs::read_to_string(src.join(file)).unwrap();
        let code = text.split("#[cfg(test)]").next().unwrap();
        assert!(
            !code.contains(bare),
            "{file}: `{bare}` is a timer Give me more time does not reach"
        );
        assert!(
            code.contains("patience"),
            "{file} does not ask kit::patience"
        );
    }
    // The banner's timer is armed through banner_wait.
    let n = std::fs::read_to_string(src.join("ui/notify.rs")).unwrap();
    assert!(n.contains("banner_wait(ms, crate::kit::patience::current())"));
}
