// SPDX-License-Identifier: GPL-2.0-or-later
//! A minute of a made-up but plausible machine (goldens, `WOL_MONITOR_DEMO=1`): the same numbers
//! every run, so the screenshots only change when the pages do.
use super::model::{Model, Startup, StartupRow};
use crate::desktop::Entry;
use wol_core::monitor::apps::{AppUse, Rest};
use wol_core::monitor::disk::{DiskUsage, Io};
use wol_core::monitor::gpu::Gpu;
use wol_core::monitor::net::{Interface, Kind};
use wol_core::monitor::procs::Proc;
use wol_core::monitor::sampler::Sample;
use wol_core::monitor::startup::Unit;
use wol_core::monitor::temps::Temperature;
use wol_core::system::Load;
use wol_core::system::proc::ProcessState;
use wol_core::system::stats::MemoryStats;

/// A smooth wave between `lo` and `hi` at sample `i`.
fn wave(i: usize, period: f64, phase: f64, lo: f64, hi: f64) -> f64 {
    let t = (i as f64 / period + phase) * std::f64::consts::TAU;
    lo + (hi - lo) * (0.5 + 0.5 * t.sin()) * (0.75 + 0.25 * (t * 2.3).cos().abs())
}

const GB: f64 = 1e9;

fn entry(id: &str, name: &str, icon: &str) -> Entry {
    Entry {
        id: format!("{id}.desktop"),
        name: name.into(),
        icon: icon.into(),
        ..Default::default()
    }
}

/// (unit, app id, memory, PSS, CPU low..high, rest).
const APPS: [(&str, &str, f64, f64, (f64, f64), Rest); 6] = [
    (
        "app-wol-helium-2100.scope",
        "helium",
        1.9,
        1.42,
        (6.0, 22.0),
        Rest::Running,
    ),
    (
        "app-gnome-org.gnome.Nautilus-1400.scope",
        "org.gnome.Nautilus",
        0.31,
        0.24,
        (0.0, 3.0),
        Rest::Running,
    ),
    (
        "app-glib-foot-1300.scope",
        "foot",
        0.12,
        0.08,
        (0.5, 2.5),
        Rest::Running,
    ),
    (
        "app-gnome-org.gnome.TextEditor-1500.scope",
        "org.gnome.TextEditor",
        0.21,
        0.16,
        (0.0, 1.0),
        Rest::Running,
    ),
    (
        "app-wol-org.gnome.music-1600.scope",
        "org.gnome.music",
        0.42,
        0.19,
        (0.0, 0.0),
        Rest::Resting,
    ),
    (
        "app-wol-discord-1700.scope",
        "discord",
        0.64,
        0.38,
        (0.0, 0.0),
        Rest::Frozen,
    ),
];

fn procs(i: usize) -> Vec<Proc> {
    let p = |pid, ppid, name: &str, user: &str, cpu: f64, mb: u64| Proc {
        pid,
        ppid,
        name: name.into(),
        user: user.into(),
        cpu: cpu as f32,
        rss: mb * 1_000_000,
        threads: 1,
        state: ProcessState::Sleeping,
        start: 0,
        frozen: false,
    };
    let w = |lo, hi, ph| (wave(i, 17.0, ph, lo, hi) * 10.0).round() / 10.0;
    vec![
        p(1, 0, "systemd", "root", 0.0, 14),
        p(412, 1, "systemd-journal", "root", 0.0, 22),
        p(640, 1, "wold", "root", 0.3, 18),
        p(901, 1, "systemd", "ana", 0.0, 11),
        p(950, 901, "compositor", "ana", w(2.0, 6.0, 0.1), 210),
        p(1001, 901, "wol-shell", "ana", w(0.5, 2.0, 0.4), 96),
        p(1010, 901, "pipewire", "ana", 0.4, 19),
        p(1011, 901, "wireplumber", "ana", 0.1, 28),
        p(1300, 950, "foot", "ana", w(0.5, 2.5, 0.7), 80),
        p(1301, 1300, "bash", "ana", 0.0, 6),
        p(1302, 1301, "cargo", "ana", w(0.0, 40.0, 0.2), 420),
        p(1400, 950, "nautilus", "ana", 0.2, 240),
        p(1500, 950, "gnome-text-editor", "ana", 0.0, 160),
        p(2100, 950, "helium", "ana", w(4.0, 12.0, 0.3), 610),
        p(
            2101,
            2100,
            "helium --type=gpu",
            "ana",
            w(1.0, 6.0, 0.5),
            230,
        ),
        p(
            2102,
            2100,
            "helium --type=renderer",
            "ana",
            w(1.0, 9.0, 0.9),
            580,
        ),
    ]
}

