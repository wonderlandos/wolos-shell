// SPDX-License-Identifier: GPL-2.0-or-later
//! First sign-in Setup (`wol-setup --first-login`): the person's own questions, asked once in
//! their own session before the desktop. The install-time Setup (`crate::setup`) keeps what
//! belongs to the whole computer (disk, the first administrator, network, Secure Boot); this one
//! asks privacy, look, online accounts, keyboard and the helper, then Done.
//!
//! The marker `$XDG_STATE_HOME/wolos/setup-done` ends it for good. "Later" counts in
//! `wolos/setup-later`; after [`MAX_LATER`] it stops asking and System Settings > General keeps a
//! "Finish Setting Up" row instead. `wol-session` writes [`RUNTIME_FLAG`] when the marker is
//! missing; the compositor's start line then runs this before the shell. Every account gets it at
//! its own first sign-in, the first administrator and every account added later alike.
//!
//! Pure: the UI (`ui::first_login`) renders a [`Flow`] and runs its [`Job`]s.
use crate::setup::KEYBOARDS;
use crate::system::ActionError;
use shared::theme::Variant;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Privacy,
    Look,
    Accounts,
    Keyboard,
    Helper,
    Done,
}

/// In the order the lead asked for: privacy, look, accounts, keyboard, the helper, then Done.
pub const PAGES: [Page; 6] = [
    Page::Privacy,
    Page::Look,
    Page::Accounts,
    Page::Keyboard,
    Page::Helper,
    Page::Done,
];

impl Page {
    pub fn title(self) -> &'static str {
        match self {
            Page::Privacy => crate::i18n::tr("Privacy"),
            Page::Look => crate::i18n::tr("Your Look"),
            Page::Accounts => crate::i18n::tr("Online Accounts"),
            Page::Keyboard => crate::i18n::tr("Keyboard & Typing"),
            Page::Helper => crate::i18n::tr("Assistants"),
            Page::Done => crate::i18n::tr("You’re All Set"),
        }
    }

    pub fn subtitle(self) -> &'static str {
        crate::i18n::tr(crate::copy::first_login::subtitle(self))
    }

    /// Stack page name (and golden name part).
    pub fn id(self) -> &'static str {
        match self {
            Page::Privacy => "privacy",
            Page::Look => "look",
            Page::Accounts => "accounts",
            Page::Keyboard => "keyboard",
            Page::Helper => "helper",
            Page::Done => "done",
        }
    }

    fn index(self) -> usize {
        PAGES.iter().position(|p| *p == self).unwrap_or(0)
    }

    pub fn next(self) -> Option<Page> {
        PAGES.get(self.index() + 1).copied()
    }

    pub fn prev(self) -> Option<Page> {
        self.index().checked_sub(1).map(|i| PAGES[i])
    }

    /// "N of M" at the right of the title (HIG: the installer and Setup count their pages).
    pub fn position(self) -> (usize, usize) {
        (self.index() + 1, PAGES.len())
    }
}

/// Done marker, relative to the state home.
pub const MARKER: &str = "wolos/setup-done";
/// "Later" count, relative to the state home.
pub const LATER_FILE: &str = "wolos/setup-later";
/// After this many Laters the setup stops asking; Settings > General keeps the way back.
pub const MAX_LATER: u32 = 3;
/// Written by `wol-session` in `$XDG_RUNTIME_DIR` when [`MARKER`] is missing: the compositor's
/// start line runs `wol-setup --first-login` before the shell.
pub const RUNTIME_FLAG: &str = "wol-first-login";
/// The argument that starts this setup instead of the install-time one.
pub const ARG: &str = "--first-login";

/// The two files of one account's first sign-in.
#[derive(Clone, Debug)]
pub struct Marks {
    state: PathBuf,
}

impl Marks {
    /// Under `state` (`$XDG_STATE_HOME`, else `~/.local/state`).
    pub fn at(state: &Path) -> Marks {
        Marks {
            state: state.to_path_buf(),
        }
    }

    /// This account's.
    pub fn here() -> Marks {
        Marks::at(&wol_core::journal::state_home())
    }

    pub fn marker(&self) -> PathBuf {
        self.state.join(MARKER)
    }

    /// Done once: never again for this account.
    pub fn done(&self) -> bool {
        self.marker().exists()
    }

    /// Laters so far (0 when none or unreadable).
    pub fn laters(&self) -> u32 {
        std::fs::read_to_string(self.state.join(LATER_FILE))
            .ok()
            .and_then(|t| t.trim().parse().ok())
            .unwrap_or(0)
    }

