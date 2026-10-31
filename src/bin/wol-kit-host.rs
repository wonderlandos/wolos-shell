// SPDX-License-Identifier: GPL-2.0-or-later
//! One app's Kit host (ADR 0020): started by wol-shell with the app's connection as stdin.
fn main() -> gtk::glib::ExitCode {
    shell::ui::app_host::run_host()
}
