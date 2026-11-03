// SPDX-License-Identifier: GPL-2.0-or-later
//! Helper approvals (archicture/modules/automation.md): a helper (a `wol mcp` client, a
//! `WOL_HELPER` script) asks before a change (`approve <json>` on the shell socket). One glass
//! sheet names the helper and says what will change, in plain words. User-level changes: Deny,
//! Always Allow, Allow Once; the helper runs them. Administrator changes: the password, then they
//! run here (wol-admind), never in the helper. One sheet on screen, [`MAX_WAITING`] behind it,
//! more get Busy; each request is denied [`APPROVAL_TIMEOUT_SECS`] after it arrived. The answer
//! goes back on the request's connection and the requester writes the audit record (this side
//! only logs an administrator change it ran when the requester is gone). wold's own sheets (system
//! packages, ADR 0021) arrive as a `SessionAsk` on the shell's wold subscription
//! (`crate::app_session::set_sheet`), never on the socket (audit S1). A same-account requester is
//! named by the executable the kernel reports; the name it gives itself is only a second line
//! (audit S18).
use crate::copy;
use crate::kit::{surface, widgets as w};
use crate::system::{ActionError, System};
use gtk::{gio, glib, prelude::*};
use shared::shell::{APPROVAL_TIMEOUT_SECS, Answer, Approval, ApprovalRequest};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use wol_core::automation::{Outcome, Record, audit, policy};

/// Requests waiting beyond the one on screen; more are refused (a helper cannot flood the screen).
pub const MAX_WAITING: usize = 4;

/// Requests in arrival order; the front one is on screen. `T` is how to answer it.
pub struct Queue<T> {
    items: VecDeque<(u64, Approval, T)>,
    next: u64,
}

impl<T> Default for Queue<T> {
    fn default() -> Self {
        Self {
            items: VecDeque::new(),
            next: 0,
        }
    }
}

impl<T> Queue<T> {
    /// Its id, or the request back when [`MAX_WAITING`] already wait (boxed: an [`Approval`]
    /// is large, and the refusal is the rare path).
    pub fn push(&mut self, a: Approval, t: T) -> Result<u64, Box<(Approval, T)>> {
        if self.items.len() > MAX_WAITING {
            return Err(Box::new((a, t)));
        }
        self.next += 1;
        self.items.push_back((self.next, a, t));
        Ok(self.next)
    }
    pub fn front(&self) -> Option<&Approval> {
        self.items.front().map(|i| &i.1)
    }
    pub fn front_id(&self) -> Option<u64> {
        self.items.front().map(|i| i.0)
    }
    /// How the request on screen is answered.
    pub fn front_item(&self) -> Option<&T> {
        self.items.front().map(|i| &i.2)
    }
    /// Requests behind the one on screen.
    pub fn waiting(&self) -> usize {
        self.items.len().saturating_sub(1)
    }
    pub fn pop_front(&mut self) -> Option<(Approval, T)> {
        self.items.pop_front().map(|i| (i.1, i.2))
    }
    /// Removes request `id`: (was on screen, request, answerer).
    pub fn remove(&mut self, id: u64) -> Option<(bool, Approval, T)> {
        let at = self.items.iter().position(|i| i.0 == id)?;
        self.items.remove(at).map(|i| (at == 0, i.1, i.2))
    }
}

/// Executables that may put a sheet on screen (`approve` on wol-shell.sock): wold only, as root.
pub const APPROVERS: &[&str] = &["/usr/bin/wold", "/usr/lib/wolos/wold"];

/// Who may send `approve`: the peer the kernel reports (uid, executable from /proc/<pid>/exe),
/// never what the message says (audit A23). Root wold may ask anything; the shell's own account
/// (`wol mcp`, `WOL_HELPER` scripts) may ask only user-level consent ([`ApprovalRequest::Change`]);
/// administrator requests from anyone but wold are refused.
pub fn may_approve(peer_uid: u32, own_uid: u32, peer_exe: &str, request: &ApprovalRequest) -> bool {
    let wold = peer_uid == 0 && APPROVERS.contains(&peer_exe);
    wold || (peer_uid == own_uid && !request.is_admin())
}