    /// The setup opens at this sign-in: not done, not put off [`MAX_LATER`] times, not live.
    pub fn due(&self, live: bool) -> bool {
        !live && !self.done() && self.laters() < MAX_LATER
    }

    /// Settings > General shows "Finish Setting Up": put off and never finished.
    pub fn unfinished(&self, live: bool) -> bool {
        !live && !self.done() && self.laters() > 0
    }

    /// Later: one more. Returns the new count.
    pub fn later(&self) -> std::io::Result<u32> {
        let n = self.laters().saturating_add(1);
        write_atomic(&self.state.join(LATER_FILE), &format!("{n}\n"))?;
        Ok(n)
    }

    /// Done: the marker (what finished it, when), the Later count gone.
    pub fn finish(&self, now: u64) -> std::io::Result<()> {
        write_atomic(&self.marker(), &format!("done {now}\n"))?;
        let _ = std::fs::remove_file(self.state.join(LATER_FILE));
        Ok(())
    }
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// One choice of every page. `reports` is None when this account can't change error reports
/// (not an administrator, or the computer's choice unread).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choices {
    pub reports: Option<bool>,
    pub location: bool,
    /// Light or Dark; None = Auto.
    pub mode: Option<Variant>,
    /// Accent id (`wol_core::theme::accent::ACCENTS`).
    pub accent: String,
    /// Saved wallpaper choice (`dynamic` or a picture's path); empty = the shipped default.
    pub wallpaper: String,
    /// Index into [`KEYBOARDS`].
    pub keyboard: usize,
    pub helpers: bool,
}

impl Default for Choices {
    fn default() -> Choices {
        Choices {
            reports: None,
            location: false,
            mode: None,
            accent: wol_core::theme::accent::DEFAULT.into(),
            wallpaper: String::new(),
            keyboard: 0,
            helpers: false,
        }
    }
}

impl Choices {
    /// The theme id the look stands for (None = follow light and dark in the default accent).
    pub fn theme_id(&self) -> Option<String> {
        wol_core::theme::accent::theme_id(self.mode, &self.accent)
    }

    pub fn layout(&self) -> &'static str {
        KEYBOARDS[self.keyboard.min(KEYBOARDS.len() - 1)].0
    }

    /// `layouts` (Settings > Keyboard's list) with the chosen one first, the others kept after it.
    pub fn layouts(&self, layouts: &[String]) -> Vec<String> {
        let first = self.layout().to_string();
        std::iter::once(first.clone())
            .chain(layouts.iter().filter(|l| **l != first).cloned())
            .collect()
    }
}

/// Work one Continue saves. Each carries only what changed on its page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Job {
    Privacy {
        reports: Option<bool>,
        location: Option<bool>,
        /// The account's password for error reports (asked after a `NeedsAuth`).
        password: Option<String>,
    },
    Look {
        theme: Option<Option<String>>,
        wallpaper: Option<String>,
    },
    Keyboard(String),
    Helpers(bool),
}

/// What one Continue (Return) does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Next {
    Go(Page),
    /// Stay; say this at the foot.
    Stay(String),
    /// Stay and ask for the account's password (the Privacy page's password row).
    Password(String),
    Run(Job),
    /// Done: the marker, then the desktop.
    Finish,
    /// Later: count it, then the desktop.
    Later,
}

/// Runs [`Job`]s (the session in the UI, a fake in tests).
pub trait Backend {
    fn run(&self, job: &Job) -> Result<(), ActionError>;
}

#[derive(Clone, Debug)]
pub struct Flow {
    pub page: Page,
    /// What the pages show now.
    pub draft: Choices,
    /// What the computer has (read at start, then what each Continue saved).
    saved: Choices,
    /// Error reports wait for the account's password.
    pub asking: bool,
    pub password: String,
}

impl Flow {
    pub fn new(now: Choices) -> Flow {
        Flow {
            page: Page::Privacy,
            draft: now.clone(),
            saved: now,
            asking: false,
            password: String::new(),
        }
    }

    fn changed<T: PartialEq + Clone>(now: &T, was: &T) -> Option<T> {
        (now != was).then(|| now.clone())
    }

