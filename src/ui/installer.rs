//! Install WolOS (`wol-installer`, ADR 0013, ADR 0023): one decision per card, a summary of exactly
//! what will change, then `install-run` as root. The same cards as the Setup Assistant; the steps
//! and every word are `install::flow`. `--boot` is the first screen of the live image: Install or
//! Try, in a session that holds nothing else.
use crate::copy;
use crate::install::flow::{
    self, DiskTitle, FILE_SYSTEMS, Flow, Found, Method, Next, Row, STEPS, Step, USES, Use,
};
use crate::install::{self, Check, Report, Stage};
use crate::kit::assistant::{self as a, Art, Card, Choice, ChoiceList, DiskBar, Seg, Tone};
use crate::kit::{self, reveal, widgets as w};
use crate::setup::{Field, Hints, KEYBOARDS, LANGUAGES};
use gtk::{glib, prelude::*};
use std::cell::{Cell, RefCell};
use std::io::BufRead;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};
use wol_core::install::disk::Disk;
use wol_core::install::plan::Protect;
use wol_core::region::zone_label;

/// Rows the zone search builds at once: the list is for choosing, the search for finding.
const ZONE_ROWS: usize = 60;

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
    flow: RefCell<Flow>,
    cards: RefCell<Vec<(Step, Card)>>,
    zones: Vec<String>,
    keyboard: RefCell<Option<gtk::DropDown>>,
    zone_search: RefCell<Option<gtk::SearchEntry>>,
    zone_list: gtk::Box,
    disk_list: gtk::Box,
    method_list: gtk::Box,
    method_warn: gtk::Box,
    method_warning: gtk::Label,
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
    recovery: gtk::Box,
    recovery_key: gtk::Label,
    boot_note: gtk::Label,
    /// Last report and when its stage began; first package sample for the time estimate.
    report: RefCell<(Report, Instant)>,
    sample: Cell<Option<(f64, Instant)>>,
    /// Disks were looked at. Golden renders hand theirs in.
    probed: Cell<bool>,
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

fn holder() -> gtk::Box {
    let b = w::vbox(0);
    b.set_vexpand(true);
    b
}

impl Installer {
    pub fn new(
        hints: &Hints,
        boot: bool,
        tpm: bool,
        zones: Vec<String>,
        disks: Option<Vec<Found>>,
    ) -> Rc<Installer> {
        let pager = a::pager();
        let (along_warn, along_warning) = a::warning("");
        let (method_warn, method_warning) = a::warning("");
        let (summary_warning, summary_text) = a::warning("");
        let method_list = holder();
        let along_box = w::vbox(w::S4);
        let recovery = w::vbox(w::S2);
        let recovery_key = w::centered("", "assistant-key");
        recovery_key.set_selectable(true);
        recovery_key.set_focusable(false);
        recovery.append(&a::wrapped(
            copy::INSTALL_RECOVERY_NOTE,
            "assistant-subtitle",
        ));
        recovery.append(&recovery_key);
        recovery.set_visible(false);
        let done_body = w::vbox(w::S2);
        let boot_note = a::wrapped("", "assistant-subtitle");
        boot_note.set_visible(false);
        done_body.append(&boot_note);
        done_body.append(&recovery);
        let mut flow = Flow::new(hints, boot, tpm);
        let probed = disks.is_some();
        if let Some(d) = disks {
            flow.set_disks(d);
        }
        let s = Rc::new(Installer {
            root: super::lock::screen(&pager),
            pager,
            flow: RefCell::new(flow),
            cards: Default::default(),
            zones,
            keyboard: Default::default(),
            zone_search: Default::default(),
            zone_list: holder(),
            disk_list: holder(),
            method_list,
            method_warn,
            method_warning,
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
            recovery,
            recovery_key,
            boot_note,
            report: RefCell::new((Report::new(Stage::Prepare, 0), Instant::now())),
            sample: Cell::new(None),
            probed: Cell::new(probed),
            awake: RefCell::new(install::Awake::default()),
            keep_awake: RefCell::new(None),
        });
        for st in STEPS {
            let card = match st {
                Step::Begin => s.begin(),
                Step::Language => s.language(),
                Step::Zone => s.zone(),
                Step::Disk => s.simple(st, &s.disk_list, "drive-harddisk-symbolic"),
                Step::Method => s.method(),
                Step::Alongside => s.alongside(),
                Step::Manual => s.manual(),
                Step::Edit => s.edit_page(),
                Step::Protect => s.protect(),
                Step::Account => s.account(),
                Step::Summary => s.summary(),
                Step::SecureBoot => s.secure_boot(),
                Step::Installing => s.installing(),
                Step::Done => s.done(&done_body),
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
            Step::Begin | Step::Done | Step::Failed | Step::Installing
        ));
    }

