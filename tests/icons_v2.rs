// SPDX-License-Identifier: GPL-2.0-or-later
//! Icon set v2 (design/tools/icons.py): one geometry for every icon the shell draws.
//! Symbolic: 16 grid, filled outlines of 1.5 px lines, `currentColor` only (GTK recolours them for
//! light and dark through CSS). App icons: 24-cell pixel tiles, rendered to PNG at 48/128/512 in
//! desktop/overlay/usr/share/icons/WolOS, whose edge reads on the light and the dark ground.
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// String literals in the shell sources that pass `keep` (same scan as symbolic_icons.rs).
fn quoted(keep: impl Fn(&str) -> bool) -> Vec<String> {
    let mut files = Vec::new();
    rs_files(&root().join("crates/shell/src"), &mut files);
    let mut names: Vec<String> = files
        .iter()
        .flat_map(|f| {
            let s = std::fs::read_to_string(f).unwrap();
            s.split('"')
                .skip(1)
                .step_by(2)
                .filter(|t| {
                    t.bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                        && keep(t)
                })
                .map(String::from)
                .collect::<Vec<_>>()
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Names GTK 4 and libadwaita look up in the icon theme for our widgets (entries, dropdowns,
/// expanders, spin buttons, dialogs, header bars), so they come from the WolOS set too.
const TOOLKIT: &[&str] = &[
    "pan-down-symbolic",
    "pan-up-symbolic",
    "pan-start-symbolic",
    "pan-end-symbolic",
    "open-menu-symbolic",
    "view-more-symbolic",
    "window-close-symbolic",
    "window-minimize-symbolic",
    "window-maximize-symbolic",
    "window-restore-symbolic",
    "edit-find-symbolic",
    "edit-clear-symbolic",
    "edit-copy-symbolic",
    "edit-cut-symbolic",
    "edit-paste-symbolic",
    "edit-undo-symbolic",
    "edit-redo-symbolic",
    "edit-select-all-symbolic",
    "edit-delete-symbolic",
    "object-select-symbolic",
    "list-add-symbolic",
    "list-remove-symbolic",
    "value-increase-symbolic",
    "value-decrease-symbolic",
    "go-previous-symbolic",
    "go-next-symbolic",
    "go-up-symbolic",
    "go-down-symbolic",
    "view-reveal-symbolic",
    "view-conceal-symbolic",
    "caps-lock-symbolic",
    "process-working-symbolic",
    "dialog-information-symbolic",
    "dialog-warning-symbolic",
    "dialog-error-symbolic",
    "dialog-question-symbolic",
    "document-open-symbolic",
    "document-save-symbolic",
    "folder-symbolic",
    "folder-new-symbolic",
    "user-home-symbolic",
    "user-trash-symbolic",
    "text-x-generic-symbolic",
    "starred-symbolic",
    "non-starred-symbolic",
    "media-eject-symbolic",
];

/// App and Store/Kit tiles the set draws (brief of the v0.12 icon wave).
const APP_TILES: &[&str] = &[
    "wolos-files",
    "wolos-mail",
    "wolos-calendar",
    "wolos-notes",
    "wolos-photos",
    "wolos-settings",
    "wolos-software",
    "wolos-terminal",
    "wolos-browser",
    "wolos-installer",
    "wolos-control-center",
    "wolos-plugin-maker",
    "wolos-kit-craft-suite",
    "wolos-kit-codecs",
    "wolos-kit-dev",
    "wolos-kit-ai",
    "wolos-kit-creative",
    "wolos-kit-gaming",
    "wolos-launcher",
    "wolos-music",
    "wolos-trash",
];

/// `wolos-` literals that name packages, repositories and runtime files, not icons.
fn not_an_icon(n: &str) -> bool {
    ["wolos-cachy", "wolos-install", "wolos-sessions-offered"].contains(&n)
        || n.starts_with("wolos-module-")
}

/// Brand drawings the generator keeps as they are.
const BRAND: &[&str] = &["wolos-mark", "wolos-glyph-symbolic", "wolos-logo-symbolic"];

fn symbolic_dir() -> PathBuf {
    root().join("design/icons/symbolic")
}

fn apps_dir() -> PathBuf {
    root().join("design/icons/apps")
}

fn theme_dir() -> PathBuf {
    root().join("desktop/overlay/usr/share/icons/WolOS")
}

fn svgs(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "svg"))
        .collect();
    v.sort();
    v
}

fn stem(p: &Path) -> String {
    p.file_stem().unwrap().to_string_lossy().into_owned()
}

/// Every `attr="..."` value in `s`.
fn attr_values<'a>(s: &'a str, attr: &str) -> Vec<&'a str> {
    let key = format!(" {attr}=\"");
    s.match_indices(&key)
        .filter_map(|(i, _)| {
            let rest = &s[i + key.len()..];
            rest.find('"').map(|e| &rest[..e])
        })
        .collect()
}

