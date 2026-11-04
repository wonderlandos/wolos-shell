// SPDX-License-Identifier: GPL-2.0-or-later
//! UI consistency rules (docs/RULES-ui.md, ids RU-nnn): user-facing text, GTK/kit widgets, CSS,
//! goldens and docs, each enforced by scanning sources, kit.css, golden declarations and docs.
//! Where the tree does not comply yet the count is frozen in [`BASELINE`] (2026-11-02): fix
//! violations and lower the number, never raise it. Run with `RULES_PRINT=1` to print the
//! current counts (and not fail) when re-baselining after a cleanup.
#[path = "support/scan.rs"]
mod scan;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// (id, one line). docs/RULES-ui.md carries the same ids and text; `rules_doc_matches_table` checks.
const RULES: &[(&str, &str)] = &[
    (
        "RU-001",
        "A sentence in copy.rs or copy_installer.rs is at most 140 characters.",
    ),
    (
        "RU-002",
        "A sentence in the copy files has at most two sentences.",
    ),
    ("RU-003", "No on-screen text says \"failed\"."),
    (
        "RU-004",
        "No on-screen text shows an errno, exit code, OS error or panic text.",
    ),
    (
        "RU-005",
        "Copy-file sentences hold no path, flag, code token or file name.",
    ),
    (
        "RU-006",
        "Copy-file sentences name no program or plumbing (wol-, nmcli, daemon, socket); no on-screen text names an upstream project.",
    ),
    ("RU-007", "No exclamation marks in on-screen text."),
    ("RU-008", "No double spaces in on-screen text."),
    (
        "RU-009",
        "Quotes in copy are typographic: no escaped ASCII double quote in a sentence.",
    ),
    (
        "RU-010",
        "Apostrophes in sentences are typographic (’), not ASCII (').",
    ),
    (
        "RU-011",
        "An ellipsis is the single character …, never three dots.",
    ),
    (
        "RU-012",
        "A sentence has no leading or trailing space and no line break.",
    ),
    ("RU-013", "Body text is sentence case, not Title Case."),
    ("RU-014", "Titles (window, page, section) are Title Case."),
    ("RU-015", "A title has no trailing full stop."),
    ("RU-016", "A title is at most 40 characters."),
    (
        "RU-017",
        "No courtesy padding: please, sorry, oops, whoops, successfully, click here.",
    ),
    ("RU-018", "Glossary: say Desktop, never workspace."),
    ("RU-019", "Glossary: Wi-Fi, never `WiFi`, `Wifi` or `wifi`."),
    ("RU-020", "Glossary: Bluetooth, capital B."),
    ("RU-021", "Glossary: startup, never start-up."),
    ("RU-022", "Glossary: email, never e-mail."),
    (
        "RU-023",
        "Glossary: the key is Space, never spacebar or space bar.",
    ),
    (
        "RU-024",
        "Glossary: Settings, never Preferences or Control Panel.",
    ),
    ("RU-025", "Glossary: Trash, never Recycle Bin."),
    ("RU-026", "Glossary: app, never application."),
    ("RU-027", "Glossary: WolOS is spelled WolOS."),
    ("RU-028", "Glossary: USB is upper case."),
    (
        "RU-030",
        "A button label starts with an allowlisted verb or is an allowlisted noun.",
    ),
    ("RU-031", "A button label is Title Case."),
    ("RU-032", "A button label is at most four words."),
    (
        "RU-033",
        "A button label has no full stop or exclamation mark and no three-dot ellipsis.",
    ),
    ("RU-034", "No OK, Yes or No buttons: say the action."),
    (
        "RU-035",
        "A file with a destructive button also has a way out (Cancel, Not Now, Close, Done, Back).",
    ),
    (
        "RU-040",
        "Every add_css_class literal is styled in kit.css or contrast.css, or is a named hook.",
    ),
    ("RU-041", "CSS class names are lower-case kebab-case."),
    (
        "RU-042",
        "Every symbolic icon name the shell requests ships in design/icons/symbolic.",
    ),
    (
        "RU-043",
        "Every file in design/icons/symbolic is named name-symbolic.svg in lower-case.",
    ),
    (
        "RU-044",
        "Every wolos- app icon the shell requests ships in design/icons/apps.",
    ),
    ("RU-045", "Every window the shell creates sets a title."),
    ("RU-046", "Every AlertDialog has a cancel button."),
    (
        "RU-047",
        "Every entry has a placeholder, label or accessible name.",
    ),
    ("RU-048", "No list row is made unfocusable."),
    (
        "RU-049",
        "No fixed width above 600 px in code (set_size_request, set_width_request).",
    ),
    ("RU-050", "An icon-only button has a tooltip."),
    ("RU-055", "No fixed width above 600 px in kit.css."),
    ("RU-056", "No !important in the stylesheets."),
    ("RU-057", "Braces balance in kit.css and contrast.css."),
    (
        "RU-058",
        "Stylesheets have no tabs, no trailing whitespace and end with a newline.",
    ),
    ("RU-059", "No font-size below 11px."),
    ("RU-060", "No transition or animation longer than 500 ms."),
    (
        "RU-061",
        "Every var(--x) in kit.css is defined somewhere (stylesheet or theme.rs).",
    ),
    (
        "RU-062",
        "No hex colour literal outside a variable definition in kit.css.",
    ),
    ("RU-063", "No empty rule block in kit.css."),
    (
        "RU-064",
        "kit.css and contrast.css start with a comment saying what they are.",
    ),
    ("RU-070", "Every golden has a light and a dark PNG."),
    ("RU-071", "Golden names are lower-case kebab-case."),
    (
        "RU-072",
        "Installer and setup goldens end in a size; settings goldens end in wide or narrow.",
    ),
    ("RU-073", "A golden PNG is at most 400 KiB."),
    (
        "RU-074",
        "Every Settings pane has wide and narrow goldens, each light and dark.",
    ),
    (
        "RU-075",
        "Every surface file in src/ui has at least one golden.",
    ),
    (
        "RU-076",
        "Golden case names are unique in the declarations.",
    ),
    (
        "RU-077",
        "Every golden case declared with a literal name has its light and dark PNG.",
    ),
    ("RU-078", "The golden folder holds only PNG files."),
    (
        "RU-080",
        "Every docs/modules/*.md starts with a single # title.",
    ),
    (
        "RU-081",
        "A module document names its ADR in its first twelve lines.",
    ),
    ("RU-082", "A module document names its code by crate path."),
    (
        "RU-083",
        "A module document opens with a prose line saying what the thing is.",
    ),
    ("RU-084", "docs/README.md lists every module document."),
    (
        "RU-085",
        "Every ADR has a Status line in its first three lines.",
    ),
    ("RU-086", "ADR files are named NNNN-slug.md, lower-case."),
    ("RU-087", "An ADR title starts with its own number."),
    ("RU-088", "An ADR has Context, Decision and Consequences."),
    ("RU-089", "ADR numbers are unique and contiguous."),
    (
        "RU-090",
        "A superseded ADR names the existing ADR that replaces it.",
    ),
    ("RU-091", "No broken relative link in docs."),
    ("RU-092", "Every docs/...md path named in backticks exists."),
    (
        "RU-093",
        "Every wol verb in catalog.rs is documented as `wol <verb>` in docs.",
    ),
    (
        "RU-094",
        "Every Settings pane has a doc section (a heading or its source path).",
    ),
    ("RU-095", "No TODO, FIXME or XXX markers in docs."),
    ("RU-096", "Docs end with a newline and have no tabs."),
    (
        "RU-097",
        "docs/modules file names are lower-case kebab-case.",
    ),
    (
        "RU-098",
        "Docs prose spells Wi-Fi and Bluetooth as the glossary does.",
    ),
    ("RU-099", "Docs use no emoji."),
];