    /// Refreshes `step`'s page and shows it.
    fn show(self: &Rc<Self>, step: Step) {
        for (_, c) in self.cards.borrow().iter() {
            c.say("", false);
        }
        match step {
            Step::Language => {
                let cancel = !self.flow.borrow().boot;
                self.with_card(step, |c| {
                    c.back.set_label(if cancel { "Cancel" } else { "Back" })
                });
            }
            Step::Zone => self.fill_zones(""),
            Step::Disk => self.enter_disks(),
            Step::Method => self.fill_method(),
            Step::Alongside => self.fill_alongside(),
            Step::Manual => self.fill_manual(),
            Step::Edit => self.fill_edit(),
            Step::Protect => self.fill_protect(),
            Step::Account => {
                if let Some((_, e)) = self.editors.borrow().first() {
                    e.grab_focus();
                }
            }
            Step::Summary => self.fill_summary(),
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

    /// Failed page, stopped during `at`.
    pub fn show_failed(self: &Rc<Self>, at: Stage) {
        self.with_card(Step::Failed, |c| {
            c.subtitle.set_label(install::stopped_text(at))
        });
        self.flow.borrow_mut().jump(Step::Failed);
        self.show(Step::Failed);
    }

    /// Done page; a recovery key shows here once, and what the startup setup has to say.
    pub fn show_done(self: &Rc<Self>, key: Option<&str>, note: Option<&str>) {
        self.boot_note.set_visible(note.is_some());
        self.boot_note.set_label(note.unwrap_or_default());
        self.recovery.set_visible(key.is_some());
        self.recovery_key.set_label(key.unwrap_or_default());
        self.flow.borrow_mut().jump(Step::Done);
        self.show(Step::Done);
    }

    pub fn is_installing(&self) -> bool {
        self.flow.borrow().step == Step::Installing
    }

    /// Continue (button or Return).
    pub fn next(self: &Rc<Self>) {
        let step = self.flow.borrow().step;
        match step {
            Step::Done => return kit::spawn(&install::restart_command()),
            Step::Installing => return,
            Step::Failed => {
                // The disk may be anything now: look again, and start from it.
                self.probed.set(false);
                self.flow.borrow_mut().jump(Step::Disk);
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
        c.next.set_label("Install WolOS");
        c.add_action(&w::button("Try WolOS", w::Kind::Default, try_wolos));
        c
    }

    fn simple(&self, st: Step, body: &gtk::Box, icon: &str) -> Card {
        Card::new(Art::Icon(icon), st.title(), st.subtitle(), body)
    }

    fn language(self: &Rc<Self>) -> Card {
        let (language, keyboard) = {
            let f = self.flow.borrow();
            (f.draft.language, f.draft.keyboard)
        };
        let body = w::vbox(w::S4);
        let rows = LANGUAGES
            .iter()
            .map(|l| Choice {
                icon: None,
                title: l.name.into(),
                detail: l.region.into(),
                trail: None,
            })
            .collect();
        let me = Rc::downgrade(self);
        let list = ChoiceList::new(rows, Some(language), move |i| {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().set_language(i);
                let k = s.flow.borrow().draft.keyboard;
                if let Some(dd) = &*s.keyboard.borrow() {
                    dd.set_selected(k as u32);
                }
            }
        });
        body.append(&list.root);
        let names: Vec<&str> = KEYBOARDS.iter().map(|k| k.1).collect();
        let kb = w::dropdown(&names, keyboard as u32);
        let me = Rc::downgrade(self);
        kb.connect_selected_notify(move |dd| {
            let Some(s) = me.upgrade() else {
                return;
            };
            let i = (dd.selected() as usize).min(KEYBOARDS.len() - 1);
            s.flow.borrow_mut().draft.keyboard = i;
            if std::env::var_os("WOL_FAKE_NOW").is_none() {
                kit::spawn(&crate::setup::keyboard_command(KEYBOARDS[i].0));
            }
        });
        body.append(&a::form_row("Keyboard", &kb));
        *self.keyboard.borrow_mut() = Some(kb);
        Card::new(
            Art::Icon("preferences-desktop-locale-symbolic"),
            Step::Language.title(),
            Step::Language.subtitle(),
            &body,
        )
    }

    fn zone(self: &Rc<Self>) -> Card {
        let body = w::vbox(w::S2);
        let search = w::search_field("Search for a city");
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
                    "No matching place",
                    "Try the name of a big city near you.",
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

    /// The disk page: the disks at once when known, else a look at them off the main thread.
    fn enter_disks(self: &Rc<Self>) {
        if self.probed.get() {
            return self.fill_disks();
        }
        replace_child(&self.disk_list, &a::waiting("Looking at your disks…"));
        let me = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let found = gtk::gio::spawn_blocking(system_disks)
                .await
                .unwrap_or_default();
            if let Some(s) = me.upgrade() {
                s.probed.set(true);
                s.flow.borrow_mut().set_disks(found);
                s.fill_disks();
            }
        });
    }

    fn fill_disks(self: &Rc<Self>) {
        let (rows, selected): (Vec<Choice>, _) = {
            let f = self.flow.borrow();
            let rows = f
                .disks
                .iter()
                .map(|d| Choice {
                    icon: Some("drive-harddisk-symbolic".into()),
                    title: d.disk.title_text(),
                    detail: install::size_text(d.disk.bytes),
                    trail: None,
                })
                .collect();
            (rows, f.chosen)
        };
        if rows.is_empty() {
            replace_child(
                &self.disk_list,
                &w::empty_state(
                    "drive-harddisk-symbolic",
                    "No disk found",
                    "Connect the disk you want to install on, then open Install WolOS again.",
                    None,
                ),
            );
            return;
        }
        let me = Rc::downgrade(self);
        let list = ChoiceList::new(rows, selected, move |i| {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().pick_disk(i);
            }
        });
        replace_child(&self.disk_list, &list.root);
    }

    fn method(self: &Rc<Self>) -> Card {
        let body = w::vbox(w::S4);
        body.append(&self.method_list);
        body.append(&self.method_warn);
        Card::new(
            Art::Icon("drive-multidisk-symbolic"),
            Step::Method.title(),
            Step::Method.subtitle(),
            &body,
        )
    }

    fn fill_method(self: &Rc<Self>) {
        const ORDER: [Method; 3] = [Method::Alongside, Method::Erase, Method::Manual];
        let (rows, selected): (Vec<Choice>, _) = {
            let f = self.flow.borrow();
            let row = |title: String, detail: &str| Choice {
                icon: None,
                title,
                detail: detail.into(),
                trail: None,
            };
            (
                vec![
                    row(f.alongside_title(), "Keeps what's there"),
                    row("Erase Disk".into(), "Starts clean"),
                    row("Manual".into(), "For experts"),
                ],
                ORDER.iter().position(|m| *m == f.method),
            )
        };
        let me = Rc::downgrade(self);
        let list = ChoiceList::new(rows, selected, move |i| {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().method = ORDER[i];
                s.say_method();
            }
        });
        replace_child(&self.method_list, &list.root);
        self.say_method();
    }

