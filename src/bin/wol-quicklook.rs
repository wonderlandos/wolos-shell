// SPDX-License-Identifier: GPL-2.0-or-later
//! `wol-quicklook`: the previewer Files calls for Space (docs/modules/quicklook.md), the window in
//! `shell::ui::quicklook`.
fn main() -> gtk::glib::ExitCode {
    shell::ui::quicklook::run()
}
