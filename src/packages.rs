//! App Store search through wopkg's local index (ADR 0021): milliseconds instead of running
//! `pacman -Ss` and `flatpak search`. None when the index has not been built yet, so the caller
//! falls back to the old commands.
use serde_json::Value;
use wol_core::{controls::ControlError, flatpak, store, store::PkgHit};

/// The package tool. Installing, removing and updating go through it (ADR 0021): it runs as the
/// person and asks wold for the system change, which checks the consent grant.
pub const WOPKG: &str = "/usr/bin/wopkg";

/// Native hits (WolOS and Arch repos, `.wol`) and Flathub hits that no native package covers,
/// from one `wopkg::search` document.
pub fn parse_hits(doc: &Value) -> (Vec<PkgHit>, Vec<flatpak::Hit>) {
    let (mut pkgs, mut flat) = (Vec::new(), Vec::new());
    for hit in doc["results"].as_array().into_iter().flatten() {
        let sources = hit["sources"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        let of = |kinds: &[&str]| {
            sources
                .iter()
                .find(|s| s["kind"].as_str().is_some_and(|k| kinds.contains(&k)))
        };
        let text = |v: &Value, k: &str| v[k].as_str().unwrap_or_default().to_owned();
        if let Some(s) = of(&["pacman", "wol"]) {
            pkgs.push(PkgHit {
                name: text(s, "id"),
                summary: text(&hit, "summary"),
            });
        } else if let Some(s) = of(&["flatpak"]) {
            flat.push(flatpak::Hit {
                id: text(s, "id"),
                name: text(&hit, "name"),
                summary: text(&hit, "summary"),
            });
        }
    }
    (pkgs, flat)
}

/// Search the local index; None when there is no index (or no home) yet.
pub fn search(term: &str) -> Option<(Vec<PkgHit>, Vec<flatpak::Hit>)> {
    let ctx = wopkg::context::Context::new(Default::default()).ok()?;
    if ctx.segments().is_empty() {
        return None;
    }
    Some(parse_hits(&wopkg::search(&ctx, term, 30)))
}

/// What the App Store says about every AUR package, wherever one appears.
pub const AUR_RISK: &str = "Built on this computer from a script a stranger wrote. Nobody at WolOS reviewed it, and no check can vouch for it.";

/// Packages only the AUR has, from one `wopkg::search` document.
pub fn aur_hits(doc: &Value) -> Vec<PkgHit> {
    let mut out = Vec::new();
    for hit in doc["results"].as_array().into_iter().flatten() {
        let sources = hit["sources"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        let aur = sources.iter().find(|s| s["kind"] == "aur");
        if let (Some(s), 1) = (aur, sources.len()) {
            out.push(PkgHit {
                name: s["id"].as_str().unwrap_or_default().to_owned(),
                summary: hit["summary"].as_str().unwrap_or_default().to_owned(),
            });
        }
    }
    out
}

/// The AUR's matches from the local index.
pub fn aur_search(term: &str) -> Vec<PkgHit> {
    let Ok(ctx) = wopkg::context::Context::new(Default::default()) else {
        return Vec::new();
    };
    aur_hits(&wopkg::search(&ctx, term, 30))
}

/// The AUR stays out of search until the person asks for it.
fn aur_choice() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    let base = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{home}/.config"));
    std::path::Path::new(&base).join("wolos/store-aur")
}

pub fn aur_enabled() -> bool {
    aur_choice().is_file()
}

/// Remember the choice. A failure leaves the safer answer in place.
pub fn set_aur_enabled(on: bool) {
    let p = aur_choice();
    if on {
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, "");
    } else {
        let _ = std::fs::remove_file(p);
    }
}

