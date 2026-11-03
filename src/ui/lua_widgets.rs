// SPDX-License-Identifier: GPL-2.0-or-later
//! Lua desktop widgets (ADR 0020, `wol_lua`): `~/.config/wolos/widgets/<id>.lua`, placed on the
//! desktop as kind `lua:<id>`. Scripts run sandboxed in the shell; each file reloads when it
//! changes, renders on the desktop widget timer at its own `refresh`, and after a click. The tree
//! it returns is drawn with Kit parts (`ui::app_view`) on a widget card.
//!
//! Plugin cards (ADR 0031 `[[widget]]`) are Lua widgets too: kind `lua:plugin-<plugin>-<id>`
//! (`plugins::widget_key`), run by the desktop's own plugin host with the same deadlines and the
//! plugin failure count (three in a row turn the plugin off).
use super::app_view::{Act, Sink, View};
use super::plugins;
use super::widgets::Content;
use crate::kit::data as d;
use gtk::prelude::*;
use shared::plugins::Kind;
use shared::view::Node;
use shared::widgets::Size;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use wol_lua::{Change, Deadlines, Library, Services};

/// Scripts run on the GTK thread, so each entry point has a wall-clock limit (R2C-3): 50 ms per
/// render, 200 ms per click and per load. A widget that runs out is stopped and shows the
/// "took too long" card until its file changes.
const DEADLINES: Deadlines = Deadlines::DEFAULT;
/// Seconds between tries of a plugin card whose plugin is not loaded (yet).
pub const PLUGIN_RETRY: u32 = 5;

/// One file's last render.
#[derive(Clone, Debug, PartialEq)]
enum Shown {
    Tree { name: String, node: Node },
    Failed(String),
    Missing,
}

/// A plugin card's host and its (plugin, entry), None while the plugin does not offer it.
type PluginCard = (Rc<RefCell<plugins::Host>>, Option<(String, String)>);

/// The widget folder and what each file shows now.
pub struct Lua {
    lib: Library,
    shown: HashMap<String, Shown>,
    /// The desktop's plugin host (None: plain Lua files only).
    plugins: Option<Rc<RefCell<plugins::Host>>>,
    /// Ends the plugin folder watch with this.
    life: crate::kit::lifetime::Lifetime,
}

// Drop: ends the lifetime that owns this widget's timers and monitors.
impl Drop for Lua {
    fn drop(&mut self) {
        self.life.end();
    }
}

impl Lua {
    pub fn new(dir: PathBuf, services: Rc<dyn Services>) -> Lua {
        let mut lib = Library::new(dir, services);
        lib.set_deadlines(DEADLINES);
        Lua {
            lib,
            shown: HashMap::new(),
            plugins: None,
            life: Default::default(),
        }
    }

    /// [`Lua::new`] with plugin cards from `host` (loaded and watched by the caller).
    pub fn with_plugins(
        dir: PathBuf,
        services: Rc<dyn Services>,
        host: Rc<RefCell<plugins::Host>>,
    ) -> Lua {
        let mut l = Lua::new(dir, services);
        l.plugins = Some(host);
        l
    }

    /// The person's folder, with this machine's clock and services, and the enabled plugins'
    /// cards (loaded off the GTK thread, reloaded when the plugin folder changes).
    pub fn user() -> Lua {
        let host = Rc::new(RefCell::new(plugins::Host::new(
            plugins::Paths::user(),
            plugins::DESKTOP_KINDS,
        )));
        let l = Lua::with_plugins(wol_lua::user_dir(), Rc::new(wol_lua::Live), host.clone());
        plugins::watch(&host, &l.life, Rc::new(|| {}));
        l
    }

    /// The plugin cards there are now (the gallery offers them as `lua:<key>`).
    pub fn cards(&self) -> Vec<plugins::Card> {
        self.plugins
            .as_ref()
            .map(|h| h.borrow().cards())
            .unwrap_or_default()
    }

    /// (host, plugin, entry) of a plugin card id; None for a plain Lua file.
    fn plugin(&self, id: &str) -> Option<PluginCard> {
        if !id.starts_with(plugins::WIDGET_PREFIX) {
            return None;
        }
        let h = self.plugins.clone()?;
        let at = h.borrow().card(id);
        Some((h, at))
    }

