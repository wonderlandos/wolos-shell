// SPDX-License-Identifier: GPL-2.0-or-later
//! System Monitor model: the last [`Sample`] and a minute of history per graph, plus the rows
//! the pages show (apps named, the process tree, startup services). Pure: no GTK, no files;
//! the feed (`feed.rs`) fills it, the pages (`overview.rs`, ...) read it.
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use wol_core::monitor::apps::{self, Energy, Rest};
use wol_core::monitor::procs::Proc;
use wol_core::monitor::sampler::Sample;
use wol_core::monitor::startup::Unit;

/// Samples a graph keeps: one a second for a minute.
pub const HISTORY: usize = 60;

/// The last [`HISTORY`] values of one quantity, oldest first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ring(VecDeque<f64>);

impl Ring {
    pub fn push(&mut self, v: f64) {
        if self.0.len() == HISTORY {
            self.0.pop_front();
        }
        self.0.push_back(v);
    }
    pub fn values(&self) -> Vec<f64> {
        self.0.iter().copied().collect()
    }
    /// No sample yet.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn last(&self) -> f64 {
        self.0.back().copied().unwrap_or(0.0)
    }
}

/// A pair of rings (received/sent, read/written).
pub type Pair = (Ring, Ring);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct History {
    pub cpu: Ring,
    pub memory: Ring,
    pub swap: Ring,
    pub gpu: Ring,
    pub temp: Ring,
    /// All disks together, and per disk.
    pub disk: Pair,
    pub disks: BTreeMap<String, Pair>,
    /// All interfaces together, and per interface.
    pub net: Pair,
    pub nets: BTreeMap<String, Pair>,
    /// CPU percent per app scope (energy).
    pub apps: HashMap<String, Ring>,
}

/// The Startup page's list: the services, or that they could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Startup {
    Rows(Vec<StartupRow>),
    NotRead,
}

/// One startup service as the page lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupRow {
    pub unit: Unit,
    /// `Description=`, else the unit name.
    pub title: String,
}

#[derive(Clone, Debug, Default)]
pub struct Model {
    pub last: Sample,
    pub history: History,
    /// Installed apps (names and icons of the app scopes); read once by the feed.
    pub entries: Vec<crate::desktop::Entry>,
    /// None: not read yet.
    pub startup: Option<Startup>,
    /// Processes the person folded in the tree.
    pub folded: HashSet<i32>,
    pub filter: String,
    /// The table of the last sample that had one (the Processes page asks every second).
    pub procs: Vec<Proc>,
}

fn sum(it: impl Iterator<Item = (f64, f64)>) -> (f64, f64) {
    it.fold((0.0, 0.0), |(a, b), (x, y)| (a + x, b + y))
}

/// Pushes a pair of values onto the pair of rings of `key`, creating it.
fn push_pair(map: &mut BTreeMap<String, Pair>, key: &str, v: (f64, f64)) {
    let p = map.entry(key.to_owned()).or_default();
    p.0.push(v.0);
    p.1.push(v.1);
}

fn share(used: i64, total: i64) -> f64 {
    if total > 0 {
        100.0 * used as f64 / total as f64
    } else {
        0.0
    }
}

impl Model {
    /// Takes one sample in: the histories move on, what vanished (a disk, an app) is dropped.
    pub fn push(&mut self, s: Sample) {
        let h = &mut self.history;
        h.cpu.push(s.cpu.first().copied().unwrap_or(0.0));
        h.memory.push(share(s.memory.used, s.memory.total));
        h.swap.push(share(s.memory.swap_used, s.memory.swap_total));
        h.gpu
            .push(s.gpus.iter().filter_map(|g| g.busy).fold(0.0, f64::max));
        h.temp
            .push(wol_core::monitor::temps::hottest(&s.temps).map_or(0.0, |t| t.celsius));
        let d = sum(s.disk_io.iter().map(|d| (d.read_rate, d.write_rate)));
        h.disk.0.push(d.0);
        h.disk.1.push(d.1);
        h.disks
            .retain(|k, _| s.disk_io.iter().any(|d| d.name == *k));
        for d in &s.disk_io {
            push_pair(&mut h.disks, &d.name, (d.read_rate, d.write_rate));
        }
        let n = sum(s.net.iter().map(|n| (n.rx_rate, n.tx_rate)));
        h.net.0.push(n.0);
        h.net.1.push(n.1);
        h.nets.retain(|k, _| s.net.iter().any(|n| n.name == *k));
        for i in &s.net {
            push_pair(&mut h.nets, &i.name, (i.rx_rate, i.tx_rate));
        }
        h.apps.retain(|k, _| s.apps.iter().any(|a| a.unit == *k));
        for a in &s.apps {
            h.apps.entry(a.unit.clone()).or_default().push(a.cpu);
        }
        // The table only while a sample carries one: a page that comes back never shows a
        // table from minutes ago.
        match &s.procs {
            Some(p) => self.procs.clone_from(p),
            None => self.procs.clear(),
        }
        self.last = s;
    }

