// SPDX-License-Identifier: GPL-2.0-or-later
//! Install WolOS (`wol-installer`, ADR 0013, ADR 0023): one decision per card, a summary of exactly
//! what will change, then `install-run` as root. The same cards as the Setup Assistant; the steps
//! and every word are `install::flow`. `--boot` is the first screen of the live image: Install or
//! Try, in a session that holds nothing else.
mod disks;
use crate::copy;
use crate::i18n::tr;
use crate::install::flow::{self, Flow, Found, Next, Row, STEPS, SecureOffer, Step};
use crate::install::{self, Check, Report, Stage};
use crate::kit::assistant::{self as a, Art, Card, Choice, ChoiceList, DiskBar, Seg, Tone};
use crate::kit::{self, reveal, widgets as w};
use crate::setup::{Field, Hints};
use gtk::{glib, prelude::*};
use std::cell::{Cell, RefCell};
use std::io::BufRead;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};
use wol_core::boot::{MOK_PASSWORD, key_screen_steps};
use wol_core::install::disk::Disk;
use wol_core::install::plan::Protect;
use wol_core::region::zone_label;

/// Rows the zone search builds at once: the list is for choosing, the search for finding.
const ZONE_ROWS: usize = 60;
/// Lines of the install log the Failed page's Details show.
const LOG_LINES: usize = 40;

/// What the install process reports to the page.
enum Event {
    Report(Report),
    Exit(bool),
}

/// Takes (true) or gives back (false) what keeps the machine awake.
type KeepAwake = Box<dyn Fn(bool)>;

/// The edit page's controls.
struct EditUi {
    usage: gtk::DropDown,
    folder_row: gtk::Box,
    folder: gtk::Entry,
    erase_row: gtk::Box,
    erase: gtk::Switch,
    fs_row: gtk::Box,
    fs: gtk::DropDown,
    size_row: gtk::Box,
    size: gtk::Scale,
    size_text: gtk::Label,
    delete: gtk::Button,
}

/// The account page's editors, by field.
type Editors = Vec<(Field, gtk::Widget)>;

pub struct Installer {
    pub root: gtk::Widget,
    pager: gtk::Stack,
    /// Called (after the click) when the person picks a language other than the one shown:
    /// `run` builds the installer again in it (L10N-2). None: goldens and tests.
    relang: RefCell<Option<Rc<dyn Fn()>>>,
    flow: RefCell<Flow>,
    cards: RefCell<Vec<(Step, Card)>>,
    zones: Vec<String>,
    zone_search: RefCell<Option<gtk::SearchEntry>>,
    zone_list: gtk::Box,
    disk_list: gtk::Box,
    method_list: gtk::Box,
    method_warn: gtk::Box,
    method_warning: gtk::Label,
    /// The swap choice under the method list (not for Manual).
    swap_row: gtk::Box,
    swap: RefCell<Option<gtk::DropDown>>,
    replace_list: gtk::Box,
    along_bar: DiskBar,
    along_warn: gtk::Box,
    along_warning: gtk::Label,
    along_box: gtk::Box,
    along_slider: RefCell<Option<gtk::Scale>>,
    manual_bar: DiskBar,
    manual_list: gtk::Box,
    edit: RefCell<Option<EditUi>>,
    /// The edit page is being filled in: its controls' signals say nothing.
    filling: Cell<bool>,
    protect_rows: RefCell<Vec<Protect>>,
    protect_list: RefCell<Option<ChoiceList>>,
    disk_pw_rows: gtk::Box,
    editors: RefCell<Editors>,
    summary_list: gtk::Box,
    summary_warning: gtk::Box,
    summary_text: gtk::Label,
    progress: a::Progress,
    /// A tip under the bar, changing every [`install::TIP_SECS`].
    tip: gtk::Label,
    recovery: gtk::Box,
    recovery_key: gtk::Label,
    /// "I wrote the key down": Restart waits for it.
    ack: RefCell<Option<gtk::Switch>>,
    /// "Restart Later" (live desktop only): waits for the same toggle as Restart (audit6 I6-5).
    later: RefCell<Option<gtk::Button>>,
    /// What the startup setup had to say (one or two lines).
    done_notes: gtk::Box,
    /// The Secure Boot sheet's BitLocker lines (R2B-9), filled when the sheet shows.
    secure_boot_notes: gtk::Box,
    /// The firmware's key screen comes after the restart: the steps, and the password in large type.
    key_screen: gtk::Box,
    /// The Secure Boot page: its switch (a choice only), the key screen pictured before it is
    /// chosen, and the own-keys notes (the install medium, BitLocker).
    secure_row: gtk::Box,
    secure_screen: gtk::Box,
    secure_notes: gtk::Box,
    /// The Modules page's rows and the download size under them.
    modules_list: gtk::Box,
    modules_size: gtk::Label,
    /// The Extras page's switch.
    codecs_switch: RefCell<Option<gtk::Switch>>,
    /// The Failed page's Details: the tool's own error line, then the install log's tail.
    failed_details: gtk::Box,
    failed_tool: gtk::Label,
    failed_log: gtk::Box,
    /// A refused plan's sentence, full width over the buttons (the foot's status is too narrow).
    failed_refusal: gtk::Label,
    /// The key-unread page's Details: the log and why the key was not read.
    unread_details: gtk::Box,
    unread_log: gtk::Box,
    /// Last report and when its stage began; first package sample for the time estimate.
    report: RefCell<(Report, Instant)>,
    sample: Cell<Option<(f64, Instant)>>,
    /// When the install began (the tips' clock).
    began: Cell<Option<Instant>>,
    /// Disks were looked at. Golden renders hand theirs in.
    probed: Cell<bool>,
    /// How disks are looked at, off the main thread ([`system_disks`]; tests hand in their own).
    probe: Cell<fn() -> Vec<Found>>,
    /// Whether a default route is up now, off the main thread ([`flow::online_now`]). Given
    /// disks (previews, goldens) mean a fixed world, which is online.
    online: Cell<fn() -> bool>,
    /// With the security chip the account password also opens the disk: said on the Protect
    /// and Account pages while the chip is chosen.
    protect_tpm_note: gtk::Label,
    account_tpm_note: gtk::Label,
    /// Idle sleep and screen-off held off while installing (`run` sets how).
    awake: RefCell<install::Awake>,
    keep_awake: RefCell<Option<KeepAwake>>,
}

fn replace_child(holder: &gtk::Box, child: &impl IsA<gtk::Widget>) {
    while let Some(c) = holder.first_child() {
        holder.remove(&c);
    }
    holder.append(child);
}

/// `notes` as wrapped lines in `b`, hidden when there are none (Done, Secure Boot sheet).
fn fill_notes(b: &gtk::Box, notes: &[impl AsRef<str>]) {
    replace_child(b, &a::blank());
    for n in notes {
        b.append(&a::wrapped(n.as_ref(), "assistant-subtitle"));
    }
    b.set_visible(!notes.is_empty());
}

fn holder() -> gtk::Box {
    let b = w::vbox(0);
    b.set_vexpand(true);
    b
}

/// "N of 7" on the main path: language, time zone, disk (and how), account, extras, summary, installing.
/// The pages before and after (the welcome, the end) carry no count.
fn position(step: Step) -> Option<(usize, usize)> {
    let at = match step {
        Step::Language => 1,
        Step::Zone => 2,
        Step::Disk
        | Step::Method
        | Step::Replace
        | Step::Alongside
        | Step::Manual
        | Step::Edit
        | Step::Protect
        | Step::SecureBootChoice => 3,
        Step::Account => 4,
        Step::Modules | Step::Extras => 5,
        Step::Summary | Step::SecureBoot => 6,
        Step::Installing => 7,
        Step::Begin | Step::Done | Step::Failed | Step::KeyUnread => return None,
    };
    Some((at, 7))
}

