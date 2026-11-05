// SPDX-License-Identifier: GPL-2.0-or-later
//! Menu bar app and Window menus: every item is a real action. Pure: frontmost window + clients +
//! usable zone in, compositor dispatch batch out. `wol-app <verb>` runs them (menu clicks and keybinds).
use crate::hypr::{Client, Dispatch};

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
    /// An app command (⌘C …) handed to the app in front as its own key.
    Key(Cmd),
    /// Help > Search the manual…: opens Search, whose Manual provider answers.
    Manual,
    /// Help > Keyboard Shortcuts: the ⇧⌘/ sheet.
    Shortcuts,
}

/// App commands. ⌘ is Command: the app in front gets the key it listens for (Ctrl on this
/// system, the terminal's own key in a terminal). `wol-app <verb>` sends it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    Copy,
    Paste,
    Cut,
    Undo,
    Redo,
    SelectAll,
    Save,
    Open,
    Print,
    Find,
    FindNext,
    NewTab,
    /// Closes the tab, else the window (the app's Ctrl+W).
    Close,
    New,
    Reload,
    Location,
    ZoomIn,
    ZoomOut,
    ActualSize,
    /// ⌃⌘Space: GTK text fields open their emoji chooser on Ctrl+.
    Emoji,
}

impl Cmd {
    pub const ALL: [Cmd; 20] = [
        Cmd::Copy,
        Cmd::Paste,
        Cmd::Cut,
        Cmd::Undo,
        Cmd::Redo,
        Cmd::SelectAll,
        Cmd::Save,
        Cmd::Open,
        Cmd::Print,
        Cmd::Find,
        Cmd::FindNext,
        Cmd::NewTab,
        Cmd::Close,
        Cmd::New,
        Cmd::Reload,
        Cmd::Location,
        Cmd::ZoomIn,
        Cmd::ZoomOut,
        Cmd::ActualSize,
        Cmd::Emoji,
    ];

    /// (`wol-app` verb, compositor MODS, key) of the ⌘ shortcut in the shipped conf.
    const fn spec(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Cmd::Copy => ("copy", "SUPER", "C"),
            Cmd::Paste => ("paste", "SUPER", "V"),
            Cmd::Cut => ("cut", "SUPER", "X"),
            Cmd::Undo => ("undo", "SUPER", "Z"),
            Cmd::Redo => ("redo", "SUPER SHIFT", "Z"),
            Cmd::SelectAll => ("select-all", "SUPER", "A"),
            Cmd::Save => ("save", "SUPER", "S"),
            Cmd::Open => ("open", "SUPER", "O"),
            Cmd::Print => ("print", "SUPER", "P"),
            Cmd::Find => ("find", "SUPER", "F"),
            Cmd::FindNext => ("find-next", "SUPER", "G"),
            Cmd::NewTab => ("new-tab", "SUPER", "T"),
            Cmd::Close => ("close", "SUPER", "W"),
            Cmd::New => ("new", "SUPER", "N"),
            Cmd::Reload => ("reload", "SUPER", "R"),
            Cmd::Location => ("location", "SUPER", "L"),
            Cmd::ZoomIn => ("zoom-in", "SUPER", "equal"),
            Cmd::ZoomOut => ("zoom-out", "SUPER", "minus"),
            Cmd::ActualSize => ("actual-size", "SUPER", "0"),
            Cmd::Emoji => ("emoji", "CTRL SUPER", "Space"),
        }
    }
    pub fn verb(self) -> &'static str {
        self.spec().0
    }
    /// The shortcut as the conf spells it (MODS, key).
    pub fn keys(self) -> (&'static str, &'static str) {
        let (_, m, k) = self.spec();
        (m, k)
    }
}

/// What the app in front receives for a [`Cmd`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handoff {
    /// One tap of MODS + key (compositor `sendshortcut`) to the keyboard focus.
    Keys(&'static str, &'static str),
    /// Close the window (a terminal's Ctrl+W deletes a word).
    CloseWindow,
    /// Nothing: the app has no such command and its Ctrl key would do harm.
    Nothing,
}

/// A terminal app: does it have tabs, and its find key (with Ctrl+Shift).
struct Term {
    tabs: bool,
    find: &'static str,
}

