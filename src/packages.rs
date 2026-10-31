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
pub const AUR_RISK: &str = "Built on this computer from a script a stranger wrote. Nobody at WolOS reviewed it, and no check can prove it is safe.";

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

/// What reading a package found, in the words the App Store shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Audit {
    pub heading: String,
    pub findings: Vec<String>,
    /// The drawn graph, when there is one.
    pub graph: Option<String>,
}

/// wopkg's `audit --json` answer. Anything else is "not checked", never a clean result.
pub fn parse_audit(text: &str) -> Audit {
    let doc: Value = serde_json::from_str(text).unwrap_or_default();
    let heading = match doc["verdict"].as_str() {
        Some("pass") => "No law broken. That does not mean this package is safe.",
        Some("flag" | "block") => "Needs review.",
        _ => "Not checked.",
    };
    let findings = doc["findings"].as_array().into_iter().flatten();
    Audit {
        heading: heading.to_owned(),
        findings: findings
            .filter_map(|f| f["sentence"].as_str().map(str::to_owned))
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
    fn an_audit_never_reads_as_safe_and_a_bad_answer_is_not_checked() {
        let a = parse_audit(r#"{"verdict":"pass","findings":[]}"#);
        assert_eq!(
            a.heading,
            "No law broken. That does not mean this package is safe."
        );
        let a = parse_audit(
            r#"{"verdict":"flag","findings":[{"sentence":"curl runs it."}],"graph":"/g.svg"}"#,
        );
        assert_eq!(a.heading, "Needs review.");
        assert_eq!(a.findings, ["curl runs it."]);
        assert_eq!(a.graph.as_deref(), Some("/g.svg"));
        for bad in ["", "nonsense", r#"{"verdict":"fine"}"#] {
            assert_eq!(parse_audit(bad).heading, "Not checked.");
        }
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