/// Where a package comes from, in the same words as `wopkg info`.
pub fn origin(kind: &str, repo: &str) -> String {
    match (kind, repo) {
        ("pacman", r)
            if r == "wolos" || r.starts_with("wolos-cachy") || r.starts_with("cachyos") =>
        {
            "WolOS"
        }
        ("pacman", _) => "Arch",
        ("flatpak", _) => "Flathub",
        ("aur", _) => "AUR",
        ("appimage", _) => "AppImage",
        ("recipe", _) => "WolOS Recipe",
        ("wol", _) => "WolOS",
        _ => "Unknown",
    }
    .to_owned()
}

/// One place an app can come from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppSource {
    pub origin: String,
    pub kind: String,
    pub id: String,
    pub version: String,
    pub installed: bool,
}

/// The sources of one `wopkg info` answer, best first. The AUR keeps to its own tab.
pub fn parse_sources(hit: &Value) -> Vec<AppSource> {
    let text = |v: &Value, k: &str| v[k].as_str().unwrap_or_default().to_owned();
    hit["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["kind"] != "aur")
        .map(|s| AppSource {
            origin: origin(
                s["kind"].as_str().unwrap_or_default(),
                s["repo"].as_str().unwrap_or_default(),
            ),
            kind: text(s, "kind"),
            id: text(s, "id"),
            version: text(s, "version"),
            installed: s["installed"].as_bool().unwrap_or(false),
        })
        .collect()
}

/// Every source of one app, from the local index. Empty when there is no index yet.
pub fn sources(name: &str) -> Vec<AppSource> {
    let Ok(ctx) = wopkg::context::Context::new(Default::default()) else {
        return Vec::new();
    };
    wopkg::info(&ctx, name)
        .map(|h| parse_sources(&h))
        .unwrap_or_default()
}

/// Each name's first origin, for the badges on tiles.
pub fn origins(names: &[String]) -> Vec<(String, String)> {
    let Ok(ctx) = wopkg::context::Context::new(Default::default()) else {
        return Vec::new();
    };
    names
        .iter()
        .filter_map(|n| {
            let hit = wopkg::info(&ctx, n).ok()?;
            Some((n.clone(), parse_sources(&hit).first()?.origin.clone()))
        })
        .collect()
}

/// The last change to `name` that can still be undone, from `wopkg history --json`.
pub fn undoable(history: &str, name: &str) -> Option<String> {
    let doc: Value = serde_json::from_str(history).ok()?;
    doc["entries"]
        .as_array()?
        .iter()
        .rev()
        .find(|e| e["name"] == name && e["undone"] != true && !e["undo"].is_null())
        .and_then(|e| e["id"].as_str().map(str::to_owned))
}

/// Run one `wopkg` verb on a name; did it work.
pub fn wopkg_verb(args: &[&str]) -> bool {
    let mut a = vec![WOPKG];
    a.extend_from_slice(args);
    a.push("-y");
    wol_core::system::run_with_timeout(&a, std::time::Duration::from_secs(300)).is_some()
}

/// The undo id for `name`, asked of wopkg.
pub fn last_change(name: &str) -> Option<String> {
    let out = wol_core::system::run_with_timeout(
        &[WOPKG, "history", "--json"],
        std::time::Duration::from_secs(10),
    )?;
    undoable(&out, name)
}

/// The version on this computer, when the system says.
pub fn installed_version(id: &str, flatpak: bool) -> Option<String> {
    let t = std::time::Duration::from_secs(5);
    if flatpak {
        let out = wol_core::system::run_with_timeout(&["flatpak", "info", id], t)?;
        return out.lines().find_map(|l| {
            l.trim()
                .strip_prefix("Version:")
                .map(|v| v.trim().to_owned())
        });
    }
    let out = wol_core::system::run_with_timeout(&["pacman", "-Q", id], t)?;
    out.split_whitespace().nth(1).map(str::to_owned)
}

/// Is this package pinned? `$XDG_STATE_HOME/wopkg/pins`, written by `wopkg pin`.
pub fn pinned(id: &str) -> bool {
    let home = std::env::var("HOME").unwrap_or_default();
    let state = std::env::var("XDG_STATE_HOME").unwrap_or_else(|_| format!("{home}/.local/state"));
    std::fs::read_to_string(format!("{state}/wopkg/pins"))
        .is_ok_and(|t| t.lines().any(|l| l.trim() == id))
}

/// One state for every package, in the same words everywhere: (name, icon, what it means).
pub const STATES: [(&str, &str, &str); 4] = [
    (
        "No Law Broken",
        "object-select-symbolic",
        "The check found nothing against its laws. That is no promise about the package.",
    ),
    (
        "Needs Review",
        "dialog-warning-symbolic",
        "At least one law was broken. Read what was found before you build it.",
    ),
    (
        "Blocked",
        "system-lock-screen-symbolic",
        "A secret left the build, or the build did something it hid. wopkg does not install it.",
    ),
    (
        "Not Checked",
        "help-about-symbolic",
        "The check did not run or could not answer, so nothing is known.",
    ),
];

/// How much a flag says: (name, what it means).
pub const SEVERITIES: [(&str, &str); 3] = [
    ("High", "It runs or sends something it should not."),
    (
        "Medium",
        "It reaches past its own files, or can't be tied to upstream.",
    ),
    (
        "Low",
        "Something is out of place or could not be read. Often harmless.",
    ),
];

/// Every law as the flag people see: (title, severity, what it means). Titles match the analyser's.
pub const FLAGS: [(&str, &str, &str); 14] = [
    (
        "Runs a Download Unchecked",
        "High",
        "Something fetched from the network runs with no checksum checking it first.",
    ),
    (
        "Runs Hidden Code",
        "High",
        "Data is decoded or rebuilt, then run as code.",
    ),
    (
        "Sends Secrets Out",
        "High",
        "A key, token or password file reaches the network.",
    ),
    (
        "Reads Secrets",
        "High",
        "It opens SSH keys, browser profiles or other credentials.",
    ),
    (
        "Uses the Network While Installing",
        "High",
        "An install script reaches the network as the administrator.",
    ),
    (
        "Did What It Didn't Say",
        "High",
        "The build did something its files never mention.",
    ),
    (
        "Writes Outside Its Folders",
        "Medium",
        "The build changes files outside the package and source folders.",
    ),
    (
        "Changes Files It Doesn't Own",
        "Medium",
        "An install script changes a file the package does not declare.",
    ),
    (
        "Starts Itself Again",
        "Medium",
        "It sets itself to run at login or boot without declaring it.",
    ),
    (
        "Binary Not Tied to Upstream",
        "Medium",
        "A prebuilt binary has no checksum, or not the one upstream published.",
    ),
    (
        "New Behaviour in This Version",
        "Medium",
        "This version does something the last one did not, with no explanation.",
    ),
    (
        "Downloads From Another Site",
        "Low",
        "A source is not on the project's declared home.",
    ),
    (
        "Runs Code That Can't Be Read",
        "Low",
        "Code is put together while running, so the check can't say what it does.",
    ),
    (
        "Not Fully Read",
        "Low",
        "Part of the package could not be read, so the answer rests on less.",
    ),
];

/// A law the package broke, and how many times.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AuditFlag {
    pub title: String,
    pub severity: String,
    pub count: u64,
}