/// Violations tolerated today per rule (absent = 0). Only shrinks.
const BASELINE: &[(&str, usize)] = &[
    ("RU-003", 1),
    ("RU-004", 1),
    ("RU-019", 1),
    ("RU-024", 2),
    ("RU-026", 3),
    ("RU-035", 4),
    ("RU-040", 35),
    ("RU-045", 5),
    ("RU-050", 1),
    ("RU-060", 2),
    ("RU-062", 15),
    // +2 (INT-1): setup-welcome-focus-1280 light/dark draw the welcome art of setup-welcome-1280.
    ("RU-073", 8),
    ("RU-074", 8),
    ("RU-075", 5),
    ("RU-081", 3),
    ("RU-082", 6),
    ("RU-093", 15),
    ("RU-094", 19),
    ("RU-095", 2),
    ("RU-098", 2),
];

/// RU-089's known gaps, by number (only shrinks): ADRs written on branches that have not landed in
/// the integration tree. 0045 (i18n), 0046, 0047, 0049 and 0050 (hub-arch) land with their
/// branches; 0044 came with rel/0.12.1. 0043 is a number no branch ever used: no decision was
/// recorded under it, so no record is invented to fill it (qa100 step 0.4, A-7), and 0048 is not
/// renumbered while its references (webapp docs, tests, the catalog) are still being edited by
/// other lanes. It is dropped here when the hub-arch merge settles the numbering.
/// 0052 (federate), 0053 (Atlas private and Tor tabs), 0054 (keyring) and the others up to 0056 are
/// written on their own wave3 branches; 0057 (search by meaning) lands before them in this tree.
const RU_089_KNOWN_GAPS: &[&str] = &[
    "0043", "0045", "0046", "0047", "0049", "0050", "0052", "0053", "0054", "0055", "0056",
];

fn baseline(id: &str) -> usize {
    BASELINE.iter().find(|b| b.0 == id).map_or(0, |b| b.1)
}

/// Fails when `found` has more entries than the frozen baseline.
fn check(id: &str, found: Vec<String>) {
    assert!(RULES.iter().any(|r| r.0 == id), "{id}: not in RULES");
    if std::env::var_os("RULES_PRINT").is_some() {
        println!("BASE {id} {}", found.len());
        if std::env::var("RULES_PRINT").is_ok_and(|v| v == "2") {
            for f in found.iter().take(30) {
                println!("  - {f}");
            }
        }
        return;
    }
    let max = baseline(id);
    let text = RULES.iter().find(|r| r.0 == id).unwrap().1;
    assert!(
        found.len() <= max,
        "{id} {text}\n{} violations, baseline {max}:\n{}",
        found.len(),
        found
            .iter()
            .take(40)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    if found.len() < max {
        eprintln!("note: {id} improved to {}: lower its BASELINE", found.len());
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(p: impl AsRef<Path>) -> String {
    std::fs::read_to_string(p.as_ref()).unwrap_or_else(|e| panic!("{}: {e}", p.as_ref().display()))
}

fn walk(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, ext, out);
        } else if p.extension().is_some_and(|x| x == ext) {
            out.push(p);
        }
    }
    out.sort();
}

fn files(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut v = Vec::new();
    walk(dir, ext, &mut v);
    v
}

fn name(p: &Path) -> String {
    p.file_name().unwrap().to_string_lossy().into_owned()
}

// ---------------------------------------------------------------- text helpers

const COPY_FILES: [&str; 2] = ["copy.rs", "copy_installer.rs"];
const NOT_ON_SCREEN: [&str; 14] = [
    "eprintln!",
    "println!",
    "eprint!",
    "panic!",
    ".expect(",
    "assert!",
    "assert_eq!",
    "unreachable!",
    "debug_assert",
    "warn!",
    "info!",
    "debug!",
    "trace!",
    "log::",
];
const CSS_KIT: &str = include_str!("../src/kit/kit.css");
const CSS_CONTRAST: &str = include_str!("../src/kit/contrast.css");
const CSS_DARK: &str = include_str!("../src/kit/dark.css");

struct Lit {
    file: String,
    line: usize,
    text: String,
}

impl Lit {
    fn at(&self) -> String {
        format!("{}:{}: {}", self.file, self.line, self.text)
    }
}

/// The last line of copy.rs's BANNED table, which quotes what must never be said.
fn banned_table_end() -> usize {
    let src = include_str!("../src/copy.rs");
    let start = src
        .lines()
        .position(|l| l.starts_with("pub const BANNED"))
        .expect("copy::BANNED");
    let len = src
        .lines()
        .skip(start)
        .position(|l| l.starts_with("];"))
        .expect("end of copy::BANNED");
    start + len + 1
}

/// String literals that can reach a screen (log and assert lines dropped).
fn lits() -> Vec<Lit> {
    let mut out = Vec::new();
    let banned_end = banned_table_end();
    for s in scan::sources() {
        for (line, text) in &s.literals {
            let code = s.lines.get(line - 1).map_or("", |l| l.as_str());
            let banned_table = s.path == "copy.rs" && *line <= banned_end;
            // hypr.rs speaks the compositor's socket protocol (`workspace 2`): none of its
            // strings reach a screen.
            if banned_table
                || s.path == "hypr.rs"
                || NOT_ON_SCREEN.iter().any(|n| code.contains(n))
                || code.contains("include_str!")
            {
                continue;
            }
            out.push(Lit {
                file: s.path.clone(),
                line: *line,
                text: text.clone(),
            });
        }
    }
    out
}

fn is_copy(l: &Lit) -> bool {
    COPY_FILES.iter().any(|c| l.file == *c)
}

fn is_sentence(t: &str) -> bool {
    t.starts_with(|c: char| c.is_uppercase())
        && t.contains(' ')
        && t.ends_with(['.', '?', '!', '…'])
        && !t.contains(['\n', '=', '<', '>'])
        && !t.contains("::")
}

/// Text a person may read: capitalised, or several plain words. Ids, paths and keys are not.
fn is_text(t: &str) -> bool {
    if t.contains(['_', '=', '/', '\\', '<', '>', '%', '$'])
        || t.contains("::")
        || t.contains(".rs")
    {
        return false;
    }
    let words = t.split_whitespace().count();
    (t.starts_with(|c: char| c.is_uppercase()) && t.chars().any(|c| c.is_lowercase()))
        || (words >= 2 && t.chars().filter(|c| c.is_alphabetic()).count() >= 6)
}

/// Case-insensitive whole-word matches of `pat` in `t`: the matched slices as written.
fn words<'a>(t: &'a str, pat: &str) -> Vec<&'a str> {
    let lower = t.to_ascii_lowercase();
    let mut out = Vec::new();
    for (i, _) in lower.match_indices(pat) {
        let before = lower[..i].chars().next_back();
        let after = lower[i + pat.len()..].chars().next();
        let edge =
            |c: Option<char>| !c.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-');
        if edge(before) && edge(after) {
            out.push(&t[i..i + pat.len()]);
        }
    }
    out
}

