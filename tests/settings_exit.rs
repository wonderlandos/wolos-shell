// SPDX-License-Identifier: GPL-2.0-or-later
//! QA-LIVE3-5 / CRASH-3: closing System Settings aborted with a core dump every time: the window
//! and the Undo toast's hook sat in thread-locals that the thread-local destructors dropped after
//! GTK's own state was gone ("cannot access a Thread Local Storage value during or after
//! destruction"). `run()` takes both on the GTK thread once the main loop returns, before the
//! process exits. The live half is the journey `qa100-crash-settings-quit-abort`.

/// The body of `fn run()` in ui/settings/mod.rs.
fn run_body() -> String {
    let src = include_str!("../src/ui/settings/mod.rs");
    let at = src
        .find("pub fn run() -> glib::ExitCode {")
        .expect("settings run()");
    let rest = &src[at..];
    let end = rest.find("\n}\n").expect("end of run()");
    rest[..end].to_string()
}

#[test]
fn close_takes_thread_locals() {
    let body = run_body();
    let ran = body.find("app.run()").expect("run() runs the app");
    for take in [
        "WINDOW.with_borrow_mut(|w| w.take());",
        "CHANGED.with(|c| c.borrow_mut().take());",
    ] {
        let at = body
            .find(take)
            .unwrap_or_else(|| panic!("run() never does {take}"));
        assert!(at > ran, "{take} must come after the main loop returns");
    }
    // Both thread-locals hold GTK objects, so both are the ones taken.
    let src = include_str!("../src/ui/settings/mod.rs");
    assert!(src.contains("static WINDOW: RefCell<Option<Rc<w::AppWindow>>>"));
    assert!(src.contains("static CHANGED:"));
}
