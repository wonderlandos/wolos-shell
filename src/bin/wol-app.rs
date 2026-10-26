//! `wol-app <verb>`: menu bar app/Window menu actions on the frontmost window (also bound to keys).
//! Verbs: settings hide hide-others quit minimise zoom fill centre `desktop N`.
use shell::{appmenu, hypr};

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(act) = appmenu::parse_verb(&args.join(" ")) else {
        eprintln!(
            "usage: wol-app settings|hide|hide-others|quit|minimise|zoom|fill|centre|desktop N"
        );
        return std::process::ExitCode::from(2);
    };
    let Some(front) = hypr::request("j/activewindow").and_then(|j| hypr::parse_active_client(&j))
    else {
        return std::process::ExitCode::SUCCESS; // empty desktop: nothing to act on
    };
    let zone = hypr::request("j/monitors")
        .and_then(|j| hypr::parse_zone(&j))
        .unwrap_or_default();
    if let Some(b) = appmenu::batch(act, &front, &hypr::clients(), zone) {
        hypr::request(&b);
    }
    std::process::ExitCode::SUCCESS
}
