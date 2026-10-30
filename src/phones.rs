//! iPhone attach: asks usbmuxd every few seconds which phones are plugged in and posts
//! "iPhone connected" once per plug (the protocol and the diff are `wol_core::iphone`).
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;
use wol_core::iphone::{self, Phone};

const POLL: Duration = Duration::from_secs(3);
const IO_TIMEOUT: Duration = Duration::from_secs(2);

/// One `ListDevices` round trip; None when usbmuxd is not running or does not answer.
fn list(socket: &str) -> Option<Vec<Phone>> {
    let mut s = UnixStream::connect(socket).ok()?;
    s.set_read_timeout(Some(IO_TIMEOUT)).ok()?;
    s.set_write_timeout(Some(IO_TIMEOUT)).ok()?;
    s.write_all(&iphone::list_request(1)).ok()?;
    let mut buf = vec![0u8; 4];
    s.read_exact(&mut buf).ok()?;
    let total = iphone::reply_len(&buf)?;
    buf.resize(total, 0);
    s.read_exact(&mut buf[4..]).ok()?;
    Some(iphone::parse_devices(&buf))
}

/// The arguments of the `notify-send` for a new phone.
pub fn notice_argv() -> Vec<String> {
    ["notify-send", "-a", "WolOS", "-i", "phone-symbolic"]
        .iter()
        .map(|s| s.to_string())
        .chain([iphone::NOTICE_TITLE.into(), iphone::NOTICE_BODY.into()])
        .collect()
}

/// wol-shell: watches usbmuxd off the main thread for the life of the session.
pub fn watch() {
    std::thread::spawn(|| {
        // Phones already plugged in at login are not news.
        let mut known = list(iphone::SOCKET).unwrap_or_default();
        loop {
            std::thread::sleep(POLL);
            let Some(now) = list(iphone::SOCKET) else {
                known.clear();
                continue;
            };
            if !iphone::arrived(&known, &now).is_empty() {
                let argv = notice_argv();
                let _ = std::process::Command::new(&argv[0])
                    .args(&argv[1..])
                    .spawn();
            }
            known = now;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn a_missing_usbmuxd_lists_nothing() {
        assert_eq!(list("/nonexistent/usbmuxd"), None);
    }

    #[test]
    fn lists_what_usbmuxd_answers() {
        let dir = std::env::temp_dir().join(format!("wol-phones-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("usbmuxd");
        let l = UnixListener::bind(&path).unwrap();
        let t = std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            let mut req = [0u8; 512];
            let n = c.read(&mut req).unwrap();
            assert!(String::from_utf8_lossy(&req[..n]).contains("ListDevices"));
            let body = "<plist><dict><key>DeviceList</key><array><dict><key>DeviceID</key><integer>4</integer><key>Properties</key><dict><key>ConnectionType</key><string>USB</string><key>SerialNumber</key><string>ABC-123</string></dict></dict></array></dict></plist>";
            let mut r = ((16 + body.len()) as u32).to_le_bytes().to_vec();
            r.extend_from_slice(&[0; 12]);
            r.extend_from_slice(body.as_bytes());
            c.write_all(&r).unwrap();
        });
        let got = list(path.to_str().unwrap());
        t.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            got,
            Some(vec![Phone {
                serial: "ABC-123".into()
            }])
        );
    }

    #[test]
    fn the_notice_says_to_trust_the_phone() {
        let a = notice_argv();
        assert_eq!(a[0], "notify-send");
        assert_eq!(a[a.len() - 2], "iPhone connected");
        assert!(a[a.len() - 1].contains("Trust on the phone to see photos"));
    }
}
