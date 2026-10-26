//! The desktop hyprland.conf must blur every glass surface the shell creates.
//! Live Hyprland 0.56: `match:namespace wol-shell` matched nothing (namespaces are wol-menubar, ...),
//! so the dock and menu bar had no blur; `ignore_alpha 0` then blurred their transparent margins.
const CONF: &str =
    include_str!("../../../desktop/overlay/usr/local/share/wolos/hypr/hyprland.conf");

const GLASS: [&str; 6] = [
    "wol-menubar",
    "wol-dock",
    "wol-panel-switcher",
    "wol-panel-banners",
    "wol-spotlight",
    "wol-control-center",
];

/// Supports the anchored alternation the conf uses: `^(a|b|c-.*)$`.
fn matches(pattern: &str, ns: &str) -> bool {
    let Some(body) = pattern
        .strip_prefix("^(")
        .and_then(|p| p.strip_suffix(")$"))
    else {
        return false;
    };
    body.split('|').any(|alt| match alt.strip_suffix(".*") {
        Some(prefix) => ns.starts_with(prefix),
        None => alt == ns,
    })
}

fn rules(effect: &str) -> Vec<(String, String)> {
    CONF.lines()
        .filter_map(|l| l.trim().strip_prefix("layerrule = match:namespace "))
        .filter_map(|r| r.split_once(", "))
        .filter(|(_, e)| e.starts_with(effect))
        .map(|(p, e)| (p.to_string(), e.to_string()))
        .collect()
}

#[test]
fn every_glass_surface_is_blurred_without_its_margins() {
    let blur = rules("blur on");
    let alpha = rules("ignore_alpha");
    for ns in GLASS {
        assert!(
            blur.iter().any(|(p, _)| matches(p, ns)),
            "{ns}: no blur rule"
        );
        let a = alpha
            .iter()
            .find(|(p, _)| matches(p, ns))
            .unwrap_or_else(|| panic!("{ns}: no ignore_alpha rule"));
        let v: f32 = a.1.trim_start_matches("ignore_alpha ").parse().unwrap();
        assert!(v > 0.0, "{ns}: ignore_alpha {v} blurs transparent margins");
    }
}

/// Live: `wol shell show switcher-prev` was rejected by the CLI's panel list, so Shift+Tab,
/// Super+Up and Super+Shift+5 did nothing.
#[test]
fn every_bound_shell_panel_is_accepted_by_wol() {
    let bound: Vec<&str> = CONF
        .lines()
        .filter_map(|l| l.split_once("wol shell ").map(|(_, r)| r))
        .filter_map(|r| r.split_whitespace().nth(1))
        .collect();
    assert!(bound.len() >= 6, "{bound:?}");
    for p in bound {
        assert!(
            shared::shell::PANELS.contains(&p),
            "{p} not in shared::shell::PANELS"
        );
    }
}

/// Live: the toolbar's fade-out was in the screenshot.
#[test]
fn screenshot_surfaces_disappear_without_animation() {
    let none = rules("no_anim on");
    for ns in ["wol-panel-capture", "wol-panel-capture-thumb"] {
        assert!(
            none.iter().any(|(p, _)| matches(p, ns)),
            "{ns}: fades into the capture"
        );
    }
}

/// Every shortcut the menu bar shows is bound to the same `wol-app` action, and the image ships it.
#[test]
fn menu_shortcuts_are_bound_to_the_same_action() {
    use shell::appmenu::{app_menu, verb, window_menu};
    let rows = app_menu("App", "org.gnome.TextEditor")
        .into_iter()
        .chain(window_menu());
    let mut n = 0;
    for (label, key, act) in rows {
        let (Some(key), Some(act)) = (key, act) else {
            continue;
        };
        if key.starts_with("Ctrl+") {
            continue; // handled by the app itself (sendshortcut)
        }
        let mut parts: Vec<&str> = key.split('+').collect();
        let k = parts.pop().unwrap();
        let mods: Vec<String> = parts
            .iter()
            .map(|m| {
                if *m == "Super" {
                    "$mod".into()
                } else {
                    m.to_uppercase()
                }
            })
            .collect();
        let want = format!(
            "bind = {}, {k}, exec, wol-app {}",
            mods.join(" "),
            verb(act).unwrap()
        );
        assert!(
            CONF.lines().any(|l| l.trim() == want),
            "{label}: missing `{want}`"
        );
        n += 1;
    }
    assert_eq!(n, 4);
    assert!(include_str!("../../../image/build-rust.sh").contains(" wol-app"));
}

/// Volume keys show the OSD right away (it waited for the 10 s status poll).
#[test]
fn volume_keys_ask_the_shell_for_the_osd() {
    let script = include_str!("../../../desktop/overlay/usr/local/bin/wol-volume");
    assert!(
        script
            .lines()
            .any(|l| l.trim() == "exec wol shell show osd"),
        "{script}"
    );
}

/// Live perf: spawning wol-control-center per click took 52 ms median, 109 ms p95 to the first
/// frame (process + GTK + D-Bus setup). The shell keeps it resident; keys toggle the panel.
#[test]
fn control_center_is_a_resident_shell_panel() {
    assert!(
        CONF.lines()
            .any(|l| l.trim() == "bind = $mod, A, exec, wol shell toggle control-center")
    );
    let bar = include_str!("../src/ui/menubar.rs");
    assert!(
        !bar.contains("\"wol-control-center\""),
        "menu bar must not spawn the app"
    );
}
