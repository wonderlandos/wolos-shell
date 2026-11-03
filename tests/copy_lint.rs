// SPDX-License-Identifier: GPL-2.0-or-later
//! HIG §3 Errors: what the user reads when something did not work is a friendly sentence from
//! `shell::copy`, never raw error text (errno, exit codes, paths, "failed", program names).
use shell::copy::{self, ACTION_ERRORS};
use std::path::Path;

fn assert_clean(what: &str, s: &str) {
    let p = copy::lint(s);
    assert!(p.is_empty(), "{what}: “{s}”: {p:?}");
}

/// Raw strings the lint must catch (each was, or could be, on screen).
#[test]
fn banned_patterns_are_caught() {
    for raw in [
        "wolos/theme: Permission denied (os error 13)",
        "pkg install failed (exit 3)",
        "Unavailable on this computer",
        "Administrator password required",
        "Authentication unavailable",
        "wpa_supplicant: wlan0 not found.",
        "thread 'main' panicked at src/ui/store.rs:3:5.",
        "Could not check for updates. Make sure pkg is installed and you are online.",
        "Helpers are turned off (or `wol helpers on`).",
        "Saved to /home/alice/Pictures.",
        "Search failed.",
        "The data source failed (exit 3): no token",
        "No WolOS session is showing (wol-shell is not running).",
        "Updates install into a new boot environment.",
        "Done!",
        "the change was refused (invalid request).",
        "Could not read ~/.config/wolos/shell.json.",
        "Error: Some(\"x\").",
        "The AUR stays locked until you enter your password.",
        "Search the aur.",
        "Hyprland did not answer.",
        "Omarchy themes are not supported.",
    ] {
        assert!(!copy::lint(raw).is_empty(), "lint let through: “{raw}”");
    }
}

/// No false alarms on ordinary sentences.
#[test]
fn friendly_sentences_pass() {
    for ok in [
        "Incorrect password. Try again.",
        "“Home” didn't accept that password. Check it and try again.",
        "Wi-Fi passwords have 8 to 63 characters. Check it and try again.",
        "Firefox couldn't be installed. Check that you're online, then try again.",
        "Your Downloads folder is tidy.",
        "Restore points keep your last five updates.",
        "The Aurora wallpaper is on. Your aura colour follows it.",
        "Community packages stay locked until you enter your password.",
    ] {
        assert_clean("good sentence", ok);
    }
}

#[test]
fn every_copy_sentence_passes() {
    for e in ACTION_ERRORS {
        assert_clean(&format!("action {e:?}"), copy::action(e));
        assert_eq!(
            e.message(),
            copy::action(e),
            "ActionError::message is the copy"
        );
        assert_clean(&format!("wifi {e:?}"), &copy::wifi_join(e, "Home"));
        assert_clean(&format!("install {e:?}"), &copy::install(e, "Firefox"));
        assert_clean(&format!("update {e:?}"), copy::update(e));
    }
    assert_clean("update check", copy::UPDATE_CHECK);
    // Progress, not an outcome: an ellipsis, no full stop (HIG §3).
    assert_eq!(copy::wifi_joining("Home"), "Joining “Home”…");
    for e in copy::undo_errors() {
        assert_clean(&format!("undo {e:?}"), copy::undo(&e));
    }
}

/// Setup Assistant problems under the account fields.
#[test]
fn setup_account_problems_pass() {
    let mut d = shell::setup::Draft::default();
    let mut seen = Vec::new();
    let mut note = |d: &shell::setup::Draft| {
        if let Some((_, s)) = d.account_problem() {
            seen.push(s);
        }
    };
    note(&d);
    d.set_full_name("Alice Liddell");
    d.set_name("Alice!");
    note(&d);
    d.set_name("alice");
    note(&d);
    d.password = "curiouser".into();
    d.verify = "other".into();
    note(&d);
    d.verify = "curiouser".into();
    d.hint = "curiouser".into();
    note(&d);
    d.hint = "a:b".into();
    note(&d);
    d.hint.clear();
    d.set_full_name("Alice: Liddell");
    d.set_name("alice");
    note(&d);
    assert!(seen.len() >= 6, "{seen:?}");
    for s in seen {
        assert_clean("setup", s);
    }
}

