// SPDX-License-Identifier: GPL-2.0-or-later
//! Clipboard history (ADR 0031 W5: search, pins, clear) and session actions: pure data rules.
//! `ui::clipboard` draws the panel (⇧⌘V); this file decides what is kept. Invariant: the list
//! is bounded ([`MAX`] recent, [`MAX_PINS`] pinned, [`MAX_TEXT`] bytes each), a pinned entry
//! survives Clear and restarts (the pin file), and a one-time code is never kept.

pub const MAX: usize = 25;
/// Pinned entries kept at most (they live in the pin file as well).
pub const MAX_PINS: usize = 25;
/// Longest text kept, in bytes: a longer copy still pastes, it just is not in the history.
pub const MAX_TEXT: usize = 64 * 1024;
/// Pin file, relative to `$XDG_STATE_HOME`: JSON list of strings.
pub const PINS_FILE: &str = "wolos/clipboard-pins.json";
/// Largest pin file read, in bytes (a person pins short snippets; 25 × 64 KiB would be 1.6 MB).
pub const MAX_PINS_FILE: u64 = 256 * 1024;

/// A verification code (4 to 8 digits, maybe split by one space or dash, as banks and sign-in
/// pages send them) is pasted once: the history does not keep it.
pub fn is_one_time_code(text: &str) -> bool {
    let t = text.trim();
    let digits = t.chars().filter(char::is_ascii_digit).count();
    let seps = t.chars().filter(|c| *c == ' ' || *c == '-').count();
    (4..=8).contains(&digits) && seps <= 1 && digits + seps == t.chars().count()
}

/// MIME type a password manager adds to a copied secret (KeePassXC, Bitwarden, Firefox's password
/// field, GNOME Secrets): the de-facto standard is KDE's. A copy that carries it is pasted once,
/// never kept (QA-LN-8).
pub const PASSWORD_HINT_MIME: &str = "x-kde-passwordManagerHint";

/// A payment card number: 13 to 19 digits (spaces or dashes between groups) that pass the Luhn
/// check. It is pasted once and not kept, like a one-time code (QA-LN-8: a 16-digit number sat in
/// the history and its preview in full).
pub fn is_card_number(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty()
        || !t
            .chars()
            .all(|c| c.is_ascii_digit() || c == ' ' || c == '-')
    {
        return false;
    }
    let digits: Vec<u32> = t.chars().filter_map(|c| c.to_digit(10)).collect();
    if !(13..=19).contains(&digits.len()) {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, d)| match (!i.is_multiple_of(2), d * 2) {
            (true, x) if x > 9 => x - 9,
            (true, x) => x,
            (false, _) => *d,
        })
        .sum();
    sum.is_multiple_of(10)
}

/// One entry of the history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub text: String,
    pub pinned: bool,
}

/// Clipboard history: pinned entries first (in pin order), then recent ones, newest first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct History {
    entries: Vec<Entry>,
}

