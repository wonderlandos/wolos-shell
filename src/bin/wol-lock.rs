// SPDX-License-Identifier: GPL-2.0-or-later
fn main() -> gtk::glib::ExitCode {
    // The session's language (LANG, which the first account's locale sets), ADR 0045: without
    // it every `tr` in this process reads English (QA-A7-10).
    shell::i18n::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    shell::ui::lock::run_lock(&args)
}