const fn term(tabs: bool, find: &'static str) -> Term {
    Term { tabs, find }
}

/// Terminal window classes (lowercase). Their copy/paste/new window are Ctrl+Shift+C/V/N.
const TERMINALS: [(&str, Term); 10] = [
    ("foot", term(false, "R")),
    ("footclient", term(false, "R")),
    ("alacritty", term(false, "F")),
    ("xterm", term(false, "F")),
    ("kitty", term(true, "F")),
    ("org.wezfurlong.wezterm", term(true, "F")),
    ("com.mitchellh.ghostty", term(true, "F")),
    ("org.gnome.console", term(true, "F")),
    ("org.gnome.ptyxis", term(true, "F")),
    ("gnome-terminal-server", term(true, "F")),
];

/// What `c` sends to an app of window class `class` ("" = no window: desktop or a shell panel).
pub fn send(c: Cmd, class: &str) -> Handoff {
    let class = class.to_ascii_lowercase();
    let Some((_, t)) = TERMINALS.iter().find(|t| t.0 == class) else {
        return match c {
            Cmd::Emoji => Handoff::Keys("CTRL", "period"),
            _ => {
                let (_, m, k) = c.spec();
                Handoff::Keys(
                    if m.contains("SHIFT") {
                        "CTRL SHIFT"
                    } else {
                        "CTRL"
                    },
                    k,
                )
            }
        };
    };
    match c {
        Cmd::Copy => Handoff::Keys("CTRL SHIFT", "C"),
        Cmd::Paste => Handoff::Keys("CTRL SHIFT", "V"),
        Cmd::New => Handoff::Keys("CTRL SHIFT", "N"),
        Cmd::Find => Handoff::Keys("CTRL SHIFT", t.find),
        Cmd::ZoomIn | Cmd::ZoomOut | Cmd::ActualSize => Handoff::Keys("CTRL", c.spec().2),
        Cmd::NewTab if t.tabs => Handoff::Keys("CTRL SHIFT", "T"),
        Cmd::Close if t.tabs => Handoff::Keys("CTRL SHIFT", "W"),
        Cmd::Close => Handoff::CloseWindow,
        _ => Handoff::Nothing,
    }
}

/// Compositor request for `c` with `class`/`address` in front; `panel` = a shell panel that may hold
/// the keyboard is open (then nothing goes to the window behind it: the user may be typing in the
/// panel). `sendshortcut` over the socket is a press and release, so the app sees one tap, never a
/// held key. The key always names its window: the compositor (0.56) answers "invalid args" to an
/// empty window, its argument list drops an empty last field (e2e D7: ⌘A and ⌘C did nothing).
pub fn key_batch(c: Cmd, class: &str, address: &str, panel: bool) -> Option<String> {
    match send(c, class) {
        Handoff::Keys(m, k) if !panel && !address.is_empty() => Some(
            Dispatch::SendShortcut {
                mods: m.into(),
                key: k.into(),
                window: format!("address:{address}"),
            }
            .to_request(),
        ),
        Handoff::CloseWindow if !panel && !address.is_empty() => {
            Some(Dispatch::CloseWindow(format!("address:{address}")).to_request())
        }
        _ => None,
    }
}

/// Shell surfaces that never take the keyboard (kit::surface specs and the banner/OSD panels).
const NO_KEYBOARD: [&str; 7] = [
    "wol-wallpaper",
    "wol-desktop",
    "wol-menubar",
    "wol-dock",
    "wol-panel-banners",
    "wol-panel-osd",
    "wol-panel-capture-thumb",
];

/// True when a mapped WolOS layer may hold the keyboard (Search, Control Center, sheets…).
pub fn panel_has_keys(namespaces: &[String]) -> bool {
    namespaces
        .iter()
        .any(|n| n.starts_with("wol-") && !NO_KEYBOARD.contains(&n.as_str()))
}

