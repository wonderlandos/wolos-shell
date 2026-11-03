// SPDX-License-Identifier: GPL-2.0-or-later
//! The desktop hyprland.conf must blur every glass surface the shell creates.
//! Live on the compositor 0.56: `match:namespace wol-shell` matched nothing (namespaces are wol-menubar, ...),
//! so the dock and menu bar had no blur; `ignore_alpha 0` then blurred their transparent margins.
const CONF: &str = include_str!("../../../desktop/overlay/usr/share/wolos/hypr/hyprland.conf");
/// The compositor plugin that forwards Esc and the Super release (audit3 R2A-10).
const PLUGIN: &str = include_str!("../../../plugins/wol-hypr/main.cpp");
/// Its key gates (realtime wm_gates.rs, through wol_realtime.h): which line each key sends, which
/// keys it consumes.
const POLICY: &str = include_str!("../../realtime/src/wm_gates.rs");

/// Every shell glass surface (ui::searchbar, ui::control_center are `wol-panel-*` too).
const GLASS: [&str; 6] = [
    "wol-menubar",
    "wol-dock",
    "wol-panel-switcher",
    "wol-panel-banners",
    "wol-panel-searchbar",
    "wol-panel-controlcenter",
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
    assert!(include_str!("../../../tools/build-rust.sh").contains(" wol-app"));
}

/// Volume keys show the OSD right away (it waited for the 10 s status poll).
#[test]
fn volume_keys_ask_the_shell_for_the_osd() {
    let script = include_str!("../../../desktop/overlay/usr/bin/wol-volume");
    assert!(
        script
            .lines()
            .any(|l| l.trim() == "exec wol shell show osd"),
        "{script}"
    );
}

