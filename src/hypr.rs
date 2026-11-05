// SPDX-License-Identifier: GPL-2.0-or-later
//! Compositor IPC: `.socket2.sock` events (event-driven, no polling) and `j/clients` on `.socket.sock`.
//! Replace with the `realtime` crate's client when it lands.
use std::io::{Read, Write};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    ActiveWindow {
        class: String,
        title: String,
    },
    OpenWindow {
        address: String,
        class: String,
    },
    CloseWindow {
        address: String,
    },
    Workspace(String),
    /// A window asks for attention (`address` as `j/clients` spells it, with `0x`).
    Urgent {
        address: String,
    },
    /// The focus moved to another display (its connector name).
    FocusedMonitor(String),
    /// The compositor read its config again: every `keyword` bind is gone (audit5 S9-4), so the shell
    /// binds its fallback keys and the plugin hotkeys again.
    ConfigReloaded,
    Other,
}

/// One `EVENT>>DATA` line.
pub fn parse_event(line: &str) -> Option<Event> {
    let (name, data) = line.trim_end().split_once(">>")?;
    Some(match name {
        "activewindow" => {
            let (class, title) = data.split_once(',').unwrap_or((data, ""));
            Event::ActiveWindow {
                class: crate::appmenu::app_id(class).into(),
                title: title.into(),
            }
        }
        "openwindow" => {
            let mut p = data.splitn(4, ',');
            let address = p.next()?.into();
            let _ws = p.next()?;
            Event::OpenWindow {
                address,
                class: crate::appmenu::app_id(p.next()?).into(),
            }
        }
        "closewindow" => Event::CloseWindow {
            address: data.into(),
        },
        "workspace" => Event::Workspace(data.into()),
        "focusedmon" => Event::FocusedMonitor(data.split(',').next().unwrap_or(data).into()),
        "urgent" => Event::Urgent {
            address: format!("0x{data}"),
        },
        "configreloaded" => Event::ConfigReloaded,
        _ => Event::Other,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub struct Client {
    pub address: String,
    pub class: String,
    pub title: String,
    /// 0 = most recently focused. hyprctl says -1 for a window outside the focus history (never
    /// focused: a `nofocus` window, a tooltip surface); it reads as the farthest, never as the
    /// most recent one (QA-A10-SD-04: Minimise handed it the keyboard, the switcher put it first).
    #[serde(
        rename = "focusHistoryID",
        default = "far",
        deserialize_with = "focus_id"
    )]
    pub focus: i64,
    #[serde(default)]
    pub workspace: WorkspaceRef,
    #[serde(default)]
    pub at: [i32; 2],
    #[serde(default)]
    pub size: [i32; 2],
    #[serde(default)]
    pub tags: Vec<String>,
    /// The compositor's internal mode: 0 none, 1 maximized (Zoom), 2 full screen.
    #[serde(default)]
    pub fullscreen: i64,
}

fn far() -> i64 {
    i64::MAX
}

/// `focusHistoryID`: a negative id (not in the history) is [`far`].
fn focus_id<'de, D: serde::Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    let n = <i64 as serde::Deserialize>::deserialize(d)?;
    Ok(if n < 0 { far() } else { n })
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize)]
pub struct WorkspaceRef {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub name: String,
}

impl Client {
    pub fn new(address: &str, class: &str, title: &str) -> Self {
        Client {
            address: address.into(),
            class: class.into(),
            title: title.into(),
            focus: far(),
            workspace: WorkspaceRef::default(),
            at: [0, 0],
            size: [0, 0],
            tags: Vec::new(),
            fullscreen: 0,
        }
    }
}

pub fn parse_clients(json: &str) -> Vec<Client> {
    serde_json::from_str(json).unwrap_or_default()
}

/// `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/<name>`; None outside the compositor.
pub fn socket(name: &str) -> Option<PathBuf> {
    shared::sys::hypr_socket(|k| std::env::var(k).ok(), name)
}

/// `name` of the compositor instance under `<runtime>/hypr/` that started last and has it; None
/// when there is none. For the event socket of a shell whose environment does not name the
/// instance (a wrapper that dropped it, a dev run): it is looked for again on every reconnect.
pub fn newest_socket(runtime: &std::path::Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(runtime.join("hypr"))
        .ok()?
        .filter_map(|e| {
            let sock = e.ok()?.path().join(name);
            let made = std::fs::symlink_metadata(&sock).ok()?.modified().ok()?;
            Some((made, sock))
        })
        .max_by_key(|(made, _)| *made)
        .map(|(_, sock)| sock)
}

