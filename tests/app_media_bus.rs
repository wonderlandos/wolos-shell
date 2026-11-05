// SPDX-License-Identifier: GPL-2.0-or-later
//! Players libwol apps publish (libwol 1.16), served as MPRIS on a private dbus-daemon: a toy player
//! gets `org.mpris.MediaPlayer2.wol.<program>.instance<pid>`, a client reads what `playerctl` reads, a control
//! goes back to wold as `media.key` only when the app can do it, and the name goes with the player.
use shared::events::Event;
use shared::portal::{CAN_NEXT, CAN_SEEK, Call, MediaKey, MediaState, MediaStatus};
use shell::app_media::Players;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;
use zbus::zvariant::{ObjectPath, OwnedValue};

const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const ROOT: &str = "org.mpris.MediaPlayer2";

struct Daemon(std::process::Child, String);
// The private bus ends with the test.
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn daemon() -> Option<Daemon> {
    let mut c = std::process::Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--print-address=1"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .ok()?;
    let mut line = String::new();
    BufReader::new(c.stdout.take()?).read_line(&mut line).ok()?;
    Some(Daemon(c, line.trim().to_string()))
}

/// A wold that answers every call with ok and hands each one to the test, in order.
fn fake_wold(dir: &std::path::Path) -> (std::path::PathBuf, Receiver<Call>) {
    let sock = dir.join("wold.sock");
    let l = UnixListener::bind(&sock).unwrap();
    let (seen, calls) = channel();
    std::thread::spawn(move || {
        for conn in l.incoming().flatten() {
            if shared::jobs::greet(&conn).is_err() {
                continue;
            }
            let mut line = String::new();
            BufReader::new(&conn).read_line(&mut line).unwrap();
            if let Some(json) = line.trim().strip_prefix("call ")
                && let Ok(c) = Call::decode(json.as_bytes())
            {
                let _ = seen.send(c);
            }
            (&conn).write_all(b"{\"ok\":true}\n").unwrap();
        }
    });
    (sock, calls)
}

const WAIT: Duration = Duration::from_secs(5);

/// The next call wold got.
fn next(calls: &Receiver<Call>) -> Call {
    calls.recv_timeout(WAIT).expect("a call to wold")
}

fn key(player: i32, key: MediaKey, value_us: i64) -> Call {
    Call::MediaKey {
        player,
        key,
        value_us,
    }
}

/// Names that left the bus, as they go.
fn departures(bus: &zbus::blocking::fdo::DBusProxy<'static>) -> Receiver<String> {
    let signals = bus.receive_name_owner_changed().unwrap();
    let (gone, lost) = channel();
    std::thread::spawn(move || {
        for s in signals {
            if let Ok(a) = s.args()
                && a.new_owner().is_none()
            {
                let _ = gone.send(a.name().to_string());
            }
        }
    });
    lost
}

/// Wait for `name` to leave the bus.
fn left(lost: &Receiver<String>, name: &str) {
    while lost.recv_timeout(WAIT).expect("a name to leave") != name {}
}

fn state(status: MediaStatus, art: &str, can: u8) -> MediaState {
    MediaState {
        status,
        title: "Song".into(),
        artist: "Band".into(),
        album: "Record".into(),
        art: art.into(),
        length_us: 215_000_000,
        position_us: 3_000_000,
        can,
    }
}

fn published(pid: i32, app: &str, s: MediaState) -> Event {
    Event::MediaState {
        uid: 1000,
        pid,
        app: app.into(),
        state: s,
    }
}

