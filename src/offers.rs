// SPDX-License-Identifier: MIT
//! What a notification offers: a one-time code to copy, a link to open, a number to copy. The
//! banner and the Notification Center card show one button per offer (`ui::notify`); this file
//! finds the offers and does the clipboard part.
//!
//! Invariant: a wrong offer is worse than none. A code is only a code when a keyword
//! ([`KEYWORDS`]) sits within [`NEAR_BEFORE`] characters before or [`NEAR_AFTER`] after it,
//! "Code" in a product name ("VS Code") is not a keyword, and a year is never a code. A link is
//! offered only when [`safe_https_url`] accepts it. A copied code is marked sensitive (the
//! clipboard history skips it, `clipboard::is_one_time_code`) and cleared after [`CLEAR_AFTER`],
//! only if the clipboard still holds that code ([`expire`]).
// Portions: see docs/THIRD-PARTY.md

use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Longest text scanned, in characters (title and body together); the rest is ignored.
pub const MAX_TEXT: usize = 32 * 1024;
/// Codes, links and numbers found at most, each.
pub const MAX_FOUND: usize = 8;
/// Links offered at most.
pub const MAX_LINKS: usize = 3;
/// Offer buttons a banner shows at most (344 px banner: three would not fit next to the app's).
pub const MAX_BUTTONS: usize = 2;
/// A keyword this many characters before a code still counts ("code: 482913").
pub const NEAR_BEFORE: usize = 24;
/// A keyword this many characters after a code still counts ("482913 is your code").
pub const NEAR_AFTER: usize = 72;
/// Longest link offered, in bytes.
pub const MAX_URL: usize = 4096;
/// Longest code copied, in characters.
pub const MAX_CODE: usize = 64;
/// Longest number copied, in characters.
pub const MAX_COPY: usize = 4096;
/// A copied code stays on the clipboard this long (a phone does the same).
pub const CLEAR_AFTER: Duration = Duration::from_secs(60);
/// The banner whose only code was copied closes this long after the click, so the press is seen.
pub const CLOSE_AFTER: Duration = Duration::from_millis(600);

/// Words that make a nearby number a one-time code, lower case. Several words are one keyword
/// with a space or a dash between them ("one-time", "two factor"). Other languages add their
/// words here; [`scan_with`] takes any list.
pub const KEYWORDS: &[&str] = &[
    "code",
    "otp",
    "one-time",
    "one time",
    "onetime",
    "verification",
    "verify",
    "passcode",
    "pin",
    "2fa",
    "two-factor",
    "two factor",
    "security code",
    "login code",
    // de, es, fr, it, pt, pl, nl, sv
    "bestätigungscode",
    "sicherheitscode",
    "código",
    "codigo",
    "codice",
    "kod",
    "verificatiecode",
    "kód",
    "kode",
];

/// Capitalised words that may stand before "Code" without making it a product name.
const NOT_A_PRODUCT: &[&str] = &[
    "your",
    "the",
    "my",
    "this",
    "new",
    "login",
    "security",
    "access",
    "backup",
    "auth",
    "sign-in",
    "signin",
    "account",
    "one",
    "temporary",
];

/// What one notification offers.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Offers {
    /// One-time codes, digits and letters only ("482913", "A9F3K2").
    pub codes: Vec<String>,
    /// `https` links that passed [`safe_https_url`], canonical form.
    pub links: Vec<String>,
    /// Phone numbers as written ("+44 7911 123456").
    pub phones: Vec<String>,
}

/// One button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    Code(String),
    Link(String),
    Phone(String),
}

impl Offers {
    pub fn is_empty(&self) -> bool {
        self.codes.is_empty() && self.links.is_empty() && self.phones.is_empty()
    }

    /// The buttons, at most [`MAX_BUTTONS`]: the codes first (a code is the urgent one), then the
    /// first link, then the first number.
    pub fn buttons(&self) -> Vec<Offer> {
        let codes = self.codes.iter().cloned().map(Offer::Code);
        let link = self.links.first().cloned().map(Offer::Link);
        let phone = self.phones.first().cloned().map(Offer::Phone);
        codes.chain(link).chain(phone).take(MAX_BUTTONS).collect()
    }

