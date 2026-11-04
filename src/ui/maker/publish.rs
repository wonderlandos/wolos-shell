// SPDX-License-Identifier: GPL-2.0-or-later
//! Plugin Maker Create Package and Share (ADR 0031 "Plugin Maker / Theme Studio"): Create Package
//! runs `wol plugin pack` (the `.wolplug` format of `wol_core::package`, written to Downloads
//! (or the home folder)), and Share shows the plugin index request as text: the index entry with the
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

/// `wol plugin pack <dir> <out> --yes --json`: the CLI's own pack, into the folder `out`
/// (QA-PLUG-11: the person's Downloads, not the hidden plugin folder), replacing an older package
/// of the same version there (the person asked for a new one).
pub fn pack_argv(dir: &Path, out: &Path) -> Vec<String> {
    vec![
        "wol".into(),
        "plugin".into(),
        "pack".into(),
        dir.display().to_string(),
        out.display().to_string(),
        "--yes".into(),
        "--json".into(),
    ]
}

/// Where Create Package puts the file: `Downloads` in `home` when it is there, else `home`.
pub fn pack_out(home: &Path) -> PathBuf {
    let d = home.join("Downloads");
    if d.is_dir() { d } else { home.to_path_buf() }
}

/// The package path from `wol plugin pack --json` (`{"file": ...}`).
pub fn packed_file(out: &str) -> Option<PathBuf> {
    let v: serde_json::Value = serde_json::from_str(out).ok()?;
    v.get("file")?.as_str().map(PathBuf::from)
}

/// Stdout of `argv` when it exits 0 within `timeout`; else the CLI's own reason (its
/// `wol: ...` line without the prefix), or "" when it did not run or said nothing.
fn run_pack(argv: &[String], timeout: Duration) -> Result<String, String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let (file, args) = argv.split_first().ok_or_else(String::new)?;
    let mut child = Command::new(file)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| String::new())?;
    // Both pipes are read on their own threads (a full pipe would stop the child), at most
    // PACK_OUT_MAX bytes each: the CLI's answer is a few lines.
    let read = |name: &str, r: Option<Box<dyn Read + Send>>| {
        std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                let mut b = Vec::new();
                if let Some(r) = r {
                    let _ = r.take(PACK_OUT_MAX).read_to_end(&mut b);
                }
                String::from_utf8_lossy(&b).into_owned()
            })
    };
    let (Ok(out), Ok(err)) = (
        read("pack-out", child.stdout.take().map(|s| Box::new(s) as _)),
        read("pack-err", child.stderr.take().map(|s| Box::new(s) as _)),
    ) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(String::new());
    };
    let until = std::time::Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if std::time::Instant::now() < until => {
                std::thread::park_timeout(PACK_POLL);
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(String::new());
            }
        }
    };
    let (out, err) = (
        out.join().unwrap_or_default(),
        err.join().unwrap_or_default(),
    );
    if status.success() {
        Ok(out)
    } else {
        Err(reason_of(&err))
    }
}

/// The most of the CLI's output that is read.
const PACK_OUT_MAX: u64 = 64 * 1024;
/// How often a running pack is looked at.
const PACK_POLL: Duration = Duration::from_millis(50);

/// The CLI's reason from its stderr: the first `wol: ...` line, prefix and trailing dot kept
/// out so it reads inside the sentence; "" when there is none.
pub fn reason_of(stderr: &str) -> String {
    stderr
        .lines()
        .find_map(|l| l.strip_prefix("wol: "))
        .unwrap_or("")
        .trim()
        .to_owned()
}