    /// A plugin card's render or click outcome, kept as what the card shows.
    fn show_plugin(
        &mut self,
        id: &str,
        h: &Rc<RefCell<plugins::Host>>,
        plugin: &str,
        r: Option<plugins::Shown>,
    ) {
        let name = h.borrow().name(plugin);
        let s = match r {
            Some(Ok(node)) => Shown::Tree { name, node },
            Some(Err(m)) => Shown::Failed(m),
            None => Shown::Missing,
        };
        self.shown.insert(id.to_owned(), s);
        plugins::toast(h);
    }

    /// Look at the folder again; the files whose card changed.
    pub fn poll(&mut self) -> Vec<String> {
        let mut changed = Vec::new();
        for c in self.lib.poll() {
            match c {
                Change::Loaded(id) => {
                    self.render(&id);
                    changed.push(id);
                }
                Change::Failed(id, m) => {
                    self.shown.insert(id.clone(), Shown::Failed(m));
                    changed.push(id);
                }
                Change::Removed(id) => {
                    self.shown.insert(id.clone(), Shown::Missing);
                    changed.push(id);
                }
            }
        }
        changed
    }

    /// Run the file's render function now.
    pub fn render(&mut self, id: &str) {
        if let Some((h, at)) = self.plugin(id) {
            let Some((plugin, entry)) = at else {
                self.shown.insert(id.to_owned(), Shown::Missing);
                return;
            };
            let r = h.borrow_mut().render(Kind::Widget, &plugin, &entry);
            return self.show_plugin(id, &h, &plugin, r);
        }
        let s = match self.lib.widget(id) {
            None => return,
            Some(w) => match w.render() {
                Ok(node) => Shown::Tree {
                    name: w.name().to_owned(),
                    node,
                },
                Err(m) => Shown::Failed(m),
            },
        };
        self.shown.insert(id.to_owned(), s);
    }

    /// A button on the card: its handler, then a new render.
    pub fn click(&mut self, id: &str, button: u32) {
        if let Some((h, Some((plugin, entry)))) = self.plugin(id) {
            let r = h.borrow_mut().click(Kind::Widget, &plugin, &entry, button);
            return self.show_plugin(id, &h, &plugin, r);
        }
        let r = self.lib.widget(id).map(|w| w.click(button));
        match r {
            Some(Err(m)) => {
                self.shown.insert(id.to_owned(), Shown::Failed(m));
            }
            Some(Ok(())) => self.render(id),
            None => {}
        }
    }

    /// Seconds between renders (None: not loaded, or 0 = only after a click).
    pub fn refresh(&mut self, id: &str) -> Option<u32> {
        if let Some((h, at)) = self.plugin(id) {
            // Not loaded yet (or turned off): look again soon, so it shows once it is there.
            let Some((plugin, entry)) = at else {
                return Some(PLUGIN_RETRY);
            };
            let r = h.borrow().refresh(Kind::Widget, &plugin, &entry);
            return match r {
                None => Some(PLUGIN_RETRY),
                Some(0) => None,
                every => every,
            };
        }
        self.lib.widget(id).map(|w| w.refresh()).filter(|r| *r > 0)
    }

    /// What the card of file `id` shows (None: not read yet).
    pub fn content(&self, id: &str) -> Option<Content> {
        Some(match self.shown.get(id)? {
            Shown::Tree { name, node } => Content::View {
                name: name.clone(),
                node: node.clone(),
            },
            Shown::Failed(m) => Content::Error {
                name: format!("{id}.lua"),
                message: crate::copy::LUA_WIDGET_FAILED.into(),
                details: vec![m.clone()],
            },
            Shown::Missing => Content::Error {
                name: format!("{id}.lua"),
                message: crate::copy::WIDGET_MISSING.into(),
                details: vec![format!("Widget file: ~/.config/wolos/widgets/{id}.lua")],
            },
        })
    }
}