/// `script:backup` -> `backup`; an executable path (set by wold from the kernel) -> its file name;
/// MCP clients by their own name.
fn display_name(helper: &str) -> &str {
    if helper.starts_with('/') {
        return helper.rsplit('/').next().unwrap_or(helper);
    }
    helper.strip_prefix("script:").unwrap_or(helper)
}

/// Text from a requester or a file name made safe to show (R2C-12): control characters (C0, DEL,
/// C1) and the bidirectional marks, embeddings, overrides and isolates that could reorder a title
/// are dropped.
pub fn plain(s: &str) -> String {
    s.chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(
                    *c,
                    '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
                )
        })
        .collect()
}

/// The sheet's title: who asks, for what. An executable path is shown in full after the name,
/// so a look-alike name cannot hide where the program lives. `exe` is the requester's executable
/// as the kernel reported it (same-account requesters, audit S18): it names the requester, not
/// the `helper` text the requester chose.
pub fn title(a: &Approval, exe: Option<&str>) -> String {
    let path = exe.or_else(|| a.helper.starts_with('/').then_some(a.helper.as_str()));
    let what = plain(&a.request.describe());
    // P6-4: any process of the account can run `/usr/bin/wol` and raise this sheet, so the file
    // name "wol" says nothing; the sheet leads with what would change, package names in quotes.
    if path == Some(shared::portal::HELPER_GATE) {
        return copy::approval_via_wol(&plain(&quoted(&a.request).describe()));
    }
    match path {
        Some(p) => format!(
            "“{}” ({}) wants to {what}.",
            plain(display_name(p)),
            plain(p)
        ),
        None => format!("“{}” wants to {what}.", plain(display_name(&a.helper))),
    }
}

/// `request` with the package names of a package change in quotes (the name is what the person
/// must check), anything else as it is.
fn quoted(request: &ApprovalRequest) -> ApprovalRequest {
    match request {
        ApprovalRequest::Packages { verb, names } => ApprovalRequest::Packages {
            verb: verb.clone(),
            names: names.iter().map(|n| format!("“{}”", plain(n))).collect(),
        },
        other => other.clone(),
    }
}

/// The name a same-account requester gave itself, as a second line under the title; None when
/// the title already names it that way (or the requester is wold, which names the app itself).
pub fn self_named(a: &Approval, exe: Option<&str>) -> Option<String> {
    let exe = exe?;
    let said = display_name(&a.helper);
    (a.helper != exe && said != display_name(exe))
        .then(|| format!("It calls itself “{}”.", plain(said)))
}

/// The line under the title: what the choices mean, and how many more wait.
pub fn detail(a: &Approval, waiting: usize) -> String {
    let mut d = match &a.request {
        // C10-1: a change that leaves something behind is asked every time; no Always Allow.
        ApprovalRequest::Change { .. } if policy::once_only(&a.tool) => {
            policy::ONCE_ONLY_DETAIL.into()
        }
        ApprovalRequest::Change { capability, .. } => match policy::label(capability) {
            Some(l) => {
                let mut c = l.chars();
                let first = c.next().map(|f| f.to_lowercase().collect::<String>());
                format!(
                    "Always Allow lets it {}{} without asking again.",
                    first.unwrap_or_default(),
                    c.as_str()
                )
            }
            None => "Allow Once lets it do this one time.".into(),
        },
        // ADR 0034: the title carries a short SHA-256 prefix; the whole digest of wold's copy is
        // here, so it can be compared with the one the builder published.
        ApprovalRequest::PackageFile { sha256, .. } => {
            crate::copy::approval_package_file(&plain(sha256))
        }
        _ => "Enter your password to allow this.".into(),
    };
    match waiting {
        0 => {}
        1 => d.push_str(" 1 more request is waiting."),
        n => d.push_str(&format!(" {n} more requests are waiting.")),
    }
    d
}