/// Create Package: packs `dir` with the CLI into `out`, then hashes the file. Blocks (up to
/// [`PACK_TIMEOUT`]): run it off the GTK thread. The error is the sentence, with the CLI's reason
/// when it gave one (QA-PLUG-55: it was always "Press Check").
pub fn pack(dir: &Path, out: &Path) -> Result<Packed, String> {
    let manifest = cp::check(dir, false)
        .manifest
        .ok_or_else(|| copy::NOT_PACKED.to_owned())?;
    let argv = pack_argv(dir, out);
    let text = run_pack(&argv, PACK_TIMEOUT).map_err(|why| copy::not_packed(&why))?;
    let file = packed_file(&text).ok_or_else(|| copy::NOT_PACKED.to_owned())?;
    let sha256 = wol_core::package::sha256_file(&file).map_err(|_| copy::NOT_PACKED.to_owned())?;
    let size = std::fs::metadata(&file)
        .map_err(|_| copy::NOT_PACKED.to_owned())?
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
        // Not an address that works: the person must put theirs (QA-PLUG-54).
        url: format!("https://REPLACE-WITH-YOUR-ADDRESS.invalid/{name}"),
        sha256: p.sha256.clone(),
        size: p.size,
    }
}

/// What a plugin still has from the scaffold that the index would not take (QA-PLUG-75): no
/// author, or the starting one-line summary. The sentence to show instead of the Share sheet.
pub fn scaffold_leftover(m: &Manifest) -> Option<&'static str> {
    if m.author.trim().is_empty() || m.description.trim() == cp::SCAFFOLD_SUMMARY {
        Some(copy::SHARE_FILL_IN)
    } else {
        None
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
            pack_argv(
                Path::new("/home/a/p/alice.demo"),
                Path::new("/home/a/Downloads")
            ),
            [
                "wol",
                "plugin",
                "pack",
                "/home/a/p/alice.demo",
                "/home/a/Downloads",
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

    /// QA-PLUG-11: the package goes to Downloads when the person has one, else home.
    #[test]
    fn the_package_goes_where_the_person_looks() {
        let home = std::env::temp_dir().join(format!("wol-pack-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        assert_eq!(pack_out(&home), home);
        std::fs::create_dir_all(home.join("Downloads")).unwrap();
        assert_eq!(pack_out(&home), home.join("Downloads"));
        let _ = std::fs::remove_dir_all(&home);
    }

    /// QA-PLUG-55: a failed pack keeps the CLI's reason in the sentence; nothing to quote keeps
    /// the old advice.
    #[test]
    fn a_failed_pack_says_why() {
        assert_eq!(
            reason_of("noise\nwol: alice.demo has a problem: bar.lua is missing.\n"),
            "alice.demo has a problem: bar.lua is missing."
        );
        assert_eq!(reason_of("no prefix\n"), "");
        let t = copy::not_packed("zstd is missing, so the plugin cannot be packed.");
        assert!(t.contains("zstd is missing"), "{t}");
        assert_eq!(copy::not_packed(""), copy::NOT_PACKED);
        let dir = std::env::temp_dir().join(format!("wol-pack-fail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Not a plugin folder: refused before the CLI runs, with the generic sentence.
        assert_eq!(pack(&dir, &dir).unwrap_err(), copy::NOT_PACKED);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// QA-PLUG-75: Share is refused while author or summary are the scaffold's; the entry's
    /// address is a placeholder nobody could mistake for a working one (QA-PLUG-54).
    #[test]
    fn share_refuses_scaffold_summary() {
        let files = cp::scaffold("alice.demo", "Demo", &[Kind::Menubar]).unwrap();
        let mut m = shared::plugins::parse(&files[0].1).unwrap();
        assert_eq!(scaffold_leftover(&m), Some(copy::SHARE_FILL_IN));
        m.author = "Alice".into();
        assert_eq!(
            scaffold_leftover(&m),
            Some(copy::SHARE_FILL_IN),
            "summary still the scaffold's"
        );
        m.description = "Shows the weather.".into();
        assert_eq!(scaffold_leftover(&m), None);
        let p = Packed {
            file: PathBuf::from("/x/alice.demo-0.1.wolplug"),
            sha256: "ab".repeat(32),
            size: 1,
            manifest: m,
        };
        assert!(entry(&p).url.contains("REPLACE"), "{}", entry(&p).url);
        assert!(!steps(&p).contains("example.com"));
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
