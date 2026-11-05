// SPDX-License-Identifier: GPL-2.0-or-later
//! `wol-monitor`: System Monitor (docs/modules/monitor.md), the window in `shell::ui::monitor`.
fn main() -> gtk::glib::ExitCode {
    shell::kit::renderer::settle();
    shell::ui::monitor::run()
}