/// What `wopkg audit` said about a package, in the words the Store shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Audit {
    /// One of `STATES`.
    pub state: String,
    pub flags: Vec<AuditFlag>,
    /// (flag, the exact path in one sentence).
    pub findings: Vec<(String, String)>,
    pub graph: Option<String>,
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// One document of `wopkg audit --json`. Anything else is not an answer.
pub fn parse_audit(text: &str) -> Audit {
    let doc: Value = serde_json::from_str(text).unwrap_or_default();
    let state = match doc["verdict"].as_str() {
        Some("pass") => STATES[0].0,
        Some("flag") => STATES[1].0,
        Some("block") => STATES[2].0,
        _ => STATES[3].0,
    };
    let text_of = |v: &Value, k: &str| v[k].as_str().unwrap_or_default().to_owned();
    Audit {
        state: state.to_owned(),
        flags: doc["flags"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| AuditFlag {
                title: text_of(f, "title"),
                severity: capital(f["severity"].as_str().unwrap_or_default()),
                count: f["count"].as_u64().unwrap_or(1),
            })
            .collect(),
        findings: doc["findings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|f| f["sentence"].is_string())
            .map(|f| (text_of(f, "title"), text_of(f, "sentence")))
            .collect(),
        graph: doc["graph"].as_str().map(str::to_owned),
    }
}

pub fn audit_argv(target: &str) -> Option<Vec<String>> {
    plain(target).then(|| {
        [WOPKG, "audit", "--json", target]
            .map(String::from)
            .to_vec()
    })
}

/// Blocking: fetch the package's files and read them. Nothing is built.
pub fn audit(target: &str) -> Audit {
    let out = audit_argv(target).and_then(|a| {
        let a: Vec<&str> = a.iter().map(String::as_str).collect();
        wol_core::system::run_with_timeout(&a, std::time::Duration::from_secs(60))
    });
    parse_audit(&out.unwrap_or_default())
}

/// The package a catalog id or name stands for (`wopkg` takes package names).
fn package_of(target: &str) -> String {
    store::get(target)
        .and_then(|a| a.packages.first().map(|p| (*p).to_owned()))
        .unwrap_or_else(|| target.to_owned())
}

/// Names that cannot be mistaken for options and hold only what a package name may.
fn plain(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"@._+-".contains(&b))
}