/// Shows (true) or hides the surface holding the approvals.
type ShowFn = Box<dyn Fn(bool)>;

/// How a queued request is answered, and who the kernel says asked (same-account requesters).
pub struct Pending {
    tx: async_channel::Sender<Answer>,
    exe: Option<String>,
}

pub struct Approvals {
    pub root: gtk::Box,
    sheet: w::AuthSheet,
    sys: Rc<dyn System>,
    queue: RefCell<Queue<Pending>>,
    /// The administrator change on screen is running (wol-admind): its result answers it, not the
    /// timeout.
    running: Cell<bool>,
    /// Id of the request the sheet shows: a new arrival behind it never resets the password field.
    shown: Cell<Option<u64>>,
    visible: RefCell<Option<ShowFn>>,
}

impl Approvals {
    pub fn new(sys: Rc<dyn System>) -> Rc<Self> {
        let root = w::glass_panel(0);
        root.add_css_class("approval");
        root.set_width_request(w::CONTROL_CENTER_WIDTH + w::S8);
        let sheet = w::authenticate_sheet("A helper");
        sheet.revealer.set_reveal_child(true);
        root.append(&sheet.revealer);
        let a = Rc::new(Self {
            root,
            sheet,
            sys,
            queue: Default::default(),
            running: Cell::new(false),
            shown: Cell::new(None),
            visible: Default::default(),
        });
        let weak = Rc::downgrade(&a);
        let on = move |f: fn(&Rc<Self>)| {
            let weak = weak.clone();
            move || {
                if let Some(a) = weak.upgrade() {
                    f(&a)
                }
            }
        };
        let deny = on(|a| a.deny());
        a.sheet.cancel.connect_clicked(move |_| deny());
        let allow = on(|a| a.allow());
        a.sheet.allow.connect_clicked(move |_| allow());
        let always = on(|a| a.always());
        a.sheet.always.connect_clicked(move |_| always());
        let submit = on(|a| a.allow());
        a.sheet.field.connect_activate(move |_| submit());
        a
    }

    /// Queues `a`; it shows now when nothing else is on screen. The receiver yields the answer.
    pub fn ask(self: &Rc<Self>, a: Approval) -> async_channel::Receiver<Answer> {
        self.ask_from(a, None)
    }

    /// [`Approvals::ask`] for a requester whose executable the kernel reported as `exe`.
    pub fn ask_from(
        self: &Rc<Self>,
        a: Approval,
        exe: Option<String>,
    ) -> async_channel::Receiver<Answer> {
        let (tx, rx) = async_channel::bounded(1);
        let pushed = self.queue.borrow_mut().push(a, Pending { tx, exe });
        match pushed {
            Err(refused) => {
                let _ = refused.1.tx.try_send(Answer::Busy);
            }
            Ok(id) => {
                let weak = Rc::downgrade(self);
                glib::timeout_add_seconds_local_once(APPROVAL_TIMEOUT_SECS, move || {
                    if let Some(a) = weak.upgrade() {
                        a.expire(id);
                    }
                });
                self.show_front();
            }
        }
        rx
    }

    fn expire(&self, id: u64) {
        if self.running.get() && self.queue.borrow().front_id() == Some(id) {
            return;
        }
        let gone = self.queue.borrow_mut().remove(id);
        if let Some((_, _, p)) = gone {
            let _ = p.tx.try_send(Answer::TimedOut);
            self.show_front();
        }
    }