    fn say_method(&self) {
        let warn = self.flow.borrow().method_warning();
        self.method_warn.set_visible(warn.is_some());
        self.method_warning
            .set_label(warn.as_deref().unwrap_or_default());
    }

    fn alongside(self: &Rc<Self>) -> Card {
        let body = &self.along_box;
        body.append(&self.along_bar.root);
        let me = Rc::downgrade(self);
        let slider = w::row_slider(50.0, move |v| {
            if let Some(s) = me.upgrade() {
                s.slide(v);
            }
        });
        body.append(&slider);
        *self.along_slider.borrow_mut() = Some(slider);
        body.append(&self.along_warn);
        Card::new(
            Art::Icon("drive-multidisk-symbolic"),
            Step::Alongside.title(),
            Step::Alongside.subtitle(),
            body,
        )
    }

    /// The slider runs from the most the other system can keep, on the left, to the least.
    fn slide(self: &Rc<Self>, v: f64) {
        if self.filling.get() {
            return;
        }
        let range = self.flow.borrow().alongside_slider();
        if let Some((_, _, lo, hi)) = range {
            self.flow.borrow_mut().keep = keep_at(lo, hi, v);
            self.fill_along_bar();
        }
    }

    fn fill_alongside(self: &Rc<Self>) {
        let f = self.flow.borrow();
        if let (Some((_, _, lo, hi)), Some(sl)) =
            (f.alongside_slider(), &*self.along_slider.borrow())
        {
            self.filling.set(true);
            sl.set_value((hi - f.keep.clamp(lo, hi)) as f64 * 100.0 / (hi - lo).max(1) as f64);
            self.filling.set(false);
        }
        drop(f);
        self.fill_along_bar();
    }