impl Installer {
    pub fn new(
        hints: &Hints,
        boot: bool,
        tpm: bool,
        zones: Vec<String>,
        disks: Option<Vec<Found>>,
    ) -> Rc<Installer> {
        a::set_header_icon("wolos-installer");
        let pager = a::pager();
        let (along_warn, along_warning) = a::warning("");
        let (method_warn, method_warning) = a::warning("");
        let (summary_warning, summary_text) = a::warning("");
        let method_list = holder();
        let note = |text: &'static str| a::wrapped(tr(text), "assistant-subtitle");
        let along_box = w::vbox(w::S4);
        let recovery = w::vbox(w::S2);
        let recovery_key = w::centered("", "assistant-key");
        recovery_key.set_selectable(true);
        recovery_key.set_focusable(false);
        recovery.append(&note(copy::INSTALL_RECOVERY_NOTE));
        recovery.append(&recovery_key);
        recovery.set_visible(false);
        let done_notes = w::vbox(w::S1);
        let secure_boot_notes = w::vbox(w::S1);
        secure_boot_notes.set_visible(false);
        let key_screen = w::vbox(w::S2);
        key_screen.set_visible(false);
        let secure_notes = w::vbox(w::S1);
        secure_notes.set_visible(false);
        let done_body = w::vbox(w::S4);
        done_body.append(&done_notes);
        done_body.append(&key_screen);
        done_body.append(&recovery);
        let failed_tool = a::wrapped("", "assistant-subtitle");
        let failed_log = w::vbox(0);
        let failed_details = w::vbox(0);
        failed_details.append(&failed_tool);
        failed_details.append(&failed_log);
        failed_details.set_visible(false);
        let failed_refusal = a::wrapped("", "assistant-status");
        failed_refusal.add_css_class("problem");
        failed_refusal.set_visible(false);
        let unread_log = w::vbox(0);
        let unread_details = w::vbox(0);
        unread_details.append(&unread_log);
        unread_details.set_visible(false);
        let mut flow = Flow::new(hints, boot, tpm);
        let probed = disks.is_some();
        if let Some(d) = disks {
            flow.set_disks(d);
        }
        let s = Rc::new(Installer {
            root: super::lock::assistant_screen(&pager),
            pager,
            flow: RefCell::new(flow),
            cards: Default::default(),
            zones,
            zone_search: Default::default(),
            zone_list: holder(),
            disk_list: holder(),
            method_list,
            method_warn,
            method_warning,
            swap_row: w::vbox(w::S2),
            swap: Default::default(),
            replace_list: holder(),
            along_bar: DiskBar::new(),
            along_warn,
            along_warning,
            along_box,
            along_slider: Default::default(),
            manual_bar: DiskBar::new(),
            manual_list: holder(),
            edit: Default::default(),
            filling: Cell::new(false),
            protect_rows: Default::default(),
            protect_list: Default::default(),
            disk_pw_rows: w::vbox(w::S2),
            editors: Default::default(),
            summary_list: holder(),
            summary_warning,
            summary_text,
            progress: a::Progress::new(),
            tip: a::wrapped("", "assistant-subtitle"),
            recovery,
            recovery_key,
            ack: Default::default(),
            later: Default::default(),
            done_notes,
            secure_boot_notes,
            key_screen,
            secure_row: w::vbox(0),
            secure_screen: w::vbox(w::S2),
            secure_notes,
            modules_list: holder(),
            modules_size: a::wrapped("", "assistant-subtitle"),
            codecs_switch: Default::default(),
            failed_details,
            failed_tool,
            failed_log,
            failed_refusal,
            unread_details,
            unread_log,
            report: RefCell::new((Report::new(Stage::Prepare, 0), Instant::now())),
            sample: Cell::new(None),
            began: Cell::new(None),
            probed: Cell::new(probed),
            probe: Cell::new(system_disks),
            online: Cell::new(if probed {
                (|| true) as fn() -> bool
            } else {
                flow::online_now
            }),
            protect_tpm_note: note(copy::INSTALL_TPM_FALLBACK),
            account_tpm_note: note(copy::INSTALL_TPM_FALLBACK),
            awake: RefCell::new(install::Awake::default()),
            keep_awake: RefCell::new(None),
            relang: RefCell::new(None),
        });
        for st in STEPS {
            let card = match st {
                Step::Begin => s.begin(),
                Step::Language => s.language(),
                Step::Zone => s.zone(),
                Step::Disk => s.simple(st, &s.disk_list, "drive-harddisk-symbolic"),
                Step::Method => s.method(),
                Step::Replace => s.replace(),
                Step::Alongside => s.alongside(),
                Step::Manual => s.manual(),
                Step::Edit => s.edit_page(),
                Step::Protect => s.protect(),
                Step::SecureBootChoice => s.secure_choice(),
                Step::Account => s.account(),
                Step::Modules => s.modules(),
                Step::Extras => s.extras(),
                Step::Summary => s.summary(),
                Step::SecureBoot => s.secure_boot(),
                Step::Installing => s.installing(),
                Step::Done => s.done(&done_body),
                Step::KeyUnread => s.key_unread(),
                _ => s.failed(),
            };
            s.wire(st, &card);
            s.pager.add_named(&card.root, Some(st.id()));
            s.cards.borrow_mut().push((st, card));
        }
        let first = s.flow.borrow().step;
        s.jump(first);
        s
    }

    /// Changes the flow directly (goldens, tests).
    pub fn with_flow(&self, f: impl FnOnce(&mut Flow)) {
        f(&mut self.flow.borrow_mut());
    }

    /// Looks at disks with `f` instead of the system's tools (goldens, tests).
    pub fn set_probe(&self, f: fn() -> Vec<Found>) {
        self.probe.set(f);
    }

    /// Looks for a network with `f` instead of the route tables (goldens, tests).
    pub fn set_online_probe(&self, f: fn() -> bool) {
        self.online.set(f);
    }

    /// Shows `step` at once (goldens, tests).
    pub fn jump(self: &Rc<Self>, step: Step) {
        self.pager
            .set_transition_type(gtk::StackTransitionType::None);
        self.flow.borrow_mut().step = step;
        self.show(step);
        self.pager
            .set_transition_type(gtk::StackTransitionType::Crossfade);
    }

    /// Types into an account field as the user would (goldens, tests).
    pub fn type_into(&self, f: Field, text: &str) {
        if let Some(e) = self
            .editors
            .borrow()
            .iter()
            .find(|(g, _)| *g == f)
            .and_then(|(_, e)| e.clone().dynamic_cast::<gtk::Editable>().ok())
        {
            e.set_text(text);
        }
    }

    /// Restart and Restart Later move together: the recovery key is deleted once shown, so
    /// neither may leave the page before the toggle is on (audit6 I6-5).
    fn set_leave(&self, on: bool) {
        self.with_card(Step::Done, |c| c.next.set_sensitive(on));
        if let Some(b) = &*self.later.borrow() {
            b.set_sensitive(on);
        }
    }

    fn with_card(&self, step: Step, f: impl FnOnce(&Card)) {
        if let Some((_, c)) = self.cards.borrow().iter().find(|(s, _)| *s == step) {
            f(c)
        }
    }

    fn wire(self: &Rc<Self>, step: Step, card: &Card) {
        let me = Rc::downgrade(self);
        card.next.connect_clicked(move |_| {
            if let Some(s) = me.upgrade() {
                s.next()
            }
        });
        let me = Rc::downgrade(self);
        card.back.connect_clicked(move |_| {
            if let Some(s) = me.upgrade() {
                s.back()
            }
        });
        card.back.set_visible(!matches!(
            step,
            Step::Begin | Step::Done | Step::Failed | Step::Installing | Step::KeyUnread
        ));
        card.set_step(position(step));
    }