/// Tags open and close in order (the generator's SVGs have no comments, CDATA or text).
fn well_formed(s: &str) -> Result<(), String> {
    let mut stack: Vec<String> = Vec::new();
    let mut rest = s.trim();
    while let Some(i) = rest.find('<') {
        let j = rest[i..].find('>').ok_or("unclosed tag")? + i;
        let tag = &rest[i + 1..j];
        if let Some(name) = tag.strip_prefix('/') {
            match stack.pop() {
                Some(open) if open == name.trim() => {}
                other => return Err(format!("</{name}> closes {other:?}")),
            }
        } else if !tag.ends_with('/') && !tag.starts_with('?') && !tag.starts_with('!') {
            let name = tag.split_whitespace().next().unwrap_or_default();
            stack.push(name.to_string());
        }
        rest = &rest[j + 1..];
    }
    if stack.is_empty() {
        Ok(())
    } else {
        Err(format!("unclosed {stack:?}"))
    }
}

#[test]
fn every_symbolic_icon_the_shell_and_toolkit_ask_for_is_in_the_set() {
    let mut names = quoted(|t| t.ends_with("-symbolic") && t != "name-symbolic");
    assert!(names.len() > 30, "found only {} names", names.len());
    names.extend(TOOLKIT.iter().map(|s| s.to_string()));
    let missing: Vec<_> = names
        .iter()
        .filter(|n| !symbolic_dir().join(format!("{n}.svg")).is_file())
        .collect();
    assert!(missing.is_empty(), "not in the v2 set: {missing:?}");
}

#[test]
fn every_wolos_app_icon_the_shell_asks_for_is_in_the_set() {
    let names = quoted(|t| t.starts_with("wolos-") && !t.ends_with("-symbolic") && !not_an_icon(t));
    let missing: Vec<_> = names
        .iter()
        .chain(
            APP_TILES
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .iter(),
        )
        .filter(|n| !apps_dir().join(format!("{n}.svg")).is_file())
        .cloned()
        .collect();
    assert!(missing.is_empty(), "not in design/icons/apps: {missing:?}");
}

#[test]
fn symbolic_icons_share_one_geometry_and_recolour_with_css() {
    let all = svgs(&symbolic_dir());
    assert!(all.len() >= 180, "only {} symbolic icons", all.len());
    let mut bad = Vec::new();
    for p in &all {
        let n = stem(p);
        if BRAND.contains(&n.as_str()) {
            continue;
        }
        let s = std::fs::read_to_string(p).unwrap();
        if let Err(e) = well_formed(&s) {
            bad.push(format!("{n}: {e}"));
        }
        if !s.contains(r#"viewBox="0 0 16 16""#) {
            bad.push(format!("{n}: not on the 16 grid"));
        }
        if s.contains("stroke") {
            bad.push(format!("{n}: stroked (GTK 4.20 fills strokes)"));
        }
        // Light and dark come from CSS: every paint is currentColor, nothing hard-coded.
        let fills = attr_values(&s, "fill");
        if fills.is_empty() || fills.iter().any(|f| *f != "currentColor") {
            bad.push(format!("{n}: fill {fills:?}"));
        }
        if s.contains("<image") || s.contains("<text") || s.contains("<filter") {
            bad.push(format!("{n}: image, text or filter"));
        }
    }
    assert!(bad.is_empty(), "{} bad symbolic icons: {bad:#?}", bad.len());
}

fn hex(c: &str) -> Option<[f64; 3]> {
    let c = c.strip_prefix('#')?;
    if c.len() != 6 {
        return None;
    }
    let v = |i: usize| u8::from_str_radix(&c[i..i + 2], 16).ok().map(f64::from);
    Some([v(0)?, v(2)?, v(4)?])
}