#[test]
fn a_toy_player_is_seen_and_controlled_through_mpris() {
    let Some(d) = daemon() else {
        eprintln!("skipped: no dbus-daemon");
        return;
    };
    let dir = std::env::temp_dir().join(format!("wol-media-bus-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let art = dir.join("cover.png");
    std::fs::write(&art, b"\x89PNG\r\n\x1a\n\0\0\0\0\0\0").unwrap();
    let art = art.display().to_string();
    let (wold, calls) = fake_wold(&dir);
    let mut players = Players::new(wold, Some(d.1.clone())).with_art_roots(vec![dir.clone()]);

    let client = zbus::blocking::connection::Builder::address(d.1.as_str())
        .unwrap()
        .build()
        .unwrap();
    let bus = zbus::blocking::fdo::DBusProxy::new(&client).unwrap();
    let owned = |n: &str| {
        bus.name_has_owner(zbus::names::BusName::try_from(n).unwrap())
            .unwrap()
    };
    let name = "org.mpris.MediaPlayer2.wol.toy.instance7";
    assert!(!owned(name));

    // An event of another account does nothing; this account's makes the player.
    let foreign = Event::MediaState {
        uid: 1001,
        pid: 7,
        app: "/usr/bin/toy".into(),
        state: state(MediaStatus::Playing, "", 0),
    };
    assert!(players.event(&foreign, 1000));
    assert!(players.is_empty() && !owned(name));
    let first = state(MediaStatus::Playing, &art, CAN_NEXT | CAN_SEEK);
    assert!(players.event(&published(7, "/usr/bin/toy", first), 1000));
    assert_eq!(players.len(), 1);
    assert!(owned(name), "the desktop owns the name for the app");

    // What playerctl reads.
    let root = zbus::blocking::Proxy::new(&client, name, PATH, ROOT).unwrap();
    let p = zbus::blocking::Proxy::new(&client, name, PATH, PLAYER).unwrap();
    assert_eq!(root.get_property::<String>("Identity").unwrap(), "toy");
    assert!(!root.get_property::<bool>("CanQuit").unwrap());
    assert_eq!(
        p.get_property::<String>("PlaybackStatus").unwrap(),
        "Playing"
    );
    let meta: HashMap<String, OwnedValue> = p.get_property("Metadata").unwrap();
    let text = |k: &str| String::try_from(meta[k].try_clone().unwrap()).unwrap();
    assert_eq!(text("xesam:title"), "Song");
    assert_eq!(text("xesam:album"), "Record");
    assert_eq!(
        Vec::<String>::try_from(meta["xesam:artist"].try_clone().unwrap()).unwrap(),
        ["Band"]
    );
    assert_eq!(
        i64::try_from(meta["mpris:length"].try_clone().unwrap()).unwrap(),
        215_000_000
    );
    assert!(text("mpris:artUrl").starts_with("file://"));
    assert!(p.get_property::<bool>("CanGoNext").unwrap());
    assert!(!p.get_property::<bool>("CanGoPrevious").unwrap());
    assert!(p.get_property::<bool>("CanSeek").unwrap());
    assert!(
        p.get_property::<i64>("Position").unwrap() >= 3_000_000,
        "counts while it plays"
    );

    // Controls: only what the app can do reaches wold, with the process that owns the player. The
    // keys go to wold in order, so a call that must not arrive is shown by the next one being
    // the one after it.
    p.call_method("PlayPause", &()).unwrap();
    assert_eq!(next(&calls), key(7, MediaKey::Toggle, 0));
    p.call_method("Next", &()).unwrap();
    assert_eq!(next(&calls), key(7, MediaKey::Next, 0));
    p.call_method("Previous", &()).unwrap();
    p.call_method("Seek", &(5_000_000i64,)).unwrap();
    assert_eq!(
        next(&calls),
        key(7, MediaKey::Seek, 5_000_000),
        "Previous is not sent: the app cannot go back"
    );
    let track = ObjectPath::try_from("/org/wolos/media/track").unwrap();
    let other = ObjectPath::try_from("/org/other/track").unwrap();
    p.call_method("SetPosition", &(&other, 1_000_000i64))
        .unwrap();
    p.call_method("SetPosition", &(&track, -1i64)).unwrap();
    p.call_method("SetPosition", &(&track, 9_000_000i64))
        .unwrap();
    assert_eq!(
        next(&calls),
        key(7, MediaKey::SetPosition, 9_000_000),
        "a track that is not ours and a negative position are not sent"
    );
    p.call_method("OpenUri", &("file:///etc/passwd",)).unwrap();
    p.call_method("Stop", &()).unwrap();
    assert_eq!(
        next(&calls),
        key(7, MediaKey::Stop, 0),
        "OpenUri does nothing"
    );

    // A change is announced, not only readable.
    let props = zbus::blocking::fdo::PropertiesProxy::builder(&client)
        .destination(name)
        .unwrap()
        .path(PATH)
        .unwrap()
        .build()
        .unwrap();
    let mut changes = props.receive_properties_changed().unwrap();
    let paused = state(MediaStatus::Paused, &art, CAN_NEXT);
    players.event(&published(7, "/usr/bin/toy", paused), 1000);
    let change = changes.next().expect("PropertiesChanged");
    let args = change.args().unwrap();
    assert!(args.changed_properties().contains_key("PlaybackStatus"));
    assert!(args.changed_properties().contains_key("CanSeek"));
    // Read from the player itself: a proxy's own property cache catches up a moment later.
    let fresh = |property: &str| {
        let iface = zbus::names::InterfaceName::try_from(PLAYER).unwrap();
        props.get(iface, property).unwrap()
    };
    let status = String::try_from(fresh("PlaybackStatus")).unwrap();
    assert_eq!(status, "Paused");
    assert!(!bool::try_from(fresh("CanSeek")).unwrap());
    p.call_method("Seek", &(1_000_000i64,)).unwrap();
    p.call_method("Pause", &()).unwrap();
    assert_eq!(
        next(&calls),
        key(7, MediaKey::Pause, 0),
        "seeking stopped being allowed"
    );

    // A second process of the same program has a name of its own; each ends alone.
    let lost = departures(&bus);
    let second = "org.mpris.MediaPlayer2.wol.toy.instance8";
    let again = published(8, "/usr/bin/toy", state(MediaStatus::Stopped, "", 0));
    players.event(&again, 1000);
    assert!(owned(second));
    assert_eq!(players.len(), 2);
    // It can do nothing but play and pause: Next is not sent, Play is, to process 8.
    let p8 = zbus::blocking::Proxy::new(&client, second, PATH, PLAYER).unwrap();
    p8.call_method("Next", &()).unwrap();
    p8.call_method("Play", &()).unwrap();
    assert_eq!(next(&calls), key(8, MediaKey::Play, 0));
    assert!(players.event(&Event::MediaGone { uid: 1000, pid: 7 }, 1000));
    left(&lost, name);
    assert!(owned(second));
    players.event(&Event::MediaGone { uid: 1000, pid: 8 }, 1000);
    left(&lost, second);
    assert!(players.is_empty());
    // Nothing else of the wire reached wold.
    assert!(calls.try_recv().is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_bad_state_makes_no_player() {
    let Some(d) = daemon() else {
        eprintln!("skipped: no dbus-daemon");
        return;
    };
    let dir = std::env::temp_dir().join(format!("wol-media-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (wold, _calls) = fake_wold(&dir);
    let mut players = Players::new(wold, Some(d.1.clone()));
    let mut bad = state(MediaStatus::Playing, "", 0);
    bad.title = "two\nlines".into();
    players.event(&published(7, "/usr/bin/toy", bad), 1000);
    assert!(
        players.is_empty(),
        "wold checks too, but the desktop never trusts a line"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
