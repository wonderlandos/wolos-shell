// SPDX-License-Identifier: GPL-2.0-or-later
//! Plugin Maker Create Package and Share (ADR 0031 "Plugin Maker / Theme Studio"): Create Package
//! runs `wol plugin pack` (the `.wolplug` format of `wol_core::package`, written next to the
//! folder), and Share shows the plugin index request as text: the index entry with the
//! package's SHA-256 and size, and the command that opens the pull request.
//!
//! Invariant: the app writes nothing to the network. The person copies the steps and runs them;
//! the index CI checks the plugin again (`wol plugin check`) and signs it.
use crate::copy::maker as copy;
use shared::plugins::Manifest;
use std::path::{Path, PathBuf};
use std::time::Duration;
use wol_core::plugins as cp;

/// How long `wol plugin pack` may take (a 1 MiB folder at zstd level 19 takes about a second).
pub const PACK_TIMEOUT: Duration = Duration::from_secs(60);

/// A package made by Create Package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packed {
    pub file: PathBuf,
    /// SHA-256 of the `.wolplug` file (what the index entry pins).
    pub sha256: String,
    pub size: u64,
    pub manifest: Manifest,
}

/// `wol plugin pack <dir> --yes --json`: the CLI's own pack, replacing an older package of the
/// same version next to the folder (the person asked for a new one).
pub fn pack_argv(dir: &Path) -> Vec<String> {
    vec![
        "wol".into(),
        "plugin".into(),
        "pack".into(),
        dir.display().to_string(),
        "--yes".into(),
        "--json".into(),
    ]
}

/// The package path from `wol plugin pack --json` (`{"file": ...}`).
pub fn packed_file(out: &str) -> Option<PathBuf> {
    let v: serde_json::Value = serde_json::from_str(out).ok()?;
    v.get("file")?.as_str().map(PathBuf::from)
}

/// Create Package: packs `dir` with the CLI, then hashes the file. Blocks (up to
/// [`PACK_TIMEOUT`]): run it off the GTK thread. The error is the sentence.
pub fn pack(dir: &Path) -> Result<Packed, &'static str> {
    let manifest = cp::check(dir, false).manifest.ok_or(copy::NOT_PACKED)?;
    let argv = pack_argv(dir);
    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    let out = wol_core::system::run_with_timeout(&refs, PACK_TIMEOUT).ok_or(copy::NOT_PACKED)?;
    let file = packed_file(&out).ok_or(copy::NOT_PACKED)?;
    let sha256 = wol_core::package::sha256_file(&file).map_err(|_| copy::NOT_PACKED)?;
    let size = std::fs::metadata(&file)
        .map_err(|_| copy::NOT_PACKED)?
        .len();
    Ok(Packed {
        file,
        sha256,
        size,
        manifest,
    })
}

/// The plugin index entry for `p` (format 1, `wopkg::index::plugins::Entry`); `url` is left for
/// the person to fill with where they put the file.
pub fn entry(p: &Packed) -> wopkg::index::plugins::Entry {
    let m = &p.manifest;
    let name = p
        .file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    wopkg::index::plugins::Entry {
        id: m.id.clone(),
        name: m.name.clone(),
        version: m.version.clone(),
        summary: m.description.clone(),
        author: m.author.clone(),
        license: m.license.clone(),
        kinds: m.kinds().iter().map(|k| k.id().to_owned()).collect(),
        permissions: m.permissions.iter().map(|x| x.id().to_owned()).collect(),
        min_wolos: m.min_wolos.clone(),
        url: format!("https://example.com/{name}"),
        sha256: p.sha256.clone(),
        size: p.size,
        // WolOS Hub adds the file certificate when it publishes the entry.
        filecert: String::new(),
    }
}

/// The share sheet's text: steps, the entry, the command.
pub fn steps(p: &Packed) -> String {
    let e = entry(p);
    let json = serde_json::to_string_pretty(&e).unwrap_or_default();
    let file = p
        .file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    copy::publish_steps(&file, &e.id, &e.version, &json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::plugins::Kind;

    #[test]
    fn the_pack_is_the_cli_verb_on_the_folder() {
        assert_eq!(
            pack_argv(Path::new("/home/a/p/alice.demo")),
            [
                "wol",
                "plugin",
                "pack",
                "/home/a/p/alice.demo",
                "--yes",
                "--json"
            ]
        );
        let out =
            "{\n  \"file\": \"/home/a/p/alice.demo-0.1.wolplug\",\n  \"id\": \"alice.demo\"\n}\n";
        assert_eq!(
            packed_file(out),
            Some(PathBuf::from("/home/a/p/alice.demo-0.1.wolplug"))
        );
        assert_eq!(packed_file("not json"), None);
    }

    #[test]
    fn the_index_entry_pins_the_file_hash_and_reads_back() {
        let files = cp::scaffold("alice.demo", "Demo", &[Kind::Menubar, Kind::Tile]).unwrap();
        let m = shared::plugins::parse(&files[0].1).unwrap();
        let p = Packed {
            file: PathBuf::from("/x/alice.demo-0.1.wolplug"),
            sha256: "ab".repeat(32),
            size: 1234,
            manifest: m,
        };
        let e = entry(&p);
        assert_eq!(e.sha256, "ab".repeat(32));
        assert_eq!(e.size, 1234);
        assert_eq!(e.kinds, ["menubar", "tile", "toggle"]);
        assert_eq!(e.permissions, ["toggles"]);
        let text = steps(&p);
        assert!(text.contains(&"ab".repeat(32)), "{text}");
        assert!(text.contains("alice.demo-0.1.wolplug"));
        assert!(text.contains("gh pr create"));
        // The entry in the sheet is what the index decoder reads.
        let json = serde_json::to_string(&e).unwrap();
        let back: wopkg::index::plugins::Entry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, e);
    }
}
