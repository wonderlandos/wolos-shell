// SPDX-License-Identifier: GPL-2.0-or-later
fn main() -> gtk::glib::ExitCode {
    shell::kit::renderer::settle();
    // The language of the computer (/etc/locale.conf), ADR 0045 (QA-A7-10).
    shell::i18n::init();
    shell::ui::lock::run_login()
}
