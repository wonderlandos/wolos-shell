// SPDX-License-Identifier: GPL-2.0-or-later
//! Plugin host goldens (ADR 0031 W2): the menu bar with the reference plugin's item
//! (`plugins/shell/org.wolos.stats`, `[[menubar]] side = "right"`) before the system items, and
//! the same plugin's desktop card. Commands answer with a fixed sample, so the figures are stable.
use super::Case;
use gtk::prelude::*;
use shared::plugins::Kind;
use shared::widgets::Size;
use shell::kit::data as d;
use shell::kit::widgets as w;
use shell::ui::plugins::{self, Host, Paths};
use std::rc::Rc;
use wol_lua::{Services, Stamp};

const STATS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../plugins/shell/org.wolos.stats"
);
const SAMPLE: &str = r#"{"cpu_percent": 12.4, "memory": {"total": 17179869184, "used": 7043746365}, "uptime_secs": 7260}"#;

/// `wol sys stats --json` answers with [`SAMPLE`].
struct Sample;
impl Services for Sample {
    fn now(&self) -> Stamp {
        Stamp {
            year: 2026,
            month: 11,
            day: 2,
            weekday: 1,
            hour: 9,
            minute: 41,
            second: 0,
        }
    }
    fn system(&self) -> wol_lua::SystemInfo {
        wol_lua::Live.system()
    }
    fn notify(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn run(&self, _: &str, _: &[String], _: u32) -> Result<String, String> {
        Ok(SAMPLE.into())
    }
}

/// The stats example as a shipped plugin, enabled, under a private folder.
fn paths(tag: &str) -> Paths {
    let root =
        std::env::temp_dir().join(format!("wol-golden-plugins-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let to = root.join("system/org.wolos.stats");
    std::fs::create_dir_all(&to).expect("temp folder");
    for e in std::fs::read_dir(STATS)
        .expect("the stats example")
        .flatten()
    {
        std::fs::copy(e.path(), to.join(e.file_name())).expect("copy");
    }
    let p = Paths {
        dirs: vec![(root.join("user"), false), (root.join("system"), true)],
        config_home: root.join("config"),
        state_home: root.join("state"),
    };
    let mut on = wol_core::plugins::Enabled::default();
    let stats = wol_core::plugins::load(&to, true);
    on.enable(stats.manifest.as_ref().expect("the example is valid"));
    wol_core::plugins::save_enabled(&p.config_home, &on).expect("plugins.json");
    p
}

fn host(tag: &str, kinds: &'static [Kind]) -> Rc<std::cell::RefCell<Host>> {
    let p = paths(tag);
    let mut h = Host::with_services(p.clone(), kinds, Rc::new(Sample));
    h.apply(plugins::read_snapshot(&p, kinds));
    Rc::new(std::cell::RefCell::new(h))
}

pub fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "menubar-plugin".into(),
            size: (900, 28),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let b = shell::ui::menubar::content_with(
                    &shell::system::Fake,
                    shared::layout::Layout::default(),
                    Some(host("bar", plugins::BAR_KINDS)),
                );
                b.set_app("Files".into(), "wol-files".into());
                b.root.clone().upcast()
            }),
        },
        {
            let (wd, ht) = d::card_size(Size::Medium);
            Case {
                name: "widget-plugin-stats".into(),
                size: (wd + 2 * w::S6, ht + 2 * w::S6),
                class: "preview",
                wallpaper: true,
                build: Box::new(|| {
                    let h = host("card", plugins::DESKTOP_KINDS);
                    let node = h
                        .borrow_mut()
                        .render(Kind::Widget, "org.wolos.stats", "card")
                        .expect("the card is loaded")
                        .expect("the card renders");
                    let c = shell::ui::lua_widgets::card(Size::Medium, &node, true, |_| {});
                    c.set_halign(gtk::Align::Center);
                    c.set_valign(gtk::Align::Center);
                    c.upcast()
                }),
            }
        },
    ]
}
