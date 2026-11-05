// SPDX-License-Identifier: GPL-2.0-or-later
fn main() -> gtk::glib::ExitCode {
    shell::kit::renderer::settle();
    // The session's language (LANG; WOL_LANG=xx for layout tests), ADR 0045.
    shell::i18n::init();
    shell::ui::settings::run()
}