const MINOR: [&str; 20] = [
    "a", "an", "and", "as", "at", "but", "by", "for", "from", "in", "into", "of", "on", "or",
    "the", "to", "up", "via", "with", "over",
];

/// Title Case: every word but the minor ones in the middle starts upper case.
fn title_case_ok(s: &str) -> bool {
    let ws: Vec<&str> = s.split_whitespace().collect();
    ws.iter().enumerate().all(|(i, w)| {
        let first = w.trim_start_matches(['“', '(', '"']).chars().next();
        let Some(f) = first.filter(|c| c.is_alphabetic()) else {
            return true;
        };
        if w.chars().skip(1).any(|c| c.is_uppercase()) {
            return true;
        }
        let minor = i > 0 && i + 1 < ws.len() && MINOR.contains(&w.to_lowercase().as_str());
        minor || f.is_uppercase()
    })
}

/// Title Case body text: four or more words of four letters or more, all capitalised.
fn looks_title_cased(s: &str) -> bool {
    let long: Vec<&str> = s
        .split_whitespace()
        .skip(1)
        .filter(|w| w.chars().filter(|c| c.is_alphabetic()).count() >= 4)
        .collect();
    long.len() >= 4
        && long.iter().all(|w| {
            w.trim_start_matches(['“', '('])
                .starts_with(|c: char| c.is_uppercase())
        })
}

/// Literals that follow `needle` on a code line (needle ends with the opening quote).
fn arg_after(line: &str, needle: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (i, _) in line.match_indices(needle) {
        let before = line[..i].chars().next_back();
        if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let rest = &line[i + needle.len()..];
        if let Some(end) = rest.find('"') {
            out.push(rest[..end].to_string());
        }
    }
    out
}

fn titles() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for s in scan::sources() {
        for (n, l) in s.lines.iter().enumerate() {
            if NOT_ON_SCREEN.iter().any(|x| l.contains(x)) {
                continue;
            }
            for needle in [
                "title(\"",
                "app_title(\"",
                "section(\"",
                "heading(\"",
                "set_title(\"",
                "title: \"",
            ] {
                for t in arg_after(l, needle) {
                    out.push((format!("{}:{}", s.path, n + 1), t));
                }
            }
        }
    }
    out
}

fn buttons() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for s in scan::sources() {
        for (n, l) in s.lines.iter().enumerate() {
            for t in arg_after(l, "button(\"")
                .into_iter()
                .chain(arg_after(l, "Button::with_label(\""))
            {
                if !t.is_empty() {
                    out.push((format!("{}:{}", s.path, n + 1), t));
                }
            }
        }
    }
    out
}

fn texts() -> Vec<Lit> {
    lits().into_iter().filter(|l| is_text(&l.text)).collect()
}

fn on_text(f: impl Fn(&Lit) -> bool, only_copy: bool) -> Vec<String> {
    texts()
        .iter()
        .filter(|l| (!only_copy || is_copy(l)) && f(l))
        .map(Lit::at)
        .collect()
}

fn sentences(only_copy: bool) -> Vec<Lit> {
    lits()
        .into_iter()
        .filter(|l| is_sentence(&l.text) && (!only_copy || is_copy(l)))
        .collect()
}

fn sent_viol(only_copy: bool, f: impl Fn(&str) -> bool) -> Vec<String> {
    sentences(only_copy)
        .iter()
        .filter(|l| f(&l.text))
        .map(Lit::at)
        .collect()
}

// ---------------------------------------------------------------- copy

#[test]
fn ru_001_002_length_and_count() {
    check(
        "RU-001",
        sent_viol(true, |t| t.chars().count() > shell::copy::MAX_LEN),
    );
    check("RU-002", sent_viol(true, |t| t.matches(". ").count() > 1));
}

#[test]
fn ru_003_004_no_failed_no_errno() {
    check(
        "RU-003",
        on_text(|l| !words(&l.text, "failed").is_empty(), false),
    );
    let bad = [
        "errno",
        "os error",
        "exit status",
        "exit code",
        "(exit",
        "panicked",
        "permission denied",
        "no such file",
    ];
    check(
        "RU-004",
        on_text(
            |l| bad.iter().any(|b| l.text.to_ascii_lowercase().contains(b)),
            false,
        ),
    );
}

#[test]
fn ru_005_006_no_paths_no_program_names() {
    check(
        "RU-005",
        sent_viol(true, |t| {
            t.split_whitespace().any(|w| {
                let w = w.trim_start_matches(['“', '(']);
                w.starts_with(['/', '~', '$', '`', '-'])
                    || w.contains("::")
                    || [".conf", ".json", ".log", ".sock", ".rs", ".so"]
                        .iter()
                        .any(|x| w.trim_end_matches(['.', ',', '”']).ends_with(x))
            })
        }),
    );
    let names: Vec<&str> = shell::copy::BANNED
        .iter()
        .filter(|b| b.1.contains("program name") || b.1 == "plumbing")
        .map(|b| b.0)
        .collect();
    let mut found = sent_viol(true, |t| {
        let lower = t.to_ascii_lowercase();
        names.iter().any(|n| {
            lower.match_indices(n).any(|(i, _)| {
                !lower[..i]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_alphanumeric)
            })
        })
    });
    // Upstream projects on any screen, copy module or not (docs/THIRD-PARTY.md:4-5): whole
    // words only, so ids like `hyprland-wolos`, `aur_hits` or `aurlaws` are not text.
    let upstream: Vec<&str> = shell::copy::BANNED
        .iter()
        .filter(|b| b.1.starts_with("upstream:"))
        .map(|b| b.0)
        .collect();
    assert!(
        ["hyprland", "aur"].iter().all(|n| upstream.contains(n)),
        "copy::BANNED lost its upstream rows: {upstream:?}"
    );
    let mut allowed_seen = [false; UPSTREAM_IDS.len()];
    for l in texts() {
        if upstream.iter().all(|n| words(&l.text, n).is_empty()) {
            continue;
        }
        match UPSTREAM_IDS
            .iter()
            .position(|a| a.0 == l.file && a.1 == l.text)
        {
            Some(i) => allowed_seen[i] = true,
            None => found.push(l.at()),
        }
    }
    for (i, seen) in allowed_seen.iter().enumerate() {
        assert!(
            seen,
            "RU-006: stale UPSTREAM_IDS entry {:?}",
            UPSTREAM_IDS[i]
        );
    }
    check("RU-006", found);
}

/// Literals that hold an upstream name as an identifier, never shown: (file, literal, why).
const UPSTREAM_IDS: [(&str, &str, &str); 1] = [(
    "desktop.rs",
    "Hyprland",
    "desktop-entry id, matched against OnlyShowIn=/NotShowIn= of .desktop files",
)];

#[test]
fn ru_007_to_012_typography() {
    check("RU-007", on_text(|l| l.text.contains('!'), false));
    check("RU-008", on_text(|l| l.text.contains("  "), false));
    check("RU-009", sent_viol(false, |t| t.contains("\\\"")));
    check("RU-010", sent_viol(false, |t| t.contains('\'')));
    check("RU-011", on_text(|l| l.text.contains("..."), false));
    check(
        "RU-012",
        sent_viol(false, |t| {
            t.starts_with(' ') || t.ends_with(' ') || t.contains("\\n")
        }),
    );
}

