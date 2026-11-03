// SPDX-License-Identifier: GPL-2.0-or-later
//! Shared reader for the source ratchets (hypr_ratchet, copy_scan, unwrap_ratchet, main_thread,
//! one_helper): every `src/**/*.rs` file of the shell, with comments removed and `#[cfg(test)]`
//! items blanked (and the split-out `tests.rs` modules left out), and its string literals. A small Rust lexer (strings, raw strings, chars vs
//! lifetimes, nested block comments), so a `//` inside a URL or a `"` inside a char literal does
//! not hide or invent code. Heuristic by design: these tests ratchet counts, they do not prove.
#![allow(dead_code)]
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One source file, test code and comments gone.
pub struct Src {
    /// Path under `src/`, with `/` (`ui/dock.rs`).
    pub path: String,
    /// Code lines (1-based index = line - 1): comments removed, string contents kept.
    pub lines: Vec<String>,
    /// Same lines with string and char contents blanked (for brace and statement matching).
    pub bare: Vec<String>,
    /// String literals outside test items: (line, text with `\`-newline continuations joined).
    pub literals: Vec<(usize, String)>,
}

/// Every `.rs` file under `crates/shell/src`, sorted by path.
pub fn sources() -> Vec<Src> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files: Vec<PathBuf> = Vec::new();
    let mut dirs = vec![src.clone()];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") && !p.ends_with("tests.rs") {
                // `tests.rs` is a split-out test module (`#[cfg(test)] mod tests;` in the parent
                // file): test code, like the `#[cfg(test)]` items blanked below.
                files.push(p);
            }
        }
    }
    files.sort();
    files
        .into_iter()
        .map(|f| {
            let text = std::fs::read_to_string(&f).unwrap();
            let rel = f.strip_prefix(&src).unwrap();
            let path = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            parse(&path, &text)
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Code,
    Comment,
    Str,
}

/// Lexes `text` into [`Src`].
pub fn parse(path: &str, text: &str) -> Src {
    let c: Vec<char> = text.chars().collect();
    let mut kind = vec![Kind::Code; c.len()];
    // (start, end) char offsets of each string literal's contents.
    let mut strs: Vec<(usize, usize)> = Vec::new();
    let ident = |ch: char| ch.is_alphanumeric() || ch == '_';
    let mut i = 0;
    while i < c.len() {
        let prev_ident = i > 0 && ident(c[i - 1]);
        if c[i] == '/' && c.get(i + 1) == Some(&'/') {
            while i < c.len() && c[i] != '\n' {
                kind[i] = Kind::Comment;
                i += 1;
            }
        } else if c[i] == '/' && c.get(i + 1) == Some(&'*') {
            let mut depth = 0;
            while i < c.len() {
                if c[i] == '/' && c.get(i + 1) == Some(&'*') {
                    depth += 1;
                    kind[i] = Kind::Comment;
                    kind[i + 1] = Kind::Comment;
                    i += 2;
                } else if c[i] == '*' && c.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    kind[i] = Kind::Comment;
                    kind[i + 1] = Kind::Comment;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    if c[i] != '\n' {
                        kind[i] = Kind::Comment;
                    }
                    i += 1;
                }
            }
        } else if c[i] == 'r'
            && (!prev_ident || (c[i - 1] == 'b' && (i < 2 || !ident(c[i - 2]))))
            && matches!(c.get(i + 1), Some('"') | Some('#'))
        {
            // Raw string: r"…", r#"…"#, br#"…"#.
            let mut j = i + 1;
            let mut hashes = 0;
            while c.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            if c.get(j) != Some(&'"') {
                i += 1;
                continue;
            }
            let start = j + 1;
            let mut k = start;
            loop {
                if k >= c.len() {
                    break;
                }
                if c[k] == '"' && (0..hashes).all(|h| c.get(k + 1 + h) == Some(&'#')) {
                    break;
                }
                k += 1;
            }
            for x in kind.iter_mut().take(k.min(c.len())).skip(start) {
                *x = Kind::Str;
            }
            strs.push((start, k.min(c.len())));
            i = k + 1 + hashes;
        } else if c[i] == '"' {
            let start = i + 1;
            let mut k = start;
            while k < c.len() && c[k] != '"' {
                if c[k] == '\\' {
                    k += 1;
                }
                k += 1;
            }
            let end = k.min(c.len());
            for x in kind.iter_mut().take(end).skip(start) {
                *x = Kind::Str;
            }
            strs.push((start, end));
            i = end + 1;
        } else if c[i] == '\'' {
            // Char literal or lifetime.
            if c.get(i + 1) == Some(&'\\') {
                let mut k = i + 3;
                while k < c.len() && c[k] != '\'' {
                    k += 1;
                }
                for x in kind.iter_mut().take(k).skip(i + 1) {
                    *x = Kind::Str;
                }
                i = k + 1;
            } else if c.get(i + 2) == Some(&'\'') {
                kind[i + 1] = Kind::Str;
                i += 3;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    // Blank `#[cfg(test)]` items: the attribute up to the item's `;` or its matching `}`.
    let bare: Vec<char> = c
        .iter()
        .zip(&kind)
        .map(|(ch, k)| match k {
            Kind::Code => *ch,
            _ if *ch == '\n' => '\n',
            _ => ' ',
        })
        .collect();
    let mut dead = vec![false; c.len()];
    let attr: Vec<char> = "#[cfg(test)]".chars().collect();
    let mut i = 0;
    while i + attr.len() <= bare.len() {
        if bare[i..i + attr.len()] == attr[..] {
            let mut k = i + attr.len();
            let mut depth = 0i32;
            while k < bare.len() {
                match bare[k] {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    ';' if depth == 0 => break,
                    _ => {}
                }
                k += 1;
            }
            for d in dead.iter_mut().take((k + 1).min(c.len())).skip(i) {
                *d = true;
            }
            i = k + 1;
        } else {
            i += 1;
        }
    }
    let mut lines = vec![String::new()];
    let mut bare_lines = vec![String::new()];
    for (n, ch) in c.iter().enumerate() {
        if *ch == '\n' {
            lines.push(String::new());
            bare_lines.push(String::new());
            continue;
        }
        let last = lines.len() - 1;
        if dead[n] || kind[n] == Kind::Comment {
            continue;
        }
        lines[last].push(*ch);
        bare_lines[last].push(bare[n]);
    }
    let mut line_at = Vec::with_capacity(c.len());
    let mut n = 1;
    for ch in &c {
        line_at.push(n);
        if *ch == '\n' {
            n += 1;
        }
    }
    let line_of = |at: usize| line_at.get(at).copied().unwrap_or(n);
    let literals = strs
        .into_iter()
        .filter(|(s, _)| !dead.get(*s).copied().unwrap_or(false))
        .map(|(s, e)| {
            let raw: String = c[s..e].iter().collect();
            (line_of(s), join_continuations(&raw))
        })
        .collect();
    Src {
        path: path.to_string(),
        lines,
        bare: bare_lines,
        literals,
    }
}

/// `"a \<newline>   b"` -> `"a b"` (Rust's string continuation).
fn join_continuations(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(ch) = it.next() {
        if ch == '\\' && it.peek() == Some(&'\n') {
            it.next();
            while it.peek().is_some_and(|c| c.is_whitespace()) {
                it.next();
            }
        } else if ch == '\\' && it.peek() == Some(&'"') {
            out.push('"');
            it.next();
        } else {
            out.push(ch);
        }
    }
    out
}

/// Occurrences of any of `needles` in `s`.
pub fn count_in(s: &str, needles: &[&str]) -> usize {
    needles.iter().map(|n| s.matches(n).count()).sum()
}

/// Compares `actual` (file -> count, zeros dropped) with `baseline`. Growth fails (a new file or
/// a count above its row). A count below its row passes and prints a note with the table to
/// paste: the closing worker of each wave lowers the rows, so parallel workers do not collide on
/// the table (CTO decision 2026-11-02).
pub fn ratchet(what: &str, actual: &BTreeMap<String, usize>, baseline: &[(&str, usize)]) {
    let want: BTreeMap<String, usize> = baseline
        .iter()
        .filter(|(_, n)| *n > 0)
        .map(|(f, n)| (f.to_string(), *n))
        .collect();
    let have: BTreeMap<String, usize> = actual
        .iter()
        .filter(|(_, n)| **n > 0)
        .map(|(f, n)| (f.clone(), *n))
        .collect();
    if want == have {
        return;
    }
    let (mut grew, mut lower) = (Vec::new(), Vec::new());
    for (f, n) in &have {
        match want.get(f) {
            None => grew.push(format!("  {f}: {n} (new; the baseline allows 0)")),
            Some(w) if n > w => grew.push(format!("  {f}: {n} > baseline {w} (grew)")),
            Some(w) if n < w => lower.push(format!("  {f}: {n} < baseline {w} (lower the row)")),
            _ => {}
        }
    }
    for (f, w) in &want {
        if !have.contains_key(f) {
            lower.push(format!("  {f}: 0 < baseline {w} (delete the row)"));
        }
    }
    let table: Vec<String> = have
        .iter()
        .map(|(f, n)| format!("    (\"{f}\", {n}),"))
        .collect();
    let nl = "\n";
    if grew.is_empty() {
        eprintln!(
            "note: {what}: below the baseline, lower the table:{nl}{}{nl}current table:{nl}{}",
            lower.join(nl),
            table.join(nl)
        );
        return;
    }
    grew.extend(lower);
    panic!(
        "{what}:{nl}{}{nl}current table:{nl}{}",
        grew.join(nl),
        table.join(nl)
    );
}
