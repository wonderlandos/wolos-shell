// SPDX-License-Identifier: GPL-2.0-or-later
//! `wol-setup`: the install-time Setup Assistant (first boot, as `_wollogin`), or with
//! `--first-login` the person's own first sign-in Setup (`shell::first_login`).
fn main() -> gtk::glib::ExitCode {
    shell::kit::renderer::settle();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == shell::first_login::ARG) {
        let again = args.iter().any(|a| a == shell::ui::first_login::AGAIN);
        return shell::ui::first_login::run(again);
    }
    shell::ui::setup::run()
}