    /// Whether copying `code` finishes the notification: it carried that one code only. A note
    /// with several codes stays, the other one is still in it.
    pub fn closes_after_copy(&self, code: &str) -> bool {
        self.codes.len() == 1 && self.codes[0] == code
    }
}

/// Offers in a notification's title and body, with the built-in [`KEYWORDS`].
pub fn scan(title: &str, body: &str) -> Offers {
    scan_with(title, body, KEYWORDS)
}

/// [`scan`] with another keyword list (lower case; see [`KEYWORDS`]).
pub fn scan_with(title: &str, body: &str, keywords: &[&str]) -> Offers {
    let text: Vec<char> = title
        .chars()
        .chain(std::iter::once('\n'))
        .chain(body.chars())
        .take(MAX_TEXT)
        .collect();
    let (phones, taken) = phones(&text);
    Offers {
        codes: codes(&text, keywords, &taken),
        links: links(&text),
        phones,
    }
}

/// A word character for the edges of a code or keyword: letters (any script), digits, `_`.
fn word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn at(t: &[char], i: usize) -> Option<char> {
    t.get(i).copied()
}

/// Whether `t[i..]` starts with keyword `k` (case-insensitive; a space or dash in `k` matches a
/// space or a dash). Returns its length in characters.
fn keyword_at(t: &[char], i: usize, k: &str) -> Option<usize> {
    let mut n = 0;
    for kc in k.chars() {
        let c = at(t, i + n)?;
        let same = if kc == ' ' || kc == '-' {
            c == ' ' || c == '-'
        } else {
            c.to_lowercase().eq(kc.to_lowercase())
        };
        if !same {
            return None;
        }
        n += 1;
    }
    Some(n)
}

/// "Code" in "Claude Code" or "VS Code": capitalised, after a capitalised word that is neither
/// a keyword nor an ordinary sentence word. "use code SAVE20" and "Your Code is" are not names.
fn product_name(t: &[char], i: usize, len: usize, keywords: &[&str]) -> bool {
    let w: String = t[i..i + len].iter().collect();
    if !w.eq_ignore_ascii_case("code") || !w.starts_with('C') {
        return false;
    }
    // The word before, across spaces only.
    let mut e = i;
    while e > 0 && t[e - 1] == ' ' {
        e -= 1;
    }
    if e == i {
        return false;
    }
    let mut s = e;
    while s > 0 && (word(t[s - 1]) || t[s - 1] == '.' || t[s - 1] == '-') {
        s -= 1;
    }
    let prev: String = t[s..e].iter().collect();
    let mut cs = prev.chars();
    let Some(first) = cs.next() else { return false };
    let rest: Vec<char> = cs.collect();
    let capitalised = first.is_uppercase()
        && (rest.iter().all(|c| c.is_lowercase())
            || (!rest.is_empty() && rest.iter().all(|c| c.is_uppercase())));
    let lower = prev.to_lowercase();
    capitalised && !keywords.contains(&lower.as_str()) && !NOT_A_PRODUCT.contains(&lower.as_str())
}

/// Character ranges near each keyword, at most [`MAX_FOUND`].
fn windows(t: &[char], keywords: &[&str]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() && out.len() < MAX_FOUND {
        if i > 0 && word(t[i - 1]) {
            i += 1;
            continue;
        }
        // The longest keyword starting here, ending at a word edge.
        let hit = keywords
            .iter()
            .filter_map(|k| keyword_at(t, i, k))
            .filter(|n| !at(t, i + n).is_some_and(word))
            .max();
        match hit {
            Some(n) => {
                if !product_name(t, i, n, keywords) {
                    out.push((
                        i.saturating_sub(NEAR_BEFORE),
                        (i + n + NEAR_AFTER).min(t.len()),
                    ));
                }
                i += n;
            }
            None => i += 1,
        }
    }
    out
}