    fn fill_along_bar(&self) {
        let f = self.flow.borrow();
        let Some((_, name, ..)) = f.alongside_slider() else {
            return;
        };
        let wolos = f.alongside_room().unwrap_or(0);
        self.along_bar.set(&[
            Seg::new(Tone::Other, &name, f.keep),
            Seg::new(Tone::Wolos, "WolOS", wolos),
        ]);
        let low = wolos < wol_core::install::layout::ROOMY_ROOT;
        self.along_warn.set_visible(low);
        self.along_warning
            .set_label(&copy::install_low_space(wolos));
    }

    fn manual(self: &Rc<Self>) -> Card {
        let body = w::vbox(w::S2);
        body.append(&self.manual_bar.root);
        body.append(&self.manual_list);
        Card::new(
            Art::Icon("drive-harddisk-symbolic"),
            Step::Manual.title(),
            Step::Manual.subtitle(),
            &body,
        )
    }

    fn fill_manual(self: &Rc<Self>) {
        let f = self.flow.borrow();
        let Some(d) = f.disk().cloned() else {
            return;
        };
        let lines = flow::manual_lines(&f.manual, &d);
        self.manual_bar.set(&manual_segments(&lines, &d, &f));
        drop(f);
        let rows = lines
            .iter()
            .map(|(_, title, detail)| Choice {
                icon: None,
                title: title.clone(),
                detail: detail.clone(),
                trail: None,
            })
            .collect();
        let me = Rc::downgrade(self);
        let list = ChoiceList::new(rows, None, move |i| {
            if let Some(s) = me.upgrade() {
                let n = s.flow.borrow_mut().open_edit(&lines[i].0);
                s.apply(Step::Manual, n);
            }
        });
        replace_child(&self.manual_list, &list.root);
    }