/// Menu row: label ("" = separator), shortcut shown right-aligned, action.
pub type Row = (String, Option<&'static str>, Option<Act>);

/// Desktops offered by "Move to Desktop N" (Ctrl+Alt+1..4 switch to them).
pub const DESKTOPS: u8 = 4;
/// Title bar hyprbars draws on windows without client decorations (realtime titlebar::HEIGHT).
pub const BAR: i32 = 28;
/// Tag wol-hypr puts on windows that draw their own title bar.
pub const CSD_TAG: &str = "wol-csd";

/// Where the menu bar takes the frontmost app's own menus from (beside the app and Window menus).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// Only the shell's rows ([`app_menu`], [`window_menu`]).
    Builtin,
    /// A Wolkit native app that publishes its window menus on the session bus: its well-known
    /// name is the app's own id, the path is [`super::ui::transient::object_path`] of it and the
    /// interface [`WOLKIT_MENUS`] (`Get` -> the menus as JSON, `Activate(menu, key)`, `Changed`).
    Wolkit { bus: String, path: String },
}

/// The interface a Wolkit app's menus are published on.
pub const WOLKIT_MENUS: &str = "org.wolos.Menus";
/// A toplevel's app id that asked for glass ends in this (`realtime::glass::APP_SUFFIX`, frozen).
pub const GLASS_SUFFIX: &str = ".glass";
/// Largest `Get` reply read: 8 menus of 48 entries with their labels and keys fit.
pub const MAX_MENUS_JSON: usize = 64 * 1024;

/// The app's own id from a window class (a glass window's suffix dropped).
pub fn app_id(class: &str) -> &str {
    match class.strip_suffix(GLASS_SUFFIX) {
        Some(base) if !base.is_empty() => base,
        _ => class,
    }
}

/// The menus' source for the frontmost window's class: `publishes(bus name)` says whether that
/// name answers [`WOLKIT_MENUS`] now (the caller asks the bus; GTK apps own their id too).
pub fn source(class: &str, publishes: impl Fn(&str) -> bool) -> Source {
    let id = app_id(class);
    if id.is_empty() || !publishes(id) {
        return Source::Builtin;
    }
    Source::Wolkit {
        bus: id.to_string(),
        path: crate::ui::transient::object_path(id),
    }
}

/// A Wolkit app's `Get` reply: its menus, bounded and checked like a Kit window's menus. Anything
/// too large or invalid shows none (the app and Window menus stay).
pub fn wolkit_menus(json: &str) -> Vec<shared::view::Menu> {
    if json.len() > MAX_MENUS_JSON {
        return Vec::new();
    }
    match serde_json::from_str::<Vec<shared::view::Menu>>(json) {
        Ok(m) if shared::view::validate_chrome(&m, None).is_ok() => m,
        _ => Vec::new(),
    }
}

/// Apps known to open their preferences on a key, as (lowercase window class, MODS, key): the
/// hand-off of "Settings…". An app not listed gets no row (no fake "Settings…"); add a class here
/// once its key is known. ⌘, is the system's own Settings (`hyprland.conf`), never handed on.
pub const SETTINGS_KEYS: [(&str, &str, &str); 3] = [
    ("org.gnome.texteditor", "CTRL", "comma"),
    ("org.gnome.nautilus", "CTRL", "comma"),
    ("org.gnome.ptyxis", "CTRL", "comma"),
];

/// What "Settings…" taps into an app of window class `class`: None = the app has no known key.
pub fn settings_key(class: &str) -> Option<Handoff> {
    let class = class.to_ascii_lowercase();
    SETTINGS_KEYS
        .iter()
        .find(|k| k.0 == class)
        .map(|k| Handoff::Keys(k.1, k.2))
}