/// Length of the digit run at `t[i..]`.
fn digits_at(t: &[char], i: usize) -> usize {
    t[i.min(t.len())..]
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .count()
}

/// A code edge: the end of the text, or a character that is not a word character or a dash.
fn edge(t: &[char], i: usize) -> bool {
    !at(t, i).is_some_and(|c| word(c) || c == '-')
}

/// The numeric code starting at `t[i]` and the index after it: "482913", "482 913", "482-913",
/// "G-482913". `None` when nothing code-shaped starts here.
fn numeric_at(t: &[char], i: usize) -> Option<(String, usize)> {
    let mut p = i;
    if at(t, p).is_some_and(|c| c.is_ascii_alphabetic())
        && at(t, p + 1) == Some('-')
        && at(t, p + 2).is_some_and(|c| c.is_ascii_digit())
    {
        p += 2;
    }
    let a = digits_at(t, p);
    if a == 0 {
        return None;
    }
    let first: String = t[p..p + a].iter().collect();
    let sep = p + a;
    // Split in two: 2-4 digits, one space or dash, 2-5 digits.
    if (2..=4).contains(&a) && at(t, sep).is_some_and(|c| c == ' ' || c == '-') {
        let b = digits_at(t, sep + 1);
        if (2..=5).contains(&b) && edge(t, sep + 1 + b) {
            let second: String = t[sep + 1..sep + 1 + b].iter().collect();
            return Some((first + &second, sep + 1 + b));
        }
    }
    ((4..=8).contains(&a) && edge(t, sep)).then_some((first, sep))
}

/// "2026", "1999": a year, never a code.
fn year(v: &str) -> bool {
    v.len() == 4 && (v.starts_with("19") || v.starts_with("20"))
}

/// Upper-case letters and digits, 5 to 8, with at least one of each ("A9F3K2").
fn alnum_at(t: &[char], i: usize) -> Option<(String, usize)> {
    let n = t[i..]
        .iter()
        .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        .count();
    let v: String = t[i..i + n].iter().collect();
    let mixed = v.chars().any(|c| c.is_ascii_digit()) && v.chars().any(|c| c.is_ascii_uppercase());
    ((5..=8).contains(&n) && edge(t, i + n) && mixed).then_some((v, i + n))
}

/// A code finder: the code starting at a position and the position just past it, if any.
type FindCode = fn(&[char], usize) -> Option<(String, usize)>;

/// Codes `find` sees starting inside the keyword windows (edges checked on the whole text, so a
/// long number cut by a window's end is not a code), outside the phone numbers in `taken`.
fn harvest(
    t: &[char],
    spans: &[(usize, usize)],
    taken: &[(usize, usize)],
    find: FindCode,
    out: &mut Vec<String>,
) {
    for &(s, e) in spans {
        let mut i = s;
        while i < e {
            if let Some(&(_, end)) = taken.iter().find(|(a, b)| (*a..*b).contains(&i)) {
                i = end;
                continue;
            }
            let starts = i == 0 || edge(t, i - 1) && t[i - 1] != '+';
            if starts && let Some((v, end)) = find(t, i) {
                if (4..=8).contains(&v.len())
                    && !year(&v)
                    && !out.contains(&v)
                    && out.len() < MAX_FOUND
                {
                    out.push(v);
                }
                i = end.max(i + 1);
                continue;
            }
            i += 1;
        }
    }
}

fn codes(t: &[char], keywords: &[&str], taken: &[(usize, usize)]) -> Vec<String> {
    let spans = windows(t, keywords);
    let mut out = Vec::new();
    harvest(t, &spans, taken, numeric_at, &mut out);
    if out.is_empty() {
        harvest(t, &spans, taken, alnum_at, &mut out);
    }
    out
}

/// Trailing punctuation belongs to the sentence, not the link.
fn tidy(url: &str) -> &str {
    url.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}', '\'', '"'])
}

