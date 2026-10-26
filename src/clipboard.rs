//! Clipboard history and session actions: pure data rules.

pub const MAX: usize = 25;

/// Newest first, no duplicates, no blank entries, at most `MAX`. Returns true when the list changed.
pub fn push(history: &mut Vec<String>, text: &str) -> bool {
    if text.trim().is_empty() || history.first().is_some_and(|f| f == text) {
        return false;
    }
    history.retain(|h| h != text);
    history.insert(0, text.to_string());
    history.truncate(MAX);
    true
}

/// One line, at most 60 chars.
pub fn preview(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() > 60 {
        line.chars().take(59).chain(['…']).collect()
    } else {
        line
    }
}

/// Session menu action -> argv (FreeBSD; Hyprland exit for log out).
pub fn session_command(action: &str) -> Option<&'static [&'static str]> {
    Some(match action {
        "lock" => &["wol-lock"],
        "sleep" => &["acpiconf", "-s", "3"],
        "restart" => &["shutdown", "-r", "now"],
        "shutdown" => &["shutdown", "-p", "now"],
        "logout" => &["hyprctl", "dispatch", "exit"],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_rules() {
        let mut h = Vec::new();
        assert!(push(&mut h, "a"));
        assert!(!push(&mut h, "a"));
        assert!(!push(&mut h, "  "));
        push(&mut h, "b");
        push(&mut h, "a");
        assert_eq!(h, ["a", "b"]);
        for i in 0..40 {
            push(&mut h, &i.to_string());
        }
        assert_eq!(h.len(), MAX);
    }

    #[test]
    fn previews() {
        assert_eq!(preview("a\n  b"), "a b");
        assert_eq!(preview(&"x".repeat(80)).chars().count(), 60);
    }

    #[test]
    fn session_actions() {
        assert_eq!(
            session_command("shutdown"),
            Some(&["shutdown", "-p", "now"][..])
        );
        assert_eq!(session_command("dance"), None);
    }
}
