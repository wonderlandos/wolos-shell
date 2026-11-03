// SPDX-License-Identifier: GPL-2.0-or-later
//! Unit tests of the system data seam: labels, approvals, package sheets, and the fake and core backends.
use super::*;

#[test]
fn control_center_labels() {
    use crate::status::*;
    let nets = [WifiNetwork {
        ssid: "Home".into(),
        signal: 80,
        secure: true,
        connected: true,
    }];
    assert_eq!(
        wifi_subtitle(
            Network {
                online: true,
                wifi: true,
                signal: None
            },
            &nets
        ),
        "Home"
    );
    assert_eq!(
        wifi_subtitle(
            Network {
                online: true,
                wifi: false,
                signal: None
            },
            &[]
        ),
        "Ethernet"
    );
    assert_eq!(wifi_subtitle(Network::default(), &[]), "Not Connected");
    assert_eq!(
        battery_label(Some(Battery {
            percent: 82,
            status: BatteryStatus::Charging
        }))
        .unwrap(),
        "82% · Charging"
    );
    assert_eq!(battery_label(None), None);
    assert_eq!(signal_icon(30), "network-wireless-signal-ok-symbolic");
}

/// Helper approvals always check the password and leave no cached authorization behind.
#[test]
fn approve_revokes_around_the_admin_request() {
    use shared::admin::AdminReply;
    use shared::shell::ApprovalRequest as R;
    let sent = Arc::new(Mutex::new(Vec::<String>::new()));
    let log = sent.clone();
    let mut c = Controls::new(Arc::new(wol_core::system::fake::FakeHost::linux()));
    c.admin = Arc::new(move |req, pw| {
        log.lock()
            .unwrap()
            .push(format!("{} {}", req.action.name(), req.args.join(" ")));
        Some(match pw {
            "" => AdminReply::Ok,
            "good" => AdminReply::Ok,
            _ => AdminReply::Denied,
        })
    });
    let be = R::BootEnvironment {
        name: wol_core::update::SLOTS[1].into(),
    };
    assert_eq!(approve(&c, &be, "good"), Ok(()));
    let change = R::Change {
        capability: "appearance".into(),
        what: "switch to Dark appearance".into(),
    };
    assert_eq!(
        approve(&c, &change, "good"),
        Err(ActionError::Unavailable),
        "user-level changes never run as an admin request"
    );
    assert_eq!(approve(&c, &be, "bad"), Err(ActionError::Denied));
    let be_line = format!("boot-environment {}", wol_core::update::SLOTS[1]);
    assert_eq!(
        *sent.lock().unwrap(),
        [
            "revoke ",
            be_line.as_str(),
            "revoke ",
            "revoke ",
            be_line.as_str(),
            "revoke "
        ]
    );
}

/// Audit S7: wold's package sheet checks the typed password even while the session's
/// authorization is still cached (wol-admind would otherwise accept any text), and the
/// update verbs map to the wol-admind requests that keep holds (audit S2).
#[test]
fn package_sheets_check_the_password_even_with_a_warm_cache() {
    use shared::admin::{AdminAction, AdminReply};
    use shared::shell::ApprovalRequest as R;
    let warm = Arc::new(Mutex::new(true));
    let sent = Arc::new(Mutex::new(Vec::<String>::new()));
    let (cache, log) = (warm.clone(), sent.clone());
    let mut c = Controls::new(Arc::new(wol_core::system::fake::FakeHost::linux()));
    c.admin = Arc::new(move |req, pw| {
        let mut cached = cache.lock().unwrap();
        if req.action == AdminAction::Revoke {
            *cached = false;
            return Some(AdminReply::Ok);
        }
        log.lock()
            .unwrap()
            .push(format!("{} {}", req.action.name(), req.args.join(" ")));
        // wol-admind: a cached authorization allows; otherwise only the right password.
        Some(if *cached || pw == "good" {
            *cached = true;
            AdminReply::Ok
        } else {
            AdminReply::Denied
        })
    });
    let install = R::Packages {
        verb: "install".into(),
        names: vec!["gimp".into()],
    };
    assert_eq!(
        approve(&c, &install, "junk"),
        Err(ActionError::Denied),
        "a warm cache does not make junk a password"
    );
    assert!(!*warm.lock().unwrap(), "and nothing is left cached");
    assert_eq!(approve(&c, &install, "good"), Ok(()));
    assert!(!*warm.lock().unwrap(), "revoked after, too");
    let update = R::Packages {
        verb: "update".into(),
        names: vec!["firefox".into()],
    };
    let holding = R::Packages {
        verb: "update-holding".into(),
        names: vec!["firefox".into(), "gimp".into()],
    };
    assert_eq!(approve(&c, &update, "good"), Ok(()));
    assert_eq!(approve(&c, &holding, "good"), Ok(()));
    let sent = sent.lock().unwrap();
    assert_eq!(sent[sent.len() - 2], "pkg-install firefox", "{sent:?}");
    assert_eq!(sent[sent.len() - 1], "pkg-upgrade firefox gimp", "{sent:?}");
}