    /// Refreshes `step`'s page and shows it.
    fn show(self: &Rc<Self>, step: Step) {
        for (_, c) in self.cards.borrow().iter() {
            c.say("", false);
        }
        // Every page's paragraph: its line, what it means, what comes next (the Failed page
        // says why it stopped instead).
        if step != Step::Failed {
            let intro = self.flow.borrow().intro(step);
            self.with_card(step, |c| c.subtitle.set_label(&intro));
        }
        match step {
            Step::Language => {
                let cancel = !self.flow.borrow().boot;
                self.with_card(step, |c| {
                    c.back
                        .set_label(if cancel { tr("Cancel") } else { tr("Back") })
                });
            }
            Step::Zone => self.fill_zones(""),
            Step::Disk => self.enter_disks(),
            Step::Method => self.fill_method(),
            Step::Replace => self.fill_replace(),
            Step::Alongside => self.fill_alongside(),
            Step::Manual => self.fill_manual(),
            Step::Edit => self.fill_edit(),
            Step::Protect => self.fill_protect(),
            Step::Account => {
                let tpm = self.flow.borrow().protect == Protect::Tpm;
                self.account_tpm_note.set_visible(tpm);
                if let Some((_, e)) = self.editors.borrow().first() {
                    e.grab_focus();
                }
            }
            Step::Summary => {
                self.fill_summary();
                self.look_online();
            }
            Step::SecureBootChoice => self.fill_secure_choice(),
            Step::Modules => self.fill_modules(),
            Step::Extras => {
                let on = self.flow.borrow().codecs;
                if let Some(sw) = &*self.codecs_switch.borrow() {
                    sw.set_active(on);
                }
            }
            Step::SecureBoot => {
                let notes = self.flow.borrow().secure_boot_notes();
                fill_notes(&self.secure_boot_notes, &notes);
            }
            _ => {}
        }
        self.pager.set_visible_child_name(step.id());
        let change = self.awake.borrow_mut().set(step == Step::Installing);
        if let (Some(on), Some(f)) = (change, &*self.keep_awake.borrow()) {
            f(on);
        }
    }

    /// How to keep the machine awake while installing (the app window's inhibitors).
    pub fn set_keep_awake(&self, f: impl Fn(bool) + 'static) {
        *self.keep_awake.borrow_mut() = Some(Box::new(f));
    }

    /// Progress page at `r`, `left` seconds remaining (goldens).
    pub fn show_progress(self: &Rc<Self>, r: &Report, left: Option<f64>) {
        self.flow.borrow_mut().jump(Step::Installing);
        self.show(Step::Installing);
        self.progress.set(
            install::fraction(r, 0.0),
            &format!(
                "{} {}",
                install::doing(r.stage),
                install::remaining_text(left)
            ),
        );
    }

    /// Failed page, stopped during `at`: a plain sentence for the stage, one line pointing to
    /// Details, where the tool's own words (`message`) come first. The log behind Details follows
    /// when the run has ended ([`Self::set_failed_log`]).
    pub fn show_failed(self: &Rc<Self>, at: Stage, message: &str) {
        self.show_failed_with(at, message, "");
    }

    /// The Failed page's Details: the tail of `log`.
    fn set_failed_log(&self, log: &str) {
        let tail = install::log_tail(log, LOG_LINES);
        let lines: Vec<String> = tail.lines().map(String::from).collect();
        replace_child(&self.failed_log, &a::lines(&lines));
        // The log comes after the page: a full disk said only there still names the cause.
        if install::out_of_space(log) && !self.failed_refusal.property::<bool>("visible") {
            self.with_card(Step::Failed, |c| {
                c.subtitle.set_label(copy::installer::DISK_FULL_STOPPED);
            });
        }
    }

    /// [`Self::show_failed`] with the log given (goldens).
    pub fn show_failed_with(self: &Rc<Self>, at: Stage, message: &str, log: &str) {
        self.set_failed_log(log);
        let tool = message.trim();
        self.failed_tool.set_label(tool);
        self.failed_tool.set_visible(!tool.is_empty());
        self.failed_details.set_visible(false);
        self.flow.borrow_mut().jump(Step::Failed);
        self.show(Step::Failed);
        // A plan `wol install check` refused: its sentence; the tool's words stay in Details.
        let refused = install::refused_word(message).and_then(|(word, part)| {
            let f = self.flow.borrow();
            let d = f.disk().cloned().unwrap_or_default();
            copy::install_check_refusal(word, part, &d)
        });
        // Refused: nothing was written, and the subtitle says so instead of a stage's failure.
        self.failed_refusal
            .set_label(refused.as_deref().unwrap_or_default());
        self.failed_refusal.set_visible(refused.is_some());
        self.with_card(Step::Failed, |c| {
            c.subtitle
                .set_label(crate::i18n::tr(install::failed_subtitle(
                    at,
                    refused.is_some(),
                    message,
                    log,
                )));
            // The tool's words are behind Details; the foot keeps one plain sentence.
            c.say(
                if refused.is_some() || tool.is_empty() {
                    ""
                } else {
                    crate::i18n::tr(copy::installer::FAILED_SEE_DETAILS)
                },
                true,
            );
        });
    }

    /// The Failed page's foot sentence and the tool's own line in Details (the golden case checks
    /// that the tool's words are not on the foot).
    pub fn failed_texts(&self) -> (String, String) {
        // The label's own `visible` flag: `is_visible` also asks the parents, and Details is closed.
        let own = |w: &gtk::Label| {
            if w.property::<bool>("visible") {
                w.label().to_string()
            } else {
                String::new()
            }
        };
        let mut foot = String::new();
        self.with_card(Step::Failed, |c| foot = own(&c.status));
        (foot, own(&self.failed_tool))
    }

    /// Shows or hides the Failed page's log (goldens; the Details button does the same).
    pub fn toggle_details(&self) {
        self.failed_details
            .set_visible(!self.failed_details.is_visible());
        self.unread_details
            .set_visible(!self.unread_details.is_visible());
    }

    /// The install worked and encrypted the disk, but no recovery key came back: no Restart, and
    /// the reason is logged and put under Details with the install log.
    pub fn show_key_unread(self: &Rc<Self>, reason: &str, log: &str) {
        eprintln!("wol-installer: recovery key not read: {reason}");
        let tail = install::log_tail(&format!("{log}\n{reason}"), LOG_LINES);
        let lines: Vec<String> = tail.lines().map(String::from).collect();
        replace_child(&self.unread_log, &a::lines(&lines));
        self.unread_details.set_visible(false);
        self.with_card(Step::KeyUnread, |c| c.set_busy(false));
        self.flow.borrow_mut().jump(Step::KeyUnread);
        self.show(Step::KeyUnread);
    }

    /// Done page; a recovery key shows here once (Restart waits until it is acknowledged), what
    /// the startup setup has to say, and the firmware's key screen steps when it comes.
    pub fn show_done(self: &Rc<Self>, key: Option<&str>, note: Option<&str>, key_screen: bool) {
        let mut notes = install::done_notes(note);
        {
            let f = self.flow.borrow();
            notes.extend(f.windows_start());
            notes.extend(f.bitlocker_notes(true).into_iter().map(String::from));
            notes.extend(f.deferred_line().map(String::from));
        }
        fill_notes(&self.done_notes, &notes);
        // Repeated here only when it was chosen (or the computer's own Secure Boot made it so).
        let chosen = self.flow.borrow().key_screen_chosen();
        self.key_screen.set_visible(key_screen && chosen);
        self.recovery.set_visible(key.is_some());
        self.recovery_key.set_label(key.unwrap_or_default());
        if let Some(ack) = &*self.ack.borrow() {
            ack.set_active(false);
        }
        self.set_leave(install::done_may_leave(key.is_some(), false));
        let wide = key_screen && chosen;
        self.with_card(Step::Done, |c| {
            c.widen(if wide { a::CARD_WIDE } else { a::CARD.0 })
        });
        self.flow.borrow_mut().jump(Step::Done);
        self.show(Step::Done);
    }

    /// The Done page's leaving actions: (Restart, Restart Later). None: no Restart Later on this
    /// page (the installed machine's first start, `boot`). The golden case checks them.
    pub fn leave_buttons(&self) -> (bool, Option<bool>) {
        let mut next = false;
        self.with_card(Step::Done, |c| next = c.next.is_sensitive());
        (next, self.later.borrow().as_ref().map(|b| b.is_sensitive()))
    }

    /// "I wrote the key down", flipped as the person would.
    pub fn set_acknowledged(&self, on: bool) {
        if let Some(ack) = &*self.ack.borrow() {
            ack.set_active(on);
        }
    }

    pub fn is_installing(&self) -> bool {
        self.flow.borrow().step == Step::Installing
    }