/// [`find_socket`] for the GTK thread: the environment names the instance, or the runtime folder
/// is read on a worker thread.
pub async fn find_socket_off_thread(
    var: impl Fn(&str) -> Option<String>,
    name: &'static str,
) -> Option<PathBuf> {
    if let Some(named) = shared::sys::hypr_socket(&var, name) {
        return Some(named);
    }
    let rt = var("XDG_RUNTIME_DIR").filter(|r| r.starts_with('/'))?;
    gtk::gio::spawn_blocking(move || newest_socket(std::path::Path::new(&rt), name))
        .await
        .ok()
        .flatten()
}

/// The compositor's socket `name` as `var` gives the environment: the instance it names
/// ([`socket`]), else the newest one in the runtime directory ([`newest_socket`]).
pub fn find_socket(var: impl Fn(&str) -> Option<String>, name: &str) -> Option<PathBuf> {
    shared::sys::hypr_socket(&var, name).or_else(|| {
        let rt = var("XDG_RUNTIME_DIR").filter(|r| r.starts_with('/'))?;
        newest_socket(std::path::Path::new(&rt), name)
    })
}

/// Longest a request waits on the compositor: a stalled compositor must not stall the shell.
pub const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

/// One request on `.socket.sock`; None when the compositor is away or does not answer in time.
pub fn request(cmd: &str) -> Option<String> {
    let mut s = std::os::unix::net::UnixStream::connect(socket(".socket.sock")?).ok()?;
    s.set_read_timeout(Some(REQUEST_TIMEOUT)).ok()?;
    s.set_write_timeout(Some(REQUEST_TIMEOUT)).ok()?;
    s.write_all(cmd.as_bytes()).ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    Some(out)
}

pub fn clients() -> Vec<Client> {
    read_clients().unwrap_or_default()
}

/// `j/clients`; None when the compositor does not answer (an unreadable reply is an empty list).
pub fn read_clients() -> Option<Vec<Client>> {
    request("j/clients").map(|j| parse_clients(&j))
}

/// The focused window (`j/activewindow`); None on the empty desktop or without an answer.
pub fn active_window() -> Option<Client> {
    request("j/activewindow").and_then(|j| parse_active_client(&j))
}

/// The active workspace's id (`j/activeworkspace`); None without an answer.
pub fn active_workspace() -> Option<i64> {
    request("j/activeworkspace").and_then(|j| parse_workspace_id(&j))
}

/// The focused output minus its reserved areas ([`parse_zone`]); None without an answer.
pub fn zone() -> Option<(i32, i32, i32, i32)> {
    request("j/monitors").and_then(|j| parse_zone(&j))
}

/// Namespaces of the mapped layer surfaces ([`parse_layers`]); None without an answer.
pub fn layers() -> Option<Vec<String>> {
    request("j/layers").map(|j| parse_layers(&j))
}

/// The plugin that forwards Esc and the Super release to the shell (`plugins/wol-hypr`).
pub const KEY_PLUGIN: &str = "wol-hypr";

/// Binds the shell installs when [`KEY_PLUGIN`] is not loaded: the old per-key `wol` process, so
/// a broken plugin never leaves panels unclosable (audit3 R2A-10 fallback).
pub const KEY_FALLBACK_BINDS: [&str; 3] = [
    "keyword bindn , Escape, exec, wol shell hide transient",
    "keyword bindrt SUPER, SUPER_L, exec, wol shell hide switcher",
    "keyword bindrt SUPER, SUPER_R, exec, wol shell hide switcher",
];

/// From the `j/plugin list` reply: true when the fallback binds are needed (the compositor answered and
/// [`KEY_PLUGIN`] is not among the loaded plugins). No answer (not the compositor): nothing to do.
pub fn needs_key_fallback(reply: Option<&str>) -> bool {
    let Some(reply) = reply else {
        return false;
    };
    let loaded = serde_json::from_str::<serde_json::Value>(reply)
        .ok()
        .and_then(|v| {
            v.as_array()
                .map(|a| a.iter().any(|p| p["name"].as_str() == Some(KEY_PLUGIN)))
        })
        // Not JSON: "no plugins loaded" or an older spelling; look for the name.
        .unwrap_or_else(|| reply.contains(KEY_PLUGIN));
    !loaded
}

/// At shell start (off the UI thread): binds Esc and the Super release itself when the plugin is
/// missing. True when it did.
pub fn ensure_key_forwarding() -> bool {
    if !needs_key_fallback(request("j/plugin list").as_deref()) {
        return false;
    }
    eprintln!("wol-shell: {KEY_PLUGIN} is not loaded; Esc and the Super release spawn wol");
    for b in KEY_FALLBACK_BINDS {
        request(b);
    }
    true
}

