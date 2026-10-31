//! App Store search through wopkg's local index (ADR 0021): milliseconds instead of running
//! `pacman -Ss` and `flatpak search`. None when the index has not been built yet, so the caller
//! falls back to the old commands.
use serde_json::Value;
use wol_core::{flatpak, store::PkgHit};

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