    /// Continue (button or Return).
    pub fn next(self: &Rc<Self>) {
        let step = self.flow.borrow().step;
        match step {
            Step::Done => {
                let acked = self.ack.borrow().as_ref().is_none_or(|a| {
                    install::done_may_leave(self.recovery.is_visible(), a.is_active())
                });
                if acked {
                    kit::spawn(&install::restart_command());
                }
                return;
            }
            Step::Installing => return,
            // Read the key again; Restart only comes with it (Done).
            Step::KeyUnread => return self.read_result(None),
            Step::Failed => {
                // The disk may be anything now: look again, and start from it; Back still works.
                self.probed.set(false);
                self.flow.borrow_mut().restart();
                return self.show(Step::Disk);
            }
            _ => {}
        }
        let n = self.flow.borrow_mut().advance();
        self.apply(step, n);
    }

    /// Back (button or Escape).
    pub fn back(self: &Rc<Self>) {
        let step = self.flow.borrow().step;
        if step == Step::Language && !self.flow.borrow().boot {
            reveal::exit_faded();
            return;
        }
        let p = self.flow.borrow_mut().back();
        if let Some(p) = p {
            self.show(p);
        }
    }

    fn apply(self: &Rc<Self>, step: Step, n: Next) {
        self.clear_problems();
        match n {
            Next::Go(s) => self.show(s),
            Next::Stay(field, why) => {
                self.with_card(step, |c| c.say(&why, true));
                if let Some(f) = field {
                    self.mark(f);
                }
            }
            Next::Install => self.start(),
        }
    }

    /// Account page while typing (as Setup's): a field that was marked and is right now loses its
    /// outline and the foot's sentence goes with it; what is still wrong stays said (QA-LIVE1-36:
    /// "Enter your full name." stayed red after a name was typed).
    fn show_typing_problem(&self) {
        let live = self.flow.borrow().draft.typing_problem();
        for (g, e) in self.editors.borrow().iter() {
            if live.is_some_and(|(f, _)| f == *g) {
                e.add_css_class("problem");
            } else {
                e.remove_css_class("problem");
            }
        }
        self.with_card(Step::Account, |c| {
            c.say(live.map_or("", |p| p.1), live.is_some());
        });
    }

    fn mark(&self, f: Field) {
        if let Some((_, e)) = self.editors.borrow().iter().find(|(g, _)| *g == f) {
            e.add_css_class("problem");
            e.grab_focus();
        }
    }

    fn clear_problems(&self) {
        for (_, e) in self.editors.borrow().iter() {
            e.remove_css_class("problem");
        }
    }

    fn begin(self: &Rc<Self>) -> Card {
        let c = Card::moment(
            "white-rabbit",
            Step::Begin.title(),
            Step::Begin.subtitle(),
            &a::blank(),
        );
        c.next.set_label(tr("Install WolOS"));
        c.add_action(&w::button(tr("Try WolOS"), w::Kind::Default, try_wolos));
        c
    }

    fn simple(&self, st: Step, body: &gtk::Box, icon: &str) -> Card {
        Card::new(Art::Icon(icon), st.title(), st.subtitle(), body)
    }

    /// The shared Language page (ui::setup::language_page, with "Try it"): the page following a
    /// language is not the person's keyboard pick (R2B-8, B8).
    fn language(self: &Rc<Self>) -> Card {
        let (language, keyboard) = {
            let f = self.flow.borrow();
            (f.draft.language, f.draft.keyboard)
        };
        let (me, me2) = (Rc::downgrade(self), Rc::downgrade(self));
        super::setup::language_page(
            (Step::Language.title(), Step::Language.subtitle()),
            language,
            keyboard,
            true,
            move |i| {
                let s = me.upgrade()?;
                s.flow.borrow_mut().set_language(i);
                let k = s.flow.borrow().draft.keyboard;
                // The installer speaks the language picked (L10N-2), built again after this click.
                let lang = crate::i18n::lang_of(
                    crate::setup::LANGUAGES[i.min(crate::setup::LANGUAGES.len() - 1)].locale,
                );
                if lang != shared::i18n::lang() {
                    crate::i18n::set_lang(lang);
                    if let Some(h) = s.relang.borrow().clone() {
                        glib::idle_add_local_once(move || h());
                    }
                }
                Some(k)
            },
            move |i, by_person| {
                if let Some(s) = me2.upgrade() {
                    s.flow.borrow_mut().keyboard_shown(i, by_person);
                }
            },
        )
        .card
    }

    fn zone(self: &Rc<Self>) -> Card {
        let body = w::vbox(w::S2);
        let search = w::search_field(tr("Search for a city"));
        let me = Rc::downgrade(self);
        search.connect_search_changed(move |e| {
            if let Some(s) = me.upgrade() {
                s.fill_zones(&e.text());
            }
        });
        body.append(&search);
        body.append(&self.zone_list);
        *self.zone_search.borrow_mut() = Some(search);
        Card::new(
            Art::Icon("preferences-system-time-symbolic"),
            Step::Zone.title(),
            Step::Zone.subtitle(),
            &body,
        )
    }