/// Audit C7-1 (ADR 0034): wold's local-file, Undo Update and channel sheets become one
/// wol-admind request each, with the password typed on the sheet, revoked around it.
#[test]
fn root_package_sheets_go_through_wol_admind() {
    use shared::admin::AdminReply;
    use shared::shell::ApprovalRequest as R;
    let sent = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
    let log = sent.clone();
    let mut c = Controls::new(Arc::new(wol_core::system::fake::FakeHost::linux()));
    c.admin = Arc::new(move |req, pw| {
        log.lock().unwrap().push((
            format!("{} {}", req.action.name(), req.args.join(" ")),
            pw.to_owned(),
        ));
        Some(AdminReply::Ok)
    });
    let staged = format!(
        "{}/0123456789abcdef0123456789abcdef/x-1-1-x86_64.pkg.tar.zst",
        shared::package::STAGING_DIR
    );
    for (req, line) in [
        (
            R::PackageFile {
                staged: staged.clone(),
                name: "x".into(),
                version: "1-1".into(),
                sha256: "0".repeat(64),
            },
            format!("pkg-install-file {staged} {}", "0".repeat(64)),
        ),
        (R::SystemRollback, "system-rollback ".to_owned()),
        (
            R::SystemChannel {
                channel: "beta".into(),
            },
            "system-channel beta".to_owned(),
        ),
    ] {
        sent.lock().unwrap().clear();
        assert_eq!(approve(&c, &req, "good"), Ok(()), "{req:?}");
        let s = sent.lock().unwrap();
        let names: Vec<&str> = s.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(names, ["revoke ", line.as_str(), "revoke "], "{req:?}");
        assert_eq!(s[1].1, "good");
    }
    // A path that is not wold's staged copy never reaches wol-admind.
    sent.lock().unwrap().clear();
    let bad = R::PackageFile {
        staged: "/home/ann/x-1-1-x86_64.pkg.tar.zst".into(),
        name: "x".into(),
        version: "1-1".into(),
        sha256: "0".repeat(64),
    };
    assert!(approve(&c, &bad, "good").is_err());
    assert!(
        sent.lock()
            .unwrap()
            .iter()
            .all(|(l, _)| l.starts_with("revoke"))
    );
}

#[test]
fn battery_files() {
    assert_eq!(
        parse_battery("55\n", "Charging\n"),
        Some(Battery {
            percent: 55,
            status: BatteryStatus::Charging
        })
    );
    assert_eq!(parse_battery("x", "Full"), None);
}

#[test]
fn reads_fake_sysfs_tree() {
    let root = std::env::temp_dir().join(format!("wol-sysfs-{}", std::process::id()));
    let bat = root.join("power_supply/BAT0");
    fs::create_dir_all(&bat).unwrap();
    fs::write(bat.join("capacity"), "20\n").unwrap();
    fs::write(bat.join("status"), "Discharging\n").unwrap();
    let wl = root.join("net/wlan0/wireless");
    fs::create_dir_all(&wl).unwrap();
    fs::write(root.join("net/wlan0/operstate"), "up\n").unwrap();
    let s = Sysfs { root: root.clone() };
    assert_eq!(s.battery().unwrap().percent, 20);
    assert_eq!(
        s.network(),
        Network {
            online: true,
            wifi: true,
            signal: None
        }
    );
    assert_eq!(s.brightness(), None);
    fs::remove_dir_all(root).unwrap();
}