    /// Fills the sheet for the request on screen (or hides it).
    fn show_front(&self) {
        let q = self.queue.borrow();
        let (Some(a), id) = (q.front().cloned(), q.front_id()) else {
            drop(q);
            self.shown.set(None);
            self.set_visible(false);
            return;
        };
        let waiting = q.waiting();
        let exe = q.front_item().and_then(|p| p.exe.clone());
        drop(q);
        let fresh = self.shown.replace(id) != id;
        let s = &self.sheet;
        let admin = a.request.is_admin();
        s.title.set_label(&title(&a, exe.as_deref()));
        let why = match self_named(&a, exe.as_deref()) {
            Some(said) => format!("{said} {}", detail(&a, waiting)),
            None => detail(&a, waiting),
        };
        s.why.set_label(&why);
        s.icon.set_icon_name(Some(if admin {
            "system-lock-screen-symbolic"
        } else {
            "security-high-symbolic"
        }));
        s.field.set_visible(admin);
        s.always.set_visible(!admin && !policy::once_only(&a.tool));
        s.cancel.set_label("Deny");
        s.allow
            .set_label(if admin { "Allow" } else { "Allow Once" });
        self.set_visible(true);
        if !fresh {
            return;
        }
        self.set_message("");
        s.field.set_text("");
        s.allow.set_sensitive(true);
        // A stray Enter or Space while the sheet takes the keyboard must never say yes.
        if admin {
            s.field.grab_focus();
        } else {
            s.cancel.grab_focus();
        }
    }

    fn set_message(&self, text: &str) {
        self.sheet.message.set_label(text);
        self.sheet.message.set_visible(!text.is_empty());
    }

    fn set_visible(&self, on: bool) {
        if let Some(f) = &*self.visible.borrow() {
            f(on);
        }
    }

    /// Answers the request on screen, then shows the next one.
    fn answer(&self, ans: Answer) {
        self.running.set(false);
        let done = self.queue.borrow_mut().pop_front();
        if let Some((_, p)) = done {
            let _ = p.tx.try_send(ans);
        }
        self.show_front();
    }

    fn front(&self) -> Option<Approval> {
        self.queue.borrow().front().cloned()
    }

    pub fn deny(&self) {
        if self.front().is_some() && !self.running.get() {
            self.answer(Answer::Deny);
        }
    }

    fn always(&self) {
        let Some(a) = self.front() else { return };
        let ApprovalRequest::Change { capability, .. } = &a.request else {
            return;
        };
        // C10-1: nothing is saved for a once-only change; it counts this one time.
        if policy::once_only(&a.tool) {
            return self.answer(Answer::AllowOnce);
        }
        match self.sys.allow_helper_always(&a.helper, capability) {
            Ok(()) => self.answer(Answer::AllowAlways),
            // Not saved: allow this once; it asks again next time.
            Err(e) => {
                eprintln!("wol-shell: Always Allow not saved: {}", e.message());
                self.answer(Answer::AllowOnce)
            }
        }
    }

    fn allow(self: &Rc<Self>) {
        let Some(a) = self.front() else { return };
        if self.running.get() {
            return;
        }
        if !a.request.is_admin() {
            return self.answer(Answer::AllowOnce);
        }
        let pw = self.sheet.field.text().to_string();
        self.sheet.field.set_text("");
        if pw.is_empty() {
            self.set_message("Enter your password.");
            return;
        }
        self.running.set(true);
        self.sheet.allow.set_sensitive(false);
        let update_now = is_update_now(&a.request);
        let rx = self.sys.approve(a.request, pw);
        let me = self.clone();
        glib::spawn_future_local(async move {
            let r = rx.recv().await.unwrap_or(Err(ActionError::Failed));
            me.sheet.allow.set_sensitive(true);
            match r {
                Ok(()) => me.answer(Answer::Done),
                Err(e @ (ActionError::Denied | ActionError::NeedsAuth)) => {
                    me.running.set(false);
                    me.set_message(e.message());
                    me.sheet.field.grab_focus();
                }
                Err(e) => me.answer(Answer::Failed {
                    message: failed_sentence(e, update_now),
                }),
            }
        });
    }
}

