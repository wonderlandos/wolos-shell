//! Kit host protocol (ADR 0020), one app connection: the app names itself first
//! ([`ToHost::Hello`]), then shows, updates and closes its windows. Pure rules here; drawing is
//! `ui::app_host`. Anything out of order ends the connection; nothing an app sends can reach
//! another app's windows (one host process per connection).
use shared::view::{Menu, Node, ToHost, Toolbar};
use std::io::{BufRead, Read};
use std::path::PathBuf;

/// Socket the shell listens on, under `$XDG_RUNTIME_DIR` (libwol: `wol::client::kit_socket`).
pub const SOCKET: &str = "wol-kit.sock";
/// Override for tests and the dev session (same name as libwol's).
pub const SOCKET_ENV: &str = "WOL_KIT_SOCKET";
/// Windows one app may have open.
pub const MAX_WINDOWS: usize = 16;
/// Longest message line: a full tree (`shared::view::MAX_NODES` parts of `MAX_TEXT`).
pub const MAX_LINE: usize = 256 * 1024;

pub fn socket_path() -> Option<PathBuf> {
    std::env::var_os(SOCKET_ENV)
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_RUNTIME_DIR").map(|d| PathBuf::from(d).join(SOCKET)))
}

/// What the host does for one message.
#[derive(Debug, PartialEq)]
pub enum Step {
    Open {
        window: u32,
        title: String,
        content: Node,
        menus: Vec<Menu>,
        toolbar: Option<Toolbar>,
    },
    Update {
        window: u32,
        title: String,
        content: Node,
        menus: Vec<Menu>,
        toolbar: Option<Toolbar>,
    },
    Close {
        window: u32,
    },
    /// Nothing to draw (closing a window that is not open).
    Nothing,
}

#[derive(Default)]
pub struct Session {
    app: Option<String>,
    open: Vec<u32>,
}

impl Session {
    pub fn app(&self) -> Option<&str> {
        self.app.as_deref()
    }

    /// One parsed message; Err ends the connection with that reason.
    pub fn on(&mut self, m: ToHost) -> Result<Step, &'static str> {
        match (m, self.app.is_some()) {
            (ToHost::Hello { app }, false) => {
                self.app = Some(app);
                Ok(Step::Nothing)
            }
            (ToHost::Hello { .. }, true) => Err("hello twice"),
            (_, false) => Err("no hello first"),
            (
                ToHost::Show {
                    window,
                    title,
                    content,
                    menus,
                    toolbar,
                },
                true,
            ) => {
                if self.open.contains(&window) {
                    return Ok(Step::Update {
                        window,
                        title,
                        content,
                        menus,
                        toolbar,
                    });
                }
                if self.open.len() >= MAX_WINDOWS {
                    return Err("too many windows");
                }
                self.open.push(window);
                Ok(Step::Open {
                    window,
                    title,
                    content,
                    menus,
                    toolbar,
                })
            }
            (ToHost::Close { window }, true) => Ok(if self.forget(window) {
                Step::Close { window }
            } else {
                Step::Nothing
            }),
        }
    }

    /// The person closed `window` (the app hears [`shared::view::FromHost::Closed`]).
    pub fn forget(&mut self, window: u32) -> bool {
        let had = self.open.contains(&window);
        self.open.retain(|w| *w != window);
        had
    }
}

/// Next line, at most [`MAX_LINE`] bytes; None at the end, on an error or a longer line.
pub fn read_line(r: &mut impl BufRead) -> Option<String> {
    let mut buf = Vec::new();
    let n = r
        .by_ref()
        .take(MAX_LINE as u64 + 1)
        .read_until(b'\n', &mut buf)
        .ok()?;
    if n == 0 || n > MAX_LINE {
        return None;
    }
    String::from_utf8(buf).ok()
}

/// A `shared::view::shortcut_ok` shortcut as a GTK accelerator. `cmd` is Control: the key Command
/// hands to the app in front (`appmenu::Cmd`).
pub fn accelerator(shortcut: &str) -> Option<String> {
    if !shared::view::shortcut_ok(shortcut) {
        return None;
    }
    let mut out = String::new();
    let (mods, key) = shortcut.rsplit_once('+').unwrap_or(("", shortcut));
    for m in mods.split('+').filter(|m| !m.is_empty()) {
        out.push_str(match m {
            "cmd" => "<Control>",
            "shift" => "<Shift>",
            _ => "<Alt>",
        });
    }
    out.push_str(&if key.len() > 1 {
        key.to_uppercase()
    } else {
        key.to_owned()
    });
    Some(out)
}

/// GTK application id for the app's windows (the compositor's window class): the app's own id
/// when GLib accepts it.
pub fn gtk_app_id(app: &str) -> Option<&str> {
    let ok = app.split('.').count() >= 2
        && app.split('.').all(|e| {
            !e.is_empty()
                && !e.starts_with(|c: char| c.is_ascii_digit())
                && e.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
        && app.len() <= 255;
    ok.then_some(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn show(window: u32) -> ToHost {
        ToHost::Show {
            window,
            title: "T".into(),
            content: Node::Spacer,
            menus: Vec::new(),
            toolbar: None,
        }
    }

    #[test]
    fn hello_then_windows() {
        let mut s = Session::default();
        assert_eq!(s.on(show(1)), Err("no hello first"));
        let hello = || ToHost::Hello {
            app: "org.example.a".into(),
        };
        assert_eq!(s.on(hello()), Ok(Step::Nothing));
        assert_eq!(s.app(), Some("org.example.a"));
        assert_eq!(s.on(hello()), Err("hello twice"));
        assert!(matches!(s.on(show(1)), Ok(Step::Open { window: 1, .. })));
        assert!(matches!(s.on(show(1)), Ok(Step::Update { window: 1, .. })));
        assert_eq!(
            s.on(ToHost::Close { window: 1 }),
            Ok(Step::Close { window: 1 })
        );
        assert_eq!(s.on(ToHost::Close { window: 1 }), Ok(Step::Nothing));
        assert!(
            matches!(s.on(show(1)), Ok(Step::Open { .. })),
            "shown again"
        );
        assert!(s.forget(1));
        for w in 0..MAX_WINDOWS as u32 {
            assert!(s.on(show(10 + w)).is_ok());
        }
        assert_eq!(s.on(show(99)), Err("too many windows"));
        assert!(
            matches!(s.on(show(10)), Ok(Step::Update { .. })),
            "updates still go"
        );
    }

    #[test]
    fn lines_are_bounded() {
        let mut r = std::io::BufReader::new(&b"a\nb"[..]);
        assert_eq!(read_line(&mut r).as_deref(), Some("a\n"));
        assert_eq!(read_line(&mut r).as_deref(), Some("b"));
        assert_eq!(read_line(&mut r), None);
        let long = vec![b'x'; MAX_LINE + 1];
        assert_eq!(read_line(&mut std::io::BufReader::new(&long[..])), None);
    }

    #[test]
    fn accelerators() {
        for (s, want) in [
            ("cmd+s", Some("<Control>s")),
            ("cmd+shift+n", Some("<Control><Shift>n")),
            ("alt+f4", Some("<Alt>F4")),
            ("f5", Some("F5")),
            ("ctrl+s", None),
            ("<Shift>s", None),
        ] {
            assert_eq!(accelerator(s).as_deref(), want, "{s}");
        }
    }

    #[test]
    fn gtk_ids() {
        assert_eq!(gtk_app_id("org.example.hello"), Some("org.example.hello"));
        assert_eq!(gtk_app_id("org.1x.y"), None, "GLib: no digit first");
        assert_eq!(gtk_app_id("hello"), None);
    }
}
