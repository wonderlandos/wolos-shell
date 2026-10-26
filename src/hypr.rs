//! Hyprland IPC: `.socket2.sock` events (event-driven, no polling) and `j/clients` on `.socket.sock`.
//! Replace with the `realtime` crate's client when it lands.
use std::io::{Read, Write};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    ActiveWindow { class: String, title: String },
    OpenWindow { address: String, class: String },
    CloseWindow { address: String },
    Workspace(String),
    Other,
}

/// One `EVENT>>DATA` line.
pub fn parse_event(line: &str) -> Option<Event> {
    let (name, data) = line.trim_end().split_once(">>")?;
    Some(match name {
        "activewindow" => {
            let (class, title) = data.split_once(',').unwrap_or((data, ""));
            Event::ActiveWindow {
                class: class.into(),
                title: title.into(),
            }
        }
        "openwindow" => {
            let mut p = data.splitn(4, ',');
            let address = p.next()?.into();
            let _ws = p.next()?;
            Event::OpenWindow {
                address,
                class: p.next()?.into(),
            }
        }
        "closewindow" => Event::CloseWindow {
            address: data.into(),
        },
        "workspace" => Event::Workspace(data.into()),
        _ => Event::Other,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub struct Client {
    pub address: String,
    pub class: String,
    pub title: String,
    /// 0 = most recently focused.
    #[serde(rename = "focusHistoryID", default = "far")]
    pub focus: i64,
    #[serde(default)]
    pub workspace: WorkspaceRef,
    #[serde(default)]
    pub at: [i32; 2],
    #[serde(default)]
    pub size: [i32; 2],
}

fn far() -> i64 {
    i64::MAX
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
        }
    }
}

pub fn parse_clients(json: &str) -> Vec<Client> {
    serde_json::from_str(json).unwrap_or_default()
}

/// `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/<name>`; None outside Hyprland.
pub fn socket(name: &str) -> Option<PathBuf> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let rt = std::env::var("XDG_RUNTIME_DIR").ok()?;
    Some(PathBuf::from(rt).join("hypr").join(sig).join(name))
}

/// One request on `.socket.sock`.
pub fn request(cmd: &str) -> Option<String> {
    let mut s = std::os::unix::net::UnixStream::connect(socket(".socket.sock")?).ok()?;
    s.write_all(cmd.as_bytes()).ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    Some(out)
}

pub fn clients() -> Vec<Client> {
    request("j/clients")
        .map(|j| parse_clients(&j))
        .unwrap_or_default()
}

/// Running classes, lowercase, for the dock's running dot.
pub fn running_classes(clients: &[Client]) -> std::collections::HashSet<String> {
    clients.iter().map(|c| c.class.to_lowercase()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
            parse_event("openwindow>>55a1,1,firefox,Mozilla"),
            Some(Event::OpenWindow {
                address: "55a1".into(),
                class: "firefox".into()
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
        assert_eq!(parse_event("garbage"), None);
    }

    #[test]
    fn parses_clients_json() {
        let c = parse_clients(r#"[{"address":"0x1","class":"Foot","title":"t","pid":3}]"#);
        assert_eq!(c[0].class, "Foot");
        assert!(running_classes(&c).contains("foot"));
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