/// One compositor dispatcher call (audit3 R2A-17: the only place `dispatch …` text is built).
/// Windows are named by selector (`address:0x…`, `class:^(…)$`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dispatch {
    FocusWindow(String),
    CloseWindow(String),
    KillWindow(String),
    SetFloating(String),
    /// `movetoworkspace` (`silent`: `movetoworkspacesilent`, the focus stays) of a window.
    MoveToWorkspace {
        workspace: String,
        window: String,
        silent: bool,
    },
    /// Switch to a workspace.
    Workspace(String),
    ResizeExact {
        w: i32,
        h: i32,
        window: String,
    },
    MoveExact {
        x: i32,
        y: i32,
        window: String,
    },
    /// Internal and client full-screen modes of the focused window (-1 = leave it).
    FullscreenState {
        internal: i8,
        client: i8,
    },
    /// A key tap to `window` ("" = whatever has the keyboard).
    SendShortcut {
        mods: String,
        key: String,
        window: String,
    },
}

impl Dispatch {
    /// The `.socket.sock` request.
    pub fn to_request(&self) -> String {
        let args = match self {
            Dispatch::FocusWindow(w) => format!("focuswindow {w}"),
            Dispatch::CloseWindow(w) => format!("closewindow {w}"),
            Dispatch::KillWindow(w) => format!("killwindow {w}"),
            Dispatch::SetFloating(w) => format!("setfloating {w}"),
            Dispatch::MoveToWorkspace {
                workspace,
                window,
                silent,
            } => {
                let silent = if *silent { "silent" } else { "" };
                format!("movetoworkspace{silent} {workspace},{window}")
            }
            Dispatch::Workspace(ws) => format!("workspace {ws}"),
            Dispatch::ResizeExact { w, h, window } => {
                format!("resizewindowpixel exact {w} {h},{window}")
            }
            Dispatch::MoveExact { x, y, window } => {
                format!("movewindowpixel exact {x} {y},{window}")
            }
            Dispatch::FullscreenState { internal, client } => {
                format!("fullscreenstate {internal} {client}")
            }
            Dispatch::SendShortcut { mods, key, window } if window.is_empty() => {
                format!("sendshortcut {mods}, {key},")
            }
            Dispatch::SendShortcut { mods, key, window } => {
                format!("sendshortcut {mods}, {key}, {window}")
            }
        };
        format!("dispatch {args}")
    }
}

/// One `[[BATCH]]` request for `steps`, in order; None when there is nothing to do.
pub fn batch(steps: &[Dispatch]) -> Option<String> {
    let all: Vec<String> = steps.iter().map(Dispatch::to_request).collect();
    (!all.is_empty()).then(|| format!("[[BATCH]]{}", all.join(";")))
}

/// Runs `d` ([`request`]: blocks up to [`REQUEST_TIMEOUT`], call it off the UI thread).
pub fn dispatch(d: &Dispatch) -> Option<String> {
    request(&d.to_request())
}

/// `j/activewindow` -> (class, title); ("", "") on the empty desktop (`{}`), None if unreadable.
pub fn parse_active(json: &str) -> Option<(String, String)> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
    Some((crate::appmenu::app_id(&s("class")).to_string(), s("title")))
}

/// `j/activewindow` as a client (address, class, size, tags); None on the empty desktop.
pub fn parse_active_client(json: &str) -> Option<Client> {
    serde_json::from_str::<Client>(json)
        .ok()
        .filter(|c| !c.address.is_empty())
}

/// `j/layers` -> namespaces of every mapped layer surface, all outputs and levels.
pub fn parse_layers(json: &str) -> Vec<String> {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let mut out = Vec::new();
    for mon in v.as_object().into_iter().flat_map(|m| m.values()) {
        for level in mon["levels"]
            .as_object()
            .into_iter()
            .flat_map(|l| l.values())
        {
            for l in level.as_array().into_iter().flatten() {
                if let Some(n) = l["namespace"].as_str() {
                    out.push(n.to_string());
                }
            }
        }
    }
    out
}

