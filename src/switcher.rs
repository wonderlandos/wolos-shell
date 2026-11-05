// SPDX-License-Identifier: GPL-2.0-or-later
//! App switcher (Super+Tab): compositor clients grouped by app class, most recently used first.
use crate::hypr::Client;

/// One app in the strip; `address` is its most recently focused window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    pub class: String,
    pub name: String,
    pub icon: String,
    pub address: String,
}

/// Groups clients by class (case-insensitive), ordered by each app's most recent focus.
/// Names and icons come from the dock pins, then installed .desktop entries.
pub fn apps(clients: &[Client], installed: &[crate::desktop::Entry]) -> Vec<App> {
    use crate::apps::{frontmost_name, here, icon_for};
    let mut sorted: Vec<&Client> = clients.iter().filter(|c| !c.class.is_empty()).collect();
    sorted.sort_by_key(|c| c.focus);
    let mut out: Vec<App> = Vec::new();
    for c in sorted {
        let class = c.class.to_lowercase();
        if out.iter().all(|a| a.class != class) {
            out.push(App {
                name: frontmost_name(&class, "", here(), installed),
                icon: icon_for(&class, here(), installed),
                class,
                address: c.address.clone(),
            });
        }
    }
    out
}

/// Apps for the strip from `j/clients` (`hypr::read_clients`). Fails closed: None (switcher stays
/// hidden) when the compositor did not answer, the reply is unreadable, or no app window is open.
pub fn to_show(
    clients: Option<&[Client]>,
    installed: &[crate::desktop::Entry],
) -> Option<Vec<App>> {
    let a = apps(clients?, installed);
    (!a.is_empty()).then_some(a)
}

/// Selection while Super is held. Opening selects the previous app (index 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cycle {
    pub index: usize,
    pub len: usize,
}

impl Cycle {
    pub fn open(len: usize) -> Self {
        Cycle {
            index: usize::from(len > 1),
            len,
        }
    }
    pub fn next(self) -> Self {
        self.step(1)
    }
    pub fn prev(self) -> Self {
        self.step(self.len.saturating_sub(1))
    }
    fn step(self, by: usize) -> Self {
        Cycle {
            index: if self.len == 0 {
                0
            } else {
                (self.index + by) % self.len
            },
            ..self
        }
    }
}

/// Committing a selection, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Hide,
    /// Compositor `focuswindow` dispatch argument.
    Focus(String),
}

/// Focus first, then hide. The strip holds the keyboard on demand (`ui::switcher::SPEC`), so
/// `focuswindow` works while it is mapped; the compositor's refocus when the layer unmaps goes to the
/// window focused last (`refocusLastWindow`), which is now the selection. An exclusive layer
/// refused the focus and brought the old window forward first (60 ms, two activations).
pub fn commit(selected: Option<&App>) -> Vec<Step> {
    let mut v = Vec::new();
    if let Some(a) = selected {
        v.push(Step::Focus(format!("address:{}", a.address)));
    }
    v.push(Step::Hide);
    v
}

/// What a shell-socket command does to the switcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Open,
    Next,
    Prev,
    /// Super released: focus the selection and close.
    Commit,
    None,
}

/// `show|toggle switcher` opens or advances, `show switcher-prev` goes back, `hide switcher` commits.
pub fn op(verb: crate::protocol::Verb, target: &str, visible: bool) -> Op {
    use crate::protocol::Verb::*;
    match (verb, target, visible) {
        (Show | Toggle, "switcher", false) => Op::Open,
        (Show | Toggle, "switcher", true) => Op::Next,
        (Show | Toggle, "switcher-prev", true) => Op::Prev,
        (Show | Toggle, "switcher-prev", false) => Op::Open,
        (Hide, "switcher" | "switcher-prev", true) => Op::Commit,
        _ => Op::None,
    }
}

/// How long a `hide switcher` with nothing open cancels the next open (audit4 S5-3).
pub const CANCEL_OPEN: std::time::Duration = std::time::Duration::from_millis(300);

/// Without wol-hypr the Tab and the Super release are two `wol` processes racing to the socket:
/// the release can arrive first, find nothing open, and the show after it would open a strip that
/// waits for a release that already happened. A hide with nothing open therefore cancels an open
/// arriving within [`CANCEL_OPEN`]. With the plugin, wol-hypr sends both lines itself, in order,
/// and this never fires.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CancelOpen(Option<std::time::Instant>);

