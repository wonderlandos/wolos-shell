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
