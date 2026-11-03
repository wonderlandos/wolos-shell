// SPDX-License-Identifier: MIT
//! Notification offers: which codes, links and numbers a banner offers, and the clipboard rule
//! (a copied code is cleared after a minute only if the clipboard still holds it).
// Portions: see docs/THIRD-PARTY.md
use shell::offers::{self, Clipboard, Offer, Secrets};
use std::io;

/// (title, body, codes, link, phone). An empty list or "" means none offered.
const CASES: &[(&str, &str, &[&str], &str, &str)] = &[
    // Codes with a keyword nearby.
    ("Verification", "Your code is 938271", &["938271"], "", ""),
    ("", "Your code is 938 271", &["938271"], "", ""),
    ("", "Your code is 938-271", &["938271"], "", ""),
    ("", "Your code is A9F3K2", &["A9F3K2"], "", ""),
    (
        "codes",
        "your code is 482913, backup code is 771204",
        &["482913", "771204"],
        "",
        "",
    ),
    ("", "482913 is your verification code", &["482913"], "", ""),
    ("", "Use OTP 5521 to sign in.", &["5521"], "", ""),
    (
        "Bank",
        "Your one-time passcode: 77310544",
        &["77310544"],
        "",
        "",
    ),
    ("", "Your PIN is 4821", &["4821"], "", ""),
    ("", "2FA: 610 337", &["610337"], "", ""),
    ("", "Your Verification Code is 482913", &["482913"], "", ""),
    ("", "Your Code: 551234", &["551234"], "", ""),
    (
        "",
        "G-482913 is your Google verification code.",
        &["482913"],
        "",
        "",
    ),
    ("", "Two-factor code\n\n883012", &["883012"], "", ""),
    (
        "Sign-in",
        "Enter 7742 to verify your email",
        &["7742"],
        "",
        "",
    ),
    (
        "",
        "Dein Bestätigungscode lautet 381920",
        &["381920"],
        "",
        "",
    ),
    ("", "Tu código es 552019", &["552019"], "", ""),
    ("Login code", "4019 33", &["401933"], "", ""),
    (
        "",
        "Your code is 482913 (expires in 10 minutes)",
        &["482913"],
        "",
        "",
    ),
    (
        "",
        "Your code is 482913. Never share your code. Code 482913.",
        &["482913"],
        "",
        "",
    ),
    // Negatives: no keyword, product names, years, wrong shapes.
    ("Build", "412 passed, 0 failed", &[], "", ""),
    ("Claude Code", "build 4821 finished", &[], "", ""),
    ("", "Claude Code build 4821", &[], "", ""),
    (
        "VS Code",
        "Update 1.95 is ready, 2048 files indexed",
        &[],
        "",
        "",
    ),
    (
        "",
        "Visual Studio Code restarted after 3412 ms",
        &[],
        "",
        "",
    ),
    ("", "Your code is 2026", &[], "", ""),
    ("", "Your code is 1999", &[], "", ""),
    ("Order", "Order 48213 shipped", &[], "", ""),
    ("Mail", "Meeting moved to room 4412", &[], "", ""),
    ("", "Your code is 123", &[], "", ""),
    ("", "Your code is 123456789", &[], "", ""),
    (
        "",
        "Call +44 7911 123456 for your code",
        &[],
        "",
        "+44 7911 123456",
    ),
    ("", "Your code is HELLO", &[], "", ""),
    ("", "code: 482913x", &[], "", ""),
    ("", "Pinned 4 items, 48213 bytes", &[], "", ""),
    (
        "",
        "Your code is ready. Later in this long message, after a lot of other words that run on and on, build 482913 finished",
        &[],
        "",
        "",
    ),
    // Links: https only, safe ones, trailing punctuation dropped.
    (
        "",
        "Read it at https://example.com/post?id=4.",
        &[],
        "https://example.com/post?id=4",
        "",
    ),
    (
        "",
        "(see https://Example.COM)",
        &[],
        "https://example.com/",
        "",
    ),
    (
        "",
        "https://example.com:8443/a",
        &[],
        "https://example.com:8443/a",
        "",
    ),
    ("", "http://example.com/plain", &[], "", ""),
    ("", "https://paypal.com@evil.example", &[], "", ""),
    ("", "https://example.com/%250a", &[], "", ""),
    ("", "https://0x7f.0x0.0x0.0x1/", &[], "", ""),
    ("", "http://0x7f.0x0.0x0.0x1/", &[], "", ""),
    ("", "https://127.0.0.1/", &[], "", ""),
    ("", "https://[::ffff:127.0.0.1]", &[], "", ""),
    ("", "https://localhost/", &[], "", ""),
    ("", "https://example.com:0/", &[], "", ""),
    ("", "https://example.com:99999/", &[], "", ""),
    ("", "file:///etc/passwd", &[], "", ""),
    // Numbers in the three shapes.
    ("", "+44 7911 123456", &[], "", "+44 7911 123456"),
    (
        "Missed call",
        "Call back on +1 (555) 010-4477.",
        &[],
        "",
        "+1 (555) 010-4477",
    ),
    ("", "Office: (020) 7946 0958", &[], "", "(020) 7946 0958"),
    ("", "Ring 07911 123 456 after six", &[], "", "07911 123 456"),
    ("", "Ring 020-7946-0958", &[], "", "020-7946-0958"),
    ("", "Version 1.2.3 released, 41 fixes", &[], "", ""),
    ("", "Balance 1 234 567.89 EUR", &[], "", ""),
    ("", "Order 0012-3456-7890 shipped", &[], "", ""),
    ("", "+12 345", &[], "", ""),
    (
        "Bank",
        "Your code is 551234. Questions? +44 20 7946 0958",
        &["551234"],
        "",
        "+44 20 7946 0958",
    ),
];