/// Settings > Battery, Control Center and Privacy & Security read the policies' own status.
#[test]
fn power_and_security_status_come_from_the_host() {
    use wol_core::system::fake::FakeHost;
    let h = Arc::new(FakeHost::linux());
    h.command("ls /sys/class/power_supply", "AC\nBAT0\n")
        .command("cat /sys/class/power_supply/AC/type", "Mains\n")
        .command("cat /sys/class/power_supply/AC/online", "0\n")
        .command("cat /sys/class/power_supply/BAT0/type", "Battery\n")
        .command("cat /sys/class/power_supply/BAT0/capacity", "64\n");
    h.command(
        "systemctl show -p UnitFileState --value nftables.service",
        "enabled\n",
    );
    let sys = CoreSystem::with_ticks(
        Controls::new(h),
        Sysfs::default(),
        None,
        std::sync::mpsc::channel().1,
    );
    let p = sys.power_status().unwrap();
    assert_eq!(p.source, wol_core::power::policy::Source::Battery);
    assert_eq!(p.battery, Some(64));
    assert_eq!(sys.security_status().unwrap().firewall, Some(true));
    assert!(Fake.power_status().is_none(), "previews read nothing");
}

/// Volume keys: the OSD waited for the next poll (up to 10 s). `refresh_audio` reads the
/// sink now, off the UI thread, and replies once the snapshot has it.
#[test]
fn refresh_audio_reads_the_sink_now() {
    use wol_core::system::fake::FakeHost;
    let h = Arc::new(FakeHost::new(false));
    h.command("wpctl get-volume @DEFAULT_AUDIO_SINK@", "Volume: 0.42\n");
    let (_tick, ticks) = std::sync::mpsc::channel();
    let sys = CoreSystem::with_ticks(Controls::new(h), Sysfs::default(), None, ticks);
    assert_eq!(sys.audio().volume, None, "no tick yet");
    assert_eq!(sys.refresh_audio().recv_blocking(), Ok(Ok(())));
    assert_eq!(sys.audio().volume, Some(42));
}

/// audit5 S9-3: the mic-mute key gave no feedback; the default source was never read.
#[test]
fn refresh_audio_reads_the_muted_microphone() {
    use wol_core::system::fake::FakeHost;
    let h = Arc::new(FakeHost::new(false));
    h.command("wpctl get-volume @DEFAULT_AUDIO_SINK@", "Volume: 0.42\n")
        .command(
            "wpctl get-volume @DEFAULT_AUDIO_SOURCE@",
            "Volume: 0.50 [MUTED]\n",
        );
    let (_tick, ticks) = std::sync::mpsc::channel();
    let sys = CoreSystem::with_ticks(Controls::new(h), Sysfs::default(), None, ticks);
    assert_eq!(sys.refresh_audio().recv_blocking(), Ok(Ok(())));
    let a = sys.audio();
    assert_eq!((a.mic, a.mic_muted), (Some(50), true));
    assert_eq!(
        crate::status::mic(a),
        Some("microphone-sensitivity-muted-symbolic")
    );
}

/// Live idle: the refresh worker woke every second (10 wakeups/s in wol-shell, plus helper
/// processes) with nobody looking. Fast refresh only while a watcher (Control Center) is open.
#[test]
fn refreshes_fast_only_while_watched() {
    use wol_core::system::fake::FakeHost;
    let (_tick, ticks) = std::sync::mpsc::channel();
    let sys = CoreSystem::with_ticks(
        Controls::new(Arc::new(FakeHost::new(false))),
        Sysfs::default(),
        None,
        ticks,
    );
    assert_eq!(sys.inner.period(), IDLE_PERIOD);
    let w = sys.watch().unwrap();
    assert_eq!(sys.inner.period(), WATCHED_PERIOD);
    drop(w);
    assert_eq!(sys.inner.period(), IDLE_PERIOD);
    assert!(IDLE_PERIOD >= std::time::Duration::from_secs(10));
    assert!(
        TTLS_IDLE[1] >= 60 && TTLS_IDLE[2] >= 60,
        "scans are the expensive helpers"
    );
}

