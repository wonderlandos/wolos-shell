// SPDX-License-Identifier: GPL-2.0-or-later
//! Icon set v2 (design/tools/icons.py, design/tools/icons/): one language for every icon the shell draws.
//! Symbolic: 16 grid, filled outlines of 1.5 px lines, `currentColor` only (GTK recolours them for
//! light and dark through CSS). App icons: crafted objects on the soft card (places, devices and
//! mimetypes: pressed paper), vector SVGs on the 1024 canvas, rendered to PNG at 48/128/512
//! (rsvg-convert; 48 from the small drawing) in desktop/overlay/usr/share/icons/WolOS; their
//! silhouette reads on the light and the dark ground.
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

/// `wolos-` literals that name packages, repositories and runtime files, not icons, and prefixes
/// a name is built from (`"wolos-kit-"` + the kit id; the kits are in APP_TILES).
fn not_an_icon(n: &str) -> bool {
    ["wolos-cachy", "wolos-install", "wolos-sessions-offered"].contains(&n)
        || n.starts_with("wolos-module-")
        || n.ends_with('-')
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

/// Colours the set left behind (the previous tiles used another platform's system colours).
const BANNED: &[&str] = &[
    "#FF3B30", "#FF9500", "#FFCC00", "#34C759", "#007AFF", "#FF453A", "#FF9F0A", "#FFD60A",
    "#30D158", "#0A84FF", "#FF5F57", "#FEBC2E", "#28C840", "#FF2D55", "#5AC8FA", "#8E8E93",
];

/// Filter primitives rsvg draws the same everywhere (design/tools/icons/checks.py SAFE_FILTER).
const SAFE_FILTER: &[&str] = &[
    "feGaussianBlur",
    "feOffset",
    "feFlood",
    "feComposite",
    "feMerge",
    "feMergeNode",
    "feColorMatrix",
    "feTurbulence",
    "feBlend",
];

/// Element names opened in `s` (`<name ...>`; closing tags, comments and declarations skipped).
fn tags(s: &str) -> Vec<&str> {
    s.split('<')
        .skip(1)
        .filter_map(|t| {
            let name = t
                .split(|c: char| c.is_whitespace() || c == '>' || c == '/')
                .next()?;
            (!name.is_empty() && !t.starts_with('/') && !t.starts_with('!') && !t.starts_with('?'))
                .then_some(name)
        })
        .collect()
}

/// Contrast an icon's edge ring holds against `ground` when drawn at 128 px (the lit
/// lip, hairline or rim of the silhouette counts; the 75th percentile, as design/tools/icons/checks.py).
fn edge_contrast(p: &Path, ground: [f64; 3]) -> f64 {
    let b = gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(p, 128, 128, true).unwrap();
    let (w, h, ch, stride) = (
        b.width() as usize,
        b.height() as usize,
        b.n_channels() as usize,
        b.rowstride() as usize,
    );
    assert_eq!(ch, 4, "{}: no alpha channel", p.display());
    let px = b.read_pixel_bytes();
    let at = |x: usize, y: usize| &px[y * stride + x * ch..y * stride + x * ch + 4];
    let mut ring = Vec::new();
    for y in 0..h {
        for x in 0..w {
            if at(x, y)[3] < 230 {
                continue;
            }
            let open = (-1i32..=1).any(|dy| {
                (-1i32..=1).any(|dx| {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    nx < 0
                        || ny < 0
                        || nx >= w as i32
                        || ny >= h as i32
                        || at(nx as usize, ny as usize)[3] < 77
                })
            });
            if open {
                let c = at(x, y);
                ring.push(contrast([c[0] as f64, c[1] as f64, c[2] as f64], ground));
            }
        }
    }
    assert!(!ring.is_empty(), "{}: nothing drawn", p.display());
    ring.sort_by(|a, b| a.partial_cmp(b).unwrap());
    ring[ring.len() * 3 / 4]
}

/// Full-colour icons (design/tools/icons/, LANGUAGE-v2): crafted objects on the soft card (apps)
/// and pressed paper (places, devices, mimetypes), vector SVGs on the 1024 canvas. Gradients and
/// blurs are allowed (rsvg draws them; the PNGs the dock uses are renders), images, text, scripts
/// and styles are not; every app also has its small drawing (apps/small/, the 48 px PNG is a render
/// of it). Each carries its own ground, so one drawing serves light and dark; what each needs is
/// a silhouette that separates from both the light (`light.bg` #F3F1F8) and the dark (`dark.bg`
/// #191720) desktop of the WolOS neutral ramp (design/tokens/colors.json, icon ground-light/-dark).
#[test]
fn full_colour_icons_are_crafted_vectors_that_read_on_light_and_dark() {
    let light = hex("#F3F1F8").unwrap();
    let dark = hex("#191720").unwrap();
    let mut all = svgs(&apps_dir());
    for ctx in ["places", "devices", "mimetypes"] {
        all.extend(svgs(&root().join("design/icons").join(ctx)));
    }
    let mut bad = Vec::new();
    let mut icons = 0;
    for p in &all {
        let n = stem(p);
        if BRAND.contains(&n.as_str()) {
            continue;
        }
        icons += 1;
        let s = std::fs::read_to_string(p).unwrap();
        if let Err(e) = well_formed(&s) {
            bad.push(format!("{n}: {e}"));
        }
        if !s.contains(r#"viewBox="0 0 1024 1024""#) || s.contains("crispEdges") {
            bad.push(format!("{n}: not a vector icon on the 1024 canvas"));
        }
        for t in tags(&s) {
            if [
                "image",
                "text",
                "script",
                "style",
                "use",
                "foreignObject",
                "a",
            ]
            .contains(&t)
            {
                bad.push(format!("{n}: <{t}>"));
            }
            if t.starts_with("fe") && !SAFE_FILTER.contains(&t) {
                bad.push(format!("{n}: filter primitive <{t}>"));
            }
        }
        if s.contains("gloss") || s.contains("mix-blend-mode") {
            bad.push(format!("{n}: gloss sweep or blend mode"));
        }
        if s.len() > 40 * 1024 {
            bad.push(format!("{n}: {} KB", s.len() / 1024));
        }
        let upper = s.to_uppercase();
        for c in BANNED {
            if upper.contains(c) {
                bad.push(format!("{n}: banned colour {c}"));
            }
        }
        if p.parent() == Some(apps_dir().as_path())
            && !apps_dir().join("small").join(format!("{n}.svg")).is_file()
        {
            bad.push(format!("{n}: no small drawing in apps/small"));
        }
        for (g, ground) in [("light", light), ("dark", dark)] {
            let best = edge_contrast(p, ground);
            if best < 1.3 {
                bad.push(format!("{n}: silhouette is {best:.2}:1 on {g}"));
            }
        }
    }
    assert!(
        icons >= 51,
        "only {icons} icons (25 apps, 13 places, 2 devices, 11 mimetypes)"
    );
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

#[test]
fn prefix_literals_and_package_names_are_not_icon_names() {
    assert!(not_an_icon("wolos-kit-"));
    assert!(not_an_icon("wolos-module-calendar"));
    assert!(not_an_icon("wolos-install"));
    assert!(!not_an_icon("wolos-kit-dev"));
    assert!(!not_an_icon("wolos-files"));
}

#[test]
fn safe_filter_list_matches_the_generator_checks() {
    // design/tools/icons/checks.py SAFE_FILTER is the same list plus the <filter> element itself.
    let py = std::fs::read_to_string(root().join("design/tools/icons/checks.py")).unwrap();
    let start = py.find("SAFE_FILTER = {").expect("checks.py: SAFE_FILTER");
    let body = &py[start..start + py[start..].find('}').expect("checks.py: SAFE_FILTER end")];
    let mut want: Vec<&str> = body
        .split('"')
        .skip(1)
        .step_by(2)
        .filter(|t| *t != "filter")
        .collect();
    let mut have = SAFE_FILTER.to_vec();
    want.sort_unstable();
    have.sort_unstable();
    assert_eq!(
        have, want,
        "SAFE_FILTER here and in design/tools/icons/checks.py drifted apart"
    );
}

/// Filters are allowed in the full-colour icons only because gtkcheck proves GTK draws them like
/// librsvg. GTK 4 draws SVG icons itself, and a filter chain librsvg draws right can come out
/// there as a grey or dark block (what the retired tests/icon_filters.rs banned filters for).
/// design/tools/icons/gtkcheck/check.sh draws every full-colour icon through GtkIconPaintable and
/// compares it with librsvg (RMSE <= 0.02 at 48 and 128 px). Without a C compiler, the gtk4
/// headers, gtk4-broadwayd or rsvg-convert it is skipped, loudly; a toolkit that loads SVG icons
/// through librsvg itself (nothing to compare) is skipped the same way.
#[test]
fn gtk_draws_every_full_colour_icon_like_librsvg() {
    use std::io::Write;
    let out = std::process::Command::new("bash")
        .arg(root().join("design/tools/icons/gtkcheck/check.sh"))
        .output()
        .expect("bash");
    let text =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    if out.status.code() == Some(77) {
        // Straight to stderr, past the test harness's capture, so the skip shows in every run.
        let _ = writeln!(
            std::io::stderr(),
            "\n*** SKIPPED gtk_draws_every_full_colour_icon_like_librsvg: {} ***\n",
            text.trim()
        );
        return;
    }
    assert!(out.status.success(), "{text}");
}

/// The full-colour icon sets (design/icons/{apps,apps/small,places,devices,mimetypes}).
fn full_colour_svgs() -> Vec<PathBuf> {
    let d = root().join("design/icons");
    let mut all = Vec::new();
    for sub in ["apps", "apps/small", "places", "devices", "mimetypes"] {
        all.extend(svgs(&d.join(sub)));
    }
    all
}

/// `s` without its `filter="url(#..)"` attributes: what a renderer that ignores filters draws.
fn strip_filters(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find(" filter=\"url(#") {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 1..];
        let end = tail[8..].find('"').map_or(tail.len(), |j| j + 9);
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// Premultiplied RGBA of a PNG, each channel in 0..1.
fn premultiplied(p: &Path) -> Vec<f64> {
    let b = gtk::gdk_pixbuf::Pixbuf::from_file(p).unwrap();
    assert!(
        b.has_alpha() && b.n_channels() == 4,
        "{}: not RGBA",
        p.display()
    );
    let (w, h, stride) = (
        b.width() as usize,
        b.height() as usize,
        b.rowstride() as usize,
    );
    let px = b.read_pixel_bytes();
    let mut v = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let o = y * stride + x * 4;
            let a = px[o + 3] as f64 / 255.0;
            v.extend([
                px[o] as f64 / 255.0 * a,
                px[o + 1] as f64 / 255.0 * a,
                px[o + 2] as f64 / 255.0 * a,
                a,
            ]);
        }
    }
    v
}

fn rsvg(svg: &Path, size: u32, png: &Path) {
    let ok = std::process::Command::new("rsvg-convert")
        .args(["-w", &size.to_string(), "-h", &size.to_string(), "-o"])
        .arg(png)
        .arg(svg)
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok, "rsvg-convert {}", svg.display());
}

