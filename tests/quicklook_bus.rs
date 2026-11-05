// SPDX-License-Identifier: GPL-2.0-or-later
//! The previewer contract Files (Nautilus 45+) calls for Space, on a private dbus-daemon:
//! `org.gnome.NautilusPreviewer`, `/org/gnome/NautilusPreviewer`, interface `org.gnome.NautilusPreviewer2`
//! (source: nautilus src/nautilus-previewer.c; checked against the Nautilus 51 binary in the lane image).
//! `ShowFile(ssbs)`, `Close()`, properties `Visible` and `ParentHandle`, signals `SelectionEvent(u)` and
//! `NavigateTo(s)`.
use futures_lite::StreamExt;
use shell::quicklook::{Bus, Event, IFACE, NAME, PATH};
use std::io::BufRead;

struct Daemon(std::process::Child, String);
// Drop: kills the daemon the test started.
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
    std::io::BufReader::new(c.stdout.take()?)
        .read_line(&mut line)
        .ok()?;
    Some(Daemon(c, line.trim().to_string()))
}

/// `f`, or nothing after five seconds: a missing signal fails the test instead of hanging it.
async fn within<T>(f: impl std::future::Future<Output = T>) -> Option<T> {
    futures_lite::future::or(async { Some(f.await) }, async {
        async_io::Timer::after(std::time::Duration::from_secs(5)).await;
        None
    })
    .await
}

/// A proxy that asks the service each time (no property cache), for reading what was just set.
async fn fresh(client: &zbus::Connection) -> zbus::Proxy<'static> {
    zbus::proxy::Builder::<zbus::Proxy>::new(client)
        .destination(NAME)
        .unwrap()
        .path(PATH)
        .unwrap()
        .interface(IFACE)
        .unwrap()
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
        .unwrap()
}

#[test]
fn files_can_show_close_and_read_what_it_reads_on_the_previewer() {
    let Some(d) = daemon() else {
        eprintln!("skipped: no dbus-daemon");
        return;
    };
    async_io::block_on(async {
        let (tx, rx) = async_channel::unbounded();
        let bus = Bus::serve(Some(&d.1), tx).await.expect("serve");
        let client = zbus::connection::Builder::address(d.1.as_str())
            .unwrap()
            .build()
            .await
            .unwrap();
        let p = zbus::Proxy::new(&client, NAME, PATH, IFACE).await.unwrap();

        // The signatures Files sends and reads: a wrong one would be a silent no-op for a person.
        let xml = zbus::fdo::IntrospectableProxy::builder(&client)
            .destination(NAME)
            .unwrap()
            .path(PATH)
            .unwrap()
            .build()
            .await
            .unwrap()
            .introspect()
            .await
            .unwrap();
        assert!(
            xml.contains(&format!("interface name=\"{IFACE}\"")),
            "{xml}"
        );
        let show = xml
            .split("name=\"ShowFile\"")
            .nth(1)
            .expect("ShowFile")
            .split("</method>")
            .next()
            .unwrap();
        assert_eq!(
            show.matches("type=\"s\" direction=\"in\"").count()
                + show.matches("type=\"b\" direction=\"in\"").count(),
            4,
            "{show}"
        );
        assert!(show.contains("type=\"b\" direction=\"in\""), "{show}");

        // Files pings first (this is what starts the previewer through D-Bus activation).
        let peer = zbus::Proxy::new(&client, NAME, PATH, "org.freedesktop.DBus.Peer")
            .await
            .unwrap();
        peer.call_method("Ping", &()).await.unwrap();

        // Nothing is shown yet.
        assert!(
            !fresh(&client)
                .await
                .get_property::<bool>("Visible")
                .await
                .unwrap()
        );

        // ShowFile(uri, windowHandle, closeIfAlreadyShown, activationToken)
        p.call_method(
            "ShowFile",
            &("file:///home/a/x.png", "wayland:abc", true, "tok"),
        )
        .await
        .unwrap();
        assert_eq!(
            within(rx.recv()).await.unwrap().unwrap(),
            Event::Show {
                uri: "file:///home/a/x.png".into(),
                handle: "wayland:abc".into(),
                close_if_shown: true
            }
        );
        assert_eq!(
            fresh(&client)
                .await
                .get_property::<String>("ParentHandle")
                .await
                .unwrap(),
            "wayland:abc"
        );

        // A handle longer than the cap is cut (any session process may call ShowFile).
        let long = "w".repeat(10_000);
        p.call_method("ShowFile", &("file:///x", long.as_str(), false, ""))
            .await
            .unwrap();
        within(rx.recv()).await.unwrap().unwrap();
        assert_eq!(
            fresh(&client)
                .await
                .get_property::<String>("ParentHandle")
                .await
                .unwrap()
                .len(),
            shell::quicklook::HANDLE_LIMIT
        );

        // Visible changes are announced (Files keeps a cached copy of the property).
        let props = zbus::fdo::PropertiesProxy::builder(&client)
            .destination(NAME)
            .unwrap()
            .path(PATH)
            .unwrap()
            .build()
            .await
            .unwrap();
        let mut announced = props.receive_properties_changed().await.unwrap();
        bus.set_visible(true).await.unwrap();
        let mut told = None;
        while told.is_none() {
            let sig = within(announced.next())
                .await
                .flatten()
                .expect("PropertiesChanged");
            let args = sig.args().unwrap();
            assert_eq!(args.interface_name().as_str(), IFACE);
            told = args
                .changed_properties()
                .get("Visible")
                .map(|v| bool::try_from(v).unwrap());
        }
        assert_eq!(told, Some(true));
        assert!(
            fresh(&client)
                .await
                .get_property::<bool>("Visible")
                .await
                .unwrap()
        );

        // Close()
        p.call_method("Close", &()).await.unwrap();
        assert_eq!(within(rx.recv()).await.unwrap().unwrap(), Event::Close);

        // Arrow keys and Return on a folder go back to Files as signals.
        let mut sel = p.receive_signal("SelectionEvent").await.unwrap();
        bus.selection(5).await.unwrap();
        assert_eq!(
            within(sel.next())
                .await
                .flatten()
                .unwrap()
                .body()
                .deserialize::<u32>()
                .unwrap(),
            5
        );
        let mut nav = p.receive_signal("NavigateTo").await.unwrap();
        bus.navigate("file:///home/a").await.unwrap();
        assert_eq!(
            within(nav.next())
                .await
                .flatten()
                .unwrap()
                .body()
                .deserialize::<String>()
                .unwrap(),
            "file:///home/a"
        );

        bus.set_visible(false).await.unwrap();
        assert!(
            !fresh(&client)
                .await
                .get_property::<bool>("Visible")
                .await
                .unwrap()
        );
    });
}

#[test]
fn the_activation_file_starts_the_binary_that_owns_the_name() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../desktop/overlay/usr/share");
    let svc =
        std::fs::read_to_string(root.join("dbus-1/services/org.gnome.NautilusPreviewer.service"))
            .expect("the D-Bus activation file");
    assert!(svc.lines().any(|l| l == format!("Name={NAME}")), "{svc}");
    assert!(
        svc.lines().any(|l| l == "Exec=/usr/bin/wol-quicklook"),
        "{svc}"
    );
    let desktop = std::fs::read_to_string(root.join("applications/org.wolos.quicklook.desktop"))
        .expect("the desktop entry");
    assert!(
        desktop.lines().any(|l| l == "Exec=wol-quicklook"),
        "{desktop}"
    );
    assert!(desktop.lines().any(|l| l == "NoDisplay=true"), "{desktop}");
}