#[test]
fn ru_013_to_016_case_and_titles() {
    assert!(sentences(false).len() > 100, "scanner lost the sentences");
    check("RU-013", sent_viol(false, looks_title_cased));
    let all = titles();
    assert!(all.len() > 40, "scanner lost the titles");
    let ts: Vec<&(String, String)> = all
        .iter()
        .filter(|t| !t.1.contains('{') && is_text(&t.1))
        .collect();
    let fmt = |t: &&(String, String)| format!("{}: {}", t.0, t.1);
    check(
        "RU-014",
        ts.iter()
            .filter(|t| !title_case_ok(&t.1))
            .map(fmt)
            .collect(),
    );
    check(
        "RU-015",
        ts.iter().filter(|t| t.1.ends_with('.')).map(fmt).collect(),
    );
    check(
        "RU-016",
        ts.iter()
            .filter(|t| t.1.chars().count() > 40)
            .map(fmt)
            .collect(),
    );
}

#[test]
fn ru_017_courtesy() {
    let bad = [
        "please",
        "sorry",
        "oops",
        "whoops",
        "successfully",
        "click here",
    ];
    check(
        "RU-017",
        on_text(|l| bad.iter().any(|b| !words(&l.text, b).is_empty()), false),
    );
}

/// A term rule: `bad` patterns are never right; `exact` patterns are right only as spelled.
fn term(id: &str, bad: &[&str], exact: &[&str]) {
    check(
        id,
        on_text(
            |l| {
                bad.iter().any(|b| !words(&l.text, b).is_empty())
                    || exact.iter().any(|e| {
                        words(&l.text, &e.to_ascii_lowercase())
                            .iter()
                            .any(|w| w != e)
                    })
            },
            false,
        ),
    );
}

#[test]
fn ru_018_to_028_glossary() {
    term("RU-018", &["workspace", "workspaces"], &[]);
    let wifi = |t: &str| {
        ["wifi", "wi-fi"]
            .iter()
            .any(|p| words(t, p).iter().any(|w| *w != "Wi-Fi"))
    };
    check("RU-019", on_text(|l| wifi(&l.text), false));
    term("RU-020", &[], &["Bluetooth"]);
    term("RU-021", &["start-up"], &[]);
    term("RU-022", &["e-mail"], &[]);
    term("RU-023", &["spacebar", "space bar"], &[]);
    term("RU-024", &["preferences", "control panel"], &[]);
    term("RU-025", &["recycle bin"], &[]);
    term("RU-026", &["application"], &[]);
    term("RU-027", &[], &["WolOS"]);
    term("RU-028", &[], &["USB"]);
}

// ---------------------------------------------------------------- buttons

const VERBS: &[&str] = &[
    "Add",
    "Allow",
    "Always",
    "Apply",
    "Back",
    "Cancel",
    "Check",
    "Choose",
    "Clear",
    "Close",
    "Connect",
    "Continue",
    "Copy",
    "Create",
    "Delete",
    "Details",
    "Disconnect",
    "Don’t",
    "Done",
    "Download",
    "Edit",
    "Eject",
    "Enter",
    "Erase",
    "Export",
    "Finish",
    "Fix",
    "Force",
    "Forget",
    "Get",
    "Import",
    "Install",
    "Join",
    "Keep",
    "Learn",
    "Look",
    "Move",
    "Next",
    "Not",
    "Open",
    "Options",
    "Pair",
    "Paste",
    "Pause",
    "Play",
    "Quit",
    "Read",
    "Remove",
    "Rename",
    "Replace",
    "Reset",
    "Restart",
    "Restore",
    "Retry",
    "Roll",
    "Save",
    "Search",
    "Select",
    "Set",
    "Share",
    "Show",
    "Skip",
    "Start",
    "Stop",
    "Swap",
    "Try",
    "Turn",
    "Undo",
    "Update",
    "Use",
    "View",
];

#[test]
fn ru_030_to_035_buttons() {
    let all = buttons();
    assert!(all.len() > 30, "scanner lost the buttons");
    let fmt = |b: &&(String, String)| format!("{}: {}", b.0, b.1);
    let bs: Vec<&(String, String)> = all
        .iter()
        .filter(|b| b.1.starts_with(|c: char| c.is_alphabetic()))
        .collect();
    check(
        "RU-030",
        bs.iter()
            .filter(|b| {
                // "Import…" opens a dialog: the verb is the word before the ellipsis.
                let first = b.1.split_whitespace().next().unwrap_or("");
                !VERBS.contains(&first.trim_end_matches('…'))
            })
            .map(fmt)
            .collect(),
    );
    check(
        "RU-031",
        bs.iter()
            .filter(|b| !title_case_ok(&b.1))
            .map(fmt)
            .collect(),
    );
    check(
        "RU-032",
        bs.iter()
            .filter(|b| b.1.split_whitespace().count() > 4)
            .map(fmt)
            .collect(),
    );
    check(
        "RU-033",
        bs.iter()
            .filter(|b| b.1.ends_with(['.', '!']) || b.1.contains("..."))
            .map(fmt)
            .collect(),
    );
    check(
        "RU-034",
        bs.iter()
            .filter(|b| ["OK", "Ok", "Yes", "No"].contains(&b.1.as_str()))
            .map(fmt)
            .collect(),
    );
    let destructive = ["Delete", "Remove", "Erase", "Force Quit", "Move to Trash"];
    let outs = ["Cancel", "Not Now", "Close", "Done", "Back"];
    let mut by_file: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for b in &bs {
        by_file
            .entry(b.0.split(':').next().unwrap())
            .or_default()
            .push(&b.1);
    }
    check(
        "RU-035",
        by_file
            .iter()
            .filter(|(_, v)| {
                v.iter().any(|l| destructive.contains(l)) && !v.iter().any(|l| outs.contains(l))
            })
            .map(|(f, _)| f.to_string())
            .collect(),
    );
}

// ---------------------------------------------------------------- widgets

fn css_classes(css: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let b = css.as_bytes();
    for (i, _) in css.match_indices('.') {
        if b.get(i + 1)
            .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_')
        {
            let len = b[i + 1..]
                .iter()
                .take_while(|c| c.is_ascii_alphanumeric() || **c == b'-' || **c == b'_')
                .count();
            out.insert(css[i + 1..i + 1 + len].to_string());
        }
    }
    out
}

fn used_classes() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for s in scan::sources() {
        for (n, l) in s.lines.iter().enumerate() {
            for c in arg_after(l, "add_css_class(\"") {
                out.push((format!("{}:{}", s.path, n + 1), c));
            }
        }
    }
    out
}

