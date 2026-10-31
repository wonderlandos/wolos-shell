// SPDX-License-Identifier: GPL-2.0-or-later
//! libwol app windows (ADR 0020): a view tree with every Kit part, drawn by the Kit host.
use super::Case;
use gtk::prelude::*;
use shared::view::{Node, Role};
use shared::widgets::Size;
use shell::kit::{data as d, widgets as w};
use shell::ui::app_view::{Sink, View, app_frame};
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
        build: Box::new(|| {
            let view = View::new(&sample(), Sink::new(|_| {}));
            let aw = app_frame(&view);
            // The view lives as long as its widgets (the golden window).
            let root = aw.root.clone();
            unsafe { root.set_data("view", view) };
            root.upcast()
        }),
    });
    v
}
