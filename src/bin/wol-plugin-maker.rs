// SPDX-License-Identifier: GPL-2.0-or-later
//! `wol-plugin-maker [folder | --theme [theme.json]]`: Plugin Maker / Theme Studio (ADR 0031,
//! docs/modules/plugin-maker.md).
fn main() -> gtk::glib::ExitCode {
    shell::ui::maker::run()
}