#[test]
fn core_system_reads_snapshot_and_acts_off_thread() {
    use wol_core::system::fake::FakeHost;
    let h = Arc::new(FakeHost::new(false));
    h.command("wpctl get-volume @DEFAULT_AUDIO_SINK@", "Volume: 0.64\n")
        .command("nmcli radio wifi", "enabled\n")
        .command(
            "nmcli -t -f IN-USE,SIGNAL,SECURITY,SSID dev wifi list --rescan no",
            "*:70:WPA2:Home\n :40::Cafe\n",
        )
        .command("wpctl set-volume @DEFAULT_AUDIO_SINK@ 30%", "");
    let root = std::env::temp_dir().join(format!("wol-core-sys-{}", std::process::id()));
    let cache = root.join("snap.json");
    fs::create_dir_all(&root).unwrap();
    let (tick, ticks) = std::sync::mpsc::channel();
    let sys = CoreSystem::with_ticks(
        Controls::new(h.clone()),
        Sysfs { root: root.clone() },
        Some(cache.clone()),
        ticks,
    );
    let w = sys.watch().unwrap();
    tick.send(()).unwrap();
    w.recv_blocking().unwrap();
    assert_eq!(sys.audio().volume, Some(64));
    assert_eq!(sys.wifi_enabled(), Some(true));
    assert_eq!(wifi_subtitle(sys.network(), &sys.wifi_networks()), "Home");
    assert_eq!(sys.network().signal, Some(70));
    h.command("wpctl get-volume @DEFAULT_AUDIO_SINK@", "Volume: 0.30\n");
    let done = sys.spawn(Action::Volume(30));
    assert_eq!(done.recv_blocking(), Ok(Ok(())));
    assert_eq!(sys.audio().volume, Some(30), "refreshed before the reply");
    assert_eq!(
        sys.spawn(Action::UserAdd {
            name: "dinah".into(),
            full_name: "Dinah".into(),
            admin: false,
            password: "n3w-passw0rd".into(),
        })
        .recv_blocking(),
        Ok(Err(ActionError::Unavailable)),
        "no wol-admind on the test host"
    );
    let cached: Snapshot = serde_json::from_str(&fs::read_to_string(&cache).unwrap()).unwrap();
    assert_eq!(cached.nets.len(), 2);
    // Written through shared::atomic: no temporary is left beside the cache.
    let stray: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("snap.") && n != "snap.json")
        .collect();
    assert!(stray.is_empty(), "{stray:?}");
    drop(sys);
    fs::remove_dir_all(root).unwrap();
}

/// Audit4 S3-5: a volume step read every topic again (nmcli, bluetoothctl, hyprctl,
/// playerctl per slider step) and ate the password the Authenticate sheet stored.
#[test]
fn an_action_reads_only_its_topics_and_keeps_the_password_for_admin() {
    use wol_core::system::fake::FakeHost;
    let h = Arc::new(FakeHost::new(false));
    h.command("wpctl get-volume @DEFAULT_AUDIO_SINK@", "Volume: 0.30\n")
        .command("wpctl set-volume @DEFAULT_AUDIO_SINK@ 30%", "");
    let sys = bare(&h);
    assert_eq!(sys.spawn(Action::Volume(30)).recv_blocking(), Ok(Ok(())));
    let ran = h.ran();
    for helper in ["nmcli", "bluetoothctl", "hyprctl", "playerctl"] {
        assert!(
            !ran.iter().any(|r| r.starts_with(helper)),
            "{helper}: {ran:?}"
        );
    }
    assert!(
        ran.iter().any(|r| r.starts_with("wpctl get-volume")),
        "{ran:?}"
    );
    assert_eq!(sys.audio().volume, Some(30));

    sys.authorize("pw").unwrap();
    let _ = sys.perform(&Action::Volume(40));
    let _ = sys.perform(&Action::MediaNext);
    assert_eq!(
        sys.inner.password.lock().unwrap().as_str(),
        "pw",
        "kept for the admin retry"
    );
    let _ = sys.perform(&Action::Wifi(true));
    assert!(sys.inner.password.lock().unwrap().is_empty(), "used once");
    for a in [Action::Volume(1), Action::Zoom(true), Action::MediaToggle] {
        assert!(!may_need_password(&a), "{a:?}");
    }
    assert!(may_need_password(&Action::PkgUpgrade));
    assert_eq!(
        topics_of(&Action::Volume(1)),
        [true, false, false, false, false]
    );
    assert_eq!(topics_of(&Action::PkgUpgrade), [false; TOPICS]);
}