fn luminance(c: [f64; 3]) -> f64 {
    let ch = |v: f64| {
        let v = v / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch(c[0]) + 0.7152 * ch(c[1]) + 0.0722 * ch(c[2])
}

fn contrast(a: [f64; 3], b: [f64; 3]) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// App tiles carry their own ground, so one drawing serves light and dark; what each needs is an
/// edge that separates it from both the light (#F5F5F7) and the dark (#1C1C1E) desktop.
#[test]
fn app_tiles_are_pixel_tiles_that_read_on_light_and_dark() {
    let light = hex("#F5F5F7").unwrap();
    let dark = hex("#1C1C1E").unwrap();
    let mut all = svgs(&apps_dir());
    for ctx in ["places", "devices", "mimetypes"] {
        all.extend(svgs(&root().join("design/icons").join(ctx)));
    }
    let mut bad = Vec::new();
    let mut tiles = 0;
    for p in &all {
        let n = stem(p);
        if BRAND.contains(&n.as_str()) {
            continue;
        }
        tiles += 1;
        let s = std::fs::read_to_string(p).unwrap();
        if let Err(e) = well_formed(&s) {
            bad.push(format!("{n}: {e}"));
        }
        if !s.contains(r#"viewBox="0 0 24 24""#) || !s.contains("crispEdges") {
            bad.push(format!("{n}: not a 24-cell pixel tile"));
        }
        let edges: Vec<[f64; 3]> = s
            .split("<rect ")
            .filter(|r| r.contains(r#"class="edge""#))
            .filter_map(|r| {
                attr_values(&format!(" {r}"), "fill")
                    .first()
                    .and_then(|f| hex(f))
            })
            .collect();
        if edges.len() < 20 {
            bad.push(format!("{n}: {} edge cells", edges.len()));
        }
        for e in &edges {
            if contrast(*e, light) < 1.3 || contrast(*e, dark) < 1.3 {
                bad.push(format!("{n}: edge {e:?} vanishes on light or dark"));
                break;
            }
        }
    }
    assert!(tiles >= APP_TILES.len(), "only {tiles} app tiles");
    assert!(bad.is_empty(), "{bad:#?}");
}

/// Width and height from a PNG's IHDR.
fn png_size(p: &Path) -> Option<(u32, u32)> {
    let b = std::fs::read(p).ok()?;
    if b.len() < 24 || &b[..8] != b"\x89PNG\r\n\x1a\n" || &b[12..16] != b"IHDR" {
        return None;
    }
    let u = |i: usize| u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    Some((u(16), u(20)))
}

#[test]
fn every_app_tile_ships_png_sizes_in_the_theme() {
    let mut bad = Vec::new();
    for p in svgs(&apps_dir()) {
        let n = stem(&p);
        if BRAND.contains(&n.as_str()) {
            continue;
        }
        for size in [48u32, 128, 512] {
            let png = theme_dir().join(format!("{size}x{size}/apps/{n}.png"));
            match png_size(&png) {
                Some(got) if got == (size, size) => {}
                got => bad.push(format!("{n} @{size}: {got:?}")),
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

/// The overlay and design/ both install index.theme into /usr/share/icons/WolOS (wolos-system
/// PKGBUILD): same file, the theme name the shell asks for (kit/style.rs PRODUCT_ICONS).
#[test]
fn index_theme_is_one_file_named_wolos_with_the_png_sizes() {
    let a = std::fs::read_to_string(theme_dir().join("index.theme")).unwrap();
    let b = std::fs::read_to_string(root().join("design/icons/index.theme")).unwrap();
    assert_eq!(a, b, "overlay and design index.theme differ");
    assert!(a.contains("\nName=WolOS\n"));
    let dirs = a
        .lines()
        .find_map(|l| l.strip_prefix("Directories="))
        .unwrap();
    for d in [
        "48x48/apps",
        "128x128/apps",
        "512x512/apps",
        "scalable/apps",
        "symbolic/status",
    ] {
        assert!(dirs.split(',').any(|x| x == d), "{d} not in Directories");
        assert!(a.contains(&format!("[{d}]")), "no [{d}] section");
    }
}

/// librsvg (through gdk-pixbuf, as GTK loads them) renders every icon of the set.
#[test]
fn every_svg_renders() {
    let mut bad = Vec::new();
    let mut n = 0;
    let ctx = |c: &str| root().join("design/icons").join(c);
    for dir in [
        symbolic_dir(),
        apps_dir(),
        ctx("places"),
        ctx("devices"),
        ctx("mimetypes"),
    ] {
        for p in svgs(&dir) {
            n += 1;
            match gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(&p, 32, 32, true) {
                Ok(b) if b.width() == 32 && b.height() == 32 => {}
                Ok(b) => bad.push(format!("{}: {}x{}", stem(&p), b.width(), b.height())),
                Err(e) => bad.push(format!("{}: {e}", stem(&p))),
            }
        }
    }
    assert!(n > 230, "only {n} icons");
    assert!(bad.is_empty(), "{bad:#?}");
}