impl History {
    /// A history holding `pins` (from the pin file), deduplicated and bounded.
    pub fn with_pins(pins: Vec<String>) -> Self {
        let mut h = History::default();
        for p in pins {
            if keepable(&p) && !h.entries.iter().any(|e| e.text == p) && h.pins() < MAX_PINS {
                h.entries.push(Entry {
                    text: p,
                    pinned: true,
                });
            }
        }
        h
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    fn pins(&self) -> usize {
        self.entries.iter().filter(|e| e.pinned).count()
    }

    /// A new copy: it becomes the newest recent entry; a pinned one stays where it is. Returns
    /// true when the list changed.
    pub fn push(&mut self, text: &str) -> bool {
        if !keepable(text) || is_one_time_code(text) || is_card_number(text) {
            return false;
        }
        if self.entries.iter().any(|e| e.pinned && e.text == text) {
            return false;
        }
        let first_recent = self.pins();
        if self
            .entries
            .get(first_recent)
            .is_some_and(|e| e.text == text)
        {
            return false;
        }
        self.entries.retain(|e| e.pinned || e.text != text);
        self.entries.insert(
            first_recent,
            Entry {
                text: text.to_string(),
                pinned: false,
            },
        );
        self.entries.truncate(first_recent + MAX);
        true
    }

    /// A copy as the panel sees it: `hinted` = the copy carried [`PASSWORD_HINT_MIME`], so it is
    /// never kept and never previewed (QA-LN-8, QA-LN-17).
    pub fn push_copy(&mut self, text: &str, hinted: bool) -> bool {
        !hinted && self.push(text)
    }

    /// Whether Clear should empty the system selection too: it holds a text the history just
    /// dropped (a pinned text stays copyable). QA-LN-16: `wl-paste` still returned the last text.
    pub fn clear_wants_selection_empty(&self, selection: Option<&str>) -> bool {
        selection.is_some_and(|t| !t.is_empty() && !self.entries.iter().any(|e| e.text == t))
    }

    /// Pins or unpins the entry with this text. Pinning moves it to the end of the pins;
    /// unpinning makes it the newest recent entry. False when nothing changed (no such entry,
    /// already in that state, or [`MAX_PINS`] reached).
    pub fn set_pinned(&mut self, text: &str, pinned: bool) -> bool {
        let Some(i) = self.entries.iter().position(|e| e.text == text) else {
            return false;
        };
        let same = self.entries.get(i).is_some_and(|e| e.pinned == pinned);
        if same || (pinned && self.pins() >= MAX_PINS) {
            return false;
        }
        let mut e = self.entries.remove(i);
        e.pinned = pinned;
        let at = self.pins();
        self.entries.insert(at, e);
        self.entries.truncate(self.pins() + MAX);
        true
    }

    /// Removes one entry, pinned or not.
    pub fn remove(&mut self, text: &str) -> bool {
        let n = self.entries.len();
        self.entries.retain(|e| e.text != text);
        n != self.entries.len()
    }

    /// Clear: recent entries go, pinned ones stay (unpin to drop them).
    pub fn clear(&mut self) {
        self.entries.retain(|e| e.pinned);
    }

    /// Entries whose text contains every word of `query`, ignoring case; all for a blank query.
    pub fn search(&self, query: &str) -> Vec<&Entry> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        self.entries
            .iter()
            .filter(|e| {
                let t = e.text.to_lowercase();
                words.iter().all(|w| t.contains(w.as_str()))
            })
            .collect()
    }

    /// The pinned texts, for the pin file.
    pub fn pinned_texts(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|e| e.pinned)
            .map(|e| e.text.clone())
            .collect()
    }
}

fn keepable(text: &str) -> bool {
    !text.trim().is_empty() && text.len() <= MAX_TEXT
}

/// Pin file text -> pins. Anything that is not a JSON list of strings is no pins.
pub fn parse_pins(text: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(text).unwrap_or_default()
}

/// Reads the pin file (blocking: call it off the GTK thread). Missing or too big = no pins.
pub fn load_pins(state_home: &std::path::Path) -> Vec<String> {
    shared::bounded::read_text(&state_home.join(PINS_FILE), MAX_PINS_FILE)
        .map(|s| parse_pins(&s))
        .unwrap_or_default()
}

/// Writes the pin file through a temp file and a rename, mode 0600 (pins can be addresses or
/// snippets a person would not show). Blocking: call it off the GTK thread.
pub fn save_pins(state_home: &std::path::Path, pins: &[String]) -> std::io::Result<()> {
    let path = state_home.join(PINS_FILE);
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let text = serde_json::to_string(pins).map_err(std::io::Error::other)?;
    shared::atomic::atomic_write(&path, 0o600, text.as_bytes())
}

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

/// Session menu action -> argv. Sleep, restart and shut down go through `wol power` (wol-admind:
/// any account at the computer, no password; sleep locks first); compositor exit for log out.
/// The three that end every app save the window set first and ask the windows to close
/// (`ui::panels::end_session`, ADR 0032, QA-FILES-5, QA-LIVE1-22).
pub fn session_command(action: &str) -> Option<&'static [&'static str]> {
    Some(match action {
        "lock" => &["wol-lock"],
        "sleep" => &["wol", "power", "sleep"],
        "restart" => &["wol", "power", "restart"],
        "shutdown" => &["wol", "power", "shutdown"],
        "logout" => &["hyprctl", "dispatch", "exit"],
        _ => return None,
    })
}

