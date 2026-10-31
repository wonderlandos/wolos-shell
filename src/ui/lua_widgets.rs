//! Lua desktop widgets (ADR 0020, `wol_lua`): `~/.config/wolos/widgets/<id>.lua`, placed on the
//! desktop as kind `lua:<id>`. Scripts run sandboxed in the shell; each file reloads when it
//! changes, renders on the desktop widget timer at its own `refresh`, and after a click. The tree
//! it returns is drawn with Kit parts (`ui::app_view`) on a widget card.
use super::app_view::{Act, Sink, View};
use super::widgets::Content;
use crate::kit::data as d;
use gtk::prelude::*;
use shared::view::Node;
use shared::widgets::Size;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use wol_lua::{Change, Library, Services};

/// One file's last render.
#[derive(Clone, Debug, PartialEq)]
enum Shown {
    Tree { name: String, node: Node },
    Failed(String),
    Missing,
}

/// The widget folder and what each file shows now.
pub struct Lua {
    lib: Library,
    shown: HashMap<String, Shown>,
}

impl Lua {
    pub fn new(dir: PathBuf, services: Rc<dyn Services>) -> Lua {
        Lua {
            lib: Library::new(dir, services),
            shown: HashMap::new(),
        }
    }

    /// The person's folder, with this machine's clock and services.
    pub fn user() -> Lua {
        Lua::new(wol_lua::user_dir(), Rc::new(wol_lua::Live))
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
}
