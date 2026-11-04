// SPDX-License-Identifier: GPL-2.0-or-later
//! Quick Look goldens (E2E-QL, docs/modules/quicklook.md): the preview window for a picture, code,
//! audio, a PDF page, a folder and a file with no preview, at the Apps window size and at the
//! narrow one, light and dark. The pictures are drawn here with cairo so no fixture file is needed.
use super::Case;
use gtk::{cairo, gdk, glib, prelude::*};
use shell::copy::quicklook as copy;
use shell::kit::widgets as w;
use shell::quicklook::{Facts, FolderInfo, Kind};
use shell::ui::quicklook::{Body, View, content};

/// A texture from a cairo drawing.
fn drawn(width: i32, height: i32, draw: impl Fn(&cairo::Context)) -> gdk::Texture {
    let s = cairo::ImageSurface::create(cairo::Format::ARgb32, width, height).expect("surface");
    {
        let cr = cairo::Context::new(&s).expect("context");
        draw(&cr);
    }
    let stride = usize::try_from(s.stride()).expect("stride");
    let data = s.take_data().expect("pixels");
    gdk::MemoryTexture::new(
        width,
        height,
        gdk::MemoryFormat::B8g8r8a8Premultiplied,
        &glib::Bytes::from(&*data),
        stride,
    )
    .upcast()
}

/// A landscape: sky gradient, sun, two hills.
fn landscape() -> gdk::Texture {
    drawn(1200, 800, |cr| {
        let sky = cairo::LinearGradient::new(0.0, 0.0, 0.0, 800.0);
        sky.add_color_stop_rgb(0.0, 0.25, 0.45, 0.80);
        sky.add_color_stop_rgb(1.0, 0.95, 0.75, 0.55);
        let _ = cr.set_source(&sky);
        let _ = cr.paint();
        cr.set_source_rgb(1.0, 0.92, 0.6);
        cr.arc(880.0, 230.0, 90.0, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
        cr.set_source_rgb(0.20, 0.42, 0.30);
        cr.arc(300.0, 1000.0, 480.0, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
        cr.set_source_rgb(0.14, 0.32, 0.24);
        cr.arc(900.0, 1100.0, 560.0, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
    })
}

/// A page: white paper, a heading bar and grey lines for text.
fn page() -> gdk::Texture {
    drawn(600, 800, |cr| {
        cr.set_source_rgb(1.0, 1.0, 1.0);
        let _ = cr.paint();
        cr.set_source_rgb(0.15, 0.15, 0.2);
        cr.rectangle(60.0, 70.0, 300.0, 28.0);
        let _ = cr.fill();
        cr.set_source_rgb(0.7, 0.7, 0.74);
        for i in 0..22 {
            let y = 140.0 + f64::from(i) * 26.0;
            let len = if i % 6 == 5 { 200.0 } else { 480.0 };
            cr.rectangle(60.0, y, len, 8.0);
            let _ = cr.fill();
        }
    })
}

const CODE: &str = "// A small program.\nuse std::collections::HashMap;\n\nfn count(words: &[&str]) -> HashMap<String, usize> {\n    let mut seen = HashMap::new();\n    for w in words {\n        *seen.entry(w.to_lowercase()).or_insert(0) += 1;\n    }\n    seen\n}\n\nfn main() {\n    let counts = count(&[\"Tea\", \"tea\", \"Hatter\"]);\n    let mut names: Vec<_> = counts.iter().collect();\n    names.sort();\n    for (name, n) in names {\n        println!(\"{name}: {n}\");\n    }\n}\n";

fn facts(name: &str, kind: Kind, size: Option<&str>) -> Facts {
    Facts {
        name: name.to_owned(),
        kind,
        size: size.map(str::to_owned),
        modified: Some("3 March 2026".to_owned()),
    }
}

fn view(tag: &str) -> View {
    match tag {
        "image" => View {
            facts: facts("Rabbit hole at dusk.jpg", Kind::Image, Some("2.4 MB")),
            body: Body::Picture(landscape()),
        },
        "code" => View {
            facts: facts("count.rs", Kind::Text, Some("486 bytes")),
            body: Body::Text {
                text: CODE.to_owned(),
                cut: false,
            },
        },
        "audio" => View {
            facts: facts("Tea party.flac", Kind::Audio, Some("31.2 MB")),
            body: Body::Audio(gtk::MediaFile::new().upcast()),
        },
        "pdf" => View {
            facts: facts("Invitation.pdf", Kind::Pdf, Some("84 KB")),
            body: Body::Page(page()),
        },
        "folder" => View {
            facts: facts("Wonderland", Kind::Folder, None),
            body: Body::Folder(FolderInfo {
                folders: 3,
                files: 9,
                bytes: 4_200_000,
                partial: false,
            }),
        },
        _ => View {
            facts: facts("setup.exe", Kind::Other, Some("12.8 MB")),
            body: Body::Card(copy::NO_PREVIEW),
        },
    }
}

pub fn cases() -> Vec<Case> {
    let mut v = Vec::new();
    for tag in ["image", "code", "audio", "pdf", "folder", "unknown"] {
        for (label, size) in [("900x600", w::APP_SIZE), ("480x360", w::APP_NARROW)] {
            v.push(Case {
                name: format!("quicklook-{tag}-{label}"),
                size,
                class: "preview-app",
                wallpaper: false,
                build: Box::new(move || content(&view(tag))),
            });
        }
    }
    v
}
