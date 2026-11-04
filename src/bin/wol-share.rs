// SPDX-License-Identifier: GPL-2.0-or-later
//! wol-share: the share sheet for one file, a text on standard input or a web address. Prints
//! what the person did (copied, saved, opened or cancelled); a failure prints one sentence on
//! standard error and exits 1. Files' "Share…" starts it.
fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cwd = std::env::current_dir().unwrap_or_else(|_| "/".into());
    match shell::ui::share_sheet::run(&args, &cwd) {
        Ok(outcome) => {
            println!("{outcome}");
            std::process::ExitCode::SUCCESS
        }
        Err(sentence) => {
            eprintln!("{sentence}");
            std::process::ExitCode::FAILURE
        }
    }
}
