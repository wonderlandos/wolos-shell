//! Auth wire formats. Lock: `/usr/local/libexec/wol-authcheck`, password on stdin, exit 0 ok / 1 denied / 2 error (ADR 0023).
//! Login window: `/var/run/wol-login.sock`, `user\n` + password, write side closed, reply `ok|denied|error` (ADR 0024).
use std::io::{Read, Write};

pub const AUTHCHECK: &str = "/usr/local/libexec/wol-authcheck";
pub const LOGIN_SOCKET: &str = shared::login::SOCKET;
pub const MAX_PASSWORD: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Ok,
    Denied,
    Error,
}

impl Verdict {
    pub fn message(self) -> &'static str {
        match self {
            Verdict::Ok => "",
            Verdict::Denied => "Incorrect password",
            Verdict::Error => "Authentication unavailable",
        }
    }
}

pub fn from_exit(code: Option<i32>) -> Verdict {
    match code {
        Some(0) => Verdict::Ok,
        Some(1) => Verdict::Denied,
        _ => Verdict::Error,
    }
}

pub fn from_reply(reply: &str) -> Verdict {
    match reply.trim() {
        "ok" => Verdict::Ok,
        "denied" => Verdict::Denied,
        _ => Verdict::Error,
    }
}

/// None when the input can never be valid (too long, NUL, newline in user).
pub fn login_request(user: &str, password: &str) -> Option<Vec<u8>> {
    if user.is_empty()
        || user.contains(['\n', '\0'])
        || password.len() > MAX_PASSWORD
        || password.contains('\0')
    {
        return None;
    }
    let mut b = Vec::with_capacity(user.len() + 1 + password.len());
    b.extend_from_slice(user.as_bytes());
    b.push(b'\n');
    b.extend_from_slice(password.as_bytes());
    Some(b)
}

/// Blocking; run off the GTK thread.
pub fn authcheck(password: &str) -> Verdict {
    if password.len() > MAX_PASSWORD || password.contains('\0') {
        return Verdict::Denied;
    }
    let Ok(mut child) = std::process::Command::new(AUTHCHECK)
        .env_clear()
        .stdin(std::process::Stdio::piped())
        .spawn()
    else {
        return Verdict::Error;
    };
    if let Some(mut i) = child.stdin.take() {
        let _ = i.write_all(password.as_bytes());
    }
    from_exit(child.wait().ok().and_then(|s| s.code()))
}

/// Blocking; run off the GTK thread.
pub fn login(socket: &str, user: &str, password: &str) -> Verdict {
    let Some(req) = login_request(user, password) else {
        return Verdict::Denied;
    };
    exchange(socket, &req)
}

/// One request, write side closed, one-line reply.
fn exchange(socket: &str, req: &[u8]) -> Verdict {
    let Ok(mut s) = std::os::unix::net::UnixStream::connect(socket) else {
        return Verdict::Error;
    };
    if s.write_all(req).is_err() || s.shutdown(std::net::Shutdown::Write).is_err() {
        return Verdict::Error;
    }
    let mut r = String::new();
    if s.read_to_string(&mut r).is_err() {
        return Verdict::Error;
    }
    from_reply(&r)
}

/// Avatar initials: "Klee Lima" -> "KL", "root" -> "R".
pub fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect()
}

/// Login users: uid >= 1000, not nobody, shell listed in /etc/shells.
pub fn users(passwd: &str, shells: &str) -> Vec<String> {
    let ok: Vec<&str> = shells
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with('/'))
        .collect();
    passwd
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(':').collect();
            let uid: u32 = f.get(2)?.parse().ok()?;
            (uid >= 1000 && uid != 65534 && ok.contains(f.last()?)).then(|| f[0].to_string())
        })
        .collect()
}

pub use shared::login::Power;

pub fn power_label(p: Power) -> &'static str {
    match p {
        Power::Sleep => "Sleep",
        Power::Restart => "Restart",
        Power::ShutDown => "Shut Down",
    }
}

pub fn power_icon(p: Power) -> &'static str {
    match p {
        Power::Sleep => "weather-clear-night-symbolic",
        Power::Restart => "view-refresh-symbolic",
        Power::ShutDown => "system-shutdown-symbolic",
    }
}

/// Blocking; run off the GTK thread. wol-login (root) runs it (ADR 0003).
pub fn power(socket: &str, p: Power) -> Verdict {
    exchange(socket, &shared::login::encode_power(p))
}

/// Owned-argument form for worker threads.
pub fn authcheck_owned(password: String) -> Verdict {
    authcheck(&password)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdicts() {
        for (c, v) in [
            (Some(0), Verdict::Ok),
            (Some(1), Verdict::Denied),
            (Some(2), Verdict::Error),
            (None, Verdict::Error),
        ] {
            assert_eq!(from_exit(c), v);
        }
        for (r, v) in [
            ("ok\n", Verdict::Ok),
            ("denied", Verdict::Denied),
            ("error", Verdict::Error),
            ("", Verdict::Error),
        ] {
            assert_eq!(from_reply(r), v);
        }
    }

    #[test]
    fn request_format() {
        assert_eq!(login_request("klee", "pw").unwrap(), b"klee\npw");
        assert!(login_request("", "pw").is_none());
        assert!(login_request("a\nb", "pw").is_none());
        assert!(login_request("a", &"x".repeat(513)).is_none());
    }

    #[test]
    fn power_goes_to_wol_login() {
        assert_eq!(
            Power::ALL.map(power_label),
            ["Sleep", "Restart", "Shut Down"]
        );
        let dir = std::env::temp_dir().join(format!("wol-power-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let sock = dir.join("s");
        let l = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        let srv = std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            let mut req = Vec::new();
            c.read_to_end(&mut req).unwrap();
            c.write_all(b"ok\n").unwrap();
            req
        });
        assert_eq!(power(sock.to_str().unwrap(), Power::Restart), Verdict::Ok);
        assert_eq!(srv.join().unwrap(), b"!restart\n");
        assert_eq!(power("/nonexistent/sock", Power::Sleep), Verdict::Error);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn initials_and_users() {
        assert_eq!(initials("Klee Lima"), "KL");
        assert_eq!(initials("root"), "R");
        let pw = "root:*:0:0::0:0:Charlie:/root:/bin/sh\nklee:*:1001:1001::0:0:K:/home/klee:/bin/sh\nnobody:*:65534:65534::0:0::/:/bin/sh\nsvc:*:1002:1:::::/usr/sbin/nologin";
        assert_eq!(users(pw, "# c\n/bin/sh\n/bin/csh\n"), ["klee"]);
    }
}