#[test]
fn ru_040_041_css_classes() {
    let styled: BTreeSet<String> = css_classes(CSS_KIT)
        .union(&css_classes(CSS_CONTRAST))
        .cloned()
        .collect();
    let hooks = [
        "control",
        "device-row",
        "kit-reveal",
        "may-ellipsize",
        "no-row-check",
    ];
    let used = used_classes();
    assert!(used.len() > 50, "scanner lost the css classes");
    let unstyled: BTreeSet<String> = used
        .iter()
        .filter(|(_, c)| !styled.contains(c) && !hooks.contains(&c.as_str()))
        .map(|(_, c)| c.clone())
        .collect();
    check("RU-040", unstyled.into_iter().collect());
    let ok = |c: &str| {
        c.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    check(
        "RU-041",
        used.iter()
            .filter(|(_, c)| !ok(c))
            .map(|(a, c)| format!("{a}: {c}"))
            .collect(),
    );
}

fn quoted(keep: impl Fn(&str) -> bool) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for s in scan::sources() {
        for (_, t) in &s.literals {
            if keep(t)
                && t.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            {
                names.insert(t.clone());
            }
        }
    }
    names
}

/// A `wolos-` literal that names an app icon: not a symbolic, package, repository or runtime
/// file, and not a prefix a name is built from (`"wolos-kit-"` + the kit id).
fn is_app_icon_literal(t: &str) -> bool {
    t.starts_with("wolos-")
        && !t.ends_with("-symbolic")
        && !t.ends_with('-')
        && t != "wolos-cachy"
        // The slot names of the bootctl-shaped restore points (system.rs Fake), not icons.
        && t != "wolos-a"
        && t != "wolos-b"
        && t != "wolos-install"
        && t != "wolos-sessions-offered"
        && !t.starts_with("wolos-module-")
}

#[test]
fn ru_044_prefix_and_package_literals_are_not_icon_names() {
    assert!(is_app_icon_literal("wolos-files"));
    assert!(is_app_icon_literal("wolos-kit-dev"));
    assert!(!is_app_icon_literal("wolos-kit-"));
    assert!(!is_app_icon_literal("wolos-logo-symbolic"));
    assert!(!is_app_icon_literal("wolos-module-calendar"));
    assert!(!is_app_icon_literal("wolos-install"));
}

#[test]
fn ru_042_to_044_icons() {
    let dir = root().join("design/icons/symbolic");
    let sym = quoted(|t| t.ends_with("-symbolic"));
    check(
        "RU-042",
        sym.iter()
            .filter(|n| !dir.join(format!("{n}.svg")).is_file())
            .cloned()
            .collect(),
    );
    let bad: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| name(&e.path()))
        .filter(|n| {
            !(n.ends_with("-symbolic.svg")
                && n.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.'
                }))
        })
        .collect();
    check("RU-043", bad);
    let apps = root().join("design/icons/apps");
    let app = quoted(is_app_icon_literal);
    check(
        "RU-044",
        app.iter()
            .filter(|n| !apps.join(format!("{n}.svg")).is_file())
            .cloned()
            .collect(),
    );
}

/// Code lines containing one of `starts` where `ok` holds on none of the lines `back` before to
/// `fwd` after.
fn around(starts: &[&str], back: usize, fwd: usize, ok: impl Fn(&str) -> bool) -> Vec<String> {
    let mut out = Vec::new();
    for s in scan::sources() {
        for (i, l) in s.lines.iter().enumerate() {
            if starts.iter().any(|p| l.contains(p)) {
                let lo = i.saturating_sub(back);
                let hi = (i + fwd + 1).min(s.lines.len());
                if !s.lines[lo..hi].iter().any(|x| ok(x)) {
                    out.push(format!("{}:{}", s.path, i + 1));
                }
            }
        }
    }
    out
}

#[test]
fn ru_045_to_048_windows_dialogs_entries_rows() {
    // Only toplevels count: `gtk::Window` and `gtk::ApplicationWindow`. `ScrolledWindow` is a
    // scroller and `kit::widgets::AppWindow` a window's layout (a Box); the toplevel that holds
    // one is made with `Window::builder()` or `ApplicationWindow::builder()`, checked here.
    let toplevel = |l: &str| {
        ["Window::new(", "Window::builder()"].iter().any(|p| {
            l.match_indices(p).any(|(at, _)| {
                let ident: String = l[..at]
                    .chars()
                    .rev()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                ident.is_empty() || ident == "noitacilppA"
            })
        })
    };
    let mut windows = Vec::new();
    for s in scan::sources() {
        for (i, l) in s.lines.iter().enumerate() {
            if toplevel(l) {
                let hi = (i + 15).min(s.lines.len());
                if !s.lines[i..hi].iter().any(|x| x.contains("title(")) {
                    windows.push(format!("{}:{}", s.path, i + 1));
                }
            }
        }
    }
    check("RU-045", windows);
    check(
        "RU-046",
        around(&["AlertDialog::builder()"], 0, 10, |l| {
            l.contains("cancel_button(")
        }),
    );
    check(
        "RU-047",
        around(&["Entry::new(", "Entry::builder()"], 3, 10, |l| {
            l.contains("placeholder")
                || l.contains("accessible")
                || l.contains("tooltip")
                || l.contains("label")
        }),
    );
    let mut rows = Vec::new();
    for s in scan::sources() {
        for (i, l) in s.lines.iter().enumerate() {
            if l.contains("ListBoxRow") {
                let hi = (i + 8).min(s.lines.len());
                if s.lines[i..hi]
                    .iter()
                    .any(|x| x.contains("focusable(false)"))
                {
                    rows.push(format!("{}:{}", s.path, i + 1));
                }
            }
        }
    }
    check("RU-048", rows);
}

#[test]
fn ru_049_050_widths_and_icon_buttons() {
    let mut wide = Vec::new();
    for s in scan::sources() {
        for (i, l) in s.lines.iter().enumerate() {
            for call in ["set_size_request(", "set_width_request(", ".width_request("] {
                for (at, _) in l.match_indices(call) {
                    let n: String = l[at + call.len()..]
                        .chars()
                        .take_while(|c| c.is_ascii_digit())
                        .collect();
                    if n.parse::<u32>().is_ok_and(|n| n > 600) {
                        wide.push(format!("{}:{}: {n}", s.path, i + 1));
                    }
                }
            }
        }
    }
    check("RU-049", wide);
    check(
        "RU-050",
        around(&[".icon_name("], 2, 8, |l| {
            l.contains("tooltip") || l.contains("accessible") || l.contains("label")
        }),
    );
}

// ---------------------------------------------------------------- css

fn strip_comments(css: &str) -> String {
    let mut out = String::new();
    let mut rest = css;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        rest = rest[i..].find("*/").map_or("", |j| &rest[i + j + 2..]);
    }
    out.push_str(rest);
    out
}

fn css_all() -> [(&'static str, &'static str); 3] {
    [
        ("kit.css", CSS_KIT),
        ("contrast.css", CSS_CONTRAST),
        ("dark.css", CSS_DARK),
    ]
}

fn px_after(css: &str, prop: &str) -> Vec<u32> {
    let mut out = Vec::new();
    for (i, _) in css.match_indices(prop) {
        let before = css[..i].chars().next_back();
        if before.is_some_and(|c| c.is_alphanumeric() || c == '-') {
            continue;
        }
        let rest = css[i + prop.len()..].trim_start();
        let n: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if rest[n.len()..].starts_with("px")
            && let Ok(n) = n.parse()
        {
            out.push(n);
        }
    }
    out
}