/// `j/monitors`: the focused output's logical box minus reserved areas (menu bar, dock).
/// `width`/`height` are the mode's pixels before the transform: a display turned by 90 or 270
/// degrees (odd `transform`) has them swapped in the logical layout.
pub fn parse_zone(json: &str) -> Option<(i32, i32, i32, i32)> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let m = v
        .as_array()?
        .iter()
        .find(|m| m["focused"].as_bool() == Some(true))?;
    let n = |k: &str| m[k].as_f64().unwrap_or_default();
    let r = |i: usize| m["reserved"][i].as_f64().unwrap_or_default();
    let scale = if n("scale") > 0.0 { n("scale") } else { 1.0 };
    let (mut w, mut h) = (n("width") / scale, n("height") / scale);
    if m["transform"].as_u64().unwrap_or_default() % 2 == 1 {
        std::mem::swap(&mut w, &mut h);
    }
    let i = |f: f64| f.round() as i32;
    Some((
        i(n("x") + r(0)),
        i(n("y") + r(1)),
        i(w - r(0) - r(2)),
        i(h - r(1) - r(3)),
    ))
}

/// How the active workspace's windows fill the display: the largest internal mode among them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Fullscreen {
    #[default]
    None,
    /// Zoomed into the usable area (the menu bar and the Dock stay).
    Maximized,
    /// Real full screen: a game, a video.
    Full,
}

impl Fullscreen {
    /// `j/clients` `fullscreen` -> the mode (anything unknown is none).
    pub fn from_mode(mode: i64) -> Self {
        match mode {
            1 => Fullscreen::Maximized,
            2 => Fullscreen::Full,
            _ => Fullscreen::None,
        }
    }

    /// Notification banners wait (audit3 R2A-9): the compositor draws a Top layer mapped after a window
    /// went full screen above it, and that layer stops the game's direct scanout. The brief volume
    /// OSD still shows, by decision (audit5 S6-3; scanout drops for its 1.5 s). A zoomed window
    /// leaves the menu bar visible and holds nothing.
    pub fn holds_banners(self) -> bool {
        self == Fullscreen::Full
    }
}

/// The mode of `workspace` (an id) from `j/clients`.
pub fn fullscreen_of(clients: &[Client], workspace: i64) -> Fullscreen {
    clients
        .iter()
        .filter(|c| c.workspace.id == workspace)
        .map(|c| Fullscreen::from_mode(c.fullscreen))
        .max()
        .unwrap_or_default()
}

/// `j/monitors` -> each output's name and the id of the workspace it shows (audit5 S6-2).
pub fn parse_monitor_workspaces(json: &str) -> Vec<(String, i64)> {
    let Ok(serde_json::Value::Array(ms)) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    ms.iter()
        .filter_map(|m| {
            Some((
                m["name"].as_str()?.to_string(),
                m["activeWorkspace"]["id"].as_i64()?,
            ))
        })
        .collect()
}

/// Each output's mode: the workspace it shows, from `j/clients` (audit5 S6-2).
pub fn fullscreen_by_monitor(
    clients: &[Client],
    monitors: &[(String, i64)],
) -> std::collections::HashMap<String, Fullscreen> {
    monitors
        .iter()
        .map(|(name, ws)| (name.clone(), fullscreen_of(clients, *ws)))
        .collect()
}

/// `j/activeworkspace` -> its id.
pub fn parse_workspace_id(json: &str) -> Option<i64> {
    serde_json::from_str::<serde_json::Value>(json).ok()?["id"].as_i64()
}

/// Events after which the active workspace may have gained or lost a full-screen window.
pub fn fullscreen_may_change(event_line: &str) -> bool {
    let name = event_line.split(">>").next().unwrap_or("");
    matches!(
        name,
        "fullscreen"
            | "workspace"
            | "workspacev2"
            | "focusedmon"
            | "focusedmonv2"
            | "activespecial"
            | "openwindow"
            | "closewindow"
            | "movewindow"
            | "movewindowv2"
    )
}

/// The active workspace's mode now (two requests: run it off the UI thread); none when the compositor
/// does not answer (banners then show, as before).
pub fn read_fullscreen() -> Fullscreen {
    active_workspace().map_or(Fullscreen::None, |ws| fullscreen_of(&clients(), ws))
}

/// Every output's mode now ([`fullscreen_by_monitor`]; off the UI thread); empty without an answer.
pub fn read_monitor_fullscreen() -> std::collections::HashMap<String, Fullscreen> {
    let Some(mons) = request("j/monitors").map(|j| parse_monitor_workspaces(&j)) else {
        return Default::default();
    };
    fullscreen_by_monitor(&clients(), &mons)
}

type OnFullscreen = std::rc::Rc<dyn Fn(Fullscreen)>;
type OnMonitors = std::rc::Rc<dyn Fn()>;
type OnConfig = std::rc::Rc<dyn Fn()>;