/// Icons still allowed to draw a shape only through a filter (empty: the grain is svgk.grained, an
/// invisible shape whose noise is clipped). Shrink-only: a listed icon that passes fails the test.
const FILTER_ONLY_KNOWN: &[&str] = &[];

/// A renderer that ignores SVG filters (Qt's) still draws every full-colour icon recognisably:
/// with its filter attributes removed an icon stays within RMSE 0.08 of its normal render at 48
/// and 128 px (premultiplied RGBA), so no shape is drawn only by a filter.
#[test]
fn full_colour_icons_read_with_their_filters_ignored() {
    use std::io::Write;
    if std::process::Command::new("rsvg-convert")
        .arg("--version")
        .output()
        .is_err()
    {
        let _ = writeln!(
            std::io::stderr(),
            "\n*** SKIPPED full_colour_icons_read_with_their_filters_ignored: needs rsvg-convert ***\n"
        );
        return;
    }
    let tmp = std::env::temp_dir().join(format!("wol-nofilter-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let mut scores = Vec::new();
    for p in full_colour_svgs() {
        let s = std::fs::read_to_string(&p).unwrap();
        let stripped = tmp.join("stripped.svg");
        std::fs::write(&stripped, strip_filters(&s)).unwrap();
        for size in [48, 128] {
            let (a, b) = (tmp.join("a.png"), tmp.join("b.png"));
            rsvg(&p, size, &a);
            rsvg(&stripped, size, &b);
            let (a, b) = (premultiplied(&a), premultiplied(&b));
            let sum: f64 = a.iter().zip(&b).map(|(x, y)| (x - y) * (x - y)).sum();
            let rel = p
                .strip_prefix(root().join("design/icons"))
                .unwrap()
                .display()
                .to_string();
            scores.push(((sum / a.len() as f64).sqrt(), size, rel));
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    scores.sort_by(|x, y| y.0.total_cmp(&x.0));
    let unlisted: Vec<_> = scores
        .iter()
        .filter(|s| !FILTER_ONLY_KNOWN.contains(&s.2.as_str()))
        .take(5)
        .collect();
    let _ = writeln!(
        std::io::stderr(),
        "filters ignored, worst RMSE: {:?}; worst not listed: {unlisted:?}",
        &scores[..5.min(scores.len())]
    );
    assert!(scores.len() > 140, "only {} renders", scores.len());
    let fails = |s: &(f64, u32, String)| s.0 > 0.08;
    let bad: Vec<_> = scores
        .iter()
        .filter(|s| fails(s) && !FILTER_ONLY_KNOWN.contains(&s.2.as_str()))
        .collect();
    assert!(
        bad.is_empty(),
        "drawn only by a filter (RMSE > 0.08 with filters ignored): {bad:#?}"
    );
    let fixed: Vec<_> = FILTER_ONLY_KNOWN
        .iter()
        .filter(|k| !scores.iter().any(|s| fails(s) && s.2 == **k))
        .collect();
    assert!(
        fixed.is_empty(),
        "now read with filters ignored, remove from FILTER_ONLY_KNOWN: {fixed:?}"
    );
}

/// Outside the full-colour sets (symbolic, store, the rest) no icon uses an SVG filter: gtkcheck
/// does not look at them, and GTK recolours symbolic icons.
#[test]
fn icons_outside_the_full_colour_sets_have_no_filter() {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "svg") {
                out.push(p);
            }
        }
    }
    let mut all = Vec::new();
    walk(&root().join("design/icons"), &mut all);
    let full = full_colour_svgs();
    let bad: Vec<_> = all
        .iter()
        .filter(|p| !full.contains(p))
        .filter(|p| {
            let s = std::fs::read_to_string(p).unwrap();
            s.contains("<filter") || s.contains("filter:url") || s.contains("filter=\"url")
        })
        .map(|p| p.display().to_string())
        .collect();
    assert!(all.len() > 100, "found only {} icons", all.len());
    assert!(bad.is_empty(), "{bad:?}");
}