pub fn app_menu(name: &str, class: &str) -> Vec<Row> {
    if class.is_empty() {
        return Vec::new();
    }
    let row = |l: String, k, a| (l, k, Some(a));
    let sep = || (String::new(), None, None);
    let mut m = vec![row(format!("About {name}"), None, Act::About), sep()];
    if settings_key(class).is_some() {
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

// ---- the standard menus: File, Edit, View and Help for an app that publishes none ----------------

/// What kind of app a window class is, for the menu rows that mean something else or nothing in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Terminal,
    /// The browser (Atlas, Helium) and other web engines' windows.
    Browser,
    /// The file manager.
    Files,
    /// A document viewer (PDF).
    Document,
    /// A text editor.
    Editor,
    Other,
}

/// The kind of app of window class `class` (any case).
pub fn kind(class: &str) -> Kind {
    let class = class.to_ascii_lowercase();
    if TERMINALS.iter().any(|t| t.0 == class) {
        return Kind::Terminal;
    }
    match class.as_str() {
        "helium" | "atlas" | "org.wolos.atlas" | "chromium" | "org.chromium.chromium" => {
            Kind::Browser
        }
        "org.gnome.nautilus" | "wol-files" | "org.wolos.files" => Kind::Files,
        "org.gnome.papers" | "org.gnome.evince" => Kind::Document,
        "org.gnome.texteditor" => Kind::Editor,
        _ => Kind::Other,
    }
}

/// Commands an app of this kind is known not to have, or to give another meaning to the same key
/// (⌘L in a text editor is "go to line"; a viewer has nothing to cut). Everything else is offered
/// when [`send`] has a key for it. Terminals need no entry: [`send`] answers `Nothing` for them.
const NOT_APPLICABLE: [(Kind, &[Cmd]); 4] = [
    (Kind::Editor, &[Cmd::Reload, Cmd::Location]),
    (
        Kind::Document,
        &[
            Cmd::New,
            Cmd::NewTab,
            Cmd::Cut,
            Cmd::Paste,
            Cmd::Undo,
            Cmd::Redo,
            Cmd::Location,
            Cmd::Emoji,
        ],
    ),
    (
        Kind::Files,
        &[Cmd::Save, Cmd::Print, Cmd::FindNext, Cmd::Emoji],
    ),
    (Kind::Browser, &[Cmd::Emoji]),
];

/// Whether the row for `c` is live in an app of window class `class`: the hand-off has a key for
/// it and the app is not known to lack it. A menu row that did nothing would cost the bar its trust.
pub fn enabled(c: Cmd, class: &str) -> bool {
    if class.is_empty() || send(c, class) == Handoff::Nothing {
        return false;
    }
    let k = kind(class);
    !NOT_APPLICABLE
        .iter()
        .any(|(kk, cmds)| *kk == k && cmds.contains(&c))
}

/// A shortcut as the bar shows it ("Super+Shift+Z"), from the conf's MODS and key.
pub fn shortcut_text(mods: &str, key: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for m in ["SUPER", "CTRL", "ALT", "SHIFT"] {
        if mods.split_whitespace().any(|w| w == m) {
            out.push(match m {
                "SUPER" => "Super".into(),
                "CTRL" => "Ctrl".into(),
                "ALT" => "Alt".into(),
                _ => "Shift".into(),
            });
        }
    }
    out.push(match key {
        "equal" => "=".into(),
        "minus" => "-".into(),
        "comma" => ",".into(),
        "slash" => "/".into(),
        k => k.to_string(),
    });
    out.join("+")
}

/// One line of a standard menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StdEntry {
    Separator,
    Item {
        label: &'static str,
        /// Shown right-aligned ("" = none).
        shortcut: String,
        act: Act,
        /// False: greyed out, the app has no such command.
        enabled: bool,
    },
}

/// A menu of the bar that the shell itself draws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StdMenu {
    pub title: &'static str,
    pub entries: Vec<StdEntry>,
}

impl StdMenu {
    /// The items only: (label, act, enabled).
    pub fn items(&self) -> Vec<(&'static str, Act, bool)> {
        self.entries
            .iter()
            .filter_map(|e| match e {
                StdEntry::Item {
                    label,
                    act,
                    enabled,
                    ..
                } => Some((*label, *act, *enabled)),
                StdEntry::Separator => None,
            })
            .collect()
    }
}