    /// The page's save, None when nothing on it changed.
    fn job(&self) -> Option<Job> {
        let (d, s) = (&self.draft, &self.saved);
        let job = match self.page {
            Page::Privacy => {
                let reports = match (d.reports, s.reports) {
                    (Some(a), Some(b)) if a != b => Some(a),
                    _ => None,
                };
                let location = Flow::changed(&d.location, &s.location);
                if reports.is_none() && location.is_none() {
                    return None;
                }
                Job::Privacy {
                    reports,
                    location,
                    password: (self.asking && !self.password.is_empty())
                        .then(|| self.password.clone()),
                }
            }
            Page::Look => {
                let theme = Flow::changed(&d.theme_id(), &s.theme_id());
                let wallpaper = Flow::changed(&d.wallpaper, &s.wallpaper);
                if theme.is_none() && wallpaper.is_none() {
                    return None;
                }
                Job::Look { theme, wallpaper }
            }
            Page::Keyboard if d.keyboard != s.keyboard => Job::Keyboard(d.layout().into()),
            Page::Helper if d.helpers != s.helpers => Job::Helpers(d.helpers),
            _ => return None,
        };
        Some(job)
    }

    fn go(&mut self, p: Page) -> Next {
        self.page = p;
        Next::Go(p)
    }

    /// Continue on the current page: save what changed, then the next page.
    pub fn advance(&mut self) -> Next {
        if self.page == Page::Done {
            return Next::Finish;
        }
        if self.page == Page::Privacy && self.asking && self.password.is_empty() {
            return Next::Password(crate::copy::first_login::REPORTS_PASSWORD.into());
        }
        match self.job() {
            Some(j) => Next::Run(j),
            None => self.forward(),
        }
    }

    fn forward(&mut self) -> Next {
        match self.page.next() {
            Some(p) => self.go(p),
            None => Next::Finish,
        }
    }

    /// Back: None on the first page.
    pub fn back(&mut self) -> Option<Page> {
        let p = self.page.prev()?;
        self.page = p;
        Some(p)
    }

    /// Later, on any page but Done.
    pub fn later(&self) -> Next {
        Next::Later
    }

    /// The result of a [`Job`] from [`Flow::advance`].
    pub fn job_done(&mut self, job: &Job, r: Result<(), ActionError>) -> Next {
        use crate::copy::first_login as c;
        match (job, r) {
            (_, Ok(())) => {
                self.keep(job);
                self.asking = false;
                self.password.clear();
                self.forward()
            }
            (Job::Privacy { password, .. }, Err(ActionError::NeedsAuth | ActionError::Denied)) => {
                let typed = password.is_some();
                // Location was saved before the reports asked (it needs no password).
                if let Job::Privacy {
                    location: Some(l), ..
                } = job
                {
                    self.saved.location = *l;
                }
                self.asking = true;
                self.password.clear();
                Next::Password(
                    if typed {
                        c::REPORTS_WRONG
                    } else {
                        c::REPORTS_PASSWORD
                    }
                    .into(),
                )
            }
            (_, Err(_)) => Next::Stay(c::NOT_SAVED.into()),
        }
    }

    /// What `job` saved is now the computer's.
    fn keep(&mut self, job: &Job) {
        let s = &mut self.saved;
        match job {
            Job::Privacy {
                reports, location, ..
            } => {
                if let Some(r) = reports {
                    s.reports = Some(*r);
                }
                if let Some(l) = location {
                    s.location = *l;
                }
            }
            Job::Look { .. } => {
                s.mode = self.draft.mode;
                s.accent = self.draft.accent.clone();
                s.wallpaper = self.draft.wallpaper.clone();
            }
            Job::Keyboard(_) => s.keyboard = self.draft.keyboard,
            Job::Helpers(on) => s.helpers = *on,
        }
    }

    /// Continue, running any job right away (tests; the UI runs jobs off the main thread).
    pub fn drive(&mut self, b: &dyn Backend) -> Next {
        match self.advance() {
            Next::Run(job) => {
                let r = b.run(&job);
                self.job_done(&job, r)
            }
            n => n,
        }
    }
}

/// `gsettings` argv for the per-user location switch (`org.gnome.system.location enabled`, the
/// key apps and the location portal read).
pub fn location_argv(on: bool) -> Vec<String> {
    [
        "gsettings",
        "set",
        "org.gnome.system.location",
        "enabled",
        if on { "true" } else { "false" },
    ]
    .map(String::from)
    .to_vec()
}

/// `gsettings get org.gnome.system.location enabled` output read as a switch.
pub fn location_of(gsettings_get: &str) -> bool {
    gsettings_get.trim() == "true"
}

