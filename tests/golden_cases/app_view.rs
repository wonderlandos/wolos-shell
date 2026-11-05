// SPDX-License-Identifier: GPL-2.0-or-later
//! libwol app windows (ADR 0020): a view tree with every Kit part, drawn by the Kit host.
use super::Case;
use gtk::prelude::*;
use shared::view::{Node, Role};
use shared::widgets::Size;
use shell::kit::{data as d, widgets as w};
use shell::ui::app_view::{AppView, Sink, app_frame};
use shell::ui::lua_widgets::Lua;
use shell::ui::widgets::Content;
use std::rc::Rc;
use wol_lua::{Services, Stamp, SystemInfo};

/// 09:41 on Friday 30 October 2026, every run.
struct Fixed;
impl Services for Fixed {
    fn now(&self) -> Stamp {
        Stamp {
            year: 2026,
            month: 10,
            day: 30,
            weekday: 5,
            hour: 9,
            minute: 41,
            second: 0,
        }
    }
    fn system(&self) -> SystemInfo {
        unreachable!("clock.lua reads only the time")
    }
    fn notify(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
}

/// The shipped example `clock.lua`, loaded the way the desktop loads `~/.config/wolos/widgets/`.
fn clock_tree() -> shared::view::Node {
    let dir = std::env::temp_dir().join(format!("wol-golden-lua-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../wol-lua/widgets/clock.lua"),
        dir.join("clock.lua"),
    )
    .unwrap();
    let mut lua = Lua::new(dir, Rc::new(Fixed));
    lua.poll();
    match lua.content("clock") {
        Some(Content::View { node, .. }) => node,
        other => panic!("clock.lua: {other:?}"),
    }
}

fn label(text: &str, role: Role) -> Node {
    Node::Label {
        text: text.into(),
        role,
    }
}

/// The tree `tests/c/parts.c` builds in libwol, plus a header and buttons.
pub fn sample() -> Node {
    Node::Stack {
        spacing: 16,
        children: vec![
            Node::Row {
                spacing: 12,
                children: vec![
                    Node::Image {
                        source: "weather-clear-symbolic".into(),
                        size: 32,
                    },
                    Node::Stack {
                        spacing: 0,
                        children: vec![
                            label("Sunny", Role::Title),
                            label("Lisbon, 21°", Role::Secondary),
                        ],
                    },
                ],
            },
            Node::List {
                children: vec![
                    Node::TextField {
                        id: 1,
                        text: "Ada".into(),
                        placeholder: "Name".into(),
                    },
                    Node::Toggle {
                        id: 2,
                        on: true,
                        label: "Wi-Fi".into(),
                    },
                    Node::Slider {
                        id: 3,
                        value: 50.0,
                        min: 0.0,
                        max: 100.0,
                    },
                ],
            },
            Node::Row {
                spacing: 8,
                children: vec![
                    Node::Spacer,
                    Node::Button {
                        id: 4,
                        label: "Cancel".into(),
                    },
                    Node::Button {
                        id: 5,
                        label: "Save".into(),
                    },
                ],
            },
        ],
    }
}

pub fn cases() -> Vec<Case> {
    let mut v: Vec<Case> = [Size::Small, Size::Medium]
        .into_iter()
        .map(|size| {
            let (wd, ht) = d::card_size(size);
            Case {
                name: format!("lua-clock-{}", size.id()),
                size: (wd + 2 * w::S6, ht + 2 * w::S6),
                class: "preview",
                wallpaper: true,
                build: Box::new(move || {
                    let c = shell::ui::lua_widgets::card(size, &clock_tree(), true, |_| {});
                    c.set_halign(gtk::Align::Center);
                    c.set_valign(gtk::Align::Center);
                    c.upcast()
                }),
            }
        })
        .collect();
    v.push(Case {
        name: "app-window-parts".into(),
        size: (420, 380),
        class: "preview-app",
        wallpaper: false,
        build: Box::new(|| framed(&sample())),
    });
    // WOLKIT-1TO1 responsive: the same trees at the three widths of the HIG (narrow, app, wide);
    // crates/wolkit-render tests/page.rs draws them and must match these goldens pixel for pixel.
    for (tree, name) in [
        (sample(), "app-window-parts"),
        (settings_look(), "settings-look"),
    ] {
        for w in WIDTHS {
            let tree = tree.clone();
            v.push(Case {
                name: format!("{name}-w{w}"),
                size: (w, WIDTH_HEIGHT),
                class: "preview-app",
                wallpaper: false,
                build: Box::new(move || framed(&tree)),
            });
        }
    }
    // WOLKIT-1TO1 component cases (crates/wolkit-render/fixtures/components.json): every part the
    // Kit page draws, alone in a Kit window; wolkit-render draws the same trees.
    let comps: serde_json::Value = serde_json::from_str(include_str!(
        "../../../wolkit-render/fixtures/components.json"
    ))
    .expect("components.json");
    let size = (
        comps["width"].as_i64().unwrap_or(360) as i32,
        comps["height"].as_i64().unwrap_or(200) as i32,
    );
    for c in comps["cases"].as_array().into_iter().flatten() {
        let tree: Node = serde_json::from_value(c["tree"].clone()).expect("a view tree");
        v.push(Case {
            name: format!("kit-view-{}", c["name"].as_str().unwrap_or("?")),
            size,
            class: "preview-app",
            wallpaper: false,
            build: Box::new(move || framed(&tree)),
        });
    }
    // WOLKIT-1TO1 live theme: the window opens in the other variant, then the system theme flips
    // to this case's; the drawn window must be the plain `app-window-parts` golden of the variant.
    v.push(Case {
        name: "app-window-parts-flip".into(),
        size: (420, 380),
        class: "preview-app",
        wallpaper: false,
        build: Box::new(|| {
            let now = shell::kit::style::look();
            let id = |dark: bool| if dark { "wol-dark" } else { "wol-light" };
            let other = shell::theme::resolve(Some(id(!now.dark)), false);
            let back = shell::theme::resolve(Some(id(now.dark)), false);
            shell::kit::style::apply(&other, crate::EXTRA_CSS);
            let root = framed(&sample());
            shell::kit::style::apply(&back, crate::EXTRA_CSS);
            root
        }),
    });
    v
}

/// The widths of the responsive cases: the narrow app (design/tokens/space.json surface.app-narrow),
/// the app (surface.app) and a wide window.
pub const WIDTHS: [i32; 3] = [480, 900, 1280];
/// Their height.
pub const WIDTH_HEIGHT: i32 = 600;

/// The Settings > Look page as a view tree (the fixture the web page and wolkit-render use).
fn settings_look() -> Node {
    serde_json::from_str(include_str!(
        "../../../wolkit-render/fixtures/settings-look.json"
    ))
    .expect("settings-look.json")
}

/// The Kit app frame around `tree`, drawn by the default path (libWolkit).
fn framed(tree: &Node) -> gtk::Widget {
    let view = AppView::new(tree, Sink::new(|_| {}));
    let aw = app_frame(&view);
    // The view lives as long as its widgets (the golden window).
    let root = aw.root.clone();
    unsafe { root.set_data("view", view) };
    root.upcast()
}
