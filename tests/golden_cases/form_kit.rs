// SPDX-License-Identifier: GPL-2.0-or-later
//! The Kit form components, each on its own.
use super::*;

/// Settings-style Kit components (`kit::form` and the pickers panes share), on the app surface.
pub(super) fn cases() -> Vec<Case> {
    use shell::kit::form as f;
    use wol_core::shortcuts::Combo;
    let case = |name: &str, build: fn() -> gtk::Widget| Case {
        name: format!("kit-{name}"),
        size: (w::APP_NARROW.0, 1),
        class: "preview-app",
        wallpaper: false,
        build: Box::new(move || {
            let p = w::vbox(w::S4);
            p.add_css_class("page");
            p.add_css_class("app-window");
            p.append(&build());
            p.upcast()
        }),
    };
    fn combo(m: &str, k: &str) -> Combo {
        Combo::parse(m, k).unwrap()
    }
    vec![
        case("form-rows", || {
            w::section(
                "Windows",
                &[
                    f::toggle_row(
                        "Tile windows",
                        "New windows fill the screen side by side",
                        true,
                        |_| {},
                    ),
                    f::picker_row("Layout", "", &["Dwindle", "Master"], 0, |_| {}),
                    f::segmented_row("Appearance", "", &["Light", "Dark", "Auto"], 2, |_| {}),
                    f::stepper_row("Gaps", "", 12, (0, 32, 4), |v| format!("{v} pt"), |_| {}),
                    f::info_row("Processor", "Intel Core i5 (8 cores)"),
                    f::entry_row("Computer name", "wonderland", "Name", |_| {}),
                    f::nav_row("help-about-symbolic", "About", "Name, hardware, serial").upcast(),
                ],
            )
            .upcast()
        }),
        case("slider-row", || {
            w::section(
                "",
                &[
                    f::slider_row(
                        "Key repeat rate",
                        (5.0, 60.0, 5.0),
                        25.0,
                        ("Slow", "Fast"),
                        |_| {},
                    ),
                    f::slider_row(
                        "Delay until repeat",
                        (150.0, 1000.0, 50.0),
                        400.0,
                        ("Long", "Short"),
                        |_| {},
                    ),
                ],
            )
            .upcast()
        }),
        case("shortcut-recorder", || {
            let c = combo("SUPER", "Space");
            let ok = f::ShortcutRecorder::new(Some(&c), |_| {}, |_| {});
            let taken = f::ShortcutRecorder::new(Some(&combo("SUPER SHIFT", "4")), |_| {}, |_| {});
            taken.set_conflict(true);
            let off = f::ShortcutRecorder::new(None, |_| {}, |_| {});
            let fixed =
                f::ShortcutRecorder::new(Some(&combo("SUPER", "mouse:272")), |_| {}, |_| {});
            fixed.set_editable(false);
            let swap = w::button("Swap", w::Kind::Default, || {});
            let cancel = w::button("Cancel", w::Kind::Plain, || {});
            w::section(
                "Screenshots",
                &[
                    f::row("Show Search", "", &ok.button),
                    f::row("Capture the screen", "", &taken.button),
                    f::notice("⇧⌘4 is used by “Capture a selection”.", &[swap, cancel]).upcast(),
                    f::row("Record the screen", "", &off.button),
                    f::row("Move window", "", &fixed.button),
                ],
            )
            .upcast()
        }),
        case("permission-row", || {
            w::section(
                "Screen Recording",
                &[
                    f::permission_row(
                        "wolos-browser",
                        "Browser",
                        "Can record the screen",
                        true,
                        |_| {},
                    ),
                    f::permission_row(
                        "wolos-terminal",
                        "Terminal",
                        "Asked yesterday",
                        false,
                        |_| {},
                    ),
                ],
            )
            .upcast()
        }),
        case("wallpaper-tile", || {
            let r = w::wrap_row(w::S3, 3);
            let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../design/wallpapers");
            for (i, n) in ["wonderland-day", "wonderland-dusk", "wonderland-night"]
                .iter()
                .enumerate()
            {
                let p = std::path::PathBuf::from(format!("{dir}/{n}.svg"));
                r.insert(&f::wallpaper_tile(Some(&p), n, i == 1), -1);
            }
            r.upcast()
        }),
        case("empty-state", || {
            w::empty_state(
                "bluetooth-disabled-symbolic",
                "No Bluetooth Hardware",
                "This computer has no Bluetooth adapter. Plug in a USB adapter to use wireless devices.",
                None,
            )
            .upcast()
        }),
        case("search-field", || w::search_field("Search").upcast()),
        case("preset-card", || {
            use shell::kit::pickers::{PresetCard, Tokens};
            use wol_core::looks::Looks;
            let r = w::wrap_row(w::S3, 2);
            let soft = Looks {
                rounding: 18,
                ..Looks::default()
            };
            for (name, about, l, on) in [
                (
                    "WolOS",
                    "Soft glass, gentle motion.",
                    Looks::default(),
                    true,
                ),
                ("Rounded", "Round corners and wide gaps.", soft, false),
            ] {
                let c = PresetCard::new(name, about, &l, Tokens::current(), on);
                r.insert(&c.button, -1);
            }
            r.upcast()
        }),
        case("color-well", || {
            use shell::kit::pickers::{ColorWell, Tokens};
            let row = w::hbox(w::S2);
            for hex in ["#7c5cffff", "#33ccffee", "#ffffff40"] {
                row.append(&ColorWell::new(hex, &[], Tokens::current(), |_| {}).button);
            }
            w::section("", &[f::row("Border colours", "", &row)]).upcast()
        }),
        case("usage-bar", || {
            let parts: Vec<(String, String, f64)> = [
                ("Documents", "61 GB", 0.12),
                ("Apps", "38 GB", 0.07),
                ("System", "24 GB", 0.05),
                ("Restore points", "12 GB", 0.02),
                ("Other", "8 GB", 0.02),
            ]
            .iter()
            .map(|(n, s, f)| ((*n).into(), (*s).into(), *f))
            .collect();
            shell::kit::pickers::usage_bar(&parts).upcast()
        }),
        case("key-legend", || {
            shell::kit::pickers::key_legend(&ui::settings::KEY_LEGEND)
        }),
        case("toast", || {
            let aw = w::AppWindow::new(false, false);
            let t = shell::kit::toast::Toast::new(&aw);
            t.show("Wallpaper changed", "Undo", || {});
            t.revealer.set_transition_duration(0);
            aw.root.set_size_request(-1, 160);
            aw.root.upcast()
        }),
    ]
}
