// SPDX-License-Identifier: GPL-2.0-or-later
//! Real input for a toolkit test: a private Xvfb display and XTEST key and button events, so the
//! press and the release of one keystroke or click reach the widgets the way a person's do (the
//! tests that emit a controller's signal by hand deliver a press and never its release: QA-STORE-60
//! was exactly that release). Included with `#[path]`, like `broadway.rs`; none of it is `unsafe`
//! in this crate: the events are sent by a small python3 helper over libX11/libXtst.
#![allow(dead_code)] // not every including test calls every helper
use std::io::Write;

/// The display and the helper that types on it; both are killed on every exit path.
pub struct Input {
    xvfb: std::process::Child,
    helper: std::process::Child,
}

impl Drop for Input {
    fn drop(&mut self) {
        let _ = self.helper.kill();
        let _ = self.helper.wait();
        let _ = self.xvfb.kill();
        let _ = self.xvfb.wait();
    }
}

/// Reads `OP ARGS` lines on stdin: `move X Y`, `key NAME down|up`, `button N down|up`.
const HELPER: &str = r#"
import ctypes, sys
x11 = ctypes.CDLL('libX11.so.6'); xt = ctypes.CDLL('libXtst.so.6')
x11.XOpenDisplay.restype = ctypes.c_void_p
x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
x11.XStringToKeysym.restype = ctypes.c_ulong
x11.XStringToKeysym.argtypes = [ctypes.c_char_p]
x11.XKeysymToKeycode.restype = ctypes.c_ubyte
x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
x11.XFlush.argtypes = [ctypes.c_void_p]
xt.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
xt.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
xt.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
d = x11.XOpenDisplay(None)
if not d:
    sys.exit(3)
print('ready', flush=True)
for line in sys.stdin:
    p = line.split()
    if not p:
        continue
    if p[0] == 'move':
        xt.XTestFakeMotionEvent(d, -1, int(p[1]), int(p[2]), 0)
    elif p[0] == 'key':
        xt.XTestFakeKeyEvent(d, x11.XKeysymToKeycode(d, x11.XStringToKeysym(p[1].encode())), p[2] == 'down', 0)
    elif p[0] == 'button':
        xt.XTestFakeButtonEvent(d, int(p[1]), p[2] == 'down', 0)
    x11.XFlush(d)
"#;

/// Starts Xvfb on a free display in `base..base + 100`, points GTK at it (X11 backend) and starts
/// the helper; None when Xvfb, python3 or libXtst is missing (the test skips). Call it in the
/// test's only `#[test]`, before `gtk::init`.
pub fn start(base: u32) -> Option<Input> {
    let n = (0..100)
        .map(|i| base + (std::process::id() + i) % 100)
        .find(|n| !std::path::Path::new(&format!("/tmp/.X11-unix/X{n}")).exists())?;
    let display = format!(":{n}");
    let mut xvfb = std::process::Command::new("Xvfb")
        .args([
            display.as_str(),
            "-screen",
            "0",
            "1280x800x24",
            "-nolisten",
            "tcp",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    // The socket appears once the server accepts; waited on with the helper's own connect below.
    let helper = std::process::Command::new("python3")
        .args(["-I", "-c", HELPER])
        .env("DISPLAY", &display)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(mut helper) = helper else {
        let _ = xvfb.kill();
        let _ = xvfb.wait();
        return None;
    };
    let ready = helper.stdout.take().and_then(|o| {
        use std::io::BufRead;
        std::io::BufReader::new(o).lines().next()?.ok()
    });
    let input = Input { xvfb, helper };
    if ready.as_deref() != Some("ready") {
        return None;
    }
    // SAFETY: called by the including binary's only test, before gtk::init: no other thread of
    // this process reads the environment yet (Rust 2024 makes set_var unsafe for that reason).
    unsafe {
        std::env::set_var("GDK_BACKEND", "x11");
        std::env::set_var("DISPLAY", &display);
    }
    Some(input)
}

impl Input {
    fn send(&mut self, line: &str) {
        if let Some(i) = self.helper.stdin.as_mut() {
            let _ = writeln!(i, "{line}");
            let _ = i.flush();
        }
    }
    pub fn move_to(&mut self, x: i32, y: i32) {
        self.send(&format!("move {x} {y}"));
    }
    pub fn key_down(&mut self, name: &str) {
        self.send(&format!("key {name} down"));
    }
    pub fn key_up(&mut self, name: &str) {
        self.send(&format!("key {name} up"));
    }
    pub fn button_down(&mut self) {
        self.send("button 1 down");
    }
    pub fn button_up(&mut self) {
        self.send("button 1 up");
    }
}