fn bare(h: &Arc<wol_core::system::fake::FakeHost>) -> CoreSystem {
    let (_tick, ticks) = std::sync::mpsc::channel();
    let root = std::env::temp_dir().join("wol-no-sysfs");
    CoreSystem::with_ticks(Controls::new(h.clone()), Sysfs { root }, None, ticks)
}

/// Control Center-only helpers (playerctl, bluetoothctl, hyprctl getoption) never run while
/// nobody watches; opening a watcher reads them at once instead of after the idle period.
#[test]
fn panel_topics_refresh_only_while_watched() {
    use wol_core::system::fake::FakeHost;
    let h = Arc::new(FakeHost::new(false));
    h.command(
        &format!(
            "playerctl metadata --format {}",
            wol_core::controls::PLAYERCTL_FORMAT
        ),
        "Playing\t61000000\t152000000\tSong\tBand\t\n",
    )
    .command("powerprofilesctl get", "balanced\n");
    let sys = bare(&h);
    sys.inner.refresh(false);
    assert!(
        !h.ran()
            .iter()
            .any(|r| r.starts_with("playerctl") || r.starts_with("powerprofilesctl")),
        "{:?}",
        h.ran()
    );
    assert_eq!(sys.now_playing(), None);
    let w = sys.watch().unwrap();
    w.recv_blocking().unwrap();
    let np = sys.now_playing().unwrap();
    assert_eq!(
        (np.title.as_str(), np.position, np.length),
        ("Song", 61, Some(152))
    );
    assert_eq!(sys.power_mode(), Some(PowerMode::Automatic));
}

/// Battery and the default route come through the host (fakeable), not the sysfs mirror.
#[test]
fn battery_and_route_through_the_host() {
    use wol_core::system::fake::FakeHost;
    let h = Arc::new(FakeHost::linux());
    h.file("/sys/class/power_supply/BAT0/type", "Battery\n")
        .file("/sys/class/power_supply/BAT0/capacity", "47\n")
        .file("/sys/class/power_supply/BAT0/status", "Discharging\n")
        .file("/sys/class/power_supply/BAT0/energy_now", "19000\n")
        .file("/sys/class/power_supply/BAT0/energy_full", "40000\n")
        .file("/sys/class/power_supply/BAT0/power_now", "12000\n")
        .file(
            "/proc/net/route",
            "Iface\tDestination\tGateway\tFlags\nenp0s31f6\t00000000\t0102A8C0\t0003\n",
        );
    let sys = bare(&h);
    assert_eq!(sys.battery(), None);
    sys.inner.refresh(true);
    assert_eq!(
        sys.battery(),
        Some(Battery {
            percent: 47,
            status: BatteryStatus::Discharging
        })
    );
    assert_eq!(sys.battery_time(), Some(95));
    assert_eq!(
        crate::status::network(sys.network()),
        "network-wired-symbolic"
    );
}

#[test]
fn panel_actions_update_the_snapshot() {
    use wol_core::system::fake::FakeHost;
    let h = Arc::new(FakeHost::new(false));
    h.command("hyprctl keyword cursor:zoom_factor 2", "ok")
        .command("hyprctl -j getoption animations:enabled", r#"{"int":1}"#)
        .command(
            "hyprctl -j getoption cursor:zoom_factor",
            r#"{"float":1.0}"#,
        )
        .command("playerctl next", "");
    let sys = bare(&h);
    sys.inner.refresh(true);
    assert_eq!(sys.accessibility().zoom, Some(false));
    assert_eq!(sys.perform(&Action::Zoom(true)), Ok(()));
    assert_eq!(sys.accessibility().zoom, Some(true));
    assert_eq!(sys.perform(&Action::MediaNext), Ok(()));
    assert_eq!(
        sys.perform(&Action::BtConnect("bad".into(), true)),
        Err(ActionError::Invalid),
        "invalid address"
    );
}

/// Manual: `cargo test -p shell --lib live_host -- --ignored --nocapture` prints this machine's snapshot.
#[test]
#[ignore]
fn live_host_snapshot() {
    let sys = CoreSystem::new();
    std::thread::sleep(std::time::Duration::from_secs(3));
    let s = sys.snapshot();
    println!("{s:#?}\nnetwork {:?}", sys.network());
    assert!(s.volume.is_some() || s.wifi_on.is_some());
}
