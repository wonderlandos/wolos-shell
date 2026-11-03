// SPDX-License-Identifier: GPL-2.0-or-later
//! Plugin Maker goldens (ADR 0031 "Plugin Maker / Theme Studio"): the app on a plugin made by
//! Create Plugin (menu bar item, desktop card, Dock entry, Control Center tile), on its Editor,
//! Preview and Theme Studio pages; the start page before a plugin is open (open a folder, or the
//! kind picker of Create Plugin); and the Share sheet on a packed fixture (a fixed checksum, so
//! the long entry in its fixed-height scroller is the same every run). The preview's clock is
//! fixed, so the item reads 09:41.
use super::Case;
use gtk::prelude::*;
use shared::plugins::Kind;
use shell::kit::widgets as w;
use shell::ui::maker::{self, Maker, editor, preview, publish};
use std::rc::Rc;
use wol_lua::Stamp;

const NINE_FORTY_ONE: Stamp = Stamp {
    year: 2026,
    month: 11,
    day: 2,
    weekday: 1,
    hour: 9,
    minute: 41,
    second: 0,
};

/// A fresh scratch folder for `tag` and the app over its sandbox, nothing open yet.
fn maker_in(tag: &str) -> (std::path::PathBuf, Rc<Maker>) {
    let root = std::env::temp_dir().join(format!("wol-golden-maker-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let sandbox = preview::sandbox_in(&root.join("sandbox"));
    let services = Rc::new(preview::PreviewServices::new(
        &sandbox,
        Some(NINE_FORTY_ONE),
    ));
    let theme = shell::theme::resolve(Some("wol-dark"), true);
    (root, Maker::new(theme, services, sandbox))
}

/// The app with a fresh demo plugin open, on `page`.
fn maker_on(tag: &str, page: &'static str) -> Rc<Maker> {
    let (root, m) = maker_in(tag);
    let dir = editor::create(
        &root.join("plugins"),
        "alice.demo",
        &[Kind::Menubar, Kind::Widget, Kind::Dock, Kind::Tile],
    )
    .expect("the scaffold writes");
    let l = editor::load(&dir).expect("the demo loads");
    m.show_plugin(l, preview::read(&dir));
    m.select(page);
    m
}

/// The demo open, with the Share sheet up on a packed fixture.
fn maker_share() -> Rc<Maker> {
    let m = maker_on("maker-share", maker::EDITOR);
    let root = std::env::temp_dir().join(format!(
        "wol-golden-maker-maker-share-{}",
        std::process::id()
    ));
    let dir = root.join("plugins/alice.demo");
    let manifest = wol_core::plugins::check(&dir, false)
        .manifest
        .expect("the demo has a manifest");
    m.show_share(publish::Packed {
        file: root
            .join("plugins")
            .join(format!("alice.demo-{}.wolplug", manifest.version)),
        sha256: "3f9a1c0e7b52d4a86e1f0c93b7d25a4e6c18f0b2d7e93a51c4b6082f9e1d7a35".into(),
        size: 4096,
        manifest,
    });
    m
}

fn case(name: &str, build: impl Fn() -> Rc<Maker> + 'static) -> Case {
    Case {
        name: name.into(),
        size: w::APP_SIZE,
        class: "preview-app",
        wallpaper: false,
        build: Box::new(move || build().aw.root.clone().upcast()),
    }
}

pub fn cases() -> Vec<Case> {
    vec![
        case("maker-editor", || maker_on("maker-editor", maker::EDITOR)),
        case("maker-preview", || {
            maker_on("maker-preview", maker::PREVIEW)
        }),
        case("maker-theme", || maker_on("maker-theme", maker::STUDIO)),
        case("maker-start", || {
            let (_, m) = maker_in("maker-start");
            m.select(maker::EDITOR);
            m
        }),
        case("maker-share", maker_share),
    ]
}