fn links(t: &[char]) -> Vec<String> {
    let s: String = t.iter().collect();
    let lower = s.to_ascii_lowercase();
    let mut out = Vec::new();
    for (i, _) in lower.match_indices("https://") {
        if out.len() >= MAX_LINKS {
            break;
        }
        let end = s[i..]
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\''))
            .map_or(s.len(), |n| i + n);
        if let Some(u) = safe_https_url(tidy(&s[i..end]))
            && !out.contains(&u)
        {
            out.push(u);
        }
    }
    out
}

/// A host name in canonical form (lower case, no trailing dot), or `None`: letters, digits and
/// inner dashes, at least two labels, and a last label that is not a number ("10.0.0.1",
/// "0x7f"), so nothing a browser would read as an IP address.
pub fn canonical_host(raw: &str) -> Option<String> {
    if raw.is_empty() || raw.len() > 253 {
        return None;
    }
    let h = raw.to_ascii_lowercase();
    let h = h.strip_suffix('.').unwrap_or(&h);
    let labels: Vec<&str> = h.split('.').collect();
    let label_ok = |l: &&str| {
        let b = l.as_bytes();
        (1..=63).contains(&b.len())
            && b.iter()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
            && b[0] != b'-'
            && b[b.len() - 1] != b'-'
    };
    if labels.len() < 2 || !labels.iter().all(label_ok) {
        return None;
    }
    let last = labels[labels.len() - 1];
    let hex = last
        .strip_prefix("0x")
        .is_some_and(|r| r.chars().all(|c| c.is_ascii_hexdigit()));
    if hex || last.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(h.to_string())
}

/// `raw` as a link safe to hand to the browser, or `None`. Only `https`, no user name or
/// password before the host ("https://paypal.com@evil.example"), no control characters,
/// quotes, backslashes, encoded controls or double encoding, a host from [`canonical_host`], a
/// port 1-65535. Nothing is decoded: the link opens as written.
pub fn safe_https_url(raw: &str) -> Option<String> {
    if raw.len() > MAX_URL
        || raw
            .chars()
            .any(|c| c <= ' ' || ('\u{7f}'..='\u{9f}').contains(&c) || "\\<>\"'".contains(c))
    {
        return None;
    }
    let lower = raw.to_ascii_lowercase();
    let rest = lower.strip_prefix("https://")?;
    let raw_rest = &raw[raw.len() - rest.len()..];
    // Encoded controls (%00-%1f, %7f), double encoding (%25) and an encoded backslash.
    let b = lower.as_bytes();
    for (i, _) in lower.match_indices('%') {
        let hex = |j: usize| b.get(j).is_some_and(u8::is_ascii_hexdigit);
        if !hex(i + 1) || !hex(i + 2) {
            return None;
        }
        let pair = &lower[i + 1..i + 3];
        if pair.starts_with(['0', '1']) || matches!(pair, "7f" | "25" | "5c") {
            return None;
        }
    }
    let cut = raw_rest.find(['/', '?', '#']).unwrap_or(raw_rest.len());
    let (authority, path) = raw_rest.split_at(cut);
    if authority.contains(['@', '%']) || authority.is_empty() {
        return None;
    }
    let (host, port) = match authority.split_once(':') {
        Some((h, p)) => {
            if p.is_empty() || p.len() > 5 || !p.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let n: u32 = p.parse().ok()?;
            if !(1..=65535).contains(&n) {
                return None;
            }
            (h, Some(n))
        }
        None => (authority, None),
    };
    let host = canonical_host(host)?;
    let port = port.map(|n| format!(":{n}")).unwrap_or_default();
    let path = if path.is_empty() { "/" } else { path };
    Some(format!("https://{host}{port}{path}"))
}

/// The host of a link [`safe_https_url`] returned ("example.com"), for the button's tooltip.
pub fn host(url: &str) -> &str {
    let rest = url.strip_prefix("https://").unwrap_or(url);
    let end = rest.find([':', '/', '?', '#']).unwrap_or(rest.len());
    &rest[..end]
}

/// Phone number characters between the first and the last digit ("+44 (0)20 7946-0958").
fn phone_char(c: char) -> bool {
    c.is_ascii_digit() || matches!(c, ' ' | '(' | ')' | '.' | '-')
}