/// Live perf: spawning wol-control-center per click took 52 ms median, 109 ms p95 to the first
/// frame (process + GTK + D-Bus setup). The shell keeps it resident; the menu bar toggles the panel.
#[test]
fn control_center_is_a_resident_shell_panel() {
    let bar = include_str!("../src/ui/menubar.rs");
    assert!(bar.contains(r#"&["wol", "shell", "toggle", "control-center"]"#));
    assert!(
        !bar.contains("\"wol-control-center\""),
        "menu bar must not spawn the app"
    );
}

/// ⌘ is Command (J13): a plain ⌘+letter belongs to the app in front. Super+A/F/N/L/E took
/// ⌘A/F/N/L/E from every app; now each one is an app command, Quit, Hide or Minimise.
#[test]
fn plain_command_letters_belong_to_the_app() {
    use wol_core::shortcuts::{SUPER, parse};
    let binds = parse(CONF);
    let mut letters = 0;
    for b in &binds {
        let k = b.combo.key.as_bytes();
        if b.combo.mods != SUPER || k.len() != 1 || !k[0].is_ascii_alphabetic() {
            continue;
        }
        letters += 1;
        let verb = (b.dispatcher == "exec")
            .then(|| b.args.strip_prefix("wol-app "))
            .flatten();
        assert!(
            verb.and_then(shell::appmenu::parse_verb).is_some(),
            "⌘{}: system shortcut on an app letter ({} {})",
            b.combo.key,
            b.dispatcher,
            b.args
        );
    }
    assert!(letters >= 18, "{letters}");
    for gone in [
        "wol shell toggle control-center",
        "wol shell toggle notifications",
        "wol theme toggle",
        "nautilus",
    ] {
        assert!(
            !binds.iter().any(|b| b.args == gone),
            "{gone} keeps a shortcut"
        );
    }
}

/// Mac muscle memory: every app command ⌘ shortcut is bound to `wol-app <verb>`, and an app gets
/// the same key with Ctrl for ⌘ (⇧⌘Z: Ctrl+Shift+Z).
#[test]
fn every_command_shortcut_reaches_the_app_as_ctrl() {
    use shell::appmenu::{Cmd, Handoff, send};
    use wol_core::shortcuts::{CTRL, Combo, SUPER, parse};
    let binds = parse(CONF);
    let mut keys = Vec::new();
    for c in Cmd::ALL {
        let (mods, key) = c.keys();
        let combo = Combo::parse(mods, key).unwrap();
        let b = binds
            .iter()
            .find(|b| b.combo.same(&combo))
            .unwrap_or_else(|| panic!("{c:?}: {} not bound", combo.label()));
        assert_eq!(
            (b.dispatcher.as_str(), b.args.as_str()),
            ("exec", format!("wol-app {}", c.verb()).as_str()),
            "{c:?}"
        );
        assert_eq!(
            shell::appmenu::parse_verb(c.verb()),
            Some(shell::appmenu::Act::Key(c))
        );
        if combo.mods & CTRL != 0 {
            continue; // ⌃⌘Space (Emoji) has no Ctrl twin
        }
        let Handoff::Keys(m, k) = send(c, "org.gnome.TextEditor") else {
            panic!("{c:?}: apps get nothing");
        };
        assert_eq!(
            Combo::parse(m, k),
            Some(Combo {
                mods: combo.mods & !SUPER | CTRL,
                key: combo.key.clone()
            }),
            "{c:?}"
        );
        keys.push(combo.label());
    }
    for want in [
        "⌘C", "⌘V", "⌘X", "⌘Z", "⇧⌘Z", "⌘A", "⌘S", "⌘O", "⌘P", "⌘F", "⌘G", "⌘T", "⌘W", "⌘N", "⌘R",
        "⌘L", "⌘=", "⌘-", "⌘0",
    ] {
        assert!(keys.iter().any(|k| k == want), "{want} not an app command");
    }
}

/// System shortcuts sit off the app letters: ⌘Space, ⇧⌘3/4/5, ⌃↑ and ⌘↑, ⌘Tab, ⌃⌘Q,
/// ⌥⌘Esc (Force Quit), ⌃F2 (menu bar), ⌃F3 (Dock). Esc (closes panels, dismisses banners) is
/// forwarded by wol-hypr, not bound (`no_process_per_escape_or_super_release`).
#[test]
fn system_shortcuts_are_the_mac_ones() {
    use wol_core::shortcuts::{Combo, parse};
    let binds = parse(CONF);
    for (mods, key, flags, action) in [
        ("SUPER", "Space", "", "wol-search"),
        ("SUPER SHIFT", "3", "", "wol-screenshot full"),
        ("SUPER SHIFT", "4", "", "wol-screenshot region"),
        ("SUPER SHIFT", "5", "", "wol shell show screenshot"),
        ("CTRL", "Up", "", "wol shell toggle overview"),
        ("SUPER", "Up", "", "wol shell toggle overview"),
        ("SUPER", "Tab", "", "wol shell show switcher"),
        ("CTRL SUPER", "Q", "", "wol-lock"),
        ("SUPER ALT", "Escape", "", "wol shell toggle force-quit"),
        ("CTRL", "F2", "", "wol shell show menubar"),
        ("CTRL", "F3", "", "wol shell show dock"),
    ] {
        let c = Combo::parse(mods, key).unwrap();
        assert!(
            binds.iter().any(|b| b.combo.same(&c)
                && b.flags == flags
                && b.dispatcher == "exec"
                && b.args == action),
            "{} ({flags}) should run {action}",
            c.label()
        );
    }
}

/// Audit3 R2A-10 (after audit2 A2-13): no `wol` process per Esc press or per Super release. Every
/// Esc and every Super release in every app, games included, forked `wol`; the wol-hypr plugin
/// forwards them to the shell socket instead, only while a shell layer is on screen.
#[test]
fn no_process_per_escape_or_super_release() {
    use wol_core::shortcuts::{Combo, parse};
    let esc = Combo::parse("", "Escape").unwrap();
    for b in parse(CONF) {
        assert!(
            !(b.combo.same(&esc) && b.dispatcher == "exec"),
            "plain Esc runs {}",
            b.args
        );
        assert!(
            !(b.flags.contains('r') && b.dispatcher == "exec" && b.combo.key.starts_with("SUPER_")),
            "a Super release runs {}",
            b.args
        );
    }
    for needle in [
        "case KEY_ESC:",
        "tellShell(wol_keys_line(line))",
        "/wol-shell.sock",
        r#"starts_with("wol-panel-")"#,
        "m_events.input.keyboard.key.listen",
        "m_events.layer.opened.listen",
        "m_events.layer.closed.listen",
    ] {
        assert!(PLUGIN.contains(needle), "wol-hypr forwards: {needle}");
    }
    for needle in [r#""hide transient\n""#, r#""hide switcher\n""#] {
        assert!(POLICY.contains(needle), "wol-hypr sends: {needle}");
    }
    assert_eq!(
        shell::protocol::socket_path("/run/user/1")
            .file_name()
            .and_then(|n| n.to_str()),
        Some("wol-shell.sock"),
        "the socket the plugin writes to"
    );
}

/// Audit2 A2-6: every transient shell panel has a way in, a shortcut or a menu-bar launcher
/// (the clipboard history had none: only `wol shell toggle clipboard` from a terminal).
#[test]
fn every_transient_panel_can_be_opened() {
    let binds = wol_core::shortcuts::parse(CONF);
    let menubar = include_str!("../src/ui/menubar.rs");
    for p in shell::ui::transient::PANELS {
        let bound = binds.iter().any(|b| {
            b.dispatcher == "exec"
                && (b.args == format!("wol shell toggle {p}")
                    || b.args == format!("wol shell show {p}"))
        });
        let launcher = menubar.contains(&format!("\"{p}\""));
        assert!(
            bound || launcher,
            "{p}: no shortcut and no menu-bar launcher"
        );
    }
    let clip = wol_core::shortcuts::Combo::parse("SUPER SHIFT", "V").unwrap();
    assert!(
        binds
            .iter()
            .any(|b| b.combo.same(&clip) && b.args == "wol shell toggle clipboard"),
        "⇧⌘V opens the clipboard history"
    );
}

/// Audit2 A2-9: plain ⌃ + letter, digit or ←/→ is the app's key (Ctrl+1..9 browser tabs,
/// Ctrl+arrows word-wise cursor; Super is ⌘ here). Spaces are ⌃⌥←/→ and ⌃⌥1..4 (⌃⌘arrows resize in
/// tiling.conf). ⌃↑ stays the Overview key and ⌃F2/⌃F3 the bar keys.
#[test]
fn plain_ctrl_keys_belong_to_the_app() {
    use wol_core::shortcuts::{CTRL, Combo, parse};
    let binds = parse(CONF);
    for b in &binds {
        let k = b.combo.key.as_str();
        let app_key = (k.len() == 1 && k.as_bytes()[0].is_ascii_alphanumeric())
            || k.eq_ignore_ascii_case("Left")
            || k.eq_ignore_ascii_case("Right");
        assert!(
            !(b.combo.mods == CTRL && app_key),
            "{}: plain Ctrl shortcut taken from apps ({} {})",
            b.combo.label(),
            b.dispatcher,
            b.args
        );
    }
    for (key, arg) in [
        ("Left", "e-1"),
        ("Right", "e+1"),
        ("1", "1"),
        ("2", "2"),
        ("3", "3"),
        ("4", "4"),
    ] {
        let c = Combo::parse("CTRL ALT", key).unwrap();
        assert!(
            binds
                .iter()
                .any(|b| b.combo.same(&c) && b.dispatcher == "workspace" && b.args == arg),
            "{} should switch to space {arg}",
            c.label()
        );
    }
}

/// Audit2 A2-16, audit3 R2A-10: releasing either Super key finishes ⌘Tab, also when the strip is
/// not on screen yet (a quick tap: a Tab was seen since Super went down). wol-hypr forwards it.
#[test]
fn releasing_either_super_finishes_switching() {
    for needle in [
        "case KEY_LEFTMETA:",
        "case KEY_RIGHTMETA:",
        "case KEY_TAB:",
        "shellLayerMapped(\"wol-panel-switcher\")",
    ] {
        assert!(PLUGIN.contains(needle), "wol-hypr: {needle}");
    }
    assert!(POLICY.contains("(self.switching || e.switcher_mapped)"));
    // Audit4 S5-3: with the plugin, ⌘Tab's show goes over the same socket as the release, before
    // it (the plugin consumes the Tab, so the bind below does not run as well).
    for needle in [r#""show switcher\n""#, r#""show switcher-prev\n""#] {
        assert!(POLICY.contains(needle), "wol-hypr sends: {needle}");
    }
    assert!(PLUGIN.contains("info.cancelled = true"));
    // ⌘Tab itself stays a bind: the fallback without the plugin.
    let tab = wol_core::shortcuts::Combo::parse("SUPER", "Tab").unwrap();
    assert!(
        wol_core::shortcuts::parse(CONF)
            .iter()
            .any(|b| b.combo.same(&tab) && b.args == "wol shell show switcher")
    );
}
