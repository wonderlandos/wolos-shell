//! Menu bar app and Window menus: every item is a real action. Pure: frontmost window + clients +
//! usable zone in, Hyprland dispatch batch out. `wol-app <verb>` runs them (menu clicks and keybinds).
use crate::hypr::Client;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    About,
    Settings,
    Hide,
    HideOthers,
    Quit,
    /// Ends the app's process (a frozen app ignores Quit).
    ForceQuit,
    Minimise,
    Zoom,
    Fill,
    Centre,
    ToDesktop(u8),
}

/// Menu row: label ("" = separator), shortcut shown right-aligned, action.
pub type Row = (String, Option<&'static str>, Option<Act>);

/// Desktops offered by "Move to Desktop N" (Ctrl+1..4 switch to them).
pub const DESKTOPS: u8 = 4;
/// Title bar hyprbars draws on windows without client decorations (realtime titlebar::HEIGHT).
pub const BAR: i32 = 28;
/// Tag wol-hypr puts on windows that draw their own title bar.
pub const CSD_TAG: &str = "wol-csd";

/// Apps known to open their preferences on Ctrl+, (no fake "Settings…" elsewhere).
const SETTINGS_ON_CTRL_COMMA: &[&str] = &["org.gnome.TextEditor", "org.gnome.Nautilus"];

pub fn app_menu(name: &str, class: &str) -> Vec<Row> {
    if class.is_empty() {
        return Vec::new();
    }
    let row = |l: String, k, a| (l, k, Some(a));
    let sep = || (String::new(), None, None);
    let mut m = vec![row(format!("About {name}"), None, Act::About), sep()];
    if SETTINGS_ON_CTRL_COMMA.contains(&class) {
        m.push(row("Settings…".into(), Some("Ctrl+,"), Act::Settings));
        m.push(sep());
    }
    m.extend([
        row(format!("Hide {name}"), Some("Super+H"), Act::Hide),
        row("Hide Others".into(), Some("Super+Alt+H"), Act::HideOthers),
        sep(),
        row(format!("Quit {name}"), Some("Super+Q"), Act::Quit),
        // Named and in the menu on purpose: no key kills the frontmost app by accident.
        row(format!("Force Quit {name}"), None, Act::ForceQuit),
    ]);
    m
}

pub fn window_menu() -> Vec<Row> {
    let row = |l: &str, k, a| (l.to_string(), k, Some(a));
    let sep = || (String::new(), None, None);
    let mut m = vec![
        row("Minimise", Some("Super+M"), Act::Minimise),
        row("Zoom", None, Act::Zoom),
        sep(),
        row("Fill", None, Act::Fill),
        row("Centre", None, Act::Centre),
        sep(),
    ];
    m.extend((1..=DESKTOPS).map(|n| {
        (
            format!("Move to Desktop {n}"),
            None,
            Some(Act::ToDesktop(n)),
        )
    }));
    m
}

/// `wol-app` verb for an action (About is drawn by the shell, not dispatched).
pub fn verb(a: Act) -> Option<String> {
    Some(match a {
        Act::About => return None,
        Act::Settings => "settings".into(),
        Act::Hide => "hide".into(),
        Act::HideOthers => "hide-others".into(),
        Act::Quit => "quit".into(),
        Act::ForceQuit => "force-quit".into(),
        Act::Minimise => "minimise".into(),
        Act::Zoom => "zoom".into(),
        Act::Fill => "fill".into(),
        Act::Centre => "centre".into(),
        Act::ToDesktop(n) => format!("desktop {n}"),
    })
}

pub fn parse_verb(v: &str) -> Option<Act> {
    let mut w = v.split_whitespace();
    let a = match (w.next()?, w.next()) {
        ("settings", None) => Act::Settings,
        ("hide", None) => Act::Hide,
        ("hide-others", None) => Act::HideOthers,
        ("quit", None) => Act::Quit,
        ("force-quit", None) => Act::ForceQuit,
        ("minimise", None) => Act::Minimise,
        ("zoom", None) => Act::Zoom,
        ("fill", None) => Act::Fill,
        ("centre", None) => Act::Centre,
        ("desktop", Some(n)) => match n.parse() {
            Ok(n) if (1..=DESKTOPS).contains(&n) => Act::ToDesktop(n),
            _ => return None,
        },
        _ => return None,
    };
    w.next().is_none().then_some(a)
}

const HIDDEN: &str = "special:hidden";

/// Hyprland `[[BATCH]]` request for `a` on the frontmost window; None = nothing to do.
pub fn batch(
    a: Act,
    front: &Client,
    clients: &[Client],
    zone: (i32, i32, i32, i32),
) -> Option<String> {
    let at = &front.address;
    let hide = |c: &&Client| c.workspace.name != HIDDEN;
    let hide_cmd = |c: &Client| {
        format!(
            "dispatch movetoworkspacesilent {HIDDEN},address:{}",
            c.address
        )
    };
    let bar = if front.tags.iter().any(|t| t == CSD_TAG) {
        0
    } else {
        BAR
    };
    let (zx, zy, zw, zh) = zone;
    let cmds: Vec<String> = match a {
        Act::About => return None,
        Act::Settings => vec![format!("dispatch sendshortcut CTRL, comma, address:{at}")],
        Act::Hide => clients
            .iter()
            .filter(|c| c.class == front.class)
            .filter(hide)
            .map(hide_cmd)
            .collect(),
        Act::HideOthers => clients
            .iter()
            .filter(|c| c.class != front.class)
            .filter(hide)
            .map(hide_cmd)
            .collect(),
        Act::Quit => clients
            .iter()
            .filter(|c| c.class == front.class)
            .map(|c| format!("dispatch closewindow address:{}", c.address))
            .collect(),
        Act::ForceQuit => clients
            .iter()
            .filter(|c| c.class == front.class)
            .map(|c| format!("dispatch killwindow address:{}", c.address))
            .collect(),
        Act::Minimise => vec![hide_cmd(front)],
        Act::Zoom => vec!["dispatch fullscreen 1".into()],
        Act::Fill => vec![
            format!("dispatch setfloating address:{at}"),
            format!(
                "dispatch resizewindowpixel exact {zw} {},address:{at}",
                zh - bar
            ),
            format!(
                "dispatch movewindowpixel exact {zx} {},address:{at}",
                zy + bar
            ),
        ],
        Act::Centre => {
            let (w, h) = (front.size[0], front.size[1] + bar);
            let (x, y) = (zx + (zw - w) / 2, zy + (zh - h).max(0) / 2 + bar);
            vec![format!(
                "dispatch movewindowpixel exact {x} {y},address:{at}"
            )]
        }
        Act::ToDesktop(n) => vec![format!("dispatch movetoworkspace {n},address:{at}")],
    };
    (!cmds.is_empty()).then(|| format!("[[BATCH]]{}", cmds.join(";")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(addr: &str, class: &str, ws: &str) -> Client {
        let mut c = Client::new(addr, class, "t");
        c.workspace.name = ws.into();
        c.size = [400, 300];
        c
    }
    const ZONE: (i32, i32, i32, i32) = (0, 28, 1280, 670);

    fn labels(rows: &[Row]) -> Vec<(&str, Option<&str>)> {
        rows.iter().map(|r| (r.0.as_str(), r.1)).collect()
    }

    #[test]
    fn app_menu_names_the_app_and_only_offers_real_actions() {
        assert_eq!(
            labels(&app_menu("Terminal", "Alacritty")),
            [
                ("About Terminal", None),
                ("", None),
                ("Hide Terminal", Some("Super+H")),
                ("Hide Others", Some("Super+Alt+H")),
                ("", None),
                ("Quit Terminal", Some("Super+Q")),
                ("Force Quit Terminal", None),
            ]
        );
        let editor = app_menu("Text Editor", "org.gnome.TextEditor");
        assert_eq!(editor[2].0, "Settings…");
        assert_eq!(editor[2].2, Some(Act::Settings));
        // The empty desktop has no app to act on.
        assert!(app_menu("Desktop", "").is_empty());
    }

    #[test]
    fn window_menu_has_real_actions_and_every_desktop() {
        let w = window_menu();
        let names: Vec<&str> = w.iter().map(|r| r.0.as_str()).collect();
        assert_eq!(
            names,
            [
                "Minimise",
                "Zoom",
                "",
                "Fill",
                "Centre",
                "",
                "Move to Desktop 1",
                "Move to Desktop 2",
                "Move to Desktop 3",
                "Move to Desktop 4"
            ]
        );
        assert_eq!(w[0].1, Some("Super+M"));
        assert!(w.iter().all(|r| r.0.is_empty() == r.2.is_none()));
    }

    #[test]
    fn verbs_round_trip() {
        for a in [
            Act::Settings,
            Act::Hide,
            Act::HideOthers,
            Act::Quit,
            Act::ForceQuit,
            Act::Minimise,
            Act::Zoom,
            Act::Fill,
            Act::Centre,
            Act::ToDesktop(3),
        ] {
            assert_eq!(parse_verb(&verb(a).unwrap()), Some(a), "{a:?}");
        }
        assert_eq!(verb(Act::About), None);
        assert_eq!(parse_verb("desktop 9"), None);
        assert_eq!(parse_verb("fly"), None);
    }

    #[test]
    fn batches_target_windows_by_address() {
        let front = c("0xa", "foot", "1");
        let all = [
            front.clone(),
            c("0xb", "foot", "2"),
            c("0xc", "firefox", "1"),
            c("0xd", "gimp", "special:hidden"),
        ];
        let b = |a| batch(a, &front, &all, ZONE).unwrap();
        assert_eq!(
            b(Act::Hide),
            "[[BATCH]]dispatch movetoworkspacesilent special:hidden,address:0xa;dispatch movetoworkspacesilent special:hidden,address:0xb"
        );
        // Already hidden windows stay put.
        assert_eq!(
            b(Act::HideOthers),
            "[[BATCH]]dispatch movetoworkspacesilent special:hidden,address:0xc"
        );
        assert_eq!(
            b(Act::Quit),
            "[[BATCH]]dispatch closewindow address:0xa;dispatch closewindow address:0xb"
        );
        // A frozen app ignores closewindow: Force Quit ends it, every window of the app.
        assert_eq!(
            b(Act::ForceQuit),
            "[[BATCH]]dispatch killwindow address:0xa;dispatch killwindow address:0xb"
        );
        assert_eq!(
            b(Act::Minimise),
            "[[BATCH]]dispatch movetoworkspacesilent special:hidden,address:0xa"
        );
        assert_eq!(b(Act::Zoom), "[[BATCH]]dispatch fullscreen 1");
        assert_eq!(
            b(Act::ToDesktop(2)),
            "[[BATCH]]dispatch movetoworkspace 2,address:0xa"
        );
        assert_eq!(
            b(Act::Settings),
            "[[BATCH]]dispatch sendshortcut CTRL, comma, address:0xa"
        );
        assert_eq!(batch(Act::About, &front, &all, ZONE), None);
    }

    #[test]
    fn fill_and_centre_respect_the_title_bar() {
        let front = c("0xa", "foot", "1");
        // hyprbars draws 28 px above the client area.
        assert_eq!(
            batch(Act::Fill, &front, &[], ZONE).unwrap(),
            "[[BATCH]]dispatch setfloating address:0xa;dispatch resizewindowpixel exact 1280 642,address:0xa;dispatch movewindowpixel exact 0 56,address:0xa"
        );
        // Frame 400x328 centred in 1280x670 at y 28: frame top 199, client top 227.
        assert_eq!(
            batch(Act::Centre, &front, &[], ZONE).unwrap(),
            "[[BATCH]]dispatch movewindowpixel exact 440 227,address:0xa"
        );
        let mut csd = front.clone();
        csd.tags = vec![CSD_TAG.into()];
        assert_eq!(
            batch(Act::Fill, &csd, &[], ZONE).unwrap(),
            "[[BATCH]]dispatch setfloating address:0xa;dispatch resizewindowpixel exact 1280 670,address:0xa;dispatch movewindowpixel exact 0 28,address:0xa"
        );
    }
}
