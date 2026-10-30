//! Keyboard shortcut sheet (⇧⌘/), pure side: the shortcuts in effect now, grouped like
//! Settings > Keyboard > Shortcuts, as the sheet shows them. Only keys a person presses: no
//! mouse drags or key releases, no hardware media keys, nothing turned off, tiling only while tiling is on.
use wol_core::shortcuts::{Group, Shortcut};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub title: &'static str,
    /// (what it does, keys): ("Show Spotlight search", "⌘Space").
    pub rows: Vec<(String, String)>,
}

pub fn sections(list: &[Shortcut], tiling: bool) -> Vec<Section> {
    Group::ALL
        .into_iter()
        .filter(|g| *g != Group::Media && (tiling || *g != Group::Tiling))
        .filter_map(|g| {
            let mut rows: Vec<(String, String)> = Vec::new();
            for s in list.iter().filter(|s| s.group == g) {
                let Some(c) = &s.combo else { continue };
                if s.bind.flags.contains(['m', 'r']) || rows.iter().any(|r| r.0 == s.name) {
                    continue;
                }
                rows.push((s.name.clone(), c.label()));
            }
            (!rows.is_empty()).then_some(Section {
                title: g.title(),
                rows,
            })
        })
        .collect()
}

/// Splits `list` into at most `n` columns in reading order, balanced by height (a section's
/// rows plus its title). Returns indices into `list`.
pub fn columns(list: &[Section], n: usize) -> Vec<Vec<usize>> {
    let h = |s: &Section| s.rows.len() + 2;
    let total: usize = list.iter().map(h).sum();
    let target = total.div_ceil(n.max(1));
    let mut cols: Vec<Vec<usize>> = vec![vec![]];
    let mut used = 0;
    for (i, s) in list.iter().enumerate() {
        let last = cols.len() == n.max(1);
        // Start a new column when this one is full, unless half the section would fit.
        if !last && used > 0 && used + h(s) / 2 > target {
            cols.push(vec![]);
            used = 0;
        }
        cols.last_mut().unwrap().push(i);
        used += h(s);
    }
    cols
}

#[cfg(test)]
mod tests {
    use super::*;
    use wol_core::shortcuts::{Bind, Combo};

    fn sc(name: &str, group: Group, mods: &str, key: &str, flags: &str) -> Shortcut {
        let combo = Combo::parse(mods, key);
        Shortcut {
            id: name.into(),
            name: name.into(),
            group,
            default: combo.clone(),
            combo: combo.clone(),
            bind: Bind {
                flags: flags.into(),
                combo: combo.unwrap(),
                dispatcher: "exec".into(),
                args: String::new(),
            },
            editable: flags.is_empty(),
        }
    }

    #[test]
    fn groups_in_settings_order_without_mouse_media_or_off() {
        let mut off = sc("Lock screen", Group::System, "SUPER CTRL", "Q", "");
        off.combo = None;
        let list = vec![
            sc("Force Quit apps", Group::System, "SUPER ALT", "Escape", ""),
            sc(
                "Show Spotlight search",
                Group::Spotlight,
                "SUPER",
                "Space",
                "",
            ),
            sc("Show Spotlight search", Group::Spotlight, "SUPER", "S", ""),
            sc("Move window", Group::Windows, "SUPER", "mouse:272", "m"),
            sc("Volume up", Group::Media, "", "XF86AudioRaiseVolume", "el"),
            sc("Tile left", Group::Tiling, "SUPER", "Left", ""),
            sc(
                "Finish switching apps",
                Group::Windows,
                "SUPER",
                "SUPER_L",
                "rt",
            ),
            off,
        ];
        let s = sections(&list, false);
        assert_eq!(
            s.iter().map(|x| x.title).collect::<Vec<_>>(),
            ["Spotlight", "System"]
        );
        assert_eq!(
            s[0].rows,
            [("Show Spotlight search".into(), "⌘Space".into())]
        );
        assert_eq!(s[1].rows.len(), 1, "a turned-off shortcut is not listed");
        assert!(
            !s.iter().any(|x| x.title == "Windows"),
            "no drags, no key releases"
        );
        assert!(sections(&list, true).iter().any(|x| x.title == "Tiling"));
    }

    #[test]
    fn columns_keep_order_and_balance() {
        let sec = |n: usize| Section {
            title: "x",
            rows: vec![(String::new(), String::new()); n],
        };
        let list = [sec(10), sec(2), sec(8), sec(4), sec(6), sec(1)];
        let c = columns(&list, 3);
        assert_eq!(c.len(), 3);
        assert_eq!(c.concat(), (0..6).collect::<Vec<_>>(), "reading order");
        let heights: Vec<usize> = c
            .iter()
            .map(|ix| ix.iter().map(|i| list[*i].rows.len() + 2).sum())
            .collect();
        assert!(
            heights.iter().max().unwrap() - heights.iter().min().unwrap() <= 10,
            "{heights:?}"
        );
        assert_eq!(columns(&list[..1], 3), [vec![0]]);
        assert!(columns(&[], 3).iter().all(|c| c.is_empty()));
    }

    /// The shipped conf lists the sheet's own shortcut, ⇧⌘/.
    #[test]
    fn the_shipped_conf_offers_the_sheet() {
        let list = wol_core::shortcuts::load_defaults(|_| None);
        let all = sections(&list, true);
        let sys = all.iter().find(|s| s.title == "System").expect("System");
        assert!(
            sys.rows
                .iter()
                .any(|r| r.0 == "Show keyboard shortcuts" && r.1 == "⇧⌘/"),
            "{:?}",
            sys.rows
        );
    }
}