    /// The zones the search matches; the chosen one on top until the person searches.
    fn fill_zones(self: &Rc<Self>, query: &str) {
        let chosen = self.flow.borrow().zone.clone();
        let mut hits = flow::zones_matching(&self.zones, query);
        if query.is_empty() {
            hits.sort_by_key(|z| **z != chosen);
        }
        hits.truncate(ZONE_ROWS);
        if hits.is_empty() {
            replace_child(
                &self.zone_list,
                &w::empty_state(
                    "system-search-symbolic",
                    copy::installer::no_place().0,
                    copy::installer::no_place().1,
                    None,
                ),
            );
            return;
        }
        let selected = hits.iter().position(|z| **z == chosen);
        let rows = hits
            .iter()
            .map(|z| Choice {
                icon: None,
                title: zone_label(z),
                detail: String::new(),
                trail: None,
            })
            .collect();
        let names: Vec<String> = hits.iter().map(|z| (*z).clone()).collect();
        let me = Rc::downgrade(self);
        let list = ChoiceList::new(rows, selected, move |i| {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().set_zone(&names[i]);
            }
        });
        replace_child(&self.zone_list, &list.root);
    }

    fn protect(self: &Rc<Self>) -> Card {
        let body = w::vbox(w::S4);
        let (tpm, chosen) = {
            let f = self.flow.borrow();
            (f.tpm, f.protect)
        };
        let row = |title: &str, detail: &str| Choice {
            icon: None,
            title: title.into(),
            detail: detail.into(),
            trail: None,
        };
        let mut rows = Vec::new();
        let mut kinds = Vec::new();
        if tpm {
            rows.push(row(tr("Security Chip"), tr("Opens on this computer")));
            kinds.push(Protect::Tpm);
        }
        rows.push(row(tr("Password"), tr("Asked at every start")));
        kinds.push(Protect::Password);
        rows.push(row(
            tr("Don’t Encrypt"),
            tr("Anyone with the disk can read it"),
        ));
        kinds.push(Protect::Off);
        let selected = kinds.iter().position(|k| *k == chosen);
        *self.protect_rows.borrow_mut() = kinds;
        let me = Rc::downgrade(self);
        let list = ChoiceList::new(rows, selected, move |i| {
            if let Some(s) = me.upgrade() {
                let k = s.protect_rows.borrow()[i];
                s.flow.borrow_mut().protect = k;
                s.disk_pw_rows.set_visible(k == Protect::Password);
                s.protect_tpm_note.set_visible(k == Protect::Tpm);
            }
        });
        body.append(&list.root);
        self.protect_tpm_note.set_xalign(0.0);
        self.protect_tpm_note.set_visible(chosen == Protect::Tpm);
        body.append(&self.protect_tpm_note);
        *self.protect_list.borrow_mut() = Some(list);
        let pw = w::password_field(tr("At least 8 characters"));
        let verify = w::password_field(tr("Type it again"));
        let me = Rc::downgrade(self);
        pw.connect_changed(move |e| {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().disk_password = e.text().to_string();
            }
        });
        let me = Rc::downgrade(self);
        verify.connect_changed(move |e| {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().disk_verify = e.text().to_string();
            }
        });
        // The Caps Lock note under the field, inside its row.
        let field = w::vbox(w::S1);
        field.set_hexpand(true);
        field.append(&pw);
        let caps = w::caps_lock_note();
        caps.set_halign(gtk::Align::Start);
        field.append(&caps);
        self.disk_pw_rows
            .append(&a::form_row(tr("Password"), &field));
        self.disk_pw_rows
            .append(&a::form_row(tr("Verify"), &verify));
        body.append(&self.disk_pw_rows);
        Card::new(
            Art::Icon("channel-secure-symbolic"),
            Step::Protect.title(),
            Step::Protect.subtitle(),
            &body,
        )
    }

    fn fill_protect(&self) {
        let chosen = self.flow.borrow().protect;
        if let (Some(i), Some(l)) = (
            self.protect_rows.borrow().iter().position(|k| *k == chosen),
            &*self.protect_list.borrow(),
        ) {
            l.select(i);
        }
        self.disk_pw_rows.set_visible(chosen == Protect::Password);
        self.protect_tpm_note.set_visible(chosen == Protect::Tpm);
    }

    fn account(self: &Rc<Self>) -> Card {
        let body = w::vbox(w::S2);
        let full = w::text_field("Alice Liddell");
        let name = w::text_field("alice");
        let pw = w::password_field(tr("Choose a password"));
        let verify = w::password_field(tr("Type it again"));
        for e in [&full, &name] {
            e.add_css_class("assistant-field");
        }
        let (me, nm) = (Rc::downgrade(self), name.clone());
        full.connect_changed(move |e| {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().draft.set_full_name(&e.text());
                let n = s.flow.borrow().draft.name.clone();
                if nm.text() != n {
                    nm.set_text(&n);
                }
                s.show_typing_problem();
            }
        });
        let me = Rc::downgrade(self);
        name.connect_changed(move |e| {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().draft.set_name(&e.text());
                s.show_typing_problem();
            }
        });
        let me = Rc::downgrade(self);
        pw.connect_changed(move |e| {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().draft.password = e.text().to_string();
                s.show_typing_problem();
            }
        });
        let me = Rc::downgrade(self);
        verify.connect_changed(move |e| {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().draft.verify = e.text().to_string();
                s.show_typing_problem();
            }
        });
        body.append(&a::form_row(tr("Full name"), &full));
        body.append(&a::form_row(tr("Account name"), &name));
        let field = w::vbox(w::S1);
        field.set_hexpand(true);
        field.append(&pw);
        let caps = w::caps_lock_note();
        caps.set_halign(gtk::Align::Start);
        field.append(&caps);
        body.append(&a::form_row(tr("Password"), &field));
        body.append(&a::form_row(tr("Verify"), &verify));
        self.account_tpm_note.set_xalign(0.0);
        body.append(&self.account_tpm_note);
        // The computer's name follows the account name until typed.
        let host = w::text_field(&install::hostname_for(""));
        host.add_css_class("assistant-field");
        let (me, h) = (Rc::downgrade(self), host.clone());
        name.connect_changed(move |_| {
            if let Some(s) = me.upgrade() {
                let f = s.flow.borrow();
                h.set_placeholder_text(Some(&install::hostname_for(&f.draft.name)));
            }
        });
        let me = Rc::downgrade(self);
        host.connect_changed(move |e| {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().set_hostname(&e.text());
                s.show_typing_problem();
            }
        });
        body.append(&a::form_row(tr("Computer name"), &host));
        let me = Rc::downgrade(self);
        let auto = w::toggle(false, move |on| {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().autologin = on;
            }
        });
        auto.set_halign(gtk::Align::Start);
        body.append(&a::form_row(tr("Log in automatically"), &auto));
        *self.editors.borrow_mut() = vec![
            (Field::FullName, full.upcast()),
            (Field::Name, name.upcast()),
            (Field::Password, pw.upcast()),
            (Field::Verify, verify.upcast()),
            (Field::Hostname, host.upcast()),
        ];
        Card::new(
            Art::Icon("avatar-default-symbolic"),
            Step::Account.title(),
            Step::Account.subtitle(),
            &body,
        )
    }

    /// Secure Boot, opt-in: a switch where it is a choice, the key screen pictured before it is
    /// chosen (what it looks like, the exact keys, the password), the own-keys notes.
    fn secure_choice(self: &Rc<Self>) -> Card {
        let body = w::vbox(w::S3);
        body.append(&self.secure_row);
        let mut lines = vec![crate::i18n::tr(copy::installer::KEY_SCREEN_LOOKS).to_string()];
        lines.extend(
            key_screen_steps()
                .iter()
                .enumerate()
                .map(|(i, st)| format!("{}. {st}", i + 1)),
        );
        self.secure_screen
            .append(&a::caption(crate::i18n::tr(copy::INSTALL_KEY_SCREEN_TITLE)));
        self.secure_screen.append(&a::lines(&lines));
        body.append(&self.secure_screen);
        body.append(&self.secure_notes);
        Card::new(
            Art::Icon("security-high-symbolic"),
            Step::SecureBootChoice.title(),
            Step::SecureBootChoice.subtitle(),
            &body,
        )
    }

    fn fill_secure_choice(self: &Rc<Self>) {
        let (offer, on, own_keys) = {
            let f = self.flow.borrow();
            (f.secure_offer(), f.secure, f.tier() == Some("own-keys"))
        };
        // The switch, only where it is a choice; under it, what turning it on costs.
        let me = Rc::downgrade(self);
        let sw = w::toggle(on, move |on| {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().secure = on;
                s.say_secure_notes();
            }
        });
        let detail = if own_keys {
            crate::i18n::tr(copy::installer::SECURE_SWITCH_OWN_KEYS)
        } else {
            crate::i18n::tr(copy::installer::SECURE_SWITCH_SCREEN)
        };
        let row = w::list_row(
            "",
            crate::i18n::tr(copy::installer::SECURE_SWITCH),
            detail,
            Some(sw.upcast_ref()),
        );
        row.add_css_class("assistant-row");
        replace_child(&self.secure_row, &row);
        self.secure_row
            .set_visible(matches!(offer, SecureOffer::Choice { .. }));
        // The key screen is pictured wherever it is how Secure Boot turns on.
        self.secure_screen.set_visible(!own_keys);
        self.say_secure_notes();
    }

    /// Own keys: the install medium stops starting here, said before the choice. BitLocker's
    /// recovery key (R2B-9) only while Secure Boot is on: off leaves Windows' keys alone.
    fn say_secure_notes(&self) {
        let (own_keys, bitlocker) = {
            let f = self.flow.borrow();
            (
                f.tier() == Some("own-keys"),
                f.secure_wanted() && f.bitlocker_stays(),
            )
        };
        let mut notes: Vec<&str> = Vec::new();
        if own_keys {
            notes.push(crate::i18n::tr(copy::INSTALL_OWN_KEYS_MEDIA));
            if bitlocker {
                notes.extend(copy::installer::bitlocker("own-keys", true));
            }
        }
        fill_notes(&self.secure_notes, &notes);
    }

    /// Optional modules (ADR 0032 B): one switch each, none on, the download size under them.
    fn modules(self: &Rc<Self>) -> Card {
        let body = w::vbox(w::S3);
        body.append(&a::list_scroller(&self.modules_list));
        body.append(&self.modules_size);
        Card::new(
            Art::Icon("view-app-grid-symbolic"),
            Step::Modules.title(),
            Step::Modules.subtitle(),
            &body,
        )
    }

    fn say_modules_size(&self) {
        let mb = self.flow.borrow().modules_mb();
        self.modules_size
            .set_label(&copy::installer::modules_download(mb));
    }

    fn fill_modules(self: &Rc<Self>) {
        let rows = w::vbox(0);
        rows.set_valign(gtk::Align::Start);
        let list = self.flow.borrow().module_list.clone();
        for m in list {
            let on = self.flow.borrow().modules.contains(&m.id);
            let me = Rc::downgrade(self);
            let id = m.id.clone();
            let sw = w::toggle(on, move |on| {
                if let Some(s) = me.upgrade() {
                    s.flow.borrow_mut().tick_module(&id, on);
                    s.say_modules_size();
                }
            });
            let row = w::list_row("", m.title, m.detail, Some(sw.upcast_ref()));
            row.add_css_class("assistant-row");
            rows.append(&row);
        }
        replace_child(&self.modules_list, &rows);
        self.say_modules_size();
    }

    /// Media formats, prioritised as the Store's codecs kit lists them; one switch, off.
    fn extras(self: &Rc<Self>) -> Card {
        let body = w::vbox(w::S3);
        let me = Rc::downgrade(self);
        let sw = w::toggle(false, move |on| {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().codecs = on;
            }
        });
        let kit = wol_core::store::kit(flow::CODECS_KIT);
        let row = w::list_row(
            "",
            crate::i18n::tr(copy::installer::CODECS_SWITCH),
            kit.map_or("", |k| k.summary),
            Some(sw.upcast_ref()),
        );
        row.add_css_class("assistant-row");
        body.append(&row);
        *self.codecs_switch.borrow_mut() = Some(sw);
        if let Some(k) = kit {
            let parts: Vec<String> = k
                .parts
                .iter()
                .map(|p| format!("{}: {}", p.name, p.why))
                .collect();
            body.append(&a::lines(&parts));
            let note = a::wrapped(k.note, "assistant-subtitle");
            body.append(&note);
        }
        Card::new(
            Art::Icon("media-playback-start-symbolic"),
            Step::Extras.title(),
            Step::Extras.subtitle(),
            &body,
        )
    }

    fn summary(self: &Rc<Self>) -> Card {
        let body = w::vbox(w::S4);
        body.append(&self.summary_list);
        body.append(&self.summary_warning);
        let c = Card::new(
            Art::Icon("object-select-symbolic"),
            Step::Summary.title(),
            Step::Summary.subtitle(),
            &body,
        );
        c.next.set_label(tr("Install"));
        c
    }

    /// Looks for a network off the main thread; when that changes what waits for the first
    /// start, the Summary (if still shown) says it again. Done says what the last look found.
    fn look_online(self: &Rc<Self>) {
        let me = Rc::downgrade(self);
        let probe = self.online.get();
        glib::spawn_future_local(async move {
            let online = gtk::gio::spawn_blocking(probe).await.unwrap_or(true);
            let Some(s) = me.upgrade() else { return };
            let changed = {
                let mut f = s.flow.borrow_mut();
                let was = std::mem::replace(&mut f.offline, !online);
                was != f.offline && f.step == Step::Summary
            };
            if changed {
                s.fill_summary();
            }
        });
    }

    fn fill_summary(&self) {
        let f = self.flow.borrow();
        replace_child(&self.summary_list, &a::lines(&f.summary()));
        let warn = f.summary_warning();
        self.summary_warning.set_visible(warn.is_some());
        self.summary_text
            .set_label(warn.as_deref().unwrap_or_default());
    }

    fn secure_boot(self: &Rc<Self>) -> Card {
        let c = Card::moment(
            "keyhole",
            Step::SecureBoot.title(),
            Step::SecureBoot.subtitle(),
            &self.secure_boot_notes,
        );
        c.next.set_label(tr("Install Anyway"));
        c.add_action(&w::button(tr("Restart"), w::Kind::Default, || {
            kit::spawn(&install::restart_command())
        }));
        c
    }

    fn installing(self: &Rc<Self>) -> Card {
        let body = w::vbox(w::S4);
        body.append(&self.progress.root);
        self.tip.set_label(install::tip_at(0.0));
        body.append(&self.tip);
        let c = Card::moment(
            "pocket-watch",
            Step::Installing.title(),
            Step::Installing.subtitle(),
            &body,
        );
        c.without_foot();
        c
    }

    fn done(self: &Rc<Self>, body: &gtk::Box) -> Card {
        // The key screen: numbered steps, the password in large type, the media first.
        self.key_screen
            .append(&a::caption(crate::i18n::tr(copy::INSTALL_KEY_SCREEN_TITLE)));
        for (i, step) in key_screen_steps().iter().enumerate() {
            let l = a::wrapped(&format!("{}. {step}", i + 1), "assistant-subtitle");
            l.set_xalign(0.0);
            l.set_justify(gtk::Justification::Left);
            l.set_halign(gtk::Align::Start);
            self.key_screen.append(&l);
        }
        self.key_screen
            .append(&w::centered(MOK_PASSWORD, "large-title"));
        self.key_screen.append(&a::wrapped(
            crate::i18n::tr(copy::INSTALL_KEY_SCREEN_MEDIA),
            "assistant-subtitle",
        ));
        // The recovery key: acknowledged before Restart, or saved on a USB stick.
        let me = Rc::downgrade(self);
        let ack = w::toggle(false, move |on| {
            if let Some(s) = me.upgrade() {
                s.set_leave(install::done_may_leave(true, on));
            }
        });
        ack.set_halign(gtk::Align::Start);
        self.recovery
            .append(&a::form_row(crate::i18n::tr(copy::INSTALL_KEY_ACK), &ack));
        *self.ack.borrow_mut() = Some(ack);
        let me = Rc::downgrade(self);
        let save = w::button(tr("Save to USB"), w::Kind::Default, move || {
            if let Some(s) = me.upgrade() {
                s.save_key_to_usb();
            }
        });
        save.set_halign(gtk::Align::Center);
        self.recovery.append(&save);
        let c = Card::moment("keyhole", Step::Done.title(), Step::Done.subtitle(), body);
        c.next.set_label(tr("Restart"));
        if !self.flow.borrow().boot {
            // From the live desktop: the person may go on trying WolOS and restart later.
            let later = w::button(tr("Restart Later"), w::Kind::Default, reveal::exit_faded);
            c.add_action(&later);
            *self.later.borrow_mut() = Some(later);
        }
        c
    }

    /// The recovery key onto the first USB stick that is mounted; the foot says how it went.
    fn save_key_to_usb(self: &Rc<Self>) {
        let roots: Vec<PathBuf> = install::MEDIA_ROOTS.iter().map(PathBuf::from).collect();
        let key = self.recovery_key.label().to_string();
        let said = key_to_usb(&roots, &key);
        if let (true, Some(ack)) = (said.saved, &*self.ack.borrow()) {
            ack.set_active(true);
        }
        self.with_card(Step::Done, |c| c.say(&said.text, said.problem));
    }

    fn failed(self: &Rc<Self>) -> Card {
        let body = w::vbox(w::S4);
        body.append(&self.failed_refusal);
        body.append(&self.failed_details);
        let c = Card::moment(
            "teacup",
            Step::Failed.title(),
            install::stopped_text(Stage::Finish),
            &body,
        );
        c.next.set_label(tr("Try Again"));
        c.add_action(&w::button(tr("Restart"), w::Kind::Default, || {
            kit::spawn(&install::restart_command())
        }));
        let me = Rc::downgrade(self);
        c.add_action(&w::button(tr("Details"), w::Kind::Plain, move || {
            if let Some(s) = me.upgrade() {
                s.toggle_details();
            }
        }));
        c
    }

    /// Installed, but the recovery key could not be read: Try Again reads it again, Details shows
    /// why. No Restart here.
    fn key_unread(self: &Rc<Self>) -> Card {
        let c = Card::moment(
            "teacup",
            Step::KeyUnread.title(),
            Step::KeyUnread.subtitle(),
            &self.unread_details,
        );
        c.next.set_label(tr("Try Again"));
        let me = Rc::downgrade(self);
        c.add_action(&w::button(tr("Details"), w::Kind::Plain, move || {
            if let Some(s) = me.upgrade() {
                s.toggle_details();
            }
        }));
        c
    }

    fn plan_dir() -> PathBuf {
        install::plan_dir(&PathBuf::from(
            std::env::var_os("XDG_RUNTIME_DIR").unwrap_or_else(|| "/run/wolos".into()),
        ))
    }

    /// Writes the plan and runs `install-run`; progress lines come back through a channel polled
    /// on the UI thread.
    fn start(self: &Rc<Self>) {
        let dir = Self::plan_dir();
        let written = {
            let f = self.flow.borrow();
            let disk_pw = if f.protect == Protect::Password {
                f.disk_password.as_str()
            } else {
                ""
            };
            f.plan().map(|p| {
                install::write_plan(&dir, &p, &f.plan_extras(), disk_pw, &f.draft.password)
            })
        };
        let Some(mut child) = launch(&dir, written, install::install_command) else {
            return self.show_failed(Stage::Prepare, "");
        };
        let out = child.stdout.take();
        let (tx, rx) = std::sync::mpsc::channel::<Event>();
        std::thread::spawn(move || {
            if let Some(out) = out {
                for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                    if let Some(r) = install::parse_line(&line) {
                        eprintln!("wol-installer: {:?} {}% {}", r.stage, r.percent, r.message);
                        let _ = tx.send(Event::Report(r));
                    }
                }
            }
            let ok = child.wait().is_ok_and(|s| s.success());
            let _ = tx.send(Event::Exit(ok));
        });
        *self.report.borrow_mut() = (Report::new(Stage::Prepare, 0), Instant::now());
        self.sample.set(None);
        self.began.set(Some(Instant::now()));
        self.show_progress(&Report::new(Stage::Prepare, 0), None);
        let me = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(250), move || {
            let Some(s) = me.upgrade() else {
                return glib::ControlFlow::Break;
            };
            while let Ok(e) = rx.try_recv() {
                match e {
                    Event::Report(r) if r.stage == Stage::Failed => {
                        let at = r.at.unwrap_or(s.report.borrow().0.stage);
                        s.show_failed(at, &r.message);
                    }
                    Event::Report(r) => {
                        // The bar's clock runs per stage (Files creeps from when it began).
                        let mut cur = s.report.borrow_mut();
                        let since = if r.stage == cur.0.stage {
                            cur.1
                        } else {
                            Instant::now()
                        };
                        *cur = (r, since);
                    }
                    Event::Exit(ok) => {
                        if !ok && s.flow.borrow().step != Step::Failed {
                            let at = s.report.borrow().0.stage;
                            s.show_failed(at, "");
                        }
                        // The key and the log, from root's folder, off the main thread (R2B-1).
                        if ok {
                            s.read_result(Some(dir.clone()));
                        } else {
                            let me = Rc::downgrade(&s);
                            glib::spawn_future_local(async move {
                                let (out, _) = gtk::gio::spawn_blocking(read_result)
                                    .await
                                    .unwrap_or_default();
                                if let Some(s) = me.upgrade() {
                                    s.set_failed_log(&install::parse_result(&out).log);
                                }
                            });
                        }
                        return glib::ControlFlow::Break;
                    }
                }
            }
            s.tick();
            glib::ControlFlow::Continue
        });
    }

    /// After a finished install (`dir`: the plan, removed) or Try Again on the key-unread page:
    /// `install-check --result` off the main thread, then Done, or the key-unread page when the
    /// plan encrypted the disk and no key came back.
    fn read_result(self: &Rc<Self>, dir: Option<PathBuf>) {
        self.with_card(Step::KeyUnread, |c| c.set_busy(true));
        let me = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let (out, why) = gtk::gio::spawn_blocking(read_result)
                .await
                .unwrap_or_else(|_| (String::new(), Some("the reading thread stopped".into())));
            let Some(s) = me.upgrade() else {
                return;
            };
            if let Some(d) = dir {
                install::remove_plan(&d);
            }
            let r = install::parse_result(&out);
            if s.flow.borrow().key_unread(r.key.as_deref()) {
                let why =
                    why.unwrap_or_else(|| "install-check --result gave no recovery key".into());
                return s.show_key_unread(&why, &r.log);
            }
            s.with_card(Step::KeyUnread, |c| c.set_busy(false));
            s.finish(r.key.as_deref());
        });
    }

    /// The recovery key (from `install-check --result`) shows once.
    fn finish(self: &Rc<Self>, key: Option<&str>) {
        let note = std::fs::read_to_string(install::BOOT_NOTE).ok();
        let note = note.as_deref().map(str::trim).filter(|n| !n.is_empty());
        let pending = std::path::Path::new(install::MOK_PENDING).exists();
        self.show_done(
            key.map(str::trim),
            note,
            install::key_screen_needed(note, pending),
        );
    }

    /// Bar and time left from the last report.
    fn tick(&self) {
        if !self.is_installing() {
            return;
        }
        let (r, since) = self.report.borrow().clone();
        let f = install::fraction(&r, since.elapsed().as_secs_f64());
        let now = Instant::now();
        // The copy is most of the time and reports steadily: the estimate starts with it.
        if r.stage != Stage::Prepare && self.sample.get().is_none() {
            self.sample.set(Some((f, now)));
        }
        let left = self.sample.get().and_then(|(f0, t0)| {
            install::seconds_left((f0, 0.0), (f, now.duration_since(t0).as_secs_f64()))
        });
        self.progress.set(
            f,
            &format!(
                "{} {}",
                install::doing(r.stage),
                install::remaining_text(left)
            ),
        );
        if let Some(b) = self.began.get() {
            let tip = install::tip_at(b.elapsed().as_secs_f64());
            if self.tip.label() != tip {
                self.tip.set_label(tip);
            }
        }
    }
}