/// Surfaces never format an error's own text into a label: `{e}`, `e.to_string()` and the like
/// only appear in log lines (eprintln!/println!/diag::say! within a line of them), and `copy.rs` is the one
/// place that maps errors.
#[test]
fn no_raw_error_text_on_screen() {
    const RAW: [&str; 7] = [
        "{e}",
        "{err}",
        "{e:?}",
        "{err:?}",
        "e.to_string()",
        "err.to_string()",
        "{error}",
    ];
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    let mut dirs = vec![src.clone()];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "rs")
                && !p.ends_with("copy.rs")
                // A split-out test module (`#[cfg(test)] mod tests;` in the parent) is test code.
                && !p.ends_with("tests.rs")
            {
                files.push(p);
            }
        }
    }
    let mut bad = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        let code = text.split("#[cfg(test)]").next().unwrap_or("");
        let lines: Vec<&str> = code.lines().collect();
        for (i, l) in lines.iter().enumerate() {
            // `name.to_string()` is not an error: the match starts at a word boundary.
            let raw = RAW.iter().any(|r| {
                l.match_indices(r).any(|(at, _)| {
                    !l[..at]
                        .chars()
                        .next_back()
                        .is_some_and(|c| c.is_alphanumeric() || c == '_')
                })
            });
            if !raw || l.trim_start().starts_with("//") {
                continue;
            }
            let logged = lines[i.saturating_sub(2)..(i + 2).min(lines.len())]
                .iter()
                .any(|x| {
                    x.contains("eprintln!") || x.contains("println!") || x.contains("diag::say!")
                });
            if !logged {
                bad.push(format!(
                    "{}:{}: {}",
                    f.strip_prefix(&src).unwrap().display(),
                    i + 1,
                    l.trim()
                ));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "raw error text outside logs (map it in shell::copy):\n{}",
        bad.join("\n")
    );
}

/// A word twice in a row inside a string literal ("Search Search", the Search field's placeholder
/// live in v0.11) is a copy mistake: a name pasted where a label already said it.
fn doubled_word(s: &str) -> Option<&str> {
    let words: Vec<&str> = s.split_whitespace().collect();
    words
        .windows(2)
        .find(|w| w[0] == w[1] && w[0].len() >= 2 && w[0].chars().all(char::is_alphabetic))
        .map(|w| w[0])
}