pub fn install_argv(target: &str) -> Option<Vec<String>> {
    let p = package_of(target);
    plain(&p).then(|| {
        ["install", "-y", "--from", "pacman", "--", &p]
            .map(String::from)
            .to_vec()
    })
}

pub fn remove_argv(target: &str) -> Option<Vec<String>> {
    let p = package_of(target);
    plain(&p).then(|| {
        ["remove", "-y", "--from", "pacman", "--", &p]
            .map(String::from)
            .to_vec()
    })
}

pub fn upgrade_argv() -> Vec<String> {
    ["update", "-y", "--only", "system,pacman"]
        .map(String::from)
        .to_vec()
}

/// wopkg's exit code in the Store's own error kinds (`copy.rs` words them): 0 done, 2 bad request,
/// 77 wold refused (the consent grant is off), anything else did not finish.
pub fn outcome(code: Option<i32>) -> Result<(), ControlError> {
    match code {
        Some(0) => Ok(()),
        Some(2) => Err(ControlError::Invalid),
        Some(77) => Err(ControlError::Denied),
        Some(_) => Err(ControlError::Failed),
        None => Err(ControlError::Unavailable),
    }
}

/// Run wopkg with `argv`; its exit code, None when it could not start. Replaced in tests.
pub type Run<'a> = &'a dyn Fn(&[String]) -> Option<i32>;

pub fn run_wopkg(argv: &[String]) -> Option<i32> {
    std::process::Command::new(WOPKG)
        .args(argv)
        .stdin(std::process::Stdio::null())
        .status()
        .ok()
        .and_then(|s| s.code().or(Some(1)))
}

/// Install through wopkg; None when wopkg is not on this system (the caller keeps the old path).
pub fn install(target: &str, present: bool, run: Run) -> Option<Result<(), ControlError>> {
    present.then(|| install_argv(target).map_or(Err(ControlError::Invalid), |a| outcome(run(&a))))
}

pub fn remove(target: &str, present: bool, run: Run) -> Option<Result<(), ControlError>> {
    present.then(|| remove_argv(target).map_or(Err(ControlError::Invalid), |a| outcome(run(&a))))
}

pub fn upgrade(present: bool, run: Run) -> Option<Result<(), ControlError>> {
    present.then(|| outcome(run(&upgrade_argv())))
}