fn sample(i: usize) -> Sample {
    let total = wave(i, 23.0, 0.0, 8.0, 46.0);
    let mut cpu = vec![(total * 10.0).round() / 10.0];
    cpu.extend((0..8).map(|c| wave(i, 11.0 + c as f64, c as f64 * 0.13, 2.0, 70.0)));
    let rx = wave(i, 9.0, 0.2, 20e3, 2.4e6);
    let tx = wave(i, 13.0, 0.6, 5e3, 180e3);
    let read = wave(i, 19.0, 0.4, 0.0, 38e6);
    let write = wave(i, 7.0, 0.1, 0.0, 9e6);
    Sample {
        at: i as f64,
        cpu,
        memory: MemoryStats {
            total: 16_000_000_000,
            used: (wave(i, 60.0, 0.0, 5.9, 6.5) * GB) as i64,
            swap_total: 8_000_000_000,
            swap_used: 410_000_000,
        },
        load: Some(Load {
            one: 0.62,
            five: 0.55,
            fifteen: 0.48,
        }),
        net: vec![
            Interface {
                name: "wlan0".into(),
                kind: Kind::Wireless,
                up: true,
                rx_rate: rx,
                tx_rate: tx,
                rx_total: 1_840_000_000,
                tx_total: 212_000_000,
            },
            Interface {
                name: "enp3s0".into(),
                kind: Kind::Wired,
                up: false,
                rx_rate: 0.0,
                tx_rate: 0.0,
                rx_total: 0,
                tx_total: 0,
            },
        ],
        disk_io: vec![Io {
            name: "nvme0n1".into(),
            read_rate: read,
            write_rate: write,
        }],
        mounts: Some(vec![
            DiskUsage {
                mount: "/".into(),
                device: "/dev/nvme0n1p2".into(),
                total: 511_000_000_000,
                used: 214_000_000_000,
                available: 297_000_000_000,
            },
            DiskUsage {
                mount: "/boot".into(),
                device: "/dev/nvme0n1p1".into(),
                total: 1_070_000_000,
                used: 98_000_000,
                available: 972_000_000,
            },
        ]),
        gpus: vec![Gpu {
            card: "card0".into(),
            driver: "amdgpu".into(),
            busy: Some(wave(i, 15.0, 0.3, 3.0, 35.0).round()),
            vram_used: Some(1 << 30),
            vram_total: Some(8 << 30),
        }],
        temps: vec![
            Temperature {
                name: "k10temp Tctl".into(),
                celsius: wave(i, 30.0, 0.2, 48.0, 61.0).round(),
            },
            Temperature {
                name: "nvme Composite".into(),
                celsius: 41.0,
            },
        ],
        apps: APPS
            .iter()
            .enumerate()
            .map(|(n, (unit, app, mem, pss, (lo, hi), rest))| AppUse {
                unit: (*unit).into(),
                app: (*app).into(),
                pids: vec![1300 + i32::try_from(n).unwrap_or(0)],
                targets: Vec::new(),
                memory: (mem * GB) as u64,
                swap: 0,
                pss: Some((pss * GB) as u64),
                cpu: (wave(i, 21.0, n as f64 * 0.17, *lo, *hi) * 10.0).round() / 10.0,
                frozen: *rest == Rest::Frozen,
                rest: *rest,
            })
            .collect(),
        procs: Some(procs(i)),
        saver_on: true,
    }
}

fn unit(name: &str, title: &str, enabled: bool, mine: bool) -> StartupRow {
    StartupRow {
        unit: Unit {
            name: name.into(),
            path: format!("/home/ana/.config/systemd/user/{name}"),
            enabled,
            mine,
        },
        title: title.into(),
    }
}

pub fn model() -> Model {
    let mut m = Model {
        entries: vec![
            entry("helium", "Helium", "wolos-browser"),
            entry("org.gnome.Nautilus", "Files", "wolos-files"),
            entry("foot", "Terminal", "wolos-terminal"),
            entry("org.gnome.TextEditor", "Text Editor", "wolos-text-editor"),
            entry("org.gnome.Music", "Music", "wolos-music"),
        ],
        startup: Some(Startup::Rows(vec![
            unit("backup.service", "Back up to the home server", false, true),
            unit(
                "pipewire.service",
                "PipeWire Multimedia Service",
                true,
                false,
            ),
            unit("syncthing.service", "File synchronization", true, true),
            unit(
                "wireplumber.service",
                "Multimedia Service Session Manager",
                true,
                false,
            ),
        ])),
        ..Default::default()
    };
    for i in 0..super::model::HISTORY {
        m.push(sample(i));
    }
    m
}