#[test]
fn no_string_literal_says_a_word_twice() {
    assert_eq!(doubled_word("Search Search"), Some("Search"));
    assert_eq!(doubled_word("Search"), None);
    assert_eq!(doubled_word("{} {}"), None, "placeholders are not words");
    assert_eq!(doubled_word("a a"), None, "one-letter words (grid specs)");
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut dirs = vec![src.clone()];
    let mut bad = Vec::new();
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
                continue;
            }
            if !p.extension().is_some_and(|x| x == "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&p).unwrap();
            for (i, l) in text.lines().enumerate() {
                let code = l.split("//").next().unwrap_or("");
                // Odd segments between quotes are the literals.
                for lit in code.split('"').skip(1).step_by(2) {
                    if let Some(w) = doubled_word(lit) {
                        bad.push(format!(
                            "{}:{}: “{lit}” says “{w}” twice",
                            p.strip_prefix(&src).unwrap().display(),
                            i + 1
                        ));
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "doubled words:\n{}", bad.join("\n"));
}

/// Desktop widgets: what the card says when a widget can't show its data. The developer's detail
/// (exit code, JSON path) is behind "Show Details", never the sentence.
#[test]
fn widget_sentences_pass() {
    use wol_core::widgets::SourceError as E;
    let all = [
        E::NotFound("./friends.sh".into()),
        E::Timeout(5),
        E::Failed {
            code: Some(3),
            stderr: "no token".into(),
        },
        E::TooLarge,
        E::BadJson("line 1, column 2".into()),
        E::NeedsConsent("api.example.com".into()),
        E::Offline("api.example.com".into()),
    ];
    for e in &all {
        assert_clean(
            &format!("widget {e:?}"),
            &copy::widget_source(e, "Game Friends"),
        );
    }
    assert_clean("widget problems", &copy::widget_problems("Stocks"));
    assert_clean("widget missing", copy::WIDGET_MISSING);
    assert_clean("widget newer", copy::WIDGET_NEWER);
    assert_clean("city search", copy::CITY_SEARCH);
    assert_clean("weather offline", copy::WEATHER_OFFLINE);
}

/// `wol games steam` notifications: Steam opened from the Dock or Apps has no terminal.
#[test]
fn games_notices_pass() {
    for (title, body) in wol_core::games::notice::ALL {
        assert_clean(&format!("games “{title}”"), body);
    }
}

/// Every refusal, blocked partition and summary line of the installer reads as one plain sentence.
#[test]
fn the_installer_says_everything_in_plain_sentences() {
    use wol_core::install::disk::{Blocked, Disk, Part};
    use wol_core::install::layout::Refusal as R;
    use wol_core::install::plan::Change as C;
    let d = Disk {
        parts: vec![Part {
            uuid: "u".into(),
            fstype: "ntfs".into(),
            ..Part::default()
        }],
        ..Disk::default()
    };
    let u = || "u".to_string();
    for r in [
        R::DiskChanged,
        R::NotGpt,
        R::TooSmall,
        R::UnknownPartition,
        R::Conflict(u()),
        R::Mounted(u()),
        R::CantResize(u()),
        R::NoRoom,
        R::NoRoot,
        R::NoEsp,
        R::RootMustFormat,
        R::EspInUse,
        R::NotAnEsp(u()),
        R::BadMount(u()),
        R::DuplicateMount(u()),
        R::BadFilesystem(u()),
        R::BadFilesystem("gone".into()),
    ] {
        assert_clean(&format!("{r:?}"), &copy::install_refusal(&r, &d));
    }
    for b in [
        Blocked::BitLocker,
        Blocked::NotShutDown,
        Blocked::MacOs,
        Blocked::Unreadable,
    ] {
        assert_clean(&format!("{b:?}"), copy::install_blocked(b));
    }
    let n = || "Windows".to_string();
    let m = |s: &str| s.to_string();
    for c in [
        C::EraseDisk {
            bytes: 1,
            others: vec![],
        },
        C::EraseDisk {
            bytes: 1,
            others: vec![n(), m("Linux")],
        },
        C::Delete {
            name: n(),
            bytes: 1,
        },
        C::Resize {
            name: n(),
            from: 2,
            to: 1,
        },
        C::Resize {
            name: n(),
            from: 1,
            to: 2,
        },
        C::Format {
            name: n(),
            mount: m("/"),
            encrypted: true,
        },
        C::Format {
            name: n(),
            mount: m("/efi"),
            encrypted: false,
        },
        C::New {
            bytes: 1,
            mount: m("/data"),
            encrypted: true,
        },
        C::New {
            bytes: 1,
            mount: m("swap"),
            encrypted: false,
        },
        C::ReuseEsp,
        C::Keep {
            name: m("Linux"),
            mount: m("/home"),
            plain: true,
        },
        C::Keep {
            name: m("Linux"),
            mount: m("/home"),
            plain: false,
        },
        C::Untouched { name: n() },
        C::OneCopy,
    ] {
        assert_clean(&format!("{c:?}"), &copy::install_change(&c));
    }
    assert_clean("low space", &copy::install_low_space(30_000_000_000));
    for s in [
        copy::INSTALL_BLANK,
        copy::INSTALL_POINT_OF_NO_RETURN,
        copy::INSTALL_RECOVERY_NOTE,
        copy::INSTALL_UNENCRYPTED,
        copy::INSTALL_PASSPHRASE_ASCII,
        copy::INSTALL_NAME_TAKEN,
        copy::INSTALL_OWN_KEYS_MEDIA,
        copy::INSTALL_KEY_SCREEN_MEDIA,
        copy::INSTALL_RESTART,
        copy::INSTALL_WINDOWS_START,
        copy::INSTALL_TPM_FALLBACK,
        copy::INSTALL_NO_USB,
        copy::INSTALL_KEY_NOT_SAVED,
        copy::INSTALL_NOTHING_TO_REPLACE,
        copy::INSTALL_AUTOLOGIN,
    ] {
        assert_clean(s, s);
    }
    use shell::install::flow::Swap;
    for s in [Swap::Zram, Swap::Swapfile(4)] {
        let l = copy::install_swap(s).unwrap();
        assert_clean(&format!("{s:?}"), &l);
    }
    assert_eq!(copy::install_swap(Swap::None), None);
    assert_clean("hostname", &copy::install_hostname("alice-wolos"));
    assert_clean("key saved", &copy::install_key_saved("STICK"));
}

/// The body of `fn <name>` in `src` (up to the first line that is a lone `}`).
fn fn_body<'a>(src: &'a str, name: &str) -> &'a str {
    let at = src
        .find(&format!("fn {name}("))
        .unwrap_or_else(|| panic!("fn {name} is gone"));
    let rest = &src[at..];
    let end = rest.find("\n}\n").unwrap_or(rest.len());
    &rest[..end]
}