/// A sheet for Update Now (every pending package, with or without holds): wol-admind runs it
/// and may say why it failed.
fn is_update_now(r: &ApprovalRequest) -> bool {
    matches!(r, ApprovalRequest::Packages { verb, names }
        if verb == "update-holding" || (verb == "update" && names.is_empty()))
}

/// What the sheet tells the requester when the change failed: for Update Now, wol-admind's own
/// sentence when it gave one (the restore point it kept, or why none could be kept); else the
/// shell's words for `e`.
fn failed_sentence(e: ActionError, update_now: bool) -> String {
    update_now
        .then(|| wol_core::controls::take_failure(shared::admin::AdminAction::PkgUpgrade))
        .flatten()
        .unwrap_or_else(|| e.message().into())
}

/// Audit record the shell writes itself: only an administrator change it ran for a requester
/// that is gone (the requester logs everything else). Never the Wi-Fi password.
fn log(a: &Approval, o: Outcome, detail: &str) {
    let r = Record {
        t: wol_core::journal::now(),
        helper: a.helper.clone(),
        via: "shell".into(),
        tool: a.tool.clone(),
        args: Vec::new(),
        result: o,
        detail: format!("{}: {detail}", a.request.describe()),
    };
    if let Err(e) = audit::append(&wol_core::journal::state_home(), &r) {
        eprintln!("wol-shell: helper audit log: {e}");
    }
}

/// wol-shell: the approval surface (centred, takes the keyboard while it asks) and the socket's
/// `approve` handler: answers on the request's connection, then closes it.
pub fn handler(app: &gtk::Application, sys: Rc<dyn System>) -> super::ipc::Approver {
    let ap = Approvals::new(sys);
    let win = super::window(
        app,
        "panel",
        surface::Spec::panel(
            "wol-panel-approval",
            [false; 4],
            surface::Keyboard::Exclusive,
        ),
    );
    win.set_child(Some(&ap.root));
    win.set_hide_on_close(true);
    {
        let a = ap.clone();
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                a.deny();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        win.add_controller(keys);
    }
    {
        let a = ap.clone();
        win.connect_close_request(move |_| {
            a.deny();
            glib::Propagation::Proceed
        });
    }
    *ap.visible.borrow_mut() = Some(Box::new(move |on| {
        if on {
            win.present();
        } else {
            win.set_visible(false);
        }
    }));
    wold_sheets(&ap);
    Rc::new(
        move |json: String, conn: gio::SocketConnection, exe: Option<String>| {
            let Some(a) = Approval::parse(&json) else {
                eprintln!("wol-shell: bad approval request");
                return;
            };
            // The requester as the kernel reported it at accept (ui::ipc pins it, R2C-12).
            let rx = ap.ask_from(a.clone(), exe);
            glib::spawn_future_local(async move {
                let ans = rx.recv().await.unwrap_or(Answer::Deny);
                let out = conn.output_stream();
                if let Err(e) = out
                    .write_all_future(ans.line().into_bytes(), glib::Priority::DEFAULT)
                    .await
                {
                    eprintln!("wol-shell: approval answer: {}", e.1);
                    if ans == Answer::Done {
                        log(&a, Outcome::Approved, "ran after the helper had gone");
                    }
                }
                let _ = conn.close_future(glib::Priority::DEFAULT).await;
            });
        },
    )
}

/// wold's sheets (audit S1): the wold subscription thread hands each `Ask::Approval` here through
/// `crate::app_session::set_sheet` and blocks until the person answered (or the sheet timed out).
fn wold_sheets(ap: &Rc<Approvals>) {
    type Asked = (Approval, async_channel::Sender<Answer>);
    let (tx, rx) = async_channel::bounded::<Asked>(MAX_WAITING + 1);
    crate::app_session::set_sheet(Box::new(move |a| {
        let (back, answer) = async_channel::bounded(1);
        match tx.try_send((a, back)) {
            Ok(()) => answer.recv_blocking().ok(),
            Err(async_channel::TrySendError::Full(_)) => Some(Answer::Busy),
            Err(async_channel::TrySendError::Closed(_)) => None,
        }
    }));
    let ap = ap.clone();
    glib::spawn_future_local(async move {
        while let Ok((a, back)) = rx.recv().await {
            let answer = ap.ask(a);
            glib::spawn_future_local(async move {
                let ans = answer.recv().await.unwrap_or(Answer::Deny);
                let _ = back.send(ans).await;
            });
        }
    });
}