/// The KEYBOARDS index of `layout` (`us(intl)` included), 0 when not one of ours.
pub fn keyboard_index(layout: Option<&str>) -> usize {
    layout
        .and_then(|l| KEYBOARDS.iter().position(|k| k.0 == l))
        .unwrap_or(0)
}

/// Online accounts as the list shows them: (provider, who), from GNOME Online Accounts' objects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OnlineAccount {
    pub provider: String,
    pub identity: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wol-first-login-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn pages_in_the_asked_order_with_their_count() {
        assert_eq!(
            PAGES.map(Page::id),
            ["privacy", "look", "accounts", "keyboard", "helper", "done"]
        );
        assert_eq!(Page::Privacy.position(), (1, 6));
        assert_eq!(Page::Done.position(), (6, 6));
        assert_eq!(Page::Privacy.prev(), None);
        assert_eq!(Page::Done.next(), None);
        for p in PAGES {
            let l = crate::copy::lint(p.subtitle());
            assert!(l.is_empty(), "{p:?}: {l:?}");
        }
    }

    /// The marker ends it; three Laters stop it asking; a new account (empty state) is due.
    #[test]
    fn marker_and_later_decide_each_sign_in() {
        let d = tmp("marks");
        let m = Marks::at(&d);
        assert!(m.due(false), "a new account gets the setup");
        assert!(!m.due(true), "never on the live system");
        assert!(!m.unfinished(false), "nothing to finish before a Later");
        assert_eq!(m.later().unwrap(), 1);
        assert!(m.due(false) && m.unfinished(false));
        m.later().unwrap();
        assert!(m.due(false), "asks again after two Laters");
        assert_eq!(m.later().unwrap(), 3);
        assert!(!m.due(false), "the third Later is the last");
        assert!(m.unfinished(false), "Settings keeps the way back");
        m.finish(42).unwrap();
        assert!(m.done() && !m.due(false) && !m.unfinished(false));
        assert_eq!(m.laters(), 0, "Done forgets the Laters");
        assert_eq!(
            std::fs::read_to_string(d.join(MARKER)).unwrap(),
            "done 42\n"
        );
        // Another account has its own state home: due there.
        assert!(Marks::at(&tmp("other")).due(false));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_broken_later_file_counts_as_none() {
        let d = tmp("broken");
        std::fs::create_dir_all(d.join("wolos")).unwrap();
        std::fs::write(d.join(LATER_FILE), "lots\n").unwrap();
        assert_eq!(Marks::at(&d).laters(), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    struct Fake {
        jobs: RefCell<Vec<Job>>,
        answers: RefCell<Vec<Result<(), ActionError>>>,
    }

    impl Fake {
        fn new(answers: &[Result<(), ActionError>]) -> Fake {
            Fake {
                jobs: Default::default(),
                answers: RefCell::new(answers.iter().rev().copied().collect()),
            }
        }
    }

    impl Backend for Fake {
        fn run(&self, job: &Job) -> Result<(), ActionError> {
            self.jobs.borrow_mut().push(job.clone());
            self.answers.borrow_mut().pop().unwrap_or(Ok(()))
        }
    }

    /// Continue through every page without touching a thing: nothing is saved, Done finishes.
    #[test]
    fn keeping_every_default_saves_nothing() {
        let fake = Fake::new(&[]);
        let mut f = Flow::new(Choices::default());
        for p in &PAGES[1..] {
            assert_eq!(f.drive(&fake), Next::Go(*p));
        }
        assert_eq!(f.drive(&fake), Next::Finish);
        assert!(fake.jobs.borrow().is_empty());
        assert_eq!(f.back(), Some(Page::Helper));
    }

    /// Each page saves only what changed on it; error reports ask for the password once.
    #[test]
    fn each_page_saves_its_own_change() {
        let fake = Fake::new(&[
            Err(ActionError::NeedsAuth), // reports without a password
            Err(ActionError::Denied),    // a wrong one
            Ok(()),                      // the right one
            Ok(()),                      // look
            Err(ActionError::Failed),    // keyboard, once
            Ok(()),                      // keyboard
            Ok(()),                      // helpers
        ]);
        let now = Choices {
            reports: Some(false),
            ..Choices::default()
        };
        let mut f = Flow::new(now);
        f.draft.reports = Some(true);
        f.draft.location = true;
        assert_eq!(
            f.drive(&fake),
            Next::Password(crate::copy::first_login::REPORTS_PASSWORD.into())
        );
        assert_eq!(
            f.drive(&fake),
            Next::Password(crate::copy::first_login::REPORTS_PASSWORD.into()),
            "Continue without typing asks again, sends nothing"
        );
        f.password = "wrong".into();
        assert_eq!(
            f.drive(&fake),
            Next::Password(crate::copy::first_login::REPORTS_WRONG.into())
        );
        f.password = "curiouser".into();
        assert_eq!(f.drive(&fake), Next::Go(Page::Look));
        assert!(f.password.is_empty(), "the password is not kept");

        f.draft.mode = Some(Variant::Dark);
        f.draft.accent = "green".into();
        f.draft.wallpaper = "dynamic".into();
        assert_eq!(f.drive(&fake), Next::Go(Page::Accounts));
        assert_eq!(f.drive(&fake), Next::Go(Page::Keyboard));
        f.draft.keyboard = keyboard_index(Some("br"));
        assert_eq!(
            f.drive(&fake),
            Next::Stay(crate::copy::first_login::NOT_SAVED.into())
        );
        assert_eq!(f.page, Page::Keyboard);
        assert_eq!(f.drive(&fake), Next::Go(Page::Helper));
        f.draft.helpers = true;
        assert_eq!(f.drive(&fake), Next::Go(Page::Done));
        // Back and Continue again: saved already, nothing sent twice.
        f.back();
        assert_eq!(f.drive(&fake), Next::Go(Page::Done));
        assert_eq!(f.drive(&fake), Next::Finish);

        let jobs = fake.jobs.borrow();
        assert_eq!(
            jobs[0],
            Job::Privacy {
                reports: Some(true),
                location: Some(true),
                password: None
            }
        );
        assert_eq!(
            jobs[1],
            Job::Privacy {
                reports: Some(true),
                location: None,
                password: Some("wrong".into())
            },
            "location was saved with the first try"
        );
        assert_eq!(
            jobs[3],
            Job::Look {
                theme: Some(Some("wol-dark-green".into())),
                wallpaper: Some("dynamic".into())
            }
        );
        assert_eq!(jobs[4], Job::Keyboard("br".into()));
        assert_eq!(jobs[6], Job::Helpers(true));
        assert_eq!(jobs.len(), 7);
    }

    /// An account that can't change error reports never sends them, whatever the draft says.
    #[test]
    fn reports_are_left_alone_for_other_accounts() {
        let fake = Fake::new(&[]);
        let mut f = Flow::new(Choices::default());
        f.draft.reports = Some(true);
        assert_eq!(f.drive(&fake), Next::Go(Page::Look));
        assert!(fake.jobs.borrow().is_empty());
        assert_eq!(f.later(), Next::Later);
    }

    #[test]
    fn location_and_keyboard_read_back() {
        assert_eq!(
            location_argv(true),
            [
                "gsettings",
                "set",
                "org.gnome.system.location",
                "enabled",
                "true"
            ]
        );
        assert!(location_of("true\n") && !location_of("false\n") && !location_of(""));
        assert_eq!(KEYBOARDS[keyboard_index(Some("us(intl)"))].0, "us(intl)");
        assert_eq!(keyboard_index(Some("xx")), 0);
        let c = Choices {
            keyboard: keyboard_index(Some("br")),
            ..Choices::default()
        };
        assert_eq!(c.layouts(&["us".into(), "br".into()]), ["br", "us"]);
        assert_eq!(
            Choices::default().theme_id(),
            None,
            "Auto in violet: no file"
        );
    }

    #[test]
    fn the_sentences_pass_the_copy_rules() {
        use crate::copy::first_login as c;
        for s in [
            c::REPORTS_HINT,
            c::REPORTS_NOT_YOURS,
            c::LOCATION_HINT,
            c::REPORTS_PASSWORD,
            c::REPORTS_WRONG,
            c::NOT_SAVED,
            c::LOOK_MORE,
            c::ACCOUNTS_NONE,
            c::ACCOUNTS_MAIL,
            c::ACCOUNTS_UNREAD,
            c::IME_READY,
            c::IME_MISSING,
            c::HELPER_HINT,
            c::HELPER_MANUAL,
            c::SETTINGS_ROW,
        ] {
            assert!(
                crate::copy::lint(s).is_empty(),
                "{s}: {:?}",
                crate::copy::lint(s)
            );
            assert!(s.chars().count() <= crate::copy::MAX_LEN, "{s}");
        }
    }
}