/// Every `Some("Word")` with a capital first letter in `body`.
fn words_in(body: &str) -> Vec<String> {
    body.split("Some(\"")
        .skip(1)
        .filter_map(|t| t.split_once('"').map(|(w, _)| w.to_string()))
        .filter(|w| w.starts_with(|c: char| c.is_ascii_uppercase()))
        .collect()
}

/// Every word `wol install check` can print after `refuse` has a sentence on the Failed page:
/// the layout refusals (core `Refusal`, printed as `{r:?}`) and the plan problems (controller
/// `plan_problem`, core `unlock_problem`). Read from the sources, so a new word fails here.
#[test]
fn every_refusal_wol_install_check_prints_has_a_sentence() {
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let read = |p: &str| std::fs::read_to_string(crates.join(p)).unwrap();
    let controller = read("controller/src/install.rs");
    let plan = read("core/src/install/plan.rs");
    let layout = read("core/src/install/layout.rs");
    assert!(
        controller.contains("refusal_word(&r)") && controller.contains("format!(\"{r:?}\")"),
        "layout refusals are no longer printed as their variant name: update this test"
    );
    let mut words = words_in(fn_body(&controller, "plan_problem"));
    words.extend(words_in(fn_body(&plan, "unlock_problem")));
    assert!(words.len() >= 5, "{words:?}");
    let en = &layout[layout.find("pub enum Refusal {").unwrap()..];
    let en = &en[..en.find("\n}").unwrap()];
    let variants: Vec<String> = en
        .lines()
        .skip(1)
        .map(str::trim)
        .filter(|l| !l.starts_with("//") && !l.is_empty())
        .map(|l| {
            l.split(|c: char| !c.is_ascii_alphanumeric())
                .next()
                .unwrap()
                .to_string()
        })
        .collect();
    assert!(variants.len() >= 17, "{variants:?}");
    words.extend(variants);
    let d = wol_core::install::disk::Disk::default();
    for w in &words {
        let t = copy::install_check_refusal(w, Some("u"), &d)
            .unwrap_or_else(|| panic!("no sentence for `refuse {w}`"));
        assert_clean(w, &t);
        let line = format!("the disk is not what the plan was made for (refuse {w}(\"u\")): x");
        let (got, _) = shell::install::refused_word(&line).unwrap();
        assert_eq!(got, w);
    }
    assert_eq!(copy::install_check_refusal("Nonsense", None, &d), None);
}

/// The lines of install-run's function `name() {` up to its closing `}` (one-line functions too).
fn sh_function(src: &str, name: &str) -> String {
    let head = format!("{name}() {{");
    let mut lines = src.lines().skip_while(|l| !l.starts_with(&head));
    let first = lines
        .next()
        .unwrap_or_else(|| panic!("install-run has no {name}()"));
    if first.trim_end().ends_with('}') {
        return first.to_string();
    }
    let mut out = vec![first];
    for l in lines {
        out.push(l);
        if l == "}" {
            break;
        }
    }
    out.join("\n")
}

/// R2B-8: the refusal is parsed from the line install-run really prints. Its own `say`, `report`
/// and `refuse` functions and its own `wol install check` line run under `sh` with a fake `wol`
/// that refuses, so a change to the fail line's shape fails here, not on a person's screen.
#[test]
fn the_refusal_in_install_runs_own_fail_line_is_read_back() {
    let run = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../files/usr/lib/wolos/install-run");
    let src = std::fs::read_to_string(&run).unwrap();
    let check = src
        .lines()
        .find(|l| l.contains("$(wol install check ") && l.contains("|| refuse "))
        .expect("install-run no longer refuses with wol install check's verdict: update this test")
        .trim();
    for (word, part) in [("WeakPassword", None), ("Conflict", Some("u3"))] {
        let verdict = match part {
            Some(p) => format!("refuse {word}(\\\"{p}\\\")"),
            None => format!("refuse {word}"),
        };
        let script = [
            "stage=prepare".to_string(),
            sh_function(&src, "say"),
            sh_function(&src, "report"),
            sh_function(&src, "refuse"),
            // The log's last words and the copy for the app: not under test here.
            "why() { echo 'the tool said no'; }".into(),
            "keep_log() { :; }".into(),
            format!("wol() {{ echo \"{verdict}\"; return 2; }}"),
            "plan=/run/x/plan.json".into(),
            check.to_string(),
        ]
        .join("\n");
        let out = std::process::Command::new("sh")
            .args(["-c", &script])
            .stderr(std::process::Stdio::null())
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "refuse exits 2");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let line = stdout.lines().last().unwrap_or_default();
        let r = shell::install::parse_line(line).unwrap_or_else(|| panic!("not a line: {line}"));
        assert_eq!(r.stage, shell::install::Stage::Failed, "{line}");
        assert_eq!(r.at, Some(shell::install::Stage::Prepare), "{line}");
        assert_eq!(
            shell::install::refused_word(&r.message),
            Some((word, part)),
            "{line}"
        );
    }
}