    /// The Apps page's rows, biggest first.
    pub fn app_rows(&self) -> Vec<AppRow> {
        let mut v: Vec<AppRow> = self
            .last
            .apps
            .iter()
            .map(|a| AppRow {
                unit: a.unit.clone(),
                name: crate::apps::frontmost_name(&a.app, "", &[], &self.entries),
                icon: crate::apps::icon_for(&a.app, &[], &self.entries),
                memory: a.pss.unwrap_or(a.memory),
                cpu: a.cpu,
                energy: apps::energy(
                    &self
                        .history
                        .apps
                        .get(&a.unit)
                        .map(Ring::values)
                        .unwrap_or_default(),
                ),
                rest: a.rest,
                targets: a.targets.clone(),
            })
            .collect();
        v.sort_by(|a, b| b.memory.cmp(&a.memory).then_with(|| a.name.cmp(&b.name)));
        v
    }

    /// The Processes page's rows: the tree, or the matches of the filter as a flat list.
    pub fn proc_rows(&self) -> Vec<TreeRow> {
        let q = self.filter.trim();
        if q.is_empty() {
            tree(&self.procs, &self.folded)
        } else {
            matches(&self.procs, q)
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AppRow {
    pub unit: String,
    pub name: String,
    pub icon: String,
    /// PSS when read, else the scope's `memory.current`.
    pub memory: u64,
    pub cpu: f64,
    pub energy: Energy,
    pub rest: Rest,
    /// The processes Quit signals, as this sample saw them.
    pub targets: Vec<wol_core::monitor::procs::Target>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TreeRow {
    pub proc: Proc,
    pub depth: usize,
    /// Children it has in the tree (0 in a filtered list).
    pub children: usize,
    pub folded: bool,
}

/// Depth-first tree: roots are processes whose parent is not in the table; children by CPU then
/// pid. A folded process hides its descendants.
pub fn tree(procs: &[Proc], folded: &HashSet<i32>) -> Vec<TreeRow> {
    let pids: HashSet<i32> = procs.iter().map(|p| p.pid).collect();
    let mut kids: HashMap<i32, Vec<&Proc>> = HashMap::new();
    let mut roots = Vec::new();
    for p in procs {
        if p.ppid != p.pid && pids.contains(&p.ppid) {
            kids.entry(p.ppid).or_default().push(p);
        } else {
            roots.push(p);
        }
    }
    let order = |v: &mut Vec<&Proc>| {
        v.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(a.pid.cmp(&b.pid)));
    };
    order(&mut roots);
    for v in kids.values_mut() {
        order(v);
    }
    let mut out = Vec::with_capacity(procs.len());
    // Explicit stack: a deep chain of processes never overflows the thread's stack.
    let mut stack: Vec<(&Proc, usize)> = roots.into_iter().rev().map(|p| (p, 0)).collect();
    while let Some((p, depth)) = stack.pop() {
        let ch = kids.get(&p.pid).map_or(&[][..], Vec::as_slice);
        let f = folded.contains(&p.pid);
        out.push(TreeRow {
            proc: p.clone(),
            depth,
            children: ch.len(),
            folded: f,
        });
        if !f {
            stack.extend(ch.iter().rev().map(|c| (*c, depth + 1)));
        }
    }
    out
}

/// Processes whose name contains `q` (any case) or whose number is `q`, busiest first.
pub fn matches(procs: &[Proc], q: &str) -> Vec<TreeRow> {
    let q = q.to_lowercase();
    let mut v: Vec<TreeRow> = procs
        .iter()
        .filter(|p| p.name.to_lowercase().contains(&q) || p.pid.to_string() == q)
        .map(|p| TreeRow {
            proc: p.clone(),
            depth: 0,
            children: 0,
            folded: false,
        })
        .collect();
    v.sort_by(|a, b| {
        b.proc
            .cpu
            .total_cmp(&a.proc.cpu)
            .then(a.proc.pid.cmp(&b.proc.pid))
    });
    v
}

/// The signal that ends `p`: a frozen process cannot answer a polite TERM, so it gets KILL
/// (Force Quit), the same rule as a paused app.
pub fn end_signal(p: &Proc) -> &'static str {
    if p.frozen { "KILL" } else { "TERM" }
}

// ---- Numbers as people read them (decimal units, as Storage and Memory Saver show sizes) ----
// Not `wol_core::memsaver::size`: that one is MB/GB only for Settings; this one also needs B,
// KB, TB and rates, and changing it would move Settings' text.

/// One decimal below 10 of a unit ("1.4 GB"), none above ("16 GB", "214 GB").
fn unit(v: f64, u: &str) -> String {
    if v < 9.95 {
        format!("{v:.1} {u}")
    } else {
        format!("{v:.0} {u}")
    }
}

pub fn bytes(b: u64) -> String {
    let f = b as f64;
    if f >= 1e12 {
        unit(f / 1e12, "TB")
    } else if f >= 1e9 {
        unit(f / 1e9, "GB")
    } else if f >= 1e6 {
        unit(f / 1e6, "MB")
    } else if f >= 1e3 {
        unit(f / 1e3, "KB")
    } else {
        format!("{b} B")
    }
}

pub fn rate(bps: f64) -> String {
    format!("{}/s", bytes(bps.max(0.0).round() as u64))
}

pub fn percent(p: f64) -> String {
    format!("{:.0}%", p.clamp(0.0, 999.0))
}

pub fn celsius(c: f64) -> String {
    format!("{c:.0} °C")
}

#[cfg(test)]
mod tests {
    use super::*;
    use wol_core::system::proc::ProcessState;

    fn p(pid: i32, ppid: i32, name: &str, cpu: f32) -> Proc {
        Proc {
            pid,
            ppid,
            name: name.into(),
            user: "ana".into(),
            cpu,
            rss: 0,
            threads: 1,
            state: ProcessState::Sleeping,
            start: 0,
            frozen: false,
        }
    }

    #[test]
    fn a_frozen_process_is_force_quit_and_a_stale_table_is_dropped() {
        let mut f = p(5, 1, "paused", 0.0);
        assert_eq!(end_signal(&f), "TERM");
        f.frozen = true;
        assert_eq!(end_signal(&f), "KILL");
        let mut m = Model::default();
        m.push(Sample {
            procs: Some(vec![f]),
            ..Default::default()
        });
        assert_eq!(m.procs.len(), 1);
        m.push(Sample::default());
        assert!(m.procs.is_empty(), "no table in the sample, none kept");
    }

    #[test]
    fn the_history_keeps_one_minute() {
        let mut r = Ring::default();
        for i in 0..75 {
            r.push(f64::from(i));
        }
        let v = r.values();
        assert_eq!(v.len(), HISTORY);
        assert_eq!((v[0], r.last()), (15.0, 74.0));
    }

    #[test]
    fn the_tree_nests_children_busiest_first_and_folds() {
        let procs = [
            p(1, 0, "systemd", 0.0),
            p(10, 1, "shell", 1.0),
            p(11, 1, "browser", 9.0),
            p(12, 11, "renderer", 3.0),
            p(13, 99, "orphan", 0.0),
        ];
        let rows = tree(&procs, &HashSet::new());
        let shape: Vec<_> = rows
            .iter()
            .map(|r| (r.proc.name.as_str(), r.depth, r.children))
            .collect();
        assert_eq!(
            shape,
            [
                ("systemd", 0, 2),
                ("browser", 1, 1),
                ("renderer", 2, 0),
                ("shell", 1, 0),
                ("orphan", 0, 0)
            ]
        );
        let rows = tree(&procs, &HashSet::from([11]));
        assert!(rows.iter().all(|r| r.proc.name != "renderer"));
        assert!(rows.iter().find(|r| r.proc.pid == 11).unwrap().folded);
        let m = matches(&procs, "R");
        assert_eq!(
            m.iter().map(|r| r.proc.pid).collect::<Vec<_>>(),
            [11, 12, 13],
            "browser, renderer, orphan"
        );
        assert_eq!(matches(&procs, "12")[0].proc.name, "renderer");
    }

    #[test]
    fn a_sample_moves_the_graphs_and_drops_what_left() {
        use wol_core::monitor::disk::Io;
        let mut m = Model::default();
        let mut s = Sample {
            cpu: vec![40.0, 30.0, 50.0],
            disk_io: vec![Io {
                name: "sda".into(),
                read_rate: 10.0,
                write_rate: 5.0,
            }],
            ..Default::default()
        };
        s.memory.total = 200;
        s.memory.used = 50;
        m.push(s.clone());
        assert_eq!(m.history.cpu.last(), 40.0);
        assert_eq!(m.history.memory.last(), 25.0);
        assert_eq!(m.history.disks["sda"].0.last(), 10.0);
        s.disk_io.clear();
        m.push(s);
        assert!(m.history.disks.is_empty(), "the disk was removed");
        assert_eq!(m.history.disk.0.values(), [10.0, 0.0]);
    }

    #[test]
    fn numbers_read_like_storage() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(820_000_000), "820 MB");
        assert_eq!(bytes(1_400_000_000), "1.4 GB");
        assert_eq!(bytes(214_000_000_000), "214 GB");
        assert_eq!(rate(250_000.0), "250 KB/s");
        assert_eq!(rate(1_070_000.0), "1.1 MB/s");
        assert_eq!(percent(23.4), "23%");
        assert_eq!(celsius(54.25), "54 °C");
    }
}
