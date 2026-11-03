// SPDX-License-Identifier: GPL-2.0-or-later
//! Shell socket (port of Iara.Core.Shell.ShellCommand): one line `<verb> [panel]`, e.g. `toggle switcher`.
//! Path `$XDG_RUNTIME_DIR/wol-shell.sock`; sent by `wol shell ...` and keybinds. `approve <json>`
//! comes from `wol mcp` (helper approvals, `ui::approval`). `call <plugin> <fn> <json>` runs a
//! plugin's `[[ipc]]` function and `status` describes the shell; both answer one JSON line
//! ([`reply_ok`], [`reply_err`], [`status_json`]) on the same connection.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    Toggle,
    Show,
    Hide,
    Reload,
    /// `approve <json>`: a helper asks for an administrator change ([`shared::shell::Approval`]);
    /// the target is the JSON, verbatim. Never logged (a Wi-Fi password may be in it).
    Approve,
    /// `call <plugin> <fn> [json]`: a plugin's `[[ipc]]` function; the target is the rest of
    /// the line, read with [`Call::of`]. Answers one JSON line.
    Call,
    /// `status`: surfaces, panels and their open state, as one JSON line.
    Status,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub verb: Verb,
    pub target: String,
}

pub const SOCKET: &str = "wol-shell.sock";

pub fn socket_path(runtime_dir: &str) -> std::path::PathBuf {
    std::path::Path::new(runtime_dir).join(SOCKET)
}

/// The parts of a `call` line.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub plugin: String,
    pub entry: String,
    /// `null` when the line has none.
    pub args: serde_json::Value,
}

impl Call {
    /// `<plugin> <fn> [json]` (the target of a [`Verb::Call`]): ids checked like the manifest's,
    /// the JSON parsed. None: not a call line.
    pub fn of(target: &str) -> Option<Call> {
        let mut w = target.trim().splitn(3, ' ');
        let plugin = w.next()?;
        let entry = w.next()?;
        if !shared::plugins::is_plugin_id(plugin) || !shared::plugins::is_entry_id(entry) {
            return None;
        }
        let args = match w.next().map(str::trim).filter(|j| !j.is_empty()) {
            None => serde_json::Value::Null,
            Some(j) => serde_json::from_str(j).ok()?,
        };
        Some(Call {
            plugin: plugin.into(),
            entry: entry.into(),
            args,
        })
    }

    /// The wire line (newline included).
    pub fn line(&self) -> String {
        format!("call {} {} {}\n", self.plugin, self.entry, self.args)
    }
}

/// `{"ok":true,"value":...}`: what a `call` answered (one line, newline included).
pub fn reply_ok(value: serde_json::Value) -> String {
    format!("{}\n", serde_json::json!({"ok": true, "value": value}))
}

/// `{"ok":false,"error":"..."}`: why a `call` or `status` did not answer.
pub fn reply_err(sentence: &str) -> String {
    format!("{}\n", serde_json::json!({"ok": false, "error": sentence}))
}

/// One shell surface for [`status_json`]: its layer namespace (`wol-menubar`, `wol-panel-...`)
/// and whether it is on screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Surface {
    pub namespace: String,
    pub visible: bool,
}

/// The `status` answer: panels by name with their open state (sorted), every surface, and the
/// parts that stopped (`kit::fence`). One line, newline included.
pub fn status_json(panels: &[(String, bool)], surfaces: &[Surface], stopped: &[String]) -> String {
    let mut panels = panels.to_vec();
    panels.sort();
    let panels: Vec<_> = panels
        .iter()
        .map(|(name, open)| serde_json::json!({"name": name, "open": open}))
        .collect();
    let surfaces: Vec<_> = surfaces
        .iter()
        .map(|s| serde_json::json!({"namespace": s.namespace, "visible": s.visible}))
        .collect();
    format!(
        "{}\n",
        serde_json::json!({"ok": true, "panels": panels, "surfaces": surfaces, "stopped": stopped})
    )
}