/// A widget card drawing `node`; a button press goes to `clicked` with its id.
pub fn card(
    size: Size,
    node: &Node,
    interactive: bool,
    clicked: impl Fn(u32) + 'static,
) -> gtk::Box {
    let view = View::new(
        node,
        Sink::new(move |a| {
            if let Act::Clicked(b) = a {
                clicked(b);
            }
        }),
    );
    let body = view.widget().clone();
    body.set_valign(gtk::Align::Center);
    body.set_can_target(interactive);
    d::card(size, &body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wol_lua::Stamp;

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
        fn system(&self) -> wol_lua::SystemInfo {
            unreachable!()
        }
        fn notify(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
    }

    /// [`Fixed`] that counts `wol.now()` calls: a script that reads the clock first tells the test
    /// how often it really ran, which a stopwatch cannot do on a loaded machine.
    #[derive(Default)]
    struct Counted(std::cell::Cell<u32>);
    impl Services for Counted {
        fn now(&self) -> Stamp {
            self.0.set(self.0.get() + 1);
            Fixed.now()
        }
        fn system(&self) -> wol_lua::SystemInfo {
            unreachable!()
        }
        fn notify(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
    }

    /// Wall-clock ceiling for "stopped at the deadline". The deadline is wall-clock, so on a
    /// loaded machine (CI) the stop lands late by however long the thread waits for a core,
    /// never early. Without the deadline only the instruction budget would stop this loop, and
    /// that takes minutes: 2 s still tells the two apart with room for any scheduler.
    const STOPPED_WITHIN: std::time::Duration = std::time::Duration::from_secs(2);

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wol-lua-shell-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn files_become_cards_and_errors_say_why() {
        let d = dir("cards");
        std::fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../wol-lua/widgets/clock.lua"),
            d.join("clock.lua"),
        )
        .unwrap();
        std::fs::write(d.join("broken.lua"), "return 1").unwrap();
        let mut l = Lua::new(d.clone(), Rc::new(Fixed));
        let mut changed = l.poll();
        changed.sort();
        assert_eq!(changed, ["broken", "clock"]);
        let Some(Content::View { name, node }) = l.content("clock") else {
            panic!("{:?}", l.content("clock"))
        };
        assert_eq!(name, "Clock");
        assert!(format!("{node:?}").contains("09:41"), "{node:?}");
        assert!(matches!(l.content("broken"), Some(Content::Error { .. })));
        assert_eq!(l.refresh("clock"), Some(60));
        assert!(l.poll().is_empty(), "nothing changed");
        std::fs::remove_file(d.join("clock.lua")).unwrap();
        assert_eq!(l.poll(), ["clock"]);
        assert!(matches!(l.content("clock"), Some(Content::Error { .. })));
        assert_eq!(l.content("nope"), None);
        let _ = std::fs::remove_dir_all(d);
    }

    /// R2C-3: a loop of capped C calls stops at the render deadline, and the card stays stopped
    /// (the timer's next render does not run the script again) until the file changes.
    #[test]
    fn a_slow_widget_is_stopped_at_the_deadline_and_stays_stopped() {
        let d = dir("slow");
        std::fs::write(
            d.join("slow.lua"),
            "return wol.widget { name = 'Slow', render = function() wol.now() local t = {} while true do table.move(t, 1, 2^20 - 1, 2) end; end }",
        )
        .unwrap();
        let runs = Rc::new(Counted::default());
        let mut l = Lua::new(d.clone(), runs.clone());
        let start = std::time::Instant::now();
        assert_eq!(l.poll(), ["slow"]);
        assert!(start.elapsed() < STOPPED_WITHIN, "{:?}", start.elapsed());
        let Some(Content::Error { details, .. }) = l.content("slow") else {
            panic!("{:?}", l.content("slow"))
        };
        assert_eq!(details, [wol_lua::TOO_LONG]);
        assert_eq!(runs.0.get(), 1, "the first render ran the script");
        l.render("slow");
        assert_eq!(runs.0.get(), 1, "a stopped card answers without running");
        assert!(matches!(l.content("slow"), Some(Content::Error { .. })));
        let _ = std::fs::remove_dir_all(d);
    }
}
