// SPDX-License-Identifier: GPL-2.0-or-later
fn main() -> gtk::glib::ExitCode {
    let warm = std::env::args().any(|a| a == shell::ui::WARM_ARG);
    shell::ui::searchbar::run(warm)
}