thread_local! {
    /// Kept by the shell's one compositor event reader (`ui::ipc::hypr_events`).
    static FULLSCREEN: std::cell::Cell<Fullscreen> = const { std::cell::Cell::new(Fullscreen::None) };
    static ON_FULLSCREEN: std::cell::RefCell<Vec<OnFullscreen>> = Default::default();
    /// Per output (audit5 S6-2): desktop widgets follow their own display.
    static MONITOR_FULLSCREEN: std::cell::RefCell<std::collections::HashMap<String, Fullscreen>> = Default::default();
    static ON_MONITORS: std::cell::RefCell<Vec<OnMonitors>> = Default::default();
    /// Who must start again after the compositor reloaded its config (it drops every `keyword` bind).
    static ON_CONFIG: std::cell::RefCell<Vec<OnConfig>> = Default::default();
}

/// Calls `f` each time the compositor reloads its config. The hook stays for the session: one that
/// belongs to something that can go away holds it weakly and does nothing once it is gone.
pub fn on_config_reloaded(f: impl Fn() + 'static) {
    ON_CONFIG.with_borrow_mut(|v| v.push(std::rc::Rc::new(f)));
}

/// The compositor reloaded its config (`configreloaded`, from the one event reader): runs the hooks.
pub fn config_reloaded() {
    // A hook may register another while it runs: call a snapshot.
    let hooks: Vec<OnConfig> = ON_CONFIG.with_borrow(|v| v.clone());
    hooks.iter().for_each(|f| f());
}

/// Output `name`'s mode as last read; the focused workspace's ([`fullscreen_state`]) when the
/// output is unknown (no `j/monitors` answer yet, or no name).
pub fn monitor_fullscreen(name: Option<&str>) -> Fullscreen {
    name.and_then(|n| MONITOR_FULLSCREEN.with_borrow(|m| m.get(n).copied()))
        .unwrap_or_else(fullscreen_state)
}

/// Calls `f` each time some output's mode changes.
pub fn on_monitor_fullscreen_change(f: impl Fn() + 'static) {
    ON_MONITORS.with_borrow_mut(|v| v.push(std::rc::Rc::new(f)));
}

/// Records every output's mode just read; the hooks run when it differs. True when it changed.
pub fn set_monitor_fullscreen(modes: std::collections::HashMap<String, Fullscreen>) -> bool {
    if MONITOR_FULLSCREEN.with_borrow(|m| *m == modes) {
        return false;
    }
    MONITOR_FULLSCREEN.set(modes);
    let hooks: Vec<OnMonitors> = ON_MONITORS.with_borrow(|v| v.clone());
    hooks.iter().for_each(|f| f());
    true
}

/// The active workspace's mode as last read (UI thread; no request).
pub fn fullscreen_state() -> Fullscreen {
    FULLSCREEN.get()
}

/// Calls `f` with the new mode each time it changes (held banners replay on leaving full screen).
pub fn on_fullscreen_change(f: impl Fn(Fullscreen) + 'static) {
    ON_FULLSCREEN.with_borrow_mut(|v| v.push(std::rc::Rc::new(f)));
}

/// Records the mode just read; the change hooks run when it differs. True when it changed.
pub fn set_fullscreen_state(mode: Fullscreen) -> bool {
    if FULLSCREEN.replace(mode) == mode {
        return false;
    }
    // A hook may register another while it runs: call a snapshot.
    let hooks: Vec<OnFullscreen> = ON_FULLSCREEN.with_borrow(|v| v.clone());
    hooks.iter().for_each(|f| f(mode));
    true
}

