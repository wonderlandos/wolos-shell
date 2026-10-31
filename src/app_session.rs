// SPDX-License-Identifier: GPL-2.0-or-later
//! What wold asks the desktop to do for an app whose call passed its check (ADR 0020). wold is a
//! system daemon, so anything of the person's session runs here, in the account's own shell. No
//! GTK: a short thread per ask, its reply a `session.answer` to wold.
use serde_json::{Value, json};
use shared::portal::{Ask, Call, MAX_CLIPBOARD, MAX_LINE};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The session, apart from the rules (fake in tests).
pub trait Session {
    /// Text on the clipboard, at most `max` + 1 bytes; None when it holds no text.
    fn clipboard(&self, max: usize) -> Option<Vec<u8>>;
    fn set_clipboard(&self, text: &str) -> bool;
}

/// The running desktop.
pub struct Desktop;

impl Session for Desktop {
    fn clipboard(&self, max: usize) -> Option<Vec<u8>> {
        let mut c = Command::new("wl-paste")
            .args(["--no-newline", "--type", "text"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut out = Vec::new();
        let read = c
            .stdout
            .take()?
            .take(max as u64 + 1)
            .read_to_end(&mut out)
            .is_ok();
        let long = out.len() > max;
        if long {
            let _ = c.kill();
        }
        let done = c.wait().ok()?;
        // Killed for being long is still an answer: `answer` refuses the length.
        (read && (long || done.success())).then_some(out)
    }

    fn set_clipboard(&self, text: &str) -> bool {
        // On stdin: an argument would show in every process listing.
        let Ok(mut c) = Command::new("wl-copy")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return false;
        };
        let wrote = c
            .stdin
            .take()
            .is_some_and(|mut i| i.write_all(text.as_bytes()).is_ok());
        wrote && c.wait().is_ok_and(|s| s.success())
    }
}

/// One ask -> what goes back, or a sentence to show.
pub fn answer(s: &dyn Session, ask: Ask) -> Result<Option<Value>, String> {
    match ask {
        Ask::ClipboardRead => {
            let bytes = s
                .clipboard(MAX_CLIPBOARD)
                .ok_or("The clipboard holds no text.")?;
            if bytes.len() > MAX_CLIPBOARD {
                return Err("The text on the clipboard is too long to hand to an app.".into());
            }
            let text = String::from_utf8(bytes).map_err(|_| "The clipboard holds no text.")?;
            Ok(Some(json!({ "text": text })))
        }
        Ask::ClipboardWrite { text } => {
            if s.set_clipboard(&text) {
                Ok(None)
            } else {
                Err("The clipboard could not be changed.".into())
            }
        }
    }
}

/// The `session.answer` for one ask; never longer than wold reads.
pub fn reply_of(s: &dyn Session, id: u32, ask: Ask) -> Call {
    let (error, data) = match answer(s, ask) {
        Ok(data) => (None, data),
        Err(m) => (Some(m), None),
    };
    let c = Call::SessionAnswer { id, error, data };
    if c.encode().len() < MAX_LINE {
        return c;
    }
    Call::SessionAnswer {
        id,
        error: Some("The answer is too long to hand to an app.".into()),
        data: None,
    }
}

fn reply(wold: &Path, s: &dyn Session, id: u32, ask: Ask) {
    let a = crate::app_notices::call(wold, &reply_of(s, id, ask));
    if !a.ok {
        eprintln!("wol-shell: answer to wold: {:?}", a.message);
    }
}

/// Answer `ask` on its own thread, so a slow one holds up nothing else.
pub fn answer_later(wold: PathBuf, id: u32, ask: Ask) {
    std::thread::spawn(move || reply(&wold, &Desktop, id, ask));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        clip: Option<Vec<u8>>,
        set: RefCell<Vec<String>>,
        refuse: bool,
    }

    impl Session for Fake {
        fn clipboard(&self, _: usize) -> Option<Vec<u8>> {
            self.clip.clone()
        }
        fn set_clipboard(&self, text: &str) -> bool {
            self.set.borrow_mut().push(text.into());
            !self.refuse
        }
    }

    fn holding(clip: Option<&[u8]>) -> Fake {
        Fake {
            clip: clip.map(<[u8]>::to_vec),
            ..Fake::default()
        }
    }

    #[test]
    fn clipboard_reads() {
        assert_eq!(
            answer(&holding(Some(b"hi")), Ask::ClipboardRead),
            Ok(Some(json!({"text": "hi"})))
        );
        assert!(answer(&holding(None), Ask::ClipboardRead).is_err());
        assert!(answer(&holding(Some(&[0xff])), Ask::ClipboardRead).is_err());
        let long = vec![b'x'; MAX_CLIPBOARD + 1];
        assert!(answer(&holding(Some(&long)), Ask::ClipboardRead).is_err());
    }

    #[test]
    fn clipboard_writes() {
        let f = Fake::default();
        let ask = || Ask::ClipboardWrite { text: "hi".into() };
        assert_eq!(answer(&f, ask()), Ok(None));
        assert_eq!(*f.set.borrow(), ["hi"]);
        let no = Fake {
            refuse: true,
            ..Fake::default()
        };
        assert!(answer(&no, ask()).is_err());
    }

    #[test]
    fn an_answer_that_would_not_fit_a_line_says_so() {
        // Newlines double in JSON: legal text, too long once encoded.
        let text = vec![b'\n'; MAX_CLIPBOARD];
        let Call::SessionAnswer { error, data, .. } =
            reply_of(&holding(Some(&text)), 1, Ask::ClipboardRead)
        else {
            panic!()
        };
        assert!(error.is_some() && data.is_none());
        let c = reply_of(&holding(Some(b"short")), 1, Ask::ClipboardRead);
        assert!(c.validate().is_ok() && c.encode().len() < MAX_LINE);
    }
}