/// Bytes at slider position `v` (0-100, left = the largest).
fn keep_at(lo: u64, hi: u64, v: f64) -> u64 {
    let at = hi as f64 - (hi - lo) as f64 * v / 100.0;
    ((at as u64) / wol_core::install::disk::MIB * wol_core::install::disk::MIB).clamp(lo, hi)
}

/// Bytes at slider position `v` (0-100, right = the largest); the far right is exactly `max`.
fn bytes_at(min: u64, max: u64, v: f64) -> u64 {
    if v >= 100.0 {
        return max;
    }
    let at = min as f64 + (max - min) as f64 * v / 100.0;
    ((at as u64) / wol_core::install::disk::MIB * wol_core::install::disk::MIB).clamp(min, max)
}

fn percent_of(min: u64, max: u64, bytes: u64) -> f64 {
    if max > min {
        (bytes.clamp(min, max) - min) as f64 * 100.0 / (max - min) as f64
    } else {
        100.0
    }
}

/// The bar over the manual list, in list order.
fn manual_segments(lines: &[flow::Line], d: &Disk, f: &Flow) -> Vec<Seg> {
    lines
        .iter()
        .map(|(row, title, _)| match row {
            Row::Part(uuid) => {
                let p = d.part(uuid);
                let bytes = f
                    .manual
                    .resize
                    .iter()
                    .find(|r| r.part == *uuid)
                    .map(|r| r.bytes)
                    .or(p.map(|p| d.bytes_of(p)))
                    .unwrap_or(0);
                let tone = if f.manual.delete.contains(uuid) {
                    Tone::Free
                } else if p.is_some_and(wol_core::install::disk::Part::is_esp) {
                    Tone::Startup
                } else {
                    Tone::Other
                };
                Seg::new(tone, title, bytes)
            }
            Row::New(i) => Seg::new(Tone::Wolos, title, f.manual.create[*i].bytes.unwrap_or(0)),
            Row::Free(b) => Seg::new(Tone::Free, title, *b),
        })
        .collect()
}