impl CancelOpen {
    /// The op to run for this command, given whether the strip is open (or opening) at `now`.
    pub fn filter(
        &mut self,
        verb: crate::protocol::Verb,
        target: &str,
        open: bool,
        now: std::time::Instant,
    ) -> Op {
        let o = op(verb, target, open);
        match o {
            // Only a stray release of the switcher arms it: `hide osd` and the like are not one.
            Op::None
                if !open
                    && verb == crate::protocol::Verb::Hide
                    && matches!(target, "switcher" | "switcher-prev") =>
            {
                self.0 = now.checked_add(CANCEL_OPEN);
                Op::None
            }
            Op::Open => match self.0.take() {
                Some(until) if now < until => Op::None,
                _ => Op::Open,
            },
            _ => o,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(addr: &str, class: &str, focus: i64) -> Client {
        Client {
            focus,
            ..Client::new(addr, class, "")
        }
    }

    #[test]
    fn groups_by_class_in_mru_order() {
        let got = apps(
            &[
                c("0x1", "foot", 2),
                c("0x2", "atlas", 0),
                c("0x3", "Foot", 1),
                c("0x4", "org.gnome.Nautilus", 3),
            ],
            &[],
        );
        let v: Vec<(&str, &str)> = got
            .iter()
            .map(|a| (a.class.as_str(), a.address.as_str()))
            .collect();
        assert_eq!(
            v,
            [
                ("atlas", "0x2"),
                ("foot", "0x3"),
                ("org.gnome.nautilus", "0x4")
            ]
        );
        assert!(apps(&[], &[]).is_empty());
        let names: Vec<&str> = got.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Browser", "Terminal", "Files"]);
    }

    #[test]
    fn opens_only_on_a_readable_nonempty_client_list() {
        let one = r#"[{"address":"0x1","class":"foot","title":"~","focusHistoryID":0}]"#;
        for (reply, opens) in [
            (None, false),             // no compositor answer (not the compositor, socket down)
            (Some("not json"), false), // unreadable
            (Some("[]"), false),       // nothing open: no "No open windows" pill
            (Some(one), true),
        ] {
            let clients = reply.map(crate::hypr::parse_clients);
            assert_eq!(
                to_show(clients.as_deref(), &[]).is_some(),
                opens,
                "{reply:?}"
            );
        }
    }

    #[test]
    fn cycle_wraps_both_ways() {
        for (len, open, next, prev) in [(0, 0, 0, 0), (1, 0, 0, 0), (2, 1, 0, 0), (3, 1, 2, 0)] {
            let o = Cycle::open(len);
            assert_eq!(
                (o.index, o.next().index, o.prev().index),
                (open, next, prev),
                "len {len}"
            );
        }
        assert_eq!(Cycle::open(3).prev().prev().index, 2);
    }

    /// Live on the compositor 0.56: releasing Super left the old window focused (exclusive layer), then
    /// the hide-first order showed the old window for 60 ms before the selection.
    #[test]
    fn commit_focuses_then_hides() {
        let a = App {
            class: "foot".into(),
            name: "Terminal".into(),
            icon: "wolos-terminal".into(),
            address: "0x1".into(),
        };
        assert_eq!(
            commit(Some(&a)),
            [Step::Focus("address:0x1".into()), Step::Hide]
        );
        assert_eq!(commit(None), [Step::Hide]);
    }

    /// Audit4 S5-3: (Hide, Show) in that order opens nothing; later opens are not affected.
    #[test]
    fn a_release_before_the_show_cancels_that_open() {
        use crate::protocol::Verb::*;
        use std::time::Duration;
        let t0 = std::time::Instant::now();
        let mut c = CancelOpen::default();
        assert_eq!(c.filter(Hide, "switcher", false, t0), Op::None);
        assert_eq!(
            c.filter(Show, "switcher", false, t0 + Duration::from_millis(40)),
            Op::None,
            "the show of the tap that was already released"
        );
        assert_eq!(
            c.filter(Show, "switcher", false, t0 + Duration::from_millis(60)),
            Op::Open,
            "one cancel per stray release"
        );
        // Shift+Tab first: same.
        let mut c = CancelOpen::default();
        c.filter(Hide, "switcher", false, t0);
        assert_eq!(c.filter(Show, "switcher-prev", false, t0), Op::None);
        // Past the window: a real new ⌘Tab.
        let mut c = CancelOpen::default();
        c.filter(Hide, "switcher", false, t0);
        assert_eq!(
            c.filter(Show, "switcher", false, t0 + CANCEL_OPEN),
            Op::Open
        );
        // The normal order is untouched: show, tabs, release commits, the next show opens.
        let mut c = CancelOpen::default();
        assert_eq!(c.filter(Show, "switcher", false, t0), Op::Open);
        assert_eq!(c.filter(Show, "switcher", true, t0), Op::Next);
        assert_eq!(c.filter(Hide, "switcher", true, t0), Op::Commit);
        assert_eq!(c.filter(Show, "switcher", false, t0), Op::Open);
        // Unrelated commands neither arm nor consume it.
        let mut c = CancelOpen::default();
        assert_eq!(c.filter(Hide, "osd", false, t0), Op::None);
        assert_eq!(c.filter(Show, "switcher", false, t0), Op::Open);
    }

    #[test]
    fn socket_ops() {
        use crate::protocol::Verb::*;
        for (verb, target, visible, want) in [
            (Show, "switcher", false, Op::Open),
            (Toggle, "switcher", false, Op::Open),
            (Show, "switcher", true, Op::Next),
            (Toggle, "switcher", true, Op::Next),
            (Show, "switcher-prev", true, Op::Prev),
            (Show, "switcher-prev", false, Op::Open),
            (Hide, "switcher", true, Op::Commit),
            (Hide, "switcher", false, Op::None),
            (Reload, "", true, Op::None),
            (Show, "osd", true, Op::None),
        ] {
            assert_eq!(
                op(verb, target, visible),
                want,
                "{verb:?} {target} {visible}"
            );
        }
    }
}