/// What runs (and is waited for) right before an action's command: Log Out (compositor exit)
/// stops hypridle first, which aborted (SIGABRT) when its display went (QA-LIVE-32). An argv,
/// never a shell string (RR-020).
pub fn before_end(action: &str) -> Option<&'static [&'static str]> {
    (action == "logout").then_some(&["pkill", "-x", "hypridle"][..])
}

/// Restart, Shut Down and Log Out end every app: the shell asks the open windows to close first
/// (`ui::panels::end_session`, QA-LIVE1-22); Lock and Sleep end nothing.
pub fn ends_apps(action: &str) -> bool {
    matches!(action, "restart" | "shutdown" | "logout")
}

/// Seconds the Restart / Shut Down / Log Out sheet waits before it acts by itself.
pub const CONFIRM_SECS: u32 = 60;

/// Actions that end every open app: asked first, with a countdown.
pub fn needs_confirm(action: &str) -> bool {
    matches!(action, "restart" | "shutdown" | "logout")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// QA-LN-8: a copy with the password-manager hint is not kept.
    #[test]
    fn password_hint_skipped() {
        let mut h = History::default();
        assert!(!h.push_copy("hunter2-correct-horse", true));
        assert!(h.entries().is_empty());
        assert!(
            h.push_copy("hunter2-correct-horse", false),
            "the same text unhinted is kept"
        );
    }

    /// QA-LN-8: a 16-digit card number is not kept (Luhn), an order number that fails Luhn is.
    #[test]
    fn card_number_skipped() {
        let mut h = History::default();
        for card in [
            "4111 1111 1111 1111",
            "4111-1111-1111-1111",
            "5500000000000004",
            "378282246310005",
        ] {
            assert!(is_card_number(card), "{card}");
            assert!(!h.push(card), "{card}");
        }
        assert!(h.entries().is_empty());
        for not in [
            "4111 1111 1111 1112",
            "1234567890",
            "order 4111111111111111",
            "12345678901234567890",
        ] {
            assert!(!is_card_number(not), "{not}");
        }
        assert!(h.push("4111 1111 1111 1112"));
    }

    /// QA-LN-17: no preview ever shows a hinted secret (it never reaches the list).
    #[test]
    fn preview_masks_password_hint() {
        let mut h = History::default();
        h.push_copy("S3cret!Passw0rd#2026", true);
        h.push_copy("shopping list", false);
        let shown: Vec<String> = h.entries().iter().map(|e| preview(&e.text)).collect();
        assert_eq!(shown, ["shopping list"]);
    }

    /// QA-LN-16: Clear also empties the system selection when it holds a dropped text, and leaves
    /// a pinned text (still the user's) and an empty selection alone.
    #[test]
    fn clear_clears_selection() {
        let mut h = History::with_pins(vec!["address".into()]);
        h.push("copied text");
        assert!(
            !h.clear_wants_selection_empty(Some("copied text")),
            "before clear it is in the list"
        );
        h.clear();
        assert!(h.clear_wants_selection_empty(Some("copied text")));
        assert!(!h.clear_wants_selection_empty(Some("address")), "pinned");
        assert!(!h.clear_wants_selection_empty(Some("")));
        assert!(!h.clear_wants_selection_empty(None));
    }

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
    fn pins_survive_clear_and_new_copies() {
        let mut h = History::with_pins(vec!["addr".into(), "addr".into(), " ".into()]);
        assert_eq!(h.entries().len(), 1);
        assert!(h.push("a"));
        assert!(h.push("b"));
        assert!(!h.push("b"), "same as the newest");
        assert!(!h.push("addr"), "already pinned");
        let texts = |h: &History| {
            h.entries()
                .iter()
                .map(|e| e.text.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(texts(&h), ["addr", "b", "a"]);
        assert!(h.set_pinned("a", true));
        assert_eq!(texts(&h), ["addr", "a", "b"]);
        assert_eq!(h.pinned_texts(), ["addr", "a"]);
        h.clear();
        assert_eq!(texts(&h), ["addr", "a"]);
        assert!(h.set_pinned("addr", false));
        assert_eq!(texts(&h), ["a", "addr"]);
        assert!(!h.set_pinned("zzz", true));
        assert!(h.remove("addr") && !h.remove("addr"));
        for i in 0..40 {
            h.push(&format!("n{i}"));
        }
        assert_eq!(
            h.entries().len(),
            1 + MAX,
            "pins do not count against recent"
        );
        let mut full = History::with_pins((0..MAX_PINS).map(|i| format!("p{i}")).collect());
        full.push("x");
        assert!(!full.set_pinned("x", true), "pins are bounded");
    }

    #[test]
    fn search_matches_every_word() {
        let mut h = History::default();
        h.push("Hello World");
        h.push("hello there");
        h.push("other");
        let found = |q: &str| {
            h.search(q)
                .iter()
                .map(|e| e.text.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(found("HELLO"), ["hello there", "Hello World"]);
        assert_eq!(found("world hello"), ["Hello World"]);
        assert_eq!(found("  ").len(), 3);
        assert!(found("nope").is_empty());
    }

    #[test]
    fn one_time_codes_and_huge_copies_are_not_kept() {
        for code in ["123456", "1234", "123 456", "123-456", " 87654321 "] {
            assert!(is_one_time_code(code), "{code}");
        }
        for text in ["123", "123456789", "12 34 56", "abc123", "2026-11-02"] {
            assert!(!is_one_time_code(text), "{text}");
        }
        let mut h = History::default();
        assert!(!h.push("482913"));
        assert!(!h.push(&"x".repeat(MAX_TEXT + 1)));
        assert!(h.entries().is_empty());
    }

    #[test]
    fn pin_file_round_trip() {
        assert_eq!(parse_pins(r#"["a","b"]"#), ["a", "b"]);
        assert!(parse_pins("{}").is_empty() && parse_pins("").is_empty());
        let dir = std::env::temp_dir().join(format!("wol-pins-{}", std::process::id()));
        save_pins(&dir, &["a".into(), "b\n\"c\"".into()]).unwrap();
        assert_eq!(load_pins(&dir), ["a", "b\n\"c\""]);
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join(PINS_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        std::fs::write(dir.join(PINS_FILE), "x".repeat(MAX_PINS_FILE as usize + 10)).unwrap();
        assert!(load_pins(&dir).is_empty(), "too big");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(load_pins(&dir).is_empty(), "missing");
    }

    #[test]
    fn previews() {
        assert_eq!(preview("a\n  b"), "a b");
        assert_eq!(preview(&"x".repeat(80)).chars().count(), 60);
    }

    #[test]
    fn session_actions() {
        for (a, argv, confirm) in [
            ("lock", &["wol-lock"][..], false),
            ("sleep", &["wol", "power", "sleep"][..], false),
            ("restart", &["wol", "power", "restart"][..], true),
            ("shutdown", &["wol", "power", "shutdown"][..], true),
            ("logout", &["hyprctl", "dispatch", "exit"][..], true),
        ] {
            assert_eq!(session_command(a), Some(argv), "{a}");
            assert_eq!(needs_confirm(a), confirm, "{a}");
            assert_eq!(
                crate::copy::session_confirm(a, CONFIRM_SECS).is_some(),
                confirm
            );
        }
        assert_eq!(session_command("dance"), None);
        assert_eq!(before_end("logout"), Some(&["pkill", "-x", "hypridle"][..]));
        assert_eq!(before_end("restart"), None);
        let (q, line, button) = crate::copy::session_confirm("shutdown", 60).unwrap();
        assert_eq!(q, "Are you sure you want to shut down your computer now?");
        assert_eq!(
            line,
            "If you do nothing, your computer will shut down automatically in 60 seconds."
        );
        assert_eq!(button, "Shut Down");
        assert!(
            crate::copy::session_confirm("logout", 1)
                .unwrap()
                .1
                .ends_with("in 1 second.")
        );
        for a in ["shutdown", "restart", "logout"] {
            let (q, line, _) = crate::copy::session_confirm(a, 60).unwrap();
            assert!(q.starts_with("Are you sure") && q.ends_with('?'));
            assert!(crate::copy::lint(&line).is_empty(), "{line}");
        }
    }
}