/// One row of a standard menu: `None` = a separator.
type Spec = Option<(&'static str, Cmd)>;

const FILE: [Spec; 7] = [
    Some(("New", Cmd::New)),
    Some(("New Tab", Cmd::NewTab)),
    Some(("Open…", Cmd::Open)),
    None,
    Some(("Save", Cmd::Save)),
    Some(("Print…", Cmd::Print)),
    Some(("Close", Cmd::Close)),
];

const EDIT: [Spec; 12] = [
    Some(("Undo", Cmd::Undo)),
    Some(("Redo", Cmd::Redo)),
    None,
    Some(("Cut", Cmd::Cut)),
    Some(("Copy", Cmd::Copy)),
    Some(("Paste", Cmd::Paste)),
    Some(("Select All", Cmd::SelectAll)),
    None,
    Some(("Find…", Cmd::Find)),
    Some(("Find Next", Cmd::FindNext)),
    None,
    Some(("Emoji & Symbols", Cmd::Emoji)),
];

const VIEW: [Spec; 6] = [
    Some(("Zoom In", Cmd::ZoomIn)),
    Some(("Zoom Out", Cmd::ZoomOut)),
    Some(("Actual Size", Cmd::ActualSize)),
    None,
    Some(("Reload", Cmd::Reload)),
    Some(("Open Location…", Cmd::Location)),
];

fn std_menu(title: &'static str, specs: &[Spec], class: &str) -> StdMenu {
    StdMenu {
        title,
        entries: specs
            .iter()
            .map(|s| match s {
                None => StdEntry::Separator,
                Some((label, c)) => {
                    let (m, k) = c.keys();
                    StdEntry::Item {
                        label,
                        shortcut: shortcut_text(m, k),
                        act: Act::Key(*c),
                        enabled: enabled(*c, class),
                    }
                }
            })
            .collect(),
    }
}

/// File, Edit and View for an app of window class `class` that publishes no menus of its own (a
/// Wolkit app that does keeps them: [`source`]). Every row is the key the ⌘ shortcut taps
/// ([`send`]), greyed where the app has none. No window in front: no menus.
pub fn standard_menus(class: &str) -> Vec<StdMenu> {
    if class.is_empty() {
        return Vec::new();
    }
    vec![
        std_menu("File", &FILE, class),
        std_menu("Edit", &EDIT, class),
        std_menu("View", &VIEW, class),
    ]
}

/// What the bar shows between the app name and Window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Menus {
    /// The app published these (a Wolkit app, through `org.wolos.Menus`): they keep precedence.
    Own(Vec<shared::view::Menu>),
    /// The app published none: the shell's File, Edit and View.
    Standard(Vec<StdMenu>),
}

/// The menus of the app of window class `class`, given what it `published` (nothing, or something
/// that did not pass [`wolkit_menus`]: empty).
pub fn menus(class: &str, published: Vec<shared::view::Menu>) -> Menus {
    if published.is_empty() {
        Menus::Standard(standard_menus(class))
    } else {
        Menus::Own(published)
    }
}

impl Menus {
    /// Whether the system's Help follows Window: not with no window in front, and not when the
    /// app has a Help menu of its own (the person would find two).
    pub fn help_shown(&self) -> bool {
        match self {
            Menus::Own(m) => !m.iter().any(|m| m.title == HELP),
            Menus::Standard(m) => !m.is_empty(),
        }
    }
}

/// The title of the Help menu.
pub const HELP: &str = "Help";

/// Help: the same two rows in every app (they are the system's).
pub fn help_menu() -> StdMenu {
    let item = |label, shortcut: &str, act| StdEntry::Item {
        label,
        shortcut: shortcut.to_string(),
        act,
        enabled: true,
    };
    StdMenu {
        title: HELP,
        entries: vec![
            item(crate::copy::HELP_SEARCH_MANUAL, "Super+Space", Act::Manual),
            item("Keyboard Shortcuts", "Super+Shift+/", Act::Shortcuts),
        ],
    }
}

/// The program and arguments that run `a` from a menu click (About is drawn by the shell: empty).
pub fn argv(a: Act) -> Vec<String> {
    let own = |v: &[&str]| v.iter().map(|s| s.to_string()).collect();
    match a {
        Act::About => Vec::new(),
        Act::Manual => own(&["wol-search"]),
        Act::Shortcuts => own(&["wol", "shell", "toggle", "shortcuts"]),
        a => {
            let mut v = vec!["wol-app".to_string()];
            v.extend(verb(a).unwrap_or_default().split(' ').map(String::from));
            v
        }
    }
}