pub fn present() -> bool {
    std::path::Path::new(WOPKG).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_packages_nothing_else_has_are_aur_hits() {
        let doc = serde_json::json!({"results": [
            {"name": "a", "summary": "A", "sources": [{"kind": "aur", "id": "a"}]},
            {"name": "b", "summary": "B", "sources": [{"kind": "pacman", "id": "b"}, {"kind": "aur", "id": "b"}]},
        ]});
        let hits = aur_hits(&doc);
        assert_eq!(hits.len(), 1);
        assert_eq!(
            (hits[0].name.as_str(), hits[0].summary.as_str()),
            ("a", "A")
        );
    }

    #[test]
    fn every_answer_gets_one_state_and_a_bad_answer_is_not_checked() {
        assert_eq!(
            parse_audit(r#"{"verdict":"pass","findings":[]}"#).state,
            "No Law Broken"
        );
        let a = parse_audit(
            r#"{"verdict":"flag","graph":"/g.svg","flags":[{"title":"Reads Secrets","severity":"high","count":2}],"findings":[{"title":"Reads Secrets","sentence":"cat reads it."}]}"#,
        );
        assert_eq!(a.state, "Needs Review");
        assert_eq!(
            a.flags[0],
            AuditFlag {
                title: "Reads Secrets".into(),
                severity: "High".into(),
                count: 2
            }
        );
        assert_eq!(a.findings[0].1, "cat reads it.");
        assert_eq!(a.graph.as_deref(), Some("/g.svg"));
        assert_eq!(parse_audit(r#"{"verdict":"block"}"#).state, "Blocked");
        for bad in ["", "nope", r#"{"verdict":"fine"}"#] {
            assert_eq!(parse_audit(bad).state, "Not Checked");
        }
    }

    #[test]
    fn the_legend_names_every_flag_the_analyser_can_raise_and_never_says_safe() {
        let laws = include_str!("../../aurlaws/src/laws.rs");
        for (title, severity, _) in FLAGS {
            assert!(laws.contains(&format!("\"{title}\"")), "{title}");
            assert!(SEVERITIES.iter().any(|s| s.0 == severity));
        }
        let all = STATES
            .iter()
            .flat_map(|s| [s.0, s.2])
            .chain(FLAGS.iter().map(|f| f.2))
            .chain([AUR_RISK]);
        assert!(all.into_iter().all(|t| !t.to_lowercase().contains("safe")));
    }

    #[test]
    fn origins_read_the_same_as_wopkg_and_the_aur_stays_out() {
        let hit = serde_json::json!({"sources": [
            {"kind":"pacman","repo":"wolos","id":"gimp","version":"3","installed":true},
            {"kind":"pacman","repo":"extra","id":"gimp","version":"2"},
            {"kind":"flatpak","repo":"flathub","id":"org.gimp.GIMP","version":"3.1"},
            {"kind":"aur","repo":"aur","id":"gimp-git","version":"9"},
        ]});
        let s = parse_sources(&hit);
        let names: Vec<&str> = s.iter().map(|x| x.origin.as_str()).collect();
        assert_eq!(names, ["WolOS", "Arch", "Flathub"]);
        assert!(s[0].installed && !s[1].installed);
    }

    #[test]
    fn only_a_change_that_can_be_undone_is_offered() {
        let h = r#"{"entries":[
            {"id":"1","name":"gimp","undone":false,"undo":{"type":"run"}},
            {"id":"2","name":"gimp","undone":true,"undo":{"type":"run"}},
            {"id":"3","name":"gimp","undone":false,"undo":null},
            {"id":"4","name":"vlc","undone":false,"undo":{"type":"run"}}]}"#;
        assert_eq!(undoable(h, "gimp").as_deref(), Some("1"));
        assert_eq!(undoable(h, "krita"), None);
        assert_eq!(undoable("nope", "gimp"), None);
    }

    #[test]
    fn a_name_that_looks_like_an_option_is_never_audited() {
        assert_eq!(audit_argv("--evil"), None);
        assert_eq!(audit_argv("foo").unwrap()[1..], ["audit", "--json", "foo"]);
    }

    #[test]
    fn native_wins_and_flathub_only_apps_stay_flatpaks() {
        let doc = serde_json::json!({"results":[
            {"name":"Firefox","summary":"Web browser","sources":[
                {"kind":"flatpak","id":"org.mozilla.firefox"},
                {"kind":"pacman","id":"firefox"}]},
            {"name":"Foo","summary":"A Flathub app","sources":[{"kind":"flatpak","id":"org.foo.Foo"}]},
            {"name":"bar","summary":"AUR only","sources":[{"kind":"aur","id":"bar"}]},
            {"name":"Baz","summary":"AppImage only","sources":[{"kind":"appimage","id":"gh:a/b"}]}
        ]});
        let (pkgs, flat) = parse_hits(&doc);
        assert_eq!(
            pkgs,
            vec![PkgHit {
                name: "firefox".into(),
                summary: "Web browser".into()
            }]
        );
        assert_eq!(
            flat,
            vec![flatpak::Hit {
                id: "org.foo.Foo".into(),
                name: "Foo".into(),
                summary: "A Flathub app".into()
            }]
        );
        assert_eq!(parse_hits(&serde_json::json!({})), (vec![], vec![]));
    }

    #[test]
    fn install_remove_and_upgrade_run_wopkg_and_keep_the_stores_error_kinds() {
        let seen = std::cell::RefCell::new(Vec::<Vec<String>>::new());
        let code = std::cell::Cell::new(Some(0));
        let run = |a: &[String]| {
            seen.borrow_mut().push(a.to_vec());
            code.get()
        };
        assert_eq!(install("vim", true, &run), Some(Ok(())));
        assert_eq!(remove("vim", true, &run), Some(Ok(())));
        assert_eq!(upgrade(true, &run), Some(Ok(())));
        let s = seen
            .borrow()
            .iter()
            .map(|a| a.join(" "))
            .collect::<Vec<_>>();
        assert_eq!(
            s,
            [
                "install -y --from pacman -- vim",
                "remove -y --from pacman -- vim",
                "update -y --only system,pacman"
            ]
        );
        for (c, want) in [
            (Some(77), ControlError::Denied),
            (Some(2), ControlError::Invalid),
            (Some(1), ControlError::Failed),
            (Some(3), ControlError::Failed),
            (None, ControlError::Unavailable),
        ] {
            code.set(c);
            assert_eq!(install("vim", true, &run), Some(Err(want)), "{c:?}");
            assert_eq!(remove("vim", true, &run), Some(Err(want)));
            assert_eq!(upgrade(true, &run), Some(Err(want)));
        }
    }

    #[test]
    fn without_wopkg_the_old_path_stays() {
        let run = |_: &[String]| panic!("must not run");
        assert_eq!(install("vim", false, &run), None);
        assert_eq!(remove("vim", false, &run), None);
        assert_eq!(upgrade(false, &run), None);
    }

    #[test]
    fn a_name_that_looks_like_an_option_never_reaches_wopkg() {
        let run = |_: &[String]| panic!("must not run");
        for bad in [
            "--from=aur",
            "-y",
            "",
            "a b",
            "x;rm",
            "a/b",
            &"a".repeat(200),
        ] {
            assert_eq!(
                install(bad, true, &run),
                Some(Err(ControlError::Invalid)),
                "{bad:?}"
            );
            assert_eq!(remove(bad, true, &run), Some(Err(ControlError::Invalid)));
        }
    }

    #[test]
    fn a_catalog_id_becomes_its_package() {
        let a = store::CATALOG.first().expect("catalog");
        assert_eq!(
            install_argv(a.id).unwrap().last().map(String::as_str),
            a.packages.first().copied()
        );
    }
}