#[test]
fn ru_055_to_060_css_hygiene() {
    let mut wide = Vec::new();
    let mut imp = Vec::new();
    let mut unbalanced = Vec::new();
    let mut ws = Vec::new();
    let mut small = Vec::new();
    let mut slow = Vec::new();
    for (f, css) in css_all() {
        let code = strip_comments(css);
        for prop in ["min-width:", "width:", "max-width:"] {
            wide.extend(
                px_after(&code, prop)
                    .into_iter()
                    .filter(|n| *n > 600)
                    .map(|n| format!("{f}: {prop} {n}px")),
            );
        }
        imp.extend(code.match_indices("!important").map(|_| f.to_string()));
        if code.matches('{').count() != code.matches('}').count() {
            unbalanced.push(f.to_string());
        }
        if css.contains('\t') || css.lines().any(|l| l.ends_with(' ')) || !css.ends_with('\n') {
            ws.push(f.to_string());
        }
        small.extend(
            px_after(&code, "font-size:")
                .into_iter()
                .filter(|n| *n < 11)
                .map(|n| format!("{f}: {n}px")),
        );
        for (i, _) in code.match_indices("ms") {
            let digits: String = code[..i]
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            let n: String = digits.chars().rev().collect();
            if n.parse::<u32>().is_ok_and(|n| n > 500) && code[..i - n.len()].ends_with(' ') {
                slow.push(format!("{f}: {n}ms"));
            }
        }
    }
    check("RU-055", wide);
    check("RU-056", imp);
    check("RU-057", unbalanced);
    check("RU-058", ws);
    check("RU-059", small);
    check("RU-060", slow);
}