/// The sheet as it asks for `a` (goldens, previews).
pub fn content(sys: Rc<dyn System>, a: Approval) -> gtk::Widget {
    let ap = Approvals::new(sys);
    let _answer = ap.ask(a);
    // A still picture: no blinking caret.
    ap.sheet.field.set_can_focus(false);
    ap.root.clone().upcast()
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::shell::ApprovalRequest as R;

    /// Update Now's sheet passes wol-admind's sentence on to wold (and so to wopkg and Settings);
    /// other sheets and a failure without a sentence keep the shell's words.
    #[test]
    fn update_now_sheets_pass_on_what_wol_admind_said() {
        use shared::admin::AdminAction::PkgUpgrade;
        let p = |verb: &str, names: &[&str]| R::Packages {
            verb: verb.into(),
            names: names.iter().map(|n| (*n).to_string()).collect(),
        };
        assert!(is_update_now(&p("update", &[])));
        assert!(is_update_now(&p("update-holding", &["gimp"])));
        assert!(!is_update_now(&p("update", &["gimp"])));
        assert!(!is_update_now(&p("install", &["gimp"])));
        wol_core::controls::note_failure(PkgUpgrade, Some("No restore point.".into()));
        assert_eq!(
            failed_sentence(ActionError::Failed, false),
            ActionError::Failed.message()
        );
        assert_eq!(
            failed_sentence(ActionError::Failed, true),
            "No restore point."
        );
        assert_eq!(
            failed_sentence(ActionError::Failed, true),
            ActionError::Failed.message(),
            "taken once"
        );
    }

    fn change(helper: &str) -> Approval {
        Approval {
            helper: helper.into(),
            tool: "appearance set".into(),
            request: R::Change {
                capability: "appearance".into(),
                what: "switch to Dark appearance".into(),
            },
        }
    }

    /// C10-1: a change that leaves something behind (a web app, a session) never offers Always
    /// Allow; the sheet says it asks every time. A plain `open` still does.
    #[test]
    fn once_only_changes_say_they_ask_every_time() {
        let mut a = change("Claude");
        a.tool = "webapp_add".into();
        a.request = R::Change {
            capability: "apps".into(),
            what: "add “Mail” as a web app (https://m.example.com)".into(),
        };
        assert_eq!(detail(&a, 0), policy::ONCE_ONLY_DETAIL);
        a.tool = "open".into();
        assert!(detail(&a, 0).starts_with("Always Allow lets it open apps"));
    }

    /// Audit C7-1: wold's local-file, Undo Update and channel sheets are administrator sheets
    /// only wold may put on screen; a same-account helper cannot.
    #[test]
    fn wolds_root_package_sheets_need_the_password() {
        for r in [
            R::PackageFile {
                staged: "/var/cache/wolos/install/0123456789abcdef0123456789abcdef/x.pkg.tar.zst"
                    .into(),
                name: "hello".into(),
                version: "1:2.0-3".into(),
                sha256: "ab".repeat(32),
            },
            R::SystemRollback,
            R::SystemChannel {
                channel: "beta".into(),
            },
        ] {
            assert!(may_approve(0, 1000, "/usr/bin/wold", &r));
            assert!(!may_approve(1000, 1000, "/usr/bin/wol", &r), "{r:?}");
            let file = matches!(r, R::PackageFile { .. });
            let a = Approval {
                helper: "/usr/bin/wopkg".into(),
                tool: "wopkg packages-update".into(),
                request: r,
            };
            if file {
                // ADR 0034: name, version and a SHA-256 prefix in the title, the whole digest
                // on the detail line, within the sheet's line limit.
                let t = title(&a, None);
                assert!(t.contains("“hello” 1:2.0-3 (SHA-256 abababababab…)"), "{t}");
                assert!(!t.contains("/var/cache"), "{t}");
                let d = detail(&a, 0);
                assert_eq!(
                    d,
                    format!(
                        "SHA-256 of the copy: {}. Enter your password to allow this.",
                        "ab".repeat(32)
                    )
                );
                assert!(d.chars().count() <= crate::copy::MAX_LEN, "{d}");
                assert!(crate::copy::lint(&d).is_empty(), "{d}");
            } else {
                assert_eq!(detail(&a, 0), "Enter your password to allow this.");
            }
        }
    }

    /// One on screen, MAX_WAITING behind it, then Busy; expiry removes the right one.
    #[test]
    fn queue_lines_up_refuses_floods_and_expires() {
        let mut q: Queue<u8> = Queue::default();
        let ids: Vec<u64> = (0..=MAX_WAITING as u8)
            .map(|i| q.push(change("a"), i).unwrap())
            .collect();
        assert_eq!(q.waiting(), MAX_WAITING);
        assert!(q.push(change("flood"), 99).is_err(), "Busy");
        assert_eq!(q.front_id(), Some(ids[0]));
        assert_eq!(q.remove(ids[2]).map(|r| (r.0, r.2)), Some((false, 2)));
        assert_eq!(q.remove(ids[2]), None, "expired once");
        assert_eq!(q.remove(ids[0]).map(|r| (r.0, r.2)), Some((true, 0)));
        assert_eq!(q.front_id(), Some(ids[1]));
        assert_eq!(q.pop_front().map(|r| r.1), Some(1));
        assert_eq!(q.waiting(), MAX_WAITING - 3);
    }

    #[test]
    fn only_wold_as_root_may_ask_and_the_sheet_shows_the_executable() {
        let admin = shared::shell::ApprovalRequest::Packages {
            verb: "install".into(),
            names: vec!["gimp".into()],
        };
        let consent = shared::shell::ApprovalRequest::Change {
            capability: "appearance".into(),
            what: "switch to Dark appearance".into(),
        };
        for r in [&admin, &consent] {
            assert!(
                may_approve(0, 1000, "/usr/bin/wold", r),
                "wold asks anything"
            );
            assert!(
                !may_approve(0, 1000, "/usr/bin/python3", r),
                "root, but not wold"
            );
            assert!(
                !may_approve(1001, 1000, "/usr/bin/wol", r),
                "another account"
            );
        }
        assert!(
            may_approve(1000, 1000, "/usr/bin/wol", &consent),
            "helper consent sheets"
        );
        assert!(
            !may_approve(1000, 1000, "/usr/bin/wold", &admin),
            "admin only from root wold"
        );
        assert!(!may_approve(
            1000,
            1000,
            "/usr/bin/wol",
            &shared::shell::ApprovalRequest::Wifi { on: false }
        ));
        let a = Approval {
            helper: "/usr/bin/wopkg".into(),
            tool: "wopkg packages-install".into(),
            request: shared::shell::ApprovalRequest::Packages {
                verb: "install".into(),
                names: vec!["gimp".into()],
            },
        };
        assert_eq!(
            title(&a, None),
            "“wopkg” (/usr/bin/wopkg) wants to install gimp for every account."
        );
        assert_eq!(detail(&a, 0), "Enter your password to allow this.");
    }

    #[test]
    fn sheet_text_names_the_helper_and_the_change() {
        assert_eq!(
            title(&change("Claude Desktop"), None),
            "“Claude Desktop” wants to switch to Dark appearance."
        );
        assert_eq!(
            title(&change("script:backup"), None),
            "“backup” wants to switch to Dark appearance."
        );
        assert_eq!(
            detail(&change("Claude Desktop"), 0),
            "Always Allow lets it change appearance and wallpaper without asking again."
        );
        let admin = Approval {
            request: R::Wifi { on: false },
            ..change("Claude")
        };
        assert_eq!(title(&admin, None), "“Claude” wants to turn Wi-Fi off.");
        assert_eq!(detail(&admin, 0), "Enter your password to allow this.");
        assert_eq!(
            detail(&admin, 2),
            "Enter your password to allow this. 2 more requests are waiting."
        );
        assert_eq!(
            detail(&admin, 1),
            "Enter your password to allow this. 1 more request is waiting."
        );
    }

    /// Audit S18: a same-account requester cannot pick the name the sheet leads with. The title
    /// names the executable the kernel reported; the chosen name is only a second line.
    #[test]
    fn a_same_account_requester_is_named_by_its_executable() {
        let a = change("Claude Desktop");
        let exe = Some("/home/a/.local/bin/evil");
        assert_eq!(
            title(&a, exe),
            "“evil” (/home/a/.local/bin/evil) wants to switch to Dark appearance."
        );
        assert_eq!(
            self_named(&a, exe).as_deref(),
            Some("It calls itself “Claude Desktop”.")
        );
        assert_eq!(
            self_named(&a, None),
            None,
            "wold's sheets name the app already"
        );
        assert_eq!(
            self_named(&change("/usr/bin/wol"), Some("/usr/bin/wol")),
            None,
            "nothing to add when the names agree"
        );
    }

    /// Audit P6-4: `/usr/bin/wol` is what any process of the account can be, so its sheet does not
    /// name "wol" as the requester: "a program using wol", and the package name in quotes.
    #[test]
    fn a_sheet_raised_through_the_wol_gate_names_the_change_not_wol() {
        let a = Approval {
            helper: shared::portal::HELPER_GATE.into(),
            tool: "wol: packages-install".into(),
            request: R::Packages {
                verb: "install".into(),
                names: vec!["gimp".into(), "ev\u{202E}il".into()],
            },
        };
        let t = title(&a, None);
        assert_eq!(
            t, "A program using wol wants to install “gimp”, “evil” for every account.",
            "{t}"
        );
        assert!(!t.contains("/usr/bin/wol") && !t.contains("“wol”"), "{t}");
        assert_eq!(
            title(&a, Some("/usr/bin/wol")),
            t,
            "the same from the kernel's word"
        );
        let wifi = Approval {
            request: R::Wifi { on: false },
            ..a.clone()
        };
        assert_eq!(
            title(&wifi, None),
            "A program using wol wants to turn Wi-Fi off."
        );
        let other = Approval {
            helper: "/usr/bin/wopkg".into(),
            ..a
        };
        assert!(title(&other, None).starts_with("“wopkg” (/usr/bin/wopkg)"));
    }

    /// R2C-12: a path or a name with bidi overrides or control characters cannot reorder the
    /// title or break it over lines.
    #[test]
    fn the_title_shows_no_bidi_or_control_characters() {
        let a = change("/home/a/\u{202E}gpj.exe");
        let t = title(&a, None);
        assert!(!t.contains('\u{202E}'), "{t}");
        assert!(t.contains("/home/a/gpj.exe"), "{t}");
        let named = change("Claude\u{2066}\n\u{1b}[31m");
        let t = title(&named, Some("/usr/bin/wol\u{200F}"));
        assert!(
            !t.chars().any(|c| c.is_control()
                || ('\u{2066}'..='\u{2069}').contains(&c)
                || c == '\u{200F}'),
            "{t:?}"
        );
        let s = self_named(&named, Some("/usr/bin/wol")).unwrap();
        assert!(!s.chars().any(char::is_control), "{s:?}");
        assert_eq!(plain("a\u{202A}b\u{85}c"), "abc");
    }
}
