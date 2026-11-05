// SPDX-License-Identifier: GPL-2.0-or-later
//! `wol-shell`: the desktop shell. `wol-shell --supervised` (the session start) runs it under a
//! supervisor that brings it back after a crash (`shell::supervise`).
use shell::supervise;

fn main() -> gtk::glib::ExitCode {
    shell::kit::renderer::settle();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if supervise::wanted(&args, std::env::var_os(supervise::UI_ENV).is_some()) {
        return gtk::glib::ExitCode::from(supervise::main(&args));
    }
    // The session's language (LANG; WOL_LANG=xx for layout tests), ADR 0045.
    shell::i18n::init();
    shell::ui::run_shell()
}