pub fn parse(line: &str) -> Option<Command> {
    if let Some(rest) = line.trim_start().strip_prefix("call ") {
        let rest = rest.trim();
        return Call::of(rest).map(|_| Command {
            verb: Verb::Call,
            target: rest.into(),
        });
    }
    if let Some(json) = line.trim_start().strip_prefix("approve ") {
        let json = json.trim();
        return (!json.is_empty()).then(|| Command {
            verb: Verb::Approve,
            target: json.into(),
        });
    }
    let mut w = line.split_whitespace();
    let verb = match w.next()?.to_ascii_lowercase().as_str() {
        "toggle" => Verb::Toggle,
        "show" => Verb::Show,
        "hide" => Verb::Hide,
        "reload" => Verb::Reload,
        "status" => Verb::Status,
        _ => return None,
    };
    let target = w.next().map(str::to_ascii_lowercase);
    if w.next().is_some() {
        return None;
    }
    match (verb, target) {
        (Verb::Reload | Verb::Status, None) => Some(Command {
            verb,
            target: String::new(),
        }),
        (Verb::Reload | Verb::Status, Some(_)) | (_, None) => None,
        (_, Some(target)) => Some(Command { verb, target }),
    }
}

/// New visibility for a panel.
pub fn apply(verb: Verb, visible: bool) -> bool {
    match verb {
        Verb::Show => true,
        Verb::Hide => false,
        _ => !visible,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lines() {
        assert_eq!(
            parse("toggle Switcher"),
            Some(Command {
                verb: Verb::Toggle,
                target: "switcher".into()
            })
        );
        assert_eq!(parse(" show notifications \n").unwrap().verb, Verb::Show);
        assert_eq!(parse("reload").unwrap().target, "");
        for bad in ["", "toggle", "reload x", "fly osd", "show a b"] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    /// `approve <json>`: a helper's admin request for the approval sheet; JSON kept verbatim.
    #[test]
    fn approve_keeps_the_request_verbatim() {
        let j = r#"{"helper":"Claude Desktop","tool":"wifi join","request":{"action":"wifi-join","ssid":"My Home"}}"#;
        let c = parse(&format!("approve {j}\n")).unwrap();
        assert_eq!((c.verb, c.target.as_str()), (Verb::Approve, j));
        assert_eq!(parse("approve"), None);
        assert_eq!(parse("approve   "), None);
    }

    /// `call <plugin> <fn> [json]`: ids checked, JSON parsed (spaces inside it kept), none = null.
    #[test]
    fn call_lines_name_a_plugin_function_and_one_json_value() {
        let c = parse("call alice.music volume {\"level\": 40, \"fade\": true}\n").unwrap();
        assert_eq!(c.verb, Verb::Call);
        let call = Call::of(&c.target).unwrap();
        assert_eq!(
            call,
            Call {
                plugin: "alice.music".into(),
                entry: "volume".into(),
                args: serde_json::json!({"level": 40, "fade": true}),
            }
        );
        assert_eq!(Call::of(&parse(&call.line()).unwrap().target), Some(call));
        let bare = Call::of(&parse("call alice.music next").unwrap().target).unwrap();
        assert_eq!(bare.args, serde_json::Value::Null);
        assert_eq!(bare.line(), "call alice.music next null\n");
        for bad in [
            "call",
            "call alice.music",
            "call Alice.Music next",
            "call music next",
            "call alice.music Next",
            "call alice.music next {nope",
            "call alice.music next 1 2",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn status_takes_no_target_and_answers_one_line() {
        assert_eq!(parse("status").unwrap().verb, Verb::Status);
        assert_eq!(parse("status dock"), None);
        let s = status_json(
            &[("dock".into(), false), ("control-center".into(), true)],
            &[Surface {
                namespace: "wol-menubar".into(),
                visible: true,
            }],
            &["clipboard".into()],
        );
        assert!(s.ends_with('\n') && s.matches('\n').count() == 1);
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "ok": true,
                "panels": [{"name": "control-center", "open": true}, {"name": "dock", "open": false}],
                "surfaces": [{"namespace": "wol-menubar", "visible": true}],
                "stopped": ["clipboard"],
            })
        );
        let one = |line: String| -> serde_json::Value {
            assert!(line.ends_with('\n') && line.matches('\n').count() == 1);
            serde_json::from_str(&line).unwrap()
        };
        assert_eq!(
            one(reply_ok(serde_json::json!({"track": 2}))),
            serde_json::json!({"ok": true, "value": {"track": 2}})
        );
        assert_eq!(
            one(reply_err("No such plugin.")),
            serde_json::json!({"ok": false, "error": "No such plugin."})
        );
    }

    #[test]
    fn applies_visibility() {
        assert!(apply(Verb::Toggle, false));
        assert!(!apply(Verb::Toggle, true));
        assert!(apply(Verb::Show, true));
        assert!(!apply(Verb::Hide, true));
    }
}