/// `wol-app` verb for an action (About is drawn by the shell, not dispatched).
pub fn verb(a: Act) -> Option<String> {
    Some(match a {
        Act::About | Act::Manual | Act::Shortcuts => return None,
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
        Act::Key(c) => c.verb().into(),
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
        (v, None) => Act::Key(Cmd::ALL.into_iter().find(|c| c.verb() == v)?),
        _ => return None,
    };
    w.next().is_none().then_some(a)
}

const HIDDEN: &str = "special:hidden";

/// One app's windows: the switcher groups by the lowercased class (`switcher::apps`), so a group
/// matches every window whatever the case the compositor reports (audit4 S3-1).
pub fn same_class(a: &str, b: &str) -> bool {
    a == b || a.to_lowercase() == b.to_lowercase()
}

/// Compositor `[[BATCH]]` request for `a` on the frontmost window; None = nothing to do.
pub fn batch(
    a: Act,
    front: &Client,
    clients: &[Client],
    zone: (i32, i32, i32, i32),
) -> Option<String> {
    let at = &front.address;
    let win = |c: &Client| format!("address:{}", c.address);
    let front_win = win(front);
    let hide = |c: &&Client| c.workspace.name != HIDDEN;
    let hide_cmd = |c: &Client| Dispatch::MoveToWorkspace {
        workspace: HIDDEN.into(),
        window: win(c),
        silent: true,
    };
    let bar = if front.tags.iter().any(|t| t == CSD_TAG) {
        0
    } else {
        BAR
    };
    let (zx, zy, zw, zh) = zone;
    let cmds: Vec<Dispatch> = match a {
        Act::About | Act::Manual | Act::Shortcuts => return None,
        Act::Settings => vec![Dispatch::SendShortcut {
            mods: "CTRL".into(),
            key: "comma".into(),
            window: front_win,
        }],
        Act::Hide => clients
            .iter()
            .filter(|c| same_class(&c.class, &front.class))
            .filter(hide)
            .map(hide_cmd)
            .collect(),
        Act::HideOthers => clients
            .iter()
            .filter(|c| !same_class(&c.class, &front.class))
            .filter(hide)
            .map(hide_cmd)
            .collect(),
        Act::Quit => clients
            .iter()
            .filter(|c| same_class(&c.class, &front.class))
            .map(|c| Dispatch::CloseWindow(win(c)))
            .collect(),
        Act::ForceQuit => clients
            .iter()
            .filter(|c| same_class(&c.class, &front.class))
            .map(|c| Dispatch::KillWindow(win(c)))
            .collect(),
        Act::Minimise => vec![hide_cmd(front)],
        // Zoom does nothing to a window in real full screen (the menu greys it out): the toggling
        // `fullscreen 1` dropped it to its floating box instead (audit3 R2A-13).
        Act::Zoom if front.fullscreen == 2 => return None,
        // `fullscreenstate` acts on the focused window: name the front window first, so a menu
        // click (the menu bar had the pointer) or a stale focus never zooms some other window.
        // Internal mode 1 (maximised), the client's mode left alone; the same mode again unzooms.
        Act::Zoom => vec![
            Dispatch::FocusWindow(front_win),
            Dispatch::FullscreenState {
                internal: 1,
                client: -1,
            },
        ],
        Act::Fill => vec![
            Dispatch::SetFloating(front_win.clone()),
            Dispatch::ResizeExact {
                w: zw,
                h: zh - bar,
                window: front_win.clone(),
            },
            Dispatch::MoveExact {
                x: zx,
                y: zy + bar,
                window: front_win,
            },
        ],
        Act::Centre => {
            let (w, h) = (front.size[0], front.size[1] + bar);
            let (x, y) = (zx + (zw - w) / 2, zy + (zh - h).max(0) / 2 + bar);
            vec![Dispatch::MoveExact {
                x,
                y,
                window: front_win,
            }]
        }
        Act::ToDesktop(n) => vec![Dispatch::MoveToWorkspace {
            workspace: n.to_string(),
            window: front_win,
            silent: false,
        }],
        Act::Key(c) => return key_batch(c, &front.class, at, false),
    };
    crate::hypr::batch(&cmds)
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
            c("0xc", "helium", "1"),
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
        // Classes match whatever the case (the switcher lowercases them, audit4 S3-1).
        let mixed = [
            c("0xe", "org.gnome.Nautilus", "1"),
            c("0xf", "org.gnome.nautilus", "2"),
        ];
        assert_eq!(
            batch(Act::Quit, &mixed[0], &mixed, ZONE).as_deref(),
            Some("[[BATCH]]dispatch closewindow address:0xe;dispatch closewindow address:0xf")
        );
        assert_eq!(
            b(Act::Minimise),
            "[[BATCH]]dispatch movetoworkspacesilent special:hidden,address:0xa"
        );
        // Zoom names its window: `fullscreenstate` alone acts on whatever is focused. An
        // explicit internal mode, not the `fullscreen 1` toggle (audit3 R2A-13).
        assert_eq!(
            b(Act::Zoom),
            "[[BATCH]]dispatch focuswindow address:0xa;dispatch fullscreenstate 1 -1"
        );
        let mut full = front.clone();
        full.fullscreen = 2;
        assert_eq!(
            batch(Act::Zoom, &full, &all, ZONE),
            None,
            "a window in full screen does not zoom"
        );
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

    /// Terminals: ⌘C/⌘V copy and paste (Ctrl+Shift+C/V); a terminal's Ctrl+letter is a control
    /// character (Ctrl+C interrupts, Ctrl+Z suspends, Ctrl+S freezes output), so none is ever sent.
    #[test]
    fn terminals_get_their_own_keys_and_never_a_control_character() {
        use Handoff::*;
        for (cmd, class, want) in [
            (Cmd::Copy, "foot", Keys("CTRL SHIFT", "C")),
            (Cmd::Paste, "foot", Keys("CTRL SHIFT", "V")),
            (Cmd::Paste, "Alacritty", Keys("CTRL SHIFT", "V")),
            (Cmd::New, "foot", Keys("CTRL SHIFT", "N")),
            (Cmd::Find, "foot", Keys("CTRL SHIFT", "R")),
            (Cmd::Find, "kitty", Keys("CTRL SHIFT", "F")),
            (Cmd::ZoomIn, "foot", Keys("CTRL", "equal")),
            (Cmd::ActualSize, "footclient", Keys("CTRL", "0")),
            (Cmd::Close, "foot", CloseWindow),
            (Cmd::Close, "org.gnome.Console", Keys("CTRL SHIFT", "W")),
            (Cmd::NewTab, "kitty", Keys("CTRL SHIFT", "T")),
            (Cmd::NewTab, "foot", Nothing),
            (Cmd::Undo, "foot", Nothing),
            (Cmd::Save, "foot", Nothing),
            (Cmd::Copy, "Helium", Keys("CTRL", "C")),
            (Cmd::Close, "helium", Keys("CTRL", "W")),
            (Cmd::Redo, "org.gnome.TextEditor", Keys("CTRL SHIFT", "Z")),
            (Cmd::Emoji, "org.gnome.TextEditor", Keys("CTRL", "period")),
            // Empty desktop or a shell panel in front: the keyboard focus gets the Ctrl key.
            (Cmd::Paste, "", Keys("CTRL", "V")),
        ] {
            assert_eq!(send(cmd, class), want, "{cmd:?} in {class}");
        }
        for class in TERMINALS.iter().map(|t| t.0) {
            for c in Cmd::ALL {
                if let Keys(m, k) = send(c, class) {
                    let letter = k.len() == 1 && k.as_bytes()[0].is_ascii_alphabetic();
                    assert!(!letter || m.contains("SHIFT"), "{c:?} in {class}: {m} {k}");
                }
            }
        }
    }

    #[test]
    fn commands_dispatch_to_the_keyboard_focus() {
        assert_eq!(
            key_batch(Cmd::Copy, "org.gnome.TextEditor", "0xa", false).as_deref(),
            Some("dispatch sendshortcut CTRL, C, address:0xa")
        );
        assert_eq!(
            key_batch(Cmd::SelectAll, "org.wolos.settings", "0xb", false).as_deref(),
            Some("dispatch sendshortcut CTRL, A, address:0xb"),
            "e2e D7: never an empty window, which the compositor refuses"
        );
        assert_eq!(
            key_batch(Cmd::Copy, "foot", "0xa", false).as_deref(),
            Some("dispatch sendshortcut CTRL SHIFT, C, address:0xa")
        );
        // A shell panel may hold the keyboard: nothing goes to the window behind it.
        assert_eq!(key_batch(Cmd::Copy, "foot", "0xa", true), None);
        assert_eq!(
            key_batch(Cmd::Close, "foot", "0xa", false).as_deref(),
            Some("dispatch closewindow address:0xa")
        );
        // A shell panel may hold the keyboard: ⌘W must not close the terminal behind it.
        assert_eq!(key_batch(Cmd::Close, "foot", "0xa", true), None);
        // No window in front: the compositor cannot be told "the keyboard focus" (no empty
        // window), so nothing is sent.
        assert_eq!(key_batch(Cmd::Close, "", "", false), None);
        assert_eq!(key_batch(Cmd::Copy, "", "", true), None);
        assert_eq!(key_batch(Cmd::Undo, "foot", "0xa", false), None);
        for c in Cmd::ALL {
            assert_eq!(parse_verb(&verb(Act::Key(c)).unwrap()), Some(Act::Key(c)));
        }
        assert_ne!(parse_verb("zoom"), parse_verb("zoom-in"));
    }

    /// Surfaces that never take the keyboard do not count as a panel in front.
    #[test]
    fn keyboard_panels_from_layer_namespaces() {
        let j = r#"{"eDP-1":{"levels":{"0":[{"namespace":"wol-wallpaper"}],"2":[{"namespace":"wol-menubar"},{"namespace":"wol-dock"}],"3":[{"namespace":"wol-panel-banners"},{"namespace":"wol-panel-osd"}]}}}"#;
        assert!(!panel_has_keys(&crate::hypr::parse_layers(j)));
        let j = r#"{"eDP-1":{"levels":{"3":[{"namespace":"wol-panel-searchbar"}]}}}"#;
        assert!(panel_has_keys(&crate::hypr::parse_layers(j)));
        assert!(!panel_has_keys(&crate::hypr::parse_layers(
            r#"{"eDP-1":{"levels":{"3":[{"namespace":"waybar"}]}}}"#
        )));
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

    #[test]
    fn a_wolkit_app_that_publishes_its_menus_is_the_source() {
        let publishes = |id: &str| id == "org.wolos.todos";
        let want = Source::Wolkit {
            bus: "org.wolos.todos".into(),
            path: "/org/wolos/todos".into(),
        };
        assert_eq!(source("org.wolos.todos", publishes), want);
        assert_eq!(
            source("org.wolos.todos.glass", publishes),
            want,
            "a glass window is the same app"
        );
        assert_eq!(source("org.gnome.Nautilus", publishes), Source::Builtin);
        assert_eq!(source("", |_| true), Source::Builtin);
        assert_eq!(app_id(".glass"), ".glass");
        assert_eq!(app_id("helium"), "helium");
    }

    #[test]
    fn wolkit_menus_are_bounded_like_kit_menus() {
        let ok = r#"[{"id":1,"title":"File","entries":[{"type":"item","key":"new","label":"New","shortcut":"cmd+n"},{"type":"separator"}]}]"#;
        let m = wolkit_menus(ok);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].title, "File");
        for bad in [
            "",
            "{",
            r#"[{"id":1,"title":"","entries":[]}]"#,
            r#"[{"id":1,"title":"A","entries":[]},{"id":1,"title":"B","entries":[]}]"#,
            r#"[{"id":1,"title":"A","entries":[],"x":1}]"#,
        ] {
            assert!(wolkit_menus(bad).is_empty(), "{bad}");
        }
        let huge = format!("[{}]", [ok.trim_matches(['[', ']'])].repeat(2000).join(","));
        assert!(huge.len() > MAX_MENUS_JSON && wolkit_menus(&huge).is_empty());
    }

    #[test]
    fn the_glass_suffix_is_the_compositor_s() {
        let glass = include_str!("../../realtime/src/glass.rs");
        assert!(glass.contains(&format!("pub const APP_SUFFIX: &str = \"{GLASS_SUFFIX}\";")));
    }
}