/// The string literals of Rust source `code` with their line numbers: comments skipped, `\`
/// line continuations joined, raw strings and char literals understood well enough for our files.
fn string_literals(code: &str) -> Vec<(usize, String)> {
    let c: Vec<char> = code.chars().collect();
    let (mut i, mut line, mut out) = (0, 1, Vec::new());
    while i < c.len() {
        match c[i] {
            '\n' => line += 1,
            '/' if c.get(i + 1) == Some(&'/') => {
                while i < c.len() && c[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            '/' if c.get(i + 1) == Some(&'*') => {
                while i + 1 < c.len() && !(c[i] == '*' && c[i + 1] == '/') {
                    line += usize::from(c[i] == '\n');
                    i += 1;
                }
                i += 2;
                continue;
            }
            // A char literal ('"', '\'', 'x'); anything else after a quote is a lifetime.
            '\'' if c.get(i + 1) == Some(&'\\') => {
                i += 3;
                while i < c.len() && c[i] != '\'' {
                    i += 1;
                }
            }
            '\'' if c.get(i + 2) == Some(&'\'') => i += 2,
            'r' if (c.get(i + 1) == Some(&'"') || c.get(i + 1) == Some(&'#'))
                && !c[..i]
                    .last()
                    .is_some_and(|p| p.is_alphanumeric() || *p == '_') =>
            {
                let hashes = c[i + 1..].iter().take_while(|x| **x == '#').count();
                if c.get(i + 1 + hashes) != Some(&'"') {
                    i += 1;
                    continue;
                }
                let (start, at) = (i + 2 + hashes, line);
                let mut j = start;
                while j < c.len()
                    && !(c[j] == '"' && c[j + 1..].iter().take(hashes).all(|x| *x == '#'))
                {
                    line += usize::from(c[j] == '\n');
                    j += 1;
                }
                out.push((at, c[start..j.min(c.len())].iter().collect()));
                i = j + 1 + hashes;
                continue;
            }
            '"' => {
                let (mut s, at) = (String::new(), line);
                i += 1;
                while i < c.len() && c[i] != '"' {
                    if c[i] == '\\' {
                        if c.get(i + 1) == Some(&'\n') {
                            // Continuation: the newline and the next line's indent go.
                            i += 2;
                            line += 1;
                            while i < c.len() && c[i].is_whitespace() {
                                line += usize::from(c[i] == '\n');
                                i += 1;
                            }
                            continue;
                        }
                        s.push(c[i]);
                        i += 1;
                    }
                    line += usize::from(c[i] == '\n');
                    s.push(c[i]);
                    i += 1;
                }
                out.push((at, s));
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// A sentence a person reads: a capital (or an opening quote) first, a space, end punctuation.
fn is_sentence(s: &str) -> bool {
    let s = s.trim();
    s.starts_with(|c: char| c.is_uppercase() || c == '“')
        && s.contains(' ')
        && s.ends_with(['.', '?', '!', '…'])
}

#[test]
fn the_literal_reader_sees_what_the_compiler_sees() {
    let code = "let a = '\"'; // \"Not this one.\"\nlet b = \"Read this one.\";\n\
                /* \"Nor this.\" */ let c = r#\"Raw one \"here\".\"#;\n\
                let d = \"Two \\\n    lines.\"; fn f<'a>(x: &'a str) {}\n";
    let got = string_literals(code);
    let texts: Vec<&str> = got.iter().map(|(_, s)| s.as_str()).collect();
    assert_eq!(
        texts,
        ["Read this one.", "Raw one \"here\".", "Two lines."],
        "{got:?}"
    );
    assert_eq!(got[0].0, 2);
    assert!(is_sentence("Looking at your disks…") && !is_sentence("Install WolOS"));
}

/// R2B-20, STANDARDS 1: the installer's sentences live in `shell::copy` (`copy::installer`), where
/// the lint reads them. Its pages and its flow hold none of their own.
#[test]
fn the_installer_says_nothing_outside_copy() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files: Vec<_> = std::fs::read_dir(&src)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("install") && n.ends_with(".rs"))
        })
        .collect();
    files.push(src.join("ui/installer.rs"));
    // The pages and the flow split into folders: every module there, but not its `tests.rs`.
    for dir in ["ui/installer", "install_flow"] {
        for e in std::fs::read_dir(src.join(dir)).unwrap().flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "rs") && !p.ends_with("tests.rs") {
                files.push(p);
            }
        }
    }
    assert!(files.len() >= 4, "{files:?}");
    let mut bad = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        let code = text.split("#[cfg(test)]").next().unwrap_or("");
        for (line, s) in string_literals(code) {
            if is_sentence(&s) {
                bad.push(format!(
                    "{}:{line}: “{s}”",
                    f.strip_prefix(&src).unwrap().display()
                ));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "move these into crates/shell/src/copy_installer.rs (`copy::installer`):\n{}",
        bad.join("\n")
    );
}

/// The sentences `copy::installer` holds pass the lint (the progress lines and the Secure Boot
/// sheet are status text and a sheet, not error sentences: checked for jargon only).
#[test]
fn installer_sentences_pass() {
    use shell::copy::installer as c;
    use shell::install::Stage;
    use shell::install::flow::STEPS;
    let mut all: Vec<String> = vec![
        c::CHOOSE_DISK.into(),
        c::CHOOSE_REPLACE.into(),
        c::PASSWORD_SHORT.into(),
        c::PASSWORDS_DIFFER.into(),
        c::HOSTNAME_RULE.into(),
        c::NO_PLACE.1.into(),
        c::NO_DISK.1.into(),
        c::CLOCK_LOCAL.into(),
        c::REFUSED_STOPPED.into(),
        c::other_start("Ubuntu 24.04 LTS"),
        c::erase_warning(&[]),
        c::erase_warning(&["Windows".into()]),
        c::erase_warning(&["Windows".into(), "Linux".into()]),
    ];
    all.extend(c::TIPS.iter().map(|s| s.to_string()));
    all.extend(
        ["in-use", "too-small", "not-listed"]
            .iter()
            .filter_map(|w| c::refusal_text(w, "WD Blue")),
    );
    all.extend(
        c::bitlocker("own-keys", false)
            .into_iter()
            .map(String::from),
    );
    all.extend(
        c::bitlocker("firmware", false)
            .into_iter()
            .map(String::from),
    );
    for st in [
        Stage::Prepare,
        Stage::Files,
        Stage::Keys,
        Stage::Initramfs,
        Stage::Settings,
        Stage::Finish,
    ] {
        all.push(c::stopped_text(st).into());
    }
    for st in STEPS {
        if !matches!(c::subtitle(st), "" | c::SECURE_BOOT_TEXT) {
            all.push(c::subtitle(st).into());
        }
    }
    for s in &all {
        assert_clean(s, s);
    }
    assert_eq!(
        c::other_start("Windows"),
        copy::INSTALL_WINDOWS_START,
        "Windows keeps its sentence"
    );
    for s in [
        c::SECURE_BOOT_TEXT,
        c::doing(Stage::Seal),
        c::LOOKING_AT_DISKS,
    ] {
        let jargon: Vec<String> = copy::lint(s)
            .into_iter()
            .filter(|p| p.starts_with('“'))
            .collect();
        assert!(jargon.is_empty(), "{s}: {jargon:?}");
    }
}

/// Every project credited in docs/THIRD-PARTY.md is banned on screen (its `## id` or the family
/// name before the `-`), so a new credit cannot slip into the copy (docs/THIRD-PARTY.md:4-5).
#[test]
fn credited_projects_are_banned() {
    let notice = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/THIRD-PARTY.md"),
    )
    .expect("docs/THIRD-PARTY.md");
    let ids: Vec<&str> = notice
        .lines()
        .filter_map(|l| l.strip_prefix("## "))
        .map(str::trim)
        .collect();
    assert!(ids.len() >= 4, "THIRD-PARTY.md lost its sections: {ids:?}");
    for id in ids {
        let id = id.to_lowercase();
        assert!(
            copy::BANNED
                .iter()
                .any(|b| b.1.starts_with("upstream:") && id.starts_with(b.0)),
            "copy::BANNED has no upstream entry for the credited project {id}"
        );
        assert!(!copy::lint(&format!("Made with {id}.")).is_empty(), "{id}");
    }
}