#[test]
fn offers_match_the_table() {
    assert!(CASES.len() >= 40, "{} cases", CASES.len());
    let mut bad = Vec::new();
    for (title, body, codes, link, phone) in CASES {
        let o = offers::scan(title, body);
        let got_link = o.links.first().map_or("", String::as_str);
        let got_phone = o.phones.first().map_or("", String::as_str);
        if o.codes != *codes || got_link != *link || got_phone != *phone {
            bad.push(format!("{title:?} / {body:?}: {o:?}"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// The URL corpus from the security review: none of these is ever offered.
#[test]
fn unsafe_links_are_never_offered() {
    for u in [
        "https://paypal.com@evil.example",
        "https://example.com/%250a",
        "https://example.com/%0a",
        "https://example.com/%5c",
        "https://example.com/%zz",
        "https://[::ffff:127.0.0.1]",
        "https://0x7f.0x0.0x0.0x1/",
        "https://127.0x/",
        "https://0xa.0x0.0x0.0x1/",
        "https://0x7f000001/",
        "https://2130706433/",
        "https://0177.0.0.1/",
        "https://0177.00.00.01/",
        "https://10.0.0x0.0x1/",
        "https://10.0.00.01/",
        "https://0X7F.0X0.0X0.0X1/",
        "https://-bad.example/",
        "https://exa mple.com/",
        "https://example.com\\@evil/",
        "javascript:alert(1)",
    ] {
        assert_eq!(offers::safe_https_url(u), None, "{u}");
    }
    assert_eq!(
        offers::safe_https_url("HTTPS://Example.com."),
        Some("https://example.com/".into())
    );
}

#[test]
fn at_most_three_links_and_two_buttons() {
    let o = offers::scan(
        "",
        "https://a.example/1 https://b.example/2 https://c.example/3 https://d.example/4 https://a.example/1",
    );
    assert_eq!(o.links.len(), 3, "{o:?}");
    assert_eq!(o.buttons(), [Offer::Link("https://a.example/1".into())]);
    let two = offers::scan("", "code 482913, backup code 771204, call +44 7911 123456");
    assert_eq!(
        two.buttons(),
        [Offer::Code("482913".into()), Offer::Code("771204".into())]
    );
    assert!(
        !two.closes_after_copy("482913"),
        "the other code is still in it"
    );
    assert!(offers::scan("", "Your code is 938271").closes_after_copy("938271"));
}

#[test]
fn a_link_tooltip_names_its_host() {
    assert_eq!(offers::host("https://example.com:8443/a?b"), "example.com");
    assert_eq!(offers::host("https://example.com/"), "example.com");
}

/// Localisable keywords: another list finds codes the built-in one does not.
#[test]
fn keywords_are_a_list() {
    assert!(
        offers::scan("", "Ditt engångslösenord: 482913")
            .codes
            .is_empty()
    );
    let sv = offers::scan_with("", "Ditt engångslösenord: 482913", &["engångslösenord"]);
    assert_eq!(sv.codes, ["482913"]);
}

/// A clipboard in memory that records every call.
#[derive(Default)]
struct Stub {
    held: String,
    sensitive: bool,
    log: Vec<String>,
}

impl Clipboard for Stub {
    fn copy(&mut self, text: &str, sensitive: bool) -> io::Result<()> {
        self.held = text.into();
        self.sensitive = sensitive;
        self.log.push(format!("copy {sensitive} {text}"));
        Ok(())
    }
    fn paste(&mut self) -> io::Result<String> {
        self.log.push("paste".into());
        Ok(self.held.clone())
    }
    fn clear(&mut self) -> io::Result<()> {
        self.held.clear();
        self.log.push("clear".into());
        Ok(())
    }
}

#[test]
fn a_code_is_sensitive_and_cleared_only_while_it_is_still_there() {
    let secrets = Secrets::default();
    let mut c = Stub::default();
    let t = offers::copy_code(&mut c, &secrets, "938271").unwrap();
    assert!(c.sensitive);
    assert!(offers::expire(&mut c, &secrets, &t).unwrap());
    assert_eq!(c.held, "");
    assert_eq!(c.log, ["copy true 938271", "paste", "clear"]);

    // The person copied something else meanwhile: it stays.
    let t = offers::copy_code(&mut c, &secrets, "938271").unwrap();
    c.held = "shopping list".into();
    assert!(!offers::expire(&mut c, &secrets, &t).unwrap());
    assert_eq!(c.held, "shopping list");

    // A later code restarts the clock: the first timer leaves the second code alone.
    let first = offers::copy_code(&mut c, &secrets, "111111").unwrap();
    let second = offers::copy_code(&mut c, &secrets, "222222").unwrap();
    assert!(!offers::expire(&mut c, &secrets, &first).unwrap());
    assert_eq!(c.held, "222222");
    assert!(offers::expire(&mut c, &secrets, &second).unwrap());

    // A number is a normal copy.
    offers::copy_number(&mut c, "+44 7911 123456").unwrap();
    assert!(!c.sensitive);
    assert_eq!(
        offers::copy_argv("938271", true),
        ["--sensitive", "--", "938271"]
    );
    assert_eq!(offers::copy_argv("-1", false), ["--", "-1"]);
}

#[cfg(unix)]
fn stub(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// The real `wl-copy`/`wl-paste` calls, with stub programs that log their arguments.
#[cfg(unix)]
#[test]
fn wayland_runs_wl_copy_sensitive_and_clears_only_on_the_same_code() {
    let tmp = std::env::temp_dir().join(format!("wol-offers-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let log = tmp.join("log");
    let board = tmp.join("board");
    let copy = stub(
        &tmp,
        "wl-copy",
        &format!(
            r#"echo "wl-copy $*" >> '{log}'
[ "$1" = --clear ] && {{ : > '{board}'; exit 0; }}
for a; do v=$a; done; printf %s "$v" > '{board}'
(sleep 30) &"#,
            log = log.display(),
            board = board.display()
        ),
    );
    let paste = stub(
        &tmp,
        "wl-paste",
        &format!(
            r#"echo "wl-paste $*" >> '{log}'; cat '{board}'"#,
            log = log.display(),
            board = board.display()
        ),
    );
    let mut c = offers::Wayland { copy, paste };
    let secrets = Secrets::default();
    let t0 = std::time::Instant::now();
    let t = offers::copy_code(&mut c, &secrets, "938271").unwrap();
    assert!(
        t0.elapsed().as_secs() < 5,
        "waited for the clipboard daemon"
    );
    assert!(offers::expire(&mut c, &secrets, &t).unwrap());
    let t = offers::copy_code(&mut c, &secrets, "938271").unwrap();
    std::fs::write(&board, "something else").unwrap();
    assert!(!offers::expire(&mut c, &secrets, &t).unwrap());
    let said = std::fs::read_to_string(&log).unwrap();
    let _ = std::fs::remove_dir_all(&tmp);
    assert_eq!(
        said.lines().collect::<Vec<_>>(),
        [
            "wl-copy --sensitive -- 938271",
            "wl-paste --no-newline",
            "wl-copy --clear",
            "wl-copy --sensitive -- 938271",
            "wl-paste --no-newline",
        ]
    );
}

/// The buttons' labels and tooltips are copy (`shell::copy`) and pass its lint.
#[test]
fn offer_copy_passes_the_lint() {
    use shell::copy;
    let o = offers::scan(
        "",
        "Your code is 938271, see https://example.com/a or call +44 7911 123456",
    );
    let several = offers::scan("", "code 482913, backup code 771204");
    let all = [
        o.codes.iter().cloned().map(Offer::Code).collect::<Vec<_>>(),
        o.links.iter().cloned().map(Offer::Link).collect(),
        o.phones.iter().cloned().map(Offer::Phone).collect(),
    ]
    .concat();
    assert_eq!(all.len(), 3, "{o:?}");
    for offer in &all {
        let tip = copy::offer_tip(offer);
        assert!(copy::lint(&tip).is_empty(), "{tip}: {:?}", copy::lint(&tip));
        assert!(!copy::offer_label(offer, false).is_empty());
    }
    assert_eq!(copy::offer_label(&all[0], false), "Copy Code");
    assert_eq!(copy::offer_label(&all[1], false), "Open Link");
    assert_eq!(copy::offer_label(&all[2], false), "Copy Number");
    assert_eq!(
        copy::offer_label(&Offer::Code(several.codes[1].clone()), true),
        "Copy 771204"
    );
    for s in [copy::OFFER_CODE_TIP, copy::OFFER_NUMBER_TIP] {
        assert!(copy::lint(s).is_empty(), "{s}: {:?}", copy::lint(s));
    }
}