#[test]
fn ru_061_to_064_css_tokens() {
    let code = strip_comments(CSS_KIT);
    let mut defined: BTreeSet<String> = BTreeSet::new();
    let mut all_src = String::from(CSS_CONTRAST);
    all_src.push_str(CSS_KIT);
    for f in files(&root().join("crates/shell/src"), "rs") {
        all_src.push_str(&read(f));
    }
    for (i, _) in all_src.match_indices("--") {
        let n: String = all_src[i + 2..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        if all_src[i + 2 + n.len()..].starts_with(':') {
            defined.insert(n);
        }
    }
    // The theme contract defines its tokens and scale in every stylesheet (shared::theme).
    for t in shared::theme::TOKENS {
        defined.insert(
            shared::theme::css_var(t)
                .trim_start_matches("--")
                .to_string(),
        );
    }
    for (v, _) in shared::theme::SCALE {
        defined.insert(v.trim_start_matches("--").to_string());
    }
    for v in shared::theme::DERIVED {
        defined.insert(v.trim_start_matches("--").to_string());
    }
    let mut undefined = BTreeSet::new();
    for (i, _) in code.match_indices("var(--") {
        let n: String = code[i + 6..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        let fallback = code[i + 6 + n.len()..].starts_with(',');
        if !defined.contains(&n) && !fallback {
            undefined.insert(n);
        }
    }
    check("RU-061", undefined.into_iter().collect());
    let hex = code
        .lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .filter(|l| {
            l.match_indices('#').any(|(i, _)| {
                let h: String = l[i + 1..]
                    .chars()
                    .take_while(|c| c.is_ascii_hexdigit())
                    .collect();
                matches!(h.len(), 3 | 4 | 6 | 8)
            })
        })
        .map(|l| l.trim().chars().take(70).collect::<String>())
        .collect();
    check("RU-062", hex);
    let empty = code.matches("{ }").count() + code.matches("{}").count();
    check(
        "RU-063",
        (0..empty).map(|i| format!("empty block {i}")).collect(),
    );
    check(
        "RU-064",
        css_all()
            .iter()
            .filter(|(_, c)| !c.trim_start().starts_with("/*"))
            .map(|(f, _)| f.to_string())
            .collect(),
    );
}

// ---------------------------------------------------------------- goldens

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn golden_files() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(golden_dir())
        .unwrap()
        .flatten()
        .map(|e| name(&e.path()))
        .collect();
    v.sort();
    v
}

/// Golden names without the -light/-dark.png tail.
fn golden_names() -> BTreeSet<String> {
    golden_files()
        .iter()
        .filter_map(|f| {
            f.strip_suffix("-light.png")
                .or_else(|| f.strip_suffix("-dark.png"))
        })
        .map(String::from)
        .collect()
}

#[test]
fn ru_070_to_073_golden_files() {
    let fs = golden_files();
    check(
        "RU-070",
        golden_names()
            .iter()
            .filter(|n| {
                !fs.contains(&format!("{n}-light.png")) || !fs.contains(&format!("{n}-dark.png"))
            })
            .cloned()
            .collect(),
    );
    check(
        "RU-071",
        golden_names()
            .iter()
            .filter(|n| {
                !n.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            })
            .cloned()
            .collect(),
    );
    let sized = |n: &str| {
        n.rsplit('-').next().is_some_and(|s| {
            s.split('x')
                .all(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
        })
    };
    check(
        "RU-072",
        golden_names()
            .iter()
            .filter(|n| {
                ((n.starts_with("installer-") || n.starts_with("setup-")) && !sized(n))
                    || (n.starts_with("settings-")
                        && !(n.ends_with("-wide") || n.ends_with("-narrow")))
            })
            .cloned()
            .collect(),
    );
    check(
        "RU-073",
        fs.iter()
            .filter(|f| std::fs::metadata(golden_dir().join(f)).is_ok_and(|m| m.len() > 400 * 1024))
            .cloned()
            .collect(),
    );
    check(
        "RU-078",
        fs.iter()
            .filter(|f| !f.ends_with(".png"))
            .cloned()
            .collect(),
    );
}

fn pane_stems() -> Vec<String> {
    files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui/settings"),
        "rs",
    )
    .iter()
    .map(|p| p.file_stem().unwrap().to_string_lossy().into_owned())
    .filter(|s| s != "mod")
    .collect()
}

#[test]
fn ru_074_075_golden_coverage() {
    let names = golden_names();
    // Golden pane ids squash words together and the source file names differ a little
    // (wifi.rs -> settings-wi-fi): compare with dashes and underscores removed.
    let panes: BTreeSet<String> = names
        .iter()
        .filter_map(|n| n.strip_prefix("settings-"))
        .filter_map(|r| {
            r.strip_suffix("-wide")
                .or_else(|| r.strip_suffix("-narrow"))
        })
        .map(|r| r.replace('-', ""))
        .collect();
    let mut miss = Vec::new();
    for pane in pane_stems() {
        // Panes whose golden id is a different word than the source file.
        let key = match pane.replace('_', "").as_str() {
            "accounts" => "usersgroups".to_string(),
            "shortcuts" => "keyboardshortcuts".to_string(),
            k => k.to_string(),
        };
        let hit = |size: &str, mode: &str| {
            names.iter().any(|n| {
                n.strip_prefix("settings-").is_some_and(|r| {
                    r.strip_suffix(&format!("-{size}")).is_some_and(|id| {
                        let id = id.replace('-', "");
                        id == key || id.starts_with(&key) || key.starts_with(&id)
                    })
                }) && golden_files().contains(&format!("{n}-{mode}.png"))
            })
        };
        for size in ["wide", "narrow"] {
            for mode in ["light", "dark"] {
                if !hit(size, mode) {
                    miss.push(format!("{pane}: {size} {mode}"));
                }
            }
        }
    }
    assert!(!panes.is_empty());
    check("RU-074", miss);
    let mut none = Vec::new();
    let ui = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui");
    for f in files(&ui, "rs") {
        let parent = f.parent().unwrap_or(&ui);
        if parent.ends_with("settings") || parent.ends_with("control_center") {
            continue;
        }
        // A surface split over a folder (ui/maker/editor.rs, preview.rs, ...) is one surface:
        // the folder's name is what its goldens start with.
        let stem = if parent == ui {
            f.file_stem().unwrap().to_string_lossy().replace('_', "-")
        } else {
            name(parent).replace('_', "-")
        };
        if stem == "mod" {
            continue;
        }
        let short = stem.split('-').next().unwrap();
        if !names
            .iter()
            .any(|n| n.starts_with(&stem) || n.starts_with(short))
        {
            none.push(stem);
        }
    }
    check("RU-075", none);
}

/// Case names declared as `name: "x".into()` or `name: format!("x…")` in the golden declarations.
fn declared() -> Vec<(String, bool)> {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut paths = vec![tests.join("golden.rs")];
    paths.extend(files(&tests.join("golden_cases"), "rs"));
    let mut out = Vec::new();
    for p in paths {
        let text = read(&p);
        let ls: Vec<&str> = text.lines().collect();
        for (i, l) in ls.iter().enumerate() {
            let t = l.trim();
            let lit = t
                .strip_prefix("name: \"")
                .map(|r| (r, false))
                .or_else(|| t.strip_prefix("name: format!(\"").map(|r| (r, true)));
            let Some((rest, fmt)) = lit else { continue };
            if !ls[i + 1..(i + 4).min(ls.len())]
                .iter()
                .any(|n| n.trim_start().starts_with("size:"))
            {
                continue;
            }
            if let Some(end) = rest.find('"') {
                out.push((rest[..end].to_string(), fmt));
            }
        }
    }
    out
}

#[test]
fn ru_076_077_declared_cases() {
    let d = declared();
    assert!(d.len() > 30, "found only {} declared cases", d.len());
    let mut seen = BTreeSet::new();
    let dup: Vec<String> = d
        .iter()
        .filter(|(n, f)| !f && !seen.insert(n.clone()))
        .map(|(n, _)| n.clone())
        .collect();
    check("RU-076", dup);
    let fs = golden_files();
    check(
        "RU-077",
        d.iter()
            .filter(|(n, f)| {
                !f && !(fs.contains(&format!("{n}-light.png"))
                    && fs.contains(&format!("{n}-dark.png")))
            })
            .map(|(n, _)| n.clone())
            .collect(),
    );
}

// ---------------------------------------------------------------- docs

fn docs() -> PathBuf {
    root().join("docs")
}

fn md_files() -> Vec<PathBuf> {
    files(&docs(), "md")
}

fn modules() -> Vec<PathBuf> {
    files(&docs().join("modules"), "md")
}

fn adrs() -> Vec<PathBuf> {
    files(&docs().join("adr"), "md")
}

#[test]
fn ru_080_to_084_module_docs() {
    let ms = modules();
    assert!(ms.len() > 10);
    let (mut h1, mut adr, mut code, mut what) = (vec![], vec![], vec![], vec![]);
    for m in &ms {
        let text = read(m);
        let n = name(m);
        let lines: Vec<&str> = text.lines().collect();
        let titles = lines.iter().filter(|l| l.starts_with("# ")).count();
        if !lines.first().is_some_and(|l| l.starts_with("# ")) || titles != 1 {
            h1.push(n.clone());
        }
        if !lines
            .iter()
            .take(12)
            .any(|l| l.contains("ADR") || l.contains("adr/"))
        {
            adr.push(n.clone());
        }
        if !text.contains("crates/") {
            code.push(n.clone());
        }
        let first = lines
            .iter()
            .skip(1)
            .find(|l| !l.trim().is_empty())
            .copied()
            .unwrap_or("");
        if first.starts_with(['#', '|', '-', '*', '`', '>'])
            || first.starts_with(|c: char| c.is_ascii_digit())
        {
            what.push(n);
        }
    }
    check("RU-080", h1);
    check("RU-081", adr);
    check("RU-082", code);
    check("RU-083", what);
    let readme = read(docs().join("README.md"));
    check(
        "RU-084",
        ms.iter()
            .map(|m| name(m))
            .filter(|n| !readme.contains(n.as_str()))
            .collect(),
    );
}

fn adr_num(p: &Path) -> Option<u32> {
    name(p).get(..4)?.parse().ok()
}

#[test]
fn ru_085_to_090_adrs() {
    let all = adrs();
    assert!(all.len() > 20);
    let (mut status, mut fname, mut title, mut parts, mut sup) =
        (vec![], vec![], vec![], vec![], vec![]);
    for a in &all {
        let n = name(a);
        let text = read(a);
        let lines: Vec<&str> = text.lines().collect();
        let ok_name = n.len() > 8
            && adr_num(a).is_some()
            && n[4..].starts_with('-')
            && n.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.');
        if !ok_name {
            fname.push(n.clone());
        }
        if !lines.iter().take(3).any(|l| {
            l.starts_with("Status:") || l.starts_with("## Status") || l.starts_with("**Status")
        }) {
            status.push(n.clone());
        }
        let num = n.get(..4).unwrap_or("");
        if !lines
            .first()
            .is_some_and(|l| l.starts_with(&format!("# {num}")))
        {
            title.push(n.clone());
        }
        let lower = text.to_lowercase();
        if !["context", "decision", "consequences"]
            .iter()
            .all(|p| lower.contains(&format!("{p}:")) || lower.contains(&format!("## {p}")))
        {
            parts.push(n.clone());
        }
        if let Some(l) = lines
            .iter()
            .take(3)
            .find(|l| l.to_lowercase().contains("superseded by"))
        {
            let l = l.to_lowercase();
            let after = &l[l.find("superseded by").unwrap() + 13..];
            let digits: String = after
                .chars()
                .skip_while(|c| !c.is_ascii_digit())
                .take(4)
                .collect();
            if digits.len() < 4 || !all.iter().any(|x| name(x).starts_with(&digits)) {
                sup.push(n);
            }
        }
    }
    check("RU-085", status);
    check("RU-086", fname);
    check("RU-087", title);
    check("RU-088", parts);
    check("RU-090", sup);
    let nums: Vec<u32> = all.iter().filter_map(|p| adr_num(p)).collect();
    let mut bad = Vec::new();
    for (i, n) in nums.iter().enumerate() {
        if nums[..i].contains(n) {
            bad.push(format!("duplicate {n:04}"));
        }
    }
    let set: BTreeSet<u32> = nums.iter().copied().collect();
    if let (Some(lo), Some(hi)) = (set.first(), set.last()) {
        bad.extend(
            (*lo..=*hi)
                .filter(|n| !set.contains(n))
                .map(|n| format!("gap {n:04}")),
        );
    }
    // Named, not counted: a new gap or duplicate fails even when a known gap closes the same day.
    let known: Vec<String> = RU_089_KNOWN_GAPS
        .iter()
        .map(|g| format!("gap {g}"))
        .collect();
    for k in &known {
        if !bad.contains(k) {
            eprintln!("note: RU-089 {k} is closed: drop it from RU_089_KNOWN_GAPS");
        }
    }
    bad.retain(|b| !known.contains(b));
    check("RU-089", bad);
}

/// Relative `](target)` links of a markdown file, outside code fences.
fn links(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut fence = false;
    for l in text.lines() {
        if l.trim_start().starts_with("```") {
            fence = !fence;
        }
        if fence {
            continue;
        }
        for (i, _) in l.match_indices("](") {
            let rest = &l[i + 2..];
            let Some(end) = rest.find(')') else { continue };
            let t = rest[..end].split(['#', '?']).next().unwrap_or("");
            if t.is_empty()
                || t.contains("://")
                || t.starts_with(['/', '<'])
                || t.starts_with("mailto:")
                || t.contains([' ', '{'])
            {
                continue;
            }
            out.push(t.to_string());
        }
    }
    out
}

#[test]
fn ru_091_092_links() {
    let mut broken = Vec::new();
    let mut paths = Vec::new();
    for f in md_files() {
        let text = read(&f);
        for t in links(&text) {
            if !f.parent().unwrap().join(&t).exists() {
                broken.push(format!(
                    "{}: {t}",
                    f.strip_prefix(root()).unwrap().display()
                ));
            }
        }
        for (i, _) in text.match_indices("`docs/") {
            let rest = &text[i + 1..];
            let Some(end) = rest.find('`') else { continue };
            let p = rest[..end].split([':', ' ', '#']).next().unwrap();
            if p.ends_with(".md")
                && !p.contains(['*', '{', '<', '…'])
                && !p.contains("NNNN")
                && !root().join(p).exists()
            {
                paths.push(format!("{}: {p}", name(&f)));
            }
        }
    }
    check("RU-091", broken);
    check("RU-092", paths);
}

#[test]
fn ru_093_verbs_documented() {
    let cat = read(root().join("crates/controller/src/catalog.rs"));
    let mut roots = BTreeSet::new();
    for l in cat.lines() {
        if let Some(r) = l.trim().strip_prefix("hv(&[\"")
            && let Some(end) = r.find('"')
        {
            roots.insert(r[..end].to_string());
        }
    }
    assert!(roots.len() > 10, "found {} verbs", roots.len());
    let all: String = md_files().iter().map(read).collect::<Vec<_>>().join("\n");
    let missing = roots
        .iter()
        .filter(|v| {
            let pat = format!("wol {v}");
            !all.match_indices(&pat).any(|(i, _)| {
                !all[i + pat.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '-')
            })
        })
        .cloned()
        .collect();
    check("RU-093", missing);
}

#[test]
fn ru_094_settings_panes_documented() {
    let all: Vec<String> = md_files().iter().map(read).collect();
    let missing = pane_stems()
        .into_iter()
        .filter(|s| {
            let words = s.replace('_', " ");
            !all.iter().any(|t| {
                t.contains(&format!("settings/{s}.rs"))
                    || t.lines()
                        .any(|l| l.starts_with('#') && l.to_lowercase().contains(&words))
            })
        })
        .collect();
    check("RU-094", missing);
}

#[test]
fn ru_095_to_099_doc_hygiene() {
    let (mut marks, mut ends, mut names, mut terms, mut emoji) =
        (vec![], vec![], vec![], vec![], vec![]);
    for f in md_files() {
        let text = read(&f);
        let n = name(&f);
        let mut fence = false;
        for (i, l) in text.lines().enumerate() {
            if l.trim_start().starts_with("```") {
                fence = !fence;
            }
            if ["TODO", "FIXME", "XXX"]
                .iter()
                .any(|m| !words(l, &m.to_lowercase()).is_empty() && l.contains(m))
            {
                marks.push(format!("{n}:{}", i + 1));
            }
            let mut prose = String::new();
            let mut tick = false;
            for c in l.chars() {
                if c == '`' {
                    tick = !tick;
                } else if !tick {
                    prose.push(c);
                }
            }
            let bad_term = words(&prose, "bluetooth").iter().any(|w| *w != "Bluetooth")
                || ["wifi", "wi-fi"]
                    .iter()
                    .any(|p| words(&prose, p).iter().any(|w| *w != "Wi-Fi"));
            if bad_term && !fence {
                terms.push(format!("{n}:{}", i + 1));
            }
            if l.chars().any(|c| {
                ('\u{1F300}'..='\u{1FAFF}').contains(&c)
                    || ('\u{2600}'..='\u{27BF}').contains(&c)
                        && !matches!(
                            c,
                            '\u{2713}' | '\u{2192}' | '\u{2190}' | '\u{2191}' | '\u{2193}'
                        )
            }) {
                emoji.push(format!("{n}:{}", i + 1));
            }
        }
        if !text.ends_with('\n') || text.contains('\t') {
            ends.push(n);
        }
    }
    for m in modules() {
        let n = name(&m);
        if !n
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
        {
            names.push(n);
        }
    }
    check("RU-095", marks);
    check("RU-096", ends);
    check("RU-097", names);
    check("RU-098", terms);
    check("RU-099", emoji);
}

// ---------------------------------------------------------------- the rules themselves

#[test]
fn rules_table_is_sound() {
    assert!(RULES.len() >= 60, "{} rules", RULES.len());
    let ids: BTreeSet<&str> = RULES.iter().map(|r| r.0).collect();
    assert_eq!(ids.len(), RULES.len(), "ids unique");
    let me = include_str!("rules_ui.rs");
    for (id, _) in RULES {
        assert!(id.starts_with("RU-") && id.len() == 6);
        assert!(
            me.matches(&format!("\"{id}\"")).count() >= 2,
            "{id}: no test enforces it"
        );
    }
    for (id, _) in BASELINE {
        assert!(ids.contains(id), "{id}: baseline for an unknown rule");
    }
}

#[test]
fn rules_doc_matches_table() {
    let doc = read(docs().join("RULES-ui.md"));
    let in_doc: BTreeMap<String, String> = doc
        .lines()
        .filter_map(|l| l.strip_prefix("| RU-"))
        .map(|r| {
            let (id, text) = r.split_once(" | ").unwrap();
            (format!("RU-{id}"), text.trim_end_matches(" |").to_string())
        })
        .collect();
    for (id, text) in RULES {
        assert_eq!(
            in_doc.get(*id).map(String::as_str),
            Some(*text),
            "{id}: docs/RULES-ui.md differs"
        );
    }
    assert_eq!(
        in_doc.len(),
        RULES.len(),
        "docs/RULES-ui.md lists a rule the test lacks"
    );
}

#[test]
fn helpers_behave() {
    assert!(title_case_ok("Open in Files"));
    assert!(!title_case_ok("Open in files"));
    assert!(title_case_ok("Add App…"));
    assert!(is_sentence("Try again later."));
    assert!(is_sentence("Edit widgets…") || !is_sentence("Edit"));
    assert!(looks_title_cased(
        "Turn Off Fast Startup Settings Right Now."
    ));
    assert!(!looks_title_cased("Turn off Fast Startup in Windows."));
    assert_eq!(words("Use wifi, not Wi-Fi.", "wifi"), ["wifi"]);
    assert_eq!(words("a start-up b", "start-up").len(), 1);
    assert!(words("workspaces", "workspace").is_empty());
    assert_eq!(
        arg_after("w::button(\"Cancel\", x)", "button(\""),
        ["Cancel"]
    );
    assert!(arg_after("icon_button(\"x\"", "button(\"").is_empty());
}