    fn edit_page(self: &Rc<Self>) -> Card {
        let body = w::vbox(w::S2);
        let on_flow = |me: Weak<Installer>, f: fn(&mut flow::Edit, &Installer)| {
            move || {
                if let Some(s) = me.upgrade() {
                    if s.filling.get() {
                        return;
                    }
                    if let Some(e) = s.flow.borrow_mut().edit.as_mut() {
                        f(e, &s);
                    }
                    s.show_edit_rows();
                }
            }
        };
        let names: Vec<&str> = USES.iter().map(|u| u.1).collect();
        let usage = w::dropdown(&names, 0);
        let go = on_flow(Rc::downgrade(self), |e, s| {
            if let Some(ui) = &*s.edit.borrow() {
                e.usage = USES[(ui.usage.selected() as usize).min(USES.len() - 1)].0;
            }
        });
        usage.connect_selected_notify(move |_| go());
        body.append(&a::form_row("Use for", &usage));
        let folder = w::text_field("Folder name");
        let go = on_flow(Rc::downgrade(self), |e, s| {
            if let Some(ui) = &*s.edit.borrow() {
                e.folder = ui.folder.text().to_string();
            }
        });
        folder.connect_changed(move |_| go());
        let folder_row = a::form_row("Folder", &folder);
        body.append(&folder_row);
        let go = on_flow(Rc::downgrade(self), |e, s| {
            if let Some(ui) = &*s.edit.borrow() {
                e.erase = ui.erase.is_active();
            }
        });
        let erase = w::toggle(false, move |_| go());
        erase.set_halign(gtk::Align::Start);
        let erase_row = a::form_row("Erase", &erase);
        body.append(&erase_row);
        let fs_names: Vec<&str> = FILE_SYSTEMS.iter().map(|f| f.1).collect();
        let fs = w::dropdown(&fs_names, 0);
        let go = on_flow(Rc::downgrade(self), |e, s| {
            if let Some(ui) = &*s.edit.borrow() {
                e.fs = FILE_SYSTEMS[(ui.fs.selected() as usize).min(FILE_SYSTEMS.len() - 1)].0;
            }
        });
        fs.connect_selected_notify(move |_| go());
        let fs_row = a::form_row("File system", &fs);
        body.append(&fs_row);
        let me = Rc::downgrade(self);
        let size = w::row_slider(100.0, move |v| {
            if let Some(s) = me.upgrade() {
                s.edit_size(v);
            }
        });
        let size_text = w::label("", "assistant-subtitle");
        let row = w::hbox(w::S2);
        row.append(&size);
        row.append(&size_text);
        let size_row = a::form_row("Size", &row);
        body.append(&size_row);
        let c = Card::new(
            Art::Icon("drive-harddisk-symbolic"),
            Step::Edit.title(),
            Step::Edit.subtitle(),
            &body,
        );
        let me = Rc::downgrade(self);
        let delete = w::button("Delete", w::Kind::Default, move || {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().delete_edited();
                let st = s.flow.borrow().step;
                s.show(st);
            }
        });
        c.add_action(&delete);
        c.next.set_label("Done");
        *self.edit.borrow_mut() = Some(EditUi {
            usage,
            folder_row,
            folder,
            erase_row,
            erase,
            fs_row,
            fs,
            size_row,
            size,
            size_text,
            delete,
        });
        c
    }

    fn edit_size(&self, v: f64) {
        if self.filling.get() {
            return;
        }
        let mut f = self.flow.borrow_mut();
        if let Some(e) = f.edit.as_mut() {
            e.bytes = bytes_at(e.min, e.max, v);
            if let Some(ui) = &*self.edit.borrow() {
                ui.size_text.set_label(&install::size_text(e.bytes));
            }
        }
    }

    /// Which rows the page shows for what it's set to.
    fn show_edit_rows(&self) {
        let f = self.flow.borrow();
        let (Some(e), Some(ui)) = (f.edit.as_ref(), &*self.edit.borrow()) else {
            return;
        };
        let existing = matches!(e.row, Row::Part(_));
        let plain = matches!(e.usage, Use::System | Use::Home | Use::Folder);
        ui.folder_row.set_visible(e.usage == Use::Folder);
        ui.erase_row
            .set_visible(existing && e.usage != Use::Nothing);
        ui.fs_row.set_visible(plain && (e.erase || !existing));
        ui.size_row.set_visible(e.min < e.max);
        ui.delete.set_visible(!matches!(e.row, Row::Free(_)));
    }

    fn fill_edit(&self) {
        {
            let f = self.flow.borrow();
            let (Some(e), Some(ui)) = (f.edit.as_ref(), &*self.edit.borrow()) else {
                return;
            };
            self.filling.set(true);
            let at = USES.iter().position(|u| u.0 == e.usage).unwrap_or(0);
            ui.usage.set_selected(at as u32);
            ui.folder.set_text(&e.folder);
            ui.erase.set_active(e.erase);
            let fs = FILE_SYSTEMS.iter().position(|x| x.0 == e.fs).unwrap_or(0);
            ui.fs.set_selected(fs as u32);
            ui.size.set_value(percent_of(e.min, e.max, e.bytes));
            ui.size_text.set_label(&install::size_text(e.bytes));
            self.filling.set(false);
            self.with_card(Step::Edit, |c| {
                c.subtitle
                    .set_label(&format!("{}. {}", e.title, Step::Edit.subtitle()))
            });
        }
        self.show_edit_rows();
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
            rows.push(row("Security Chip", "Opens on this computer"));
            kinds.push(Protect::Tpm);
        }
        rows.push(row("Password", "Asked at every start"));
        kinds.push(Protect::Password);
        rows.push(row("Don't Encrypt", "Anyone with the disk can read it"));
        kinds.push(Protect::Off);
        let selected = kinds.iter().position(|k| *k == chosen);
        *self.protect_rows.borrow_mut() = kinds;
        let me = Rc::downgrade(self);
        let list = ChoiceList::new(rows, selected, move |i| {
            if let Some(s) = me.upgrade() {
                let k = s.protect_rows.borrow()[i];
                s.flow.borrow_mut().protect = k;
                s.disk_pw_rows.set_visible(k == Protect::Password);
            }
        });
        body.append(&list.root);
        *self.protect_list.borrow_mut() = Some(list);
        let pw = w::password_field("At least 8 characters");
        let verify = w::password_field("Type it again");
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
        self.disk_pw_rows.append(&a::form_row("Password", &field));
        self.disk_pw_rows.append(&a::form_row("Verify", &verify));
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
    }

    fn account(self: &Rc<Self>) -> Card {
        let body = w::vbox(w::S2);
        let full = w::text_field("Alice Liddell");
        let name = w::text_field("alice");
        let pw = w::password_field("Choose a password");
        let verify = w::password_field("Type it again");
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
            }
        });
        let me = Rc::downgrade(self);
        name.connect_changed(move |e| {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().draft.set_name(&e.text());
            }
        });
        let me = Rc::downgrade(self);
        pw.connect_changed(move |e| {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().draft.password = e.text().to_string();
            }
        });
        let me = Rc::downgrade(self);
        verify.connect_changed(move |e| {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().draft.verify = e.text().to_string();
            }
        });
        body.append(&a::form_row("Full name", &full));
        body.append(&a::form_row("Account name", &name));
        let field = w::vbox(w::S1);
        field.set_hexpand(true);
        field.append(&pw);
        let caps = w::caps_lock_note();
        caps.set_halign(gtk::Align::Start);
        field.append(&caps);
        body.append(&a::form_row("Password", &field));
        body.append(&a::form_row("Verify", &verify));
        *self.editors.borrow_mut() = vec![
            (Field::FullName, full.upcast()),
            (Field::Name, name.upcast()),
            (Field::Password, pw.upcast()),
            (Field::Verify, verify.upcast()),
        ];
        Card::new(
            Art::Icon("avatar-default-symbolic"),
            Step::Account.title(),
            Step::Account.subtitle(),
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
        c.next.set_label("Install");
        c
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
            &a::blank(),
        );
        c.next.set_label("Install Anyway");
        c.add_action(&w::button("Restart", w::Kind::Default, || {
            kit::spawn(&install::restart_command())
        }));
        c
    }

    fn installing(self: &Rc<Self>) -> Card {
        let c = Card::moment(
            "pocket-watch",
            Step::Installing.title(),
            Step::Installing.subtitle(),
            &self.progress.root,
        );
        c.without_foot();
        c
    }

    fn done(self: &Rc<Self>, body: &gtk::Box) -> Card {
        let c = Card::moment("keyhole", Step::Done.title(), Step::Done.subtitle(), body);
        c.next.set_label("Restart");
        c
    }

    fn failed(self: &Rc<Self>) -> Card {
        let c = Card::moment(
            "teacup",
            Step::Failed.title(),
            install::stopped_text(Stage::Finish),
            &a::blank(),
        );
        c.next.set_label("Try Again");
        c.add_action(&w::button("Restart", w::Kind::Default, || {
            kit::spawn(&install::restart_command())
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
            f.plan()
                .map(|p| install::write_plan(&dir, &p, disk_pw, &f.draft.password))
        };
        let Some(Ok(plan)) = written else {
            return self.show_failed(Stage::Prepare);
        };
        let argv = install::install_command(&plan);
        eprintln!("wol-installer: {}", argv.join(" "));
        let child = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn();
        let Ok(mut child) = child else {
            return self.show_failed(Stage::Prepare);
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
        self.show_progress(&Report::new(Stage::Prepare, 0), None);
        let me = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(250), move || {
            let Some(s) = me.upgrade() else {
                return glib::ControlFlow::Break;
            };
            while let Ok(e) = rx.try_recv() {
                match e {
                    Event::Report(r) if r.stage == Stage::Failed => {
                        let at = s.report.borrow().0.stage;
                        s.show_failed(at);
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
                        if ok {
                            s.finish(&dir);
                        } else if s.flow.borrow().step != Step::Failed {
                            let at = s.report.borrow().0.stage;
                            s.show_failed(at);
                        }
                        return glib::ControlFlow::Break;
                    }
                }
            }
            s.tick();
            glib::ControlFlow::Continue
        });
    }

    /// The recovery key shows once; the plan and its files go.
    fn finish(self: &Rc<Self>, dir: &std::path::Path) {
        let key = std::fs::read_to_string(dir.join(install::RECOVERY_KEY)).ok();
        let _ = std::fs::remove_dir_all(dir);
        let note = std::fs::read_to_string(install::BOOT_NOTE).ok();
        self.show_done(
            key.as_deref().map(str::trim),
            note.as_deref().map(str::trim).filter(|n| !n.is_empty()),
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
    install::parse_disks(&allowed)
        .iter()
        .filter_map(|name| {
            let disk = serde_json::from_str(&output(&install::probe_command(name))).ok()?;
            let check: Check = install::parse_check(&output(&install::check_command(name, true)));
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
        let tpm = std::path::Path::new("/sys/class/tpm/tpm0").exists();
        let s = Installer::new(&hints, boot, tpm, system_zones(), None);
        // Over the live desktop: shown at once, no fade from black.
        let shown = reveal::Reveal::new(&s.root, false);
        win.set_child(Some(&shown.root));
        // Closing mid-install would cut the installer's output and stop it: not while installing.
        let s2 = s.clone();
        win.connect_close_request(move |_| {
            if s2.is_installing() {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        // Installing: no idle screen-off, lock or sleep. The compositor's idle inhibitor for the
        // window, and the session's ScreenSaver inhibit, which hypridle honours whichever
        // workspace is showing. install-run holds the lid. Released on done or stopped.
        let (app, w) = (win.application(), win.clone());
        let held: RefCell<Option<(u32, std::sync::mpsc::Sender<()>)>> = RefCell::new(None);
        s.set_keep_awake(move |on| {
            if on {
                // IDLE only: on Wayland GTK maps it to the compositor's idle inhibitor (no D-Bus
                // round trip on the UI thread); sleep from idle is hypridle's, held below.
                let flags = gtk::ApplicationInhibitFlags::IDLE;
                let cookie = app.as_ref().map_or(0, |a| {
                    a.inhibit(Some(&w), flags, Some(install::AWAKE_REASON))
                });
                *held.borrow_mut() = Some((cookie, screensaver_inhibit()));
            } else if let Some((cookie, release)) = held.borrow_mut().take() {
                if let (Some(a), true) = (&app, cookie != 0) {
                    a.uninhibit(cookie);
                }
                let _ = release.send(());
            }
        });
        let s3 = s.clone();
        a::keys(win, move || s.next(), move || s3.back());
    })
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
mod tests {
    use super::*;

    #[test]
    fn the_sliders_land_on_whole_mib_inside_their_range() {
        let (lo, hi) = (10u64 << 30, 100u64 << 30);
        assert_eq!(keep_at(lo, hi, 0.0), hi);
        assert_eq!(keep_at(lo, hi, 100.0), lo);
        assert_eq!(keep_at(lo, hi, 50.0) % (1 << 20), 0);
        assert_eq!(bytes_at(lo, hi, 100.0), hi);
        assert_eq!(bytes_at(lo, hi, 0.0), lo);
        assert_eq!(percent_of(lo, hi, hi), 100.0);
        assert_eq!(percent_of(hi, hi, hi), 100.0);
    }
}
