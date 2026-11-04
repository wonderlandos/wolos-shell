// SPDX-License-Identifier: GPL-2.0-or-later
fn main() -> gtk::glib::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    shell::ui::lock::run_lock(&args)
}