/// Try WolOS: `installer-session` starts the live desktop when this session ends.
fn try_wolos() {
    if let Some(d) = std::env::var_os("XDG_RUNTIME_DIR") {
        let _ = std::fs::write(PathBuf::from(d).join("try"), "");
    }
    reveal::exit_faded();
}

/// `install-check --result`'s output, and why it may hold no key: it could not run, or it
/// exited non-zero (sudo refused, ...), with what it said on stderr.
fn read_result() -> (String, Option<String>) {
    let argv = install::result_command();
    match std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .output()
    {
        Err(e) => {
            eprintln!("wol-installer: {}: {e}", argv.join(" "));
            (
                String::new(),
                Some(format!("could not run {}", argv.join(" "))),
            )
        }
        Ok(o) => {
            let out = String::from_utf8_lossy(&o.stdout).into_owned();
            let why = (!o.status.success()).then(|| {
                let err = String::from_utf8_lossy(&o.stderr);
                format!("{}: {} {}", argv.join(" "), o.status, err.trim())
            });
            (out, why)
        }
    }
}

fn output(argv: &[String]) -> String {
    std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// Every disk `install-disks` allows, with what `install-check` found on it. Reads only.
pub fn system_disks() -> Vec<Found> {
    let allowed = output(&[install::DISKS.to_string()]);
    disks_from(
        &allowed,
        |name| output(&install::probe_command(name)),
        |name| output(&install::check_command(name, true)),
    )
}

/// [`system_disks`] over the three readings: the allowed list, then each disk's probe and check.
/// A disk whose probe is not JSON (the tool could not run, or said something else) is left out
/// and named on stderr, not silently (QA-SHADOW-3, QA-SHADOW-4).
fn disks_from(
    allowed: &str,
    probe: impl Fn(&str) -> String,
    check: impl Fn(&str) -> String,
) -> Vec<Found> {
    install::parse_disks(allowed)
        .iter()
        .filter_map(|name| {
            let disk = match serde_json::from_str(&probe(name)) {
                Ok(d) => d,
                Err(e) => {
                    crate::diag::say!("wol-installer: {name}: probe gave no disk: {e}");
                    return None;
                }
            };
            let check: Check = install::parse_check(&check(name));
            Some(Found { disk, check })
        })
        .collect()
}

/// tz database names this system has.
fn system_zones() -> Vec<String> {
    wol_core::region::ZONE_TABS
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .map(|t| wol_core::region::parse_zone_tab(&t))
        .find(|v| !v.is_empty())
        .unwrap_or_default()
}

pub fn run() -> glib::ExitCode {
    let boot = std::env::args().any(|a| a == "--boot");
    a::run("org.wolos.installer", "installer", move |win| {
        let hints = Hints::read(std::path::Path::new("/"));
        // The language the Language page starts on (the live session's `LANG`, else the time
        // zone's), until the person picks one; `WOL_LANG` (tests, `xx`) wins.
        let start = crate::setup::Draft::detected(&hints).language;
        crate::i18n::set_lang(crate::i18n::lang_of(crate::setup::LANGUAGES[start].locale));
        if gtk::glib::getenv("WOL_LANG").is_some() {
            crate::i18n::init();
        }
        // The same check install-run makes: a chip that exists but can't be used is no chip.
        let tpm = install::tpm_from_decide(&output(&install::tpm_command()));
        let zones = system_zones();
        // The optional modules this image offers (a few small records, read once at start).
        let records = install::modules_on_image();
        let mem = std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|t| flow::meminfo_bytes(&t));
        let build = move || {
            let s = Installer::new(&hints, boot, tpm, zones.clone(), None);
            s.with_flow(|f| f.set_modules(&records));
            if let Some(mem) = mem {
                s.with_flow(|f| f.set_memory(mem));
            }
            s
        };
        // The installer on screen: the one built in the language in use (a language pick builds
        // it again, on its Language page, with the draft so far).
        let cur: Rc<RefCell<Rc<Installer>>> = Rc::new(RefCell::new(build()));
        let mount: Rc<dyn Fn()> = {
            let (cur, w) = (cur.clone(), win.clone());
            Rc::new(move || {
                let s = cur.borrow().clone();
                // Over the live desktop: shown at once, no fade from black.
                let shown = reveal::Reveal::new(&s.root, false);
                w.set_child(Some(&shown.root));
                // Installing: no idle screen-off, lock or sleep. The compositor's idle inhibitor
                // for the window, and the session's ScreenSaver inhibit, which hypridle honours
                // whichever workspace is showing. install-run holds the lid. Released on done or
                // stopped.
                let (app, w2) = (w.application(), w.clone());
                let held: RefCell<Option<(u32, std::sync::mpsc::Sender<()>)>> = RefCell::new(None);
                s.set_keep_awake(move |on| {
                    if on {
                        // IDLE only: on Wayland GTK maps it to the compositor's idle inhibitor (no
                        // D-Bus round trip on the UI thread); sleep from idle is hypridle's.
                        let flags = gtk::ApplicationInhibitFlags::IDLE;
                        let cookie = app.as_ref().map_or(0, |a| {
                            a.inhibit(Some(&w2), flags, Some(install::AWAKE_REASON))
                        });
                        *held.borrow_mut() = Some((cookie, screensaver_inhibit()));
                    } else if let Some((cookie, release)) = held.borrow_mut().take() {
                        if let (Some(a), true) = (&app, cookie != 0) {
                            a.uninhibit(cookie);
                        }
                        let _ = release.send(());
                    }
                });
            })
        };
        let relang: Rc<dyn Fn()> = {
            let (cur, mount) = (cur.clone(), mount.clone());
            Rc::new(move || {
                let old = cur.borrow().clone();
                if old.flow.borrow().step != Step::Language {
                    return;
                }
                let n = build();
                {
                    let o = old.flow.borrow();
                    let (draft, zone) = (o.draft.clone(), o.zone.clone());
                    n.with_flow(|f| {
                        f.draft = draft;
                        f.zone = zone;
                    });
                }
                // Begin, then Language: the way the person came, so Back still works.
                n.next();
                *n.relang.borrow_mut() = old.relang.borrow_mut().take();
                *cur.borrow_mut() = n;
                mount();
            })
        };
        *cur.borrow().relang.borrow_mut() = Some(relang);
        mount();
        // Closing mid-install would cut the installer's output and stop it: not while installing.
        let c2 = cur.clone();
        win.connect_close_request(move |_| {
            let s2 = c2.borrow().clone();
            if s2.is_installing() {
                glib::Propagation::Stop
            } else {
                // A failed install leaves no password file behind (audit6 S6-4).
                if s2.flow.borrow().step == Step::Failed {
                    install::remove_plan(&Installer::plan_dir());
                }
                glib::Propagation::Proceed
            }
        });
        let (c3, c4) = (cur.clone(), cur.clone());
        a::keys(
            win,
            move || {
                let s = c3.borrow().clone();
                s.next()
            },
            move || {
                let s = c4.borrow().clone();
                s.back()
            },
        );
    })
}