/// "+44 7911 123456": a plus, then 9 to 15 digits with spaces, brackets, dots or dashes.
fn plus_at(t: &[char], i: usize) -> Option<usize> {
    if at(t, i) != Some('+') || !at(t, i + 1).is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut end = i + 2;
    let mut last_digit = end;
    while end < t.len() && phone_char(t[end]) && end - i <= 24 {
        end += 1;
        if t[end - 1].is_ascii_digit() {
            last_digit = end;
        }
    }
    (last_digit - i >= 10).then_some(last_digit)
}

/// After an area code: "7946 0958", "7946-0958", "79460958" (3-4, separator, 3-4; or 6-8).
fn local_at(t: &[char], i: usize) -> Option<usize> {
    let a = digits_at(t, i);
    if (6..=8).contains(&a) {
        return Some(i + a);
    }
    if !(3..=4).contains(&a) {
        return None;
    }
    let mut p = i + a;
    if at(t, p).is_some_and(|c| c == ' ' || c == '-') {
        p += 1;
    }
    let b = digits_at(t, p);
    (3..=4).contains(&b).then_some(p + b)
}

/// "(020) 7946 0958": an area code in brackets.
fn bracket_at(t: &[char], i: usize) -> Option<usize> {
    if at(t, i) != Some('(') {
        return None;
    }
    let a = digits_at(t, i + 1);
    if !(2..=4).contains(&a) || at(t, i + 1 + a) != Some(')') {
        return None;
    }
    let mut p = i + 2 + a;
    if at(t, p) == Some(' ') {
        p += 1;
    }
    let end = local_at(t, p)?;
    (!at(t, end).is_some_and(|c| c.is_ascii_digit())).then_some(end)
}

/// "07911 123 456", "020-7946-0958": a leading 0, then a separator.
fn trunk_at(t: &[char], i: usize) -> Option<usize> {
    // "00" starts an international prefix or an order number, not a trunk call.
    if i > 0 && word(t[i - 1]) || at(t, i) != Some('0') || at(t, i + 1) == Some('0') {
        return None;
    }
    let a = digits_at(t, i);
    if !(2..=5).contains(&a) || !at(t, i + a).is_some_and(|c| c == ' ' || c == '-') {
        return None;
    }
    let end = local_at(t, i + a + 1)?;
    (!at(t, end).is_some_and(word)).then_some(end)
}

/// The numbers as written, and where they are (a code is never taken out of a phone number).
fn phones(t: &[char]) -> (Vec<String>, Vec<(usize, usize)>) {
    let mut out: Vec<String> = Vec::new();
    let mut spans = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for shape in [plus_at, bracket_at, trunk_at] {
        let mut i = 0;
        while i < t.len() {
            let Some(end) = shape(t, i) else {
                i += 1;
                continue;
            };
            let raw: String = t[i..end].iter().collect();
            spans.push((i, end));
            let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
            if (9..=15).contains(&digits.len()) && !seen.contains(&digits) && out.len() < MAX_FOUND
            {
                seen.push(digits);
                out.push(raw.trim().to_string());
            }
            i = end;
        }
    }
    (out, spans)
}

/// The clipboard as the offers use it. `Wayland` is the real one; tests use a stub.
pub trait Clipboard {
    /// Puts `text` on the clipboard; `sensitive` keeps it out of clipboard history.
    fn copy(&mut self, text: &str, sensitive: bool) -> io::Result<()>;
    /// What the clipboard holds now, as text.
    fn paste(&mut self) -> io::Result<String>;
    /// Empties the clipboard.
    fn clear(&mut self) -> io::Result<()>;
}

/// `wl-copy` arguments for `text`: `--sensitive` for a code, `--` before the text.
pub fn copy_argv(text: &str, sensitive: bool) -> Vec<String> {
    let mut v = Vec::new();
    if sensitive {
        v.push("--sensitive".to_string());
    }
    v.push("--".to_string());
    v.push(text.to_string());
    v
}