/// Running classes, lowercase, for the dock's running dot.
pub fn running_classes(clients: &[Client]) -> std::collections::HashSet<String> {
    clients
        .iter()
        .map(|c| crate::appmenu::app_id(&c.class).to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    /// Stability S4: the event socket is found without the instance in the environment, and
    /// only once it is there; the instance named in the environment wins.
    #[test]
    fn the_event_socket_is_found_once_it_appears() {
        let rt = std::env::temp_dir().join(format!("wol-hypr-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&rt);
        let rts = rt.to_str().unwrap().to_string();
        let var = |sig: Option<&'static str>| {
            let rts = rts.clone();
            move |k: &str| match k {
                "XDG_RUNTIME_DIR" => Some(rts.clone()),
                "HYPRLAND_INSTANCE_SIGNATURE" => sig.map(String::from),
                _ => None,
            }
        };
        let name = ".socket2.sock";
        assert_eq!(
            super::find_socket(var(None), name),
            None,
            "no runtime folder yet"
        );
        std::fs::create_dir_all(rt.join("hypr/old")).unwrap();
        assert_eq!(super::find_socket(var(None), name), None, "no socket yet");
        std::fs::write(rt.join("hypr/old").join(name), "").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::create_dir_all(rt.join("hypr/new")).unwrap();
        std::fs::write(rt.join("hypr/new").join(name), "").unwrap();
        assert_eq!(
            super::find_socket(var(None), name),
            Some(rt.join("hypr/new").join(name)),
            "the newest instance"
        );
        assert_eq!(
            super::find_socket(var(Some("old")), name),
            Some(rt.join("hypr/old").join(name)),
            "the named instance"
        );
        let relative = |k: &str| (k == "XDG_RUNTIME_DIR").then(|| "run".to_string());
        assert_eq!(super::find_socket(relative, name), None);
        let _ = std::fs::remove_dir_all(&rt);
    }

    #[test]
    fn urgent_names_the_window_address() {
        assert_eq!(
            parse_event("urgent>>5a1c\n"),
            Some(Event::Urgent {
                address: "0x5a1c".into()
            })
        );
    }

    use super::*;

    #[test]
    fn a_config_reload_runs_every_hook_once() {
        let n = std::rc::Rc::new(std::cell::Cell::new(0));
        for add in [1, 10] {
            let n = n.clone();
            on_config_reloaded(move || n.set(n.get() + add));
        }
        config_reloaded();
        assert_eq!(n.get(), 11);
    }

    #[test]
    fn zone_is_the_focused_output_minus_reserved() {
        let j = r#"[{"x":0,"y":0,"width":2560,"height":1600,"scale":2.0,"focused":false,"reserved":[0,0,0,0]},
                    {"x":1280,"y":0,"width":2560,"height":1600,"scale":2.0,"focused":true,"reserved":[0,28,0,102]}]"#;
        assert_eq!(parse_zone(j), Some((1280, 28, 1280, 670)));
        assert_eq!(parse_zone("[]"), None);
        // A portrait display (transform 1 = 90 degrees): the mode is 1920x1080, the layout 1080x1920.
        let turned = r#"[{"x":0,"y":0,"width":1920,"height":1080,"scale":1.0,"transform":1,"focused":true,"reserved":[0,28,0,0]}]"#;
        assert_eq!(parse_zone(turned), Some((0, 28, 1080, 1892)));
        let flipped = turned.replace(r#""transform":1"#, r#""transform":6"#);
        assert_eq!(
            parse_zone(&flipped),
            Some((0, 28, 1920, 1052)),
            "180 + flip: not swapped"
        );
        let a = parse_active_client(
            r#"{"address":"0xa","class":"foot","title":"t","size":[4,3],"tags":["wol-csd"]}"#,
        )
        .unwrap();
        assert_eq!((a.size, a.tags), ([4, 3], vec!["wol-csd".to_string()]));
        assert_eq!(parse_active_client("{}"), None);
    }

    #[test]
    fn parses_events() {
        assert_eq!(
            parse_event("activewindow>>foot,~/src, ok\n"),
            Some(Event::ActiveWindow {
                class: "foot".into(),
                title: "~/src, ok".into()
            })
        );
        assert_eq!(
            parse_event("openwindow>>55a1,1,atlas,Atlas"),
            Some(Event::OpenWindow {
                address: "55a1".into(),
                class: "atlas".into()
            })
        );
        assert_eq!(
            parse_event("closewindow>>55a1"),
            Some(Event::CloseWindow {
                address: "55a1".into()
            })
        );
        assert_eq!(
            parse_event("activewindow>>,"),
            Some(Event::ActiveWindow {
                class: "".into(),
                title: "".into()
            })
        );
        assert_eq!(
            parse_event("focusedmon>>HDMI-A-1,2"),
            Some(Event::FocusedMonitor("HDMI-A-1".into()))
        );
        assert_eq!(parse_event("garbage"), None);
        // Audit5 S9-4: a config reload wipes the runtime binds; the shell replays them.
        assert_eq!(parse_event("configreloaded>>"), Some(Event::ConfigReloaded));
    }

    /// Live: a restarted shell showed "Files" while a window was focused (no event until a change).
    #[test]
    fn parses_active_window() {
        for (json, want) in [
            (r#"{"class":"foot","title":"~"}"#, Some(("foot", "~"))),
            ("{}", Some(("", ""))),
            ("Invalid", None),
        ] {
            let got = parse_active(json);
            assert_eq!(
                got.as_ref().map(|(c, t)| (c.as_str(), t.as_str())),
                want,
                "{json}"
            );
        }
    }

    /// Audit3 R2A-9: only real full screen on the active workspace holds banners; a zoomed
    /// window or a game on another workspace does not.
    #[test]
    fn the_active_workspace_in_full_screen_holds_banners() {
        let j = r#"[{"address":"0x1","class":"steam_app_1","title":"","workspace":{"id":2,"name":"2"},"fullscreen":2},
                    {"address":"0x2","class":"foot","title":"","workspace":{"id":1,"name":"1"},"fullscreen":1},
                    {"address":"0x3","class":"files","title":"","workspace":{"id":1,"name":"1"}}]"#;
        let c = parse_clients(j);
        assert_eq!(fullscreen_of(&c, 2), Fullscreen::Full);
        assert_eq!(fullscreen_of(&c, 1), Fullscreen::Maximized);
        assert_eq!(fullscreen_of(&c, 3), Fullscreen::None);
        assert!(fullscreen_of(&c, 2).holds_banners());
        assert!(
            !fullscreen_of(&c, 1).holds_banners(),
            "zoomed: the bar shows"
        );
        assert!(!Fullscreen::from_mode(7).holds_banners());
        assert_eq!(parse_workspace_id(r#"{"id":2,"name":"2"}"#), Some(2));
        assert_eq!(parse_workspace_id("{}"), None);
        for (line, may) in [
            ("fullscreen>>1", true),
            ("workspace>>2", true),
            ("closewindow>>55a1", true),
            ("workspacev2>>2,2", true),
            ("activewindow>>foot,~", false),
            ("openlayer>>wol-desktop", false),
            ("garbage", false),
        ] {
            assert_eq!(fullscreen_may_change(line), may, "{line}");
        }
    }

    /// Audit3 R2A-17: every dispatch the shell sends is spelled here, as the compositor reads it.
    #[test]
    fn dispatches_render_as_hyprland_reads_them() {
        let a = || "address:0xa".to_string();
        for (d, want) in [
            (
                Dispatch::FocusWindow(a()),
                "dispatch focuswindow address:0xa",
            ),
            (
                Dispatch::CloseWindow(a()),
                "dispatch closewindow address:0xa",
            ),
            (Dispatch::KillWindow(a()), "dispatch killwindow address:0xa"),
            (
                Dispatch::SetFloating(a()),
                "dispatch setfloating address:0xa",
            ),
            (
                Dispatch::MoveToWorkspace {
                    workspace: "special:hidden".into(),
                    window: a(),
                    silent: true,
                },
                "dispatch movetoworkspacesilent special:hidden,address:0xa",
            ),
            (
                Dispatch::MoveToWorkspace {
                    workspace: "2".into(),
                    window: a(),
                    silent: false,
                },
                "dispatch movetoworkspace 2,address:0xa",
            ),
            (Dispatch::Workspace("3".into()), "dispatch workspace 3"),
            (
                Dispatch::ResizeExact {
                    w: 800,
                    h: 600,
                    window: a(),
                },
                "dispatch resizewindowpixel exact 800 600,address:0xa",
            ),
            (
                Dispatch::MoveExact {
                    x: 0,
                    y: 28,
                    window: a(),
                },
                "dispatch movewindowpixel exact 0 28,address:0xa",
            ),
            (
                Dispatch::FullscreenState {
                    internal: 1,
                    client: -1,
                },
                "dispatch fullscreenstate 1 -1",
            ),
            (
                Dispatch::SendShortcut {
                    mods: "CTRL".into(),
                    key: "C".into(),
                    window: String::new(),
                },
                "dispatch sendshortcut CTRL, C,",
            ),
            (
                Dispatch::SendShortcut {
                    mods: "CTRL".into(),
                    key: "comma".into(),
                    window: a(),
                },
                "dispatch sendshortcut CTRL, comma, address:0xa",
            ),
        ] {
            assert_eq!(d.to_request(), want);
        }
        assert_eq!(batch(&[]), None);
        assert_eq!(
            batch(&[Dispatch::FocusWindow(a()), Dispatch::KillWindow(a())]).as_deref(),
            Some("[[BATCH]]dispatch focuswindow address:0xa;dispatch killwindow address:0xa")
        );
    }

    /// A broken or missing wol-hypr must not leave panels unclosable: the shell binds the keys.
    #[test]
    fn the_key_binds_come_back_only_without_the_plugin() {
        let loaded =
            r#"[{"name":"hyprbars","version":"1"},{"name":"wol-hypr","version":"0.12.0"}]"#;
        assert!(!needs_key_fallback(Some(loaded)));
        assert!(needs_key_fallback(Some(r#"[{"name":"hyprbars"}]"#)));
        assert!(needs_key_fallback(Some("[]")));
        assert!(needs_key_fallback(Some("no plugins loaded")));
        assert!(!needs_key_fallback(None), "no compositor: nothing to bind");
        assert!(KEY_FALLBACK_BINDS[0].contains("Escape, exec, wol shell hide transient"));
        assert!(
            KEY_FALLBACK_BINDS[1..]
                .iter()
                .all(|b| b.contains("hide switcher"))
        );
    }

    #[test]
    fn fullscreen_changes_reach_the_hooks_once() {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let s = seen.clone();
        on_fullscreen_change(move |m| s.borrow_mut().push(m));
        assert!(set_fullscreen_state(Fullscreen::Full));
        assert!(!set_fullscreen_state(Fullscreen::Full), "no change");
        assert_eq!(fullscreen_state(), Fullscreen::Full);
        assert!(set_fullscreen_state(Fullscreen::None));
        assert_eq!(*seen.borrow(), [Fullscreen::Full, Fullscreen::None]);
    }

    #[test]
    fn fullscreen_is_per_monitor() {
        let clients = parse_clients(
            r#"[{"address":"0x1","class":"atlas","title":"t","workspace":{"id":1},"fullscreen":1}]"#,
        );
        let mons = parse_monitor_workspaces(
            r#"[{"name":"DP-1","activeWorkspace":{"id":1,"name":"1"}},
                {"name":"HDMI-A-1","activeWorkspace":{"id":2,"name":"2"}}]"#,
        );
        assert_eq!(mons, [("DP-1".into(), 1), ("HDMI-A-1".into(), 2)]);
        let by = fullscreen_by_monitor(&clients, &mons);
        assert_eq!(by["DP-1"], Fullscreen::Maximized);
        assert_eq!(by["HDMI-A-1"], Fullscreen::None);
        let seen = std::rc::Rc::new(std::cell::Cell::new(0));
        let s = seen.clone();
        on_monitor_fullscreen_change(move || s.set(s.get() + 1));
        assert!(set_monitor_fullscreen(by.clone()));
        assert!(!set_monitor_fullscreen(by), "no change");
        assert_eq!(seen.get(), 1);
        assert_eq!(monitor_fullscreen(Some("HDMI-A-1")), Fullscreen::None);
        assert_eq!(monitor_fullscreen(Some("DP-1")), Fullscreen::Maximized);
        assert_eq!(
            monitor_fullscreen(Some("gone")),
            fullscreen_state(),
            "unknown: focused"
        );
        assert!(parse_monitor_workspaces("nope").is_empty());
    }

    #[test]
    fn a_glass_window_is_its_apps_window_in_events_and_replies() {
        assert_eq!(
            parse_event("activewindow>>org.wolos.todos.glass,Todos"),
            Some(Event::ActiveWindow {
                class: "org.wolos.todos".into(),
                title: "Todos".into()
            })
        );
        assert_eq!(
            parse_event("openwindow>>0xa,1,org.wolos.todos.glass,Todos"),
            Some(Event::OpenWindow {
                address: "0xa".into(),
                class: "org.wolos.todos".into()
            })
        );
        assert_eq!(
            parse_active(r#"{"class":"org.wolos.todos.glass","title":"Todos"}"#),
            Some(("org.wolos.todos".into(), "Todos".into()))
        );
    }

    #[test]
    fn parses_clients_json() {
        let c = parse_clients(r#"[{"address":"0x1","class":"Foot","title":"t","pid":3}]"#);
        assert_eq!(c[0].class, "Foot");
        assert!(running_classes(&c).contains("foot"));
        // A window that asked for glass is its app's window (the id carries `.glass`).
        let g = parse_clients(r#"[{"address":"0x1","class":"org.wolos.todos.glass","title":"t"}]"#);
        assert!(running_classes(&g).contains("org.wolos.todos"));
        assert!(parse_clients("not json").is_empty());
        let c = parse_clients(
            r#"[{"address":"0x2","class":"a","title":"","focusHistoryID":1,"workspace":{"id":2,"name":"2"},"at":[10,20],"size":[300,200]}]"#,
        );
        assert_eq!(
            (c[0].focus, c[0].workspace.id, c[0].at, c[0].size),
            (1, 2, [10, 20], [300, 200])
        );
    }
}