/// What saving the recovery key to a USB stick came to: the foot's sentence, whether it is a
/// problem, and whether the key is on a stick now (the acknowledgement switch follows).
#[derive(Debug, PartialEq)]
struct KeySaved {
    text: String,
    problem: bool,
    saved: bool,
}

/// The key onto the first stick mounted under `roots`: no stick, a stick that cannot be written
/// (the foot says so and Restart stays blocked: QA-SHADOW-1), or saved.
fn key_to_usb(roots: &[PathBuf], key: &str) -> KeySaved {
    let (text, problem, saved) = match install::media_dirs(roots).first() {
        None => (
            crate::i18n::tr(copy::INSTALL_NO_USB).to_string(),
            true,
            false,
        ),
        Some(dir) => match install::save_key(dir, key) {
            Ok(_) => {
                let name = dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                (copy::install_key_saved(&name), false, true)
            }
            Err(e) => {
                eprintln!("wol-installer: recovery key not saved: {e}");
                (
                    crate::i18n::tr(copy::INSTALL_KEY_NOT_SAVED).to_string(),
                    true,
                    false,
                )
            }
        },
    };
    KeySaved {
        text,
        problem,
        saved,
    }
}

/// Starts `command(plan)` for the plan just written. When there is no plan (it could not be
/// written) or the command cannot start, the plan folder goes: it already holds the account and
/// disk passwords, and only `install-run` removes them (audit6 S6-4).
fn launch(
    dir: &std::path::Path,
    written: Option<std::io::Result<PathBuf>>,
    command: impl Fn(&std::path::Path) -> Vec<String>,
) -> Option<std::process::Child> {
    let Some(Ok(plan)) = written else {
        install::remove_plan(dir);
        return None;
    };
    let argv = command(&plan);
    eprintln!("wol-installer: {}", argv.join(" "));
    let child = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn();
    if child.is_err() {
        install::remove_plan(dir);
    }
    child.ok()
}

/// `org.freedesktop.ScreenSaver.Inhibit` on the session bus (hypridle serves it) until the sender
/// is used or dropped; the connection lives on its own thread, so a slow bus never blocks the UI.
fn screensaver_inhibit() -> std::sync::mpsc::Sender<()> {
    const SS: &str = "org.freedesktop.ScreenSaver";
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        let Ok(c) = zbus::blocking::Connection::session() else {
            return;
        };
        let cookie = c
            .call_method(
                Some(SS),
                "/org/freedesktop/ScreenSaver",
                Some(SS),
                "Inhibit",
                &("Install WolOS", install::AWAKE_REASON),
            )
            .ok()
            .and_then(|m| m.body().deserialize::<u32>().ok());
        let _ = rx.recv();
        if let Some(k) = cookie {
            let _ = c.call_method(
                Some(SS),
                "/org/freedesktop/ScreenSaver",
                Some(SS),
                "UnInhibit",
                &(k,),
            );
        }
    });
    tx
}

#[cfg(test)]
mod tests;