/// The Wayland clipboard through `wl-copy` and `wl-paste` (blocking: call off the GTK thread).
#[derive(Debug, Clone)]
pub struct Wayland {
    pub copy: PathBuf,
    pub paste: PathBuf,
}

impl Default for Wayland {
    fn default() -> Self {
        Wayland {
            copy: "wl-copy".into(),
            paste: "wl-paste".into(),
        }
    }
}

/// Runs `c` with no stdin and no stdout: `wl-copy` leaves a daemon behind that would hold a pipe.
fn run(mut c: Command) -> io::Result<()> {
    let s = c
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if s.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("clipboard helper ended with {s}")))
    }
}

impl Clipboard for Wayland {
    fn copy(&mut self, text: &str, sensitive: bool) -> io::Result<()> {
        let max = if sensitive { MAX_CODE } else { MAX_COPY };
        if text.is_empty() || text.chars().count() > max {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "copy too long"));
        }
        let mut c = Command::new(&self.copy);
        c.args(copy_argv(text, sensitive));
        run(c)
    }

    fn paste(&mut self) -> io::Result<String> {
        let out = Command::new(&self.paste)
            .arg("--no-newline")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()?;
        // A code is short; anything long is not ours, and is not kept in memory whole.
        let head = &out.stdout[..out.stdout.len().min(MAX_CODE * 4 + 1)];
        Ok(String::from_utf8_lossy(head).into_owned())
    }

    fn clear(&mut self) -> io::Result<()> {
        let mut c = Command::new(&self.copy);
        c.arg("--clear");
        run(c)
    }
}

/// Which copied code may still be cleared: only the latest one. Copying another code (or the
/// same one again) restarts the clock.
#[derive(Debug, Default)]
pub struct Secrets {
    generation: AtomicU64,
}

/// The shell's one [`Secrets`].
pub static SECRETS: Secrets = Secrets {
    generation: AtomicU64::new(0),
};

/// A copied code waiting to be cleared ([`expire`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    generation: u64,
    code: String,
}

impl Secrets {
    fn next(&self, code: &str) -> Ticket {
        Ticket {
            generation: self
                .generation
                .fetch_add(1, Ordering::SeqCst)
                .wrapping_add(1),
            code: code.to_string(),
        }
    }

    fn current(&self, t: &Ticket) -> bool {
        self.generation.load(Ordering::SeqCst) == t.generation
    }
}

/// Copies `code` as sensitive; returns the ticket to pass to [`expire`] after [`CLEAR_AFTER`].
pub fn copy_code(clip: &mut impl Clipboard, secrets: &Secrets, code: &str) -> io::Result<Ticket> {
    let ticket = secrets.next(code);
    clip.copy(code, true)?;
    Ok(ticket)
}

/// Clears the clipboard if it still holds the ticket's code and no later code was copied;
/// returns whether it cleared. Something the person copied after the code is never taken away.
pub fn expire(clip: &mut impl Clipboard, secrets: &Secrets, t: &Ticket) -> io::Result<bool> {
    if !secrets.current(t) || clip.paste()? != t.code {
        return Ok(false);
    }
    clip.clear()?;
    Ok(true)
}

/// Copies a phone number (not sensitive: it may stay in clipboard history).
pub fn copy_number(clip: &mut impl Clipboard, number: &str) -> io::Result<()> {
    clip.copy(number, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_window_reaches_both_ways() {
        let far = format!("Your code is {} 482913", "x".repeat(80));
        assert!(scan("", &far).codes.is_empty());
        assert_eq!(
            scan("", "482913 is your verification code").codes,
            ["482913"]
        );
    }

    #[test]
    fn buttons_put_codes_first_and_stop_at_two() {
        let o = scan(
            "Sign in",
            "Code 482913, or open https://example.com/x or call +44 7911 123456",
        );
        assert_eq!(
            o.buttons(),
            [
                Offer::Code("482913".into()),
                Offer::Link("https://example.com/x".into())
            ]
        );
        assert!(o.closes_after_copy("482913"));
    }
}
