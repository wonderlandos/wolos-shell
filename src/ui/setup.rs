// SPDX-License-Identifier: GPL-2.0-or-later
//! Setup Assistant (`wol-setup`, ADR 0004, ADR 0013): the welcome art, then one card per question
//! over the blurred wallpaper, the same glass as the login window and the lock screen. Runs as
//! `_wollogin` on first boot; Wi-Fi and the account go to wol-login over its socket. Return
//! continues, Escape goes back; Get Started fades to black into the desktop.
use crate::i18n::{tr, tr_fill};
use crate::kit::assistant::{self as a, Art, Card, Choice, ChoiceList, ROWS};
use crate::kit::{self, reveal, widgets as w};
use crate::login::{self, Verdict};
use crate::setup::{
    APPEARANCES, Appearance, Draft, Field, Flow, Hints, Job, KEYBOARDS, LANGUAGES, Next, STEPS,
    Step, THEMES, WifiPick, initials_or_placeholder,
};
use crate::system::{System, WifiNetwork};
use gtk::{glib, prelude::*};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The account form's fields: (field, label, secret).
fn fields() -> [(Field, &'static str, bool); 5] {
    [
        (Field::FullName, tr("Full name"), false),
        (Field::Name, tr("Account name"), false),
        (Field::Password, tr("Password"), true),
        (Field::Verify, tr("Verify"), true),
        (Field::Hint, tr("Hint"), false),
    ]
}

/// The Wi-Fi page's parts that change with each scan.
#[derive(Clone)]
struct WifiUi {
    holder: gtk::Box,
    pw: gtk::Widget,
    pw_row: gtk::Box,
}

/// "N of M" for a page after the welcome: the Secure Boot key screen is not counted (it shows only when a key
/// waits for an answer).
fn position(step: Step) -> Option<(usize, usize)> {
    let counted: Vec<Step> = STEPS[1..]
        .iter()
        .copied()
        .filter(|s| *s != Step::KeyScreen)
        .collect();
    let at = counted.iter().position(|s| *s == step)?;
    Some((at + 1, counted.len()))
}

/// The Language page of both assistants (setup and the installer), titled `title`/`subtitle`
/// (each assistant's own words): the language list, the
/// keyboard list under it, and with `try_it` a field to type into. `pick_language(i)` records
/// language `i` and returns the keyboard the draft now holds (None when the page is gone); the
/// keyboard list then follows it inside a guard, so `keyboard_shown(i, by_person)` tells a
/// person's pick (`true`, even of the suggested layout) from the page following a language
/// (`false`, already in the draft; audit3 R2B-8, B8). Every shown layout is applied to the
/// session at once so the keys type what the list says.
pub(crate) struct LanguagePage {
    pub card: Card,
    pub list: ChoiceList,
}

/// Rows Setup's language list shows before it scrolls: every official language (`LANGUAGES`; a test
/// keeps the two equal), so no one has to scroll to find theirs. The installer's list keeps
/// [`crate::kit::assistant::ROWS`]: its cards share the height of the tallest one, and two more rows
/// would lengthen every installer page past a 1024x600 screen.
pub const LANGUAGE_ROWS: i32 = 7;

pub(crate) fn language_page(
    (title, subtitle): (&str, &str),
    language: usize,
    keyboard: usize,
    try_it: bool,
    pick_language: impl Fn(usize) -> Option<usize> + 'static,
    keyboard_shown: impl Fn(usize, bool) + 'static,
) -> LanguagePage {
    let body = w::vbox(w::S4);
    let names: Vec<&str> = KEYBOARDS.iter().map(|k| k.1).collect();
    let kb = w::dropdown(&names, keyboard as u32);
    let filling = Rc::new(Cell::new(false));
    let rows = LANGUAGES
        .iter()
        .map(|l| Choice {
            icon: None,
            title: l.name.into(),
            detail: l.region.into(),
            trail: None,
        })
        .collect();
    let (dd, guard) = (kb.downgrade(), filling.clone());
    let shown = if try_it { ROWS } else { LANGUAGE_ROWS };
    let list = ChoiceList::with_rows(rows, Some(language), shown, move |i| {
        if let (Some(k), Some(dd)) = (pick_language(i), dd.upgrade()) {
            // The page moves the list: not the person's pick.
            guard.set(true);
            dd.set_selected(k as u32);
            guard.set(false);
        }
    });
    body.append(&list.root);
    kb.connect_selected_notify(move |dd| {
        let i = (dd.selected() as usize).min(KEYBOARDS.len() - 1);
        keyboard_shown(i, !filling.get());
        if std::env::var_os("WOL_FAKE_NOW").is_none() {
            kit::spawn(&crate::setup::keyboard_command(KEYBOARDS[i].0));
        }
    });
    body.append(&a::form_row(tr("Keyboard"), &kb));
    if try_it {
        // Somewhere to see that the keys type what they say, before any password does.
        let test = w::text_field(tr("Type here to try the keyboard"));
        test.add_css_class("assistant-field");
        body.append(&a::form_row(tr("Try it"), &test));
    }
    let card = Card::new(
        Art::Icon("preferences-desktop-locale-symbolic"),
        title,
        subtitle,
        &body,
    );
    LanguagePage { card, list }
}

/// `holder` shows `child` only.
fn replace_child(holder: &gtk::Box, child: &impl IsA<gtk::Widget>) {
    while let Some(c) = holder.first_child() {
        holder.remove(&c);
    }
    holder.append(child);
}

pub struct Setup {
    pub root: gtk::Widget,
    pager: gtk::Stack,
    flow: RefCell<Flow>,
    cards: RefCell<Vec<(Step, Card)>>,
    editors: RefCell<Vec<(Field, gtk::Widget)>>,
    caps: RefCell<Option<gtk::Label>>,
    avatar: RefCell<Option<gtk::Label>>,
    languages: RefCell<Option<ChoiceList>>,
    wifi_ui: RefCell<Option<WifiUi>>,
    /// The networks the Wi-Fi page shows last (a page built again in a new language keeps them).
    nets: RefCell<Vec<WifiNetwork>>,
    scanning: Cell<bool>,
    busy: Cell<bool>,
}

impl Setup {
    /// Language and keyboard from this machine's hints (none in golden renders).
    pub fn new(sys: &dyn System) -> Rc<Setup> {
        let hints = if std::env::var_os("WOL_FAKE_NOW").is_some() {
            Hints::default()
        } else {
            Hints::read(std::path::Path::new("/"))
        };
        Setup::with(sys, &hints)
    }

    pub fn with(sys: &dyn System, hints: &Hints) -> Rc<Setup> {
        let net = sys.network();
        a::set_header_icon("wolos-mark");
        let pager = a::pager();
        // The install queued this computer's key and the blue screen has not been answered yet.
        let key_pending = std::env::var_os("WOL_FAKE_NOW").is_none()
            && std::path::Path::new(crate::install::MOK_PENDING).exists();
        let s = Rc::new(Setup {
            root: super::lock::assistant_screen(&pager),
            pager,
            flow: RefCell::new(
                Flow::new(Draft::detected(hints), net.online && !net.wifi)
                    .with_key_pending(key_pending),
            ),
            cards: Default::default(),
            editors: Default::default(),
            caps: Default::default(),
            avatar: Default::default(),
            languages: Default::default(),
            wifi_ui: Default::default(),
            nets: Default::default(),
            scanning: Cell::new(false),
            busy: Cell::new(false),
        });
        // The welcome is full-bleed dark art: the mark and the word at the upper left take the art's light ink.
        {
            let root = s.root.downgrade();
            s.pager.connect_visible_child_name_notify(move |p| {
                if let Some(r) = root.upgrade() {
                    if p.visible_child_name().as_deref() == Some(Step::Welcome.id()) {
                        r.add_css_class("on-art");
                    } else {
                        r.remove_css_class("on-art");
                    }
                }
            });
        }
        // Setup speaks the language it starts on (the one the installer chose, or the machine's).
        let start = s.flow.borrow().draft.language;
        crate::i18n::set_lang(crate::i18n::lang_of(LANGUAGES[start].locale));
        if gtk::glib::getenv("WOL_LANG").is_some() {
            // Layout tests (`WOL_LANG=xx wol-setup`).
            crate::i18n::init();
        }
        *s.nets.borrow_mut() = sys.wifi_networks();
        s.root.add_css_class("on-art");
        s.build_cards();
        s
    }

    /// The welcome and every card after it, in the language in use; replaces the ones there.
    fn build_cards(self: &Rc<Self>) {
        let old: Vec<(Step, Card)> = self.cards.borrow_mut().drain(..).collect();
        for (_, c) in &old {
            self.pager.remove(&c.root);
        }
        self.editors.borrow_mut().clear();
        if let Some(old) = self.pager.child_by_name(Step::Welcome.id()) {
            self.pager.remove(&old);
        }
        self.pager
            .add_named(&self.welcome(), Some(Step::Welcome.id()));
        for st in &STEPS[1..] {
            let card = match st {
                Step::KeyScreen => self.key_screen(),
                Step::Language => self.language(),
                Step::Wifi => self.wifi(),
                Step::Account => self.account(),
                Step::Appearance => self.appearance(),
                _ => self.done(),
            };
            self.wire(*st, &card);
            self.pager.add_named(&card.root, Some(st.id()));
            self.cards.borrow_mut().push((*st, card));
        }
    }

    /// The person picked language row `i`: Setup speaks it from the next moment (QA-L10N-2,
    /// L10N-2). The cards are built again in it, after the click that picked it, with what was
    /// typed already; the language page stays in front.
    fn speak(self: &Rc<Self>, i: usize) {
        let lang = crate::i18n::lang_of(LANGUAGES[i.min(LANGUAGES.len() - 1)].locale);
        if lang == shared::i18n::lang() {
            return;
        }
        crate::i18n::set_lang(lang);
        let me = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(s) = me.upgrade() {
                s.relabel();
            }
        });
    }

    /// [`Setup::speak`]'s second half (goldens call it at once).
    pub fn relabel(self: &Rc<Self>) {
        let step = self.flow.borrow().step;
        self.build_cards();
        let d = self.flow.borrow().draft.clone();
        for (f, text) in [
            (Field::FullName, &d.full_name),
            (Field::Name, &d.name),
            (Field::Password, &d.password),
            (Field::Verify, &d.verify),
            (Field::Hint, &d.hint),
        ] {
            if !text.is_empty() {
                self.type_into(f, text);
            }
        }
        self.show(step);
    }

    /// Shows `step` at once (golden renders, tests).
    pub fn show(&self, step: Step) {
        self.flow.borrow_mut().step = step;
        self.pager
            .set_transition_type(gtk::StackTransitionType::None);
        self.pager.set_visible_child_name(step.id());
        self.pager
            .set_transition_type(gtk::StackTransitionType::Crossfade);
    }

    /// The Caps Lock warning, as the keyboard would set it (goldens).
    pub fn set_caps_lock(&self, on: bool) {
        if let Some(c) = &*self.caps.borrow() {
            c.set_visible(on);
        }
    }

    /// Types into an account field as the user would (goldens, tests).
    pub fn type_into(&self, f: Field, text: &str) {
        if let Some(e) = self.editor(f) {
            e.set_text(text);
        }
    }

    fn editor(&self, f: Field) -> Option<gtk::Editable> {
        self.editors
            .borrow()
            .iter()
            .find(|(g, _)| *g == f)
            .and_then(|(_, e)| e.clone().dynamic_cast::<gtk::Editable>().ok())
    }

    fn with_card(&self, step: Step, f: impl FnOnce(&Card)) {
        if let Some((_, c)) = self.cards.borrow().iter().find(|(s, _)| *s == step) {
            f(c)
        }
    }

    /// Back and Continue of a card.
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
        card.back.set_visible(step != Step::Done);
        card.set_step(position(step));
    }

    /// Continue (button or Return).
    pub fn next(self: &Rc<Self>) {
        if self.busy.get() {
            return;
        }
        let n = self.flow.borrow_mut().advance();
        self.apply(n);
    }

    /// Back (button or Escape).
    pub fn back(self: &Rc<Self>) {
        if self.busy.get() {
            return;
        }
        let from = self.flow.borrow().step;
        if let Some(p) = self.flow.borrow_mut().back() {
            self.with_card(from, |c| c.say("", false));
            self.pager.set_visible_child_name(p.id());
            self.entered(p);
        }
    }

    /// The step the flow is on.
    pub fn step(&self) -> Step {
        self.flow.borrow().step
    }

    /// `step` was just shown. The account card keeps what its form says (QA-LIVE-3): leaving it
    /// cleared the sentence and the outlined field while Continue stayed off for that very
    /// reason, so coming back showed a dead button and no why. The form is judged again.
    fn entered(&self, step: Step) {
        if step == Step::Account {
            self.show_typing_problem();
        }
    }

    fn apply(self: &Rc<Self>, n: Next) {
        let step = self.flow.borrow().step;
        self.clear_problems();
        match n {
            Next::Go(s) => {
                for (_, c) in self.cards.borrow().iter() {
                    c.say("", false);
                }
                self.pager.set_visible_child_name(s.id());
                self.entered(s);
                if s == Step::Wifi {
                    self.scan();
                }
                if s == Step::Account
                    && let Some(e) = self.editor(Field::FullName)
                {
                    e.grab_focus();
                }
            }
            Next::Stay(field, why) => {
                self.with_card(step, |c| c.say(&why, true));
                if let Some(f) = field {
                    self.mark(f);
                }
            }
            Next::Run(job) => self.run(job),
            Next::Finish => reveal::exit_faded(),
        }
    }

    /// Field with the problem: outlined and focused.
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

    /// Runs `job` through wol-login off the main thread, then lets the flow decide.
    fn run(self: &Rc<Self>, job: Job) {
        let step = self.flow.borrow().step;
        let (req, doing) = match &job {
            Job::JoinWifi { ssid, password } => (
                shared::login::encode_wifi(ssid, password),
                tr_fill("Joining “{network}”…", &[("network", ssid)]),
            ),
            Job::CreateAccount(acc) => (acc.encode(), tr("Setting up your account…").to_string()),
        };
        self.busy.set(true);
        self.with_card(step, |c| {
            c.say(&doing, false);
            c.set_busy(true);
        });
        let me = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let v = gtk::gio::spawn_blocking(move || login::request(login::LOGIN_SOCKET, &req))
                .await
                .unwrap_or(Verdict::Error);
            let Some(s) = me.upgrade() else {
                return;
            };
            s.busy.set(false);
            s.with_card(step, |c| c.set_busy(false));
            let n = s.flow.borrow_mut().job_done(&job, v);
            s.apply(n);
        });
    }

    fn welcome(self: &Rc<Self>) -> gtk::Widget {
        let me = Rc::downgrade(self);
        let go = w::button(tr("Continue"), w::Kind::Primary, move || {
            if let Some(s) = me.upgrade() {
                s.next()
            }
        });
        a::welcome(
            "setup-welcome",
            tr("Welcome to\nWolOS"),
            Step::Welcome.subtitle(),
            &go,
        )
    }

    fn language(self: &Rc<Self>) -> Card {
        let d = self.flow.borrow().draft.clone();
        let (me, me2) = (Rc::downgrade(self), Rc::downgrade(self));
        let page = language_page(
            (Step::Language.title(), Step::Language.subtitle()),
            d.language,
            d.keyboard,
            false,
            move |i| {
                let s = me.upgrade()?;
                s.flow.borrow_mut().draft.set_language(i);
                let k = s.flow.borrow().draft.keyboard;
                s.speak(i);
                Some(k)
            },
            move |i, by_person| {
                // Only the person's pick stays through language changes; the page following a
                // language is already in the draft (dedup A9, as the installer's R2B-8/B8).
                if let Some(s) = me2.upgrade()
                    && by_person
                {
                    s.flow.borrow_mut().draft.set_keyboard(i);
                }
            },
        );
        *self.languages.borrow_mut() = Some(page.list);
        page.card
    }

    /// Picks language row `i` as the person would (goldens, tests).
    pub fn pick_language(&self, i: usize) {
        if let Some(l) = &*self.languages.borrow() {
            l.pick(i);
        }
    }

    /// The draft's (language, keyboard) indexes (goldens, tests).
    pub fn language_and_keyboard(&self) -> (usize, usize) {
        let f = self.flow.borrow();
        (f.draft.language, f.draft.keyboard)
    }

    fn wifi(self: &Rc<Self>) -> Card {
        let body = w::vbox(w::S4);
        let holder = w::vbox(0);
        holder.set_vexpand(true);
        body.append(&holder);
        let pw = w::password_field(tr("Network password"));
        // The Caps Lock note under the field, inside its row (hidden with it).
        let field = w::vbox(w::S1);
        field.set_hexpand(true);
        field.append(&pw);
        let caps = w::caps_lock_note();
        caps.set_halign(gtk::Align::Start);
        field.append(&caps);
        let pw_row = a::form_row(tr("Password"), &field);
        pw_row.set_visible(false);
        body.append(&pw_row);
        let me = Rc::downgrade(self);
        pw.connect_changed(move |e| {
            if let Some(s) = me.upgrade()
                && let Some(wp) = &mut s.flow.borrow_mut().wifi
            {
                wp.password = e.text().to_string();
            }
        });
        *self.wifi_ui.borrow_mut() = Some(WifiUi {
            holder,
            pw: pw.upcast(),
            pw_row,
        });
        // What the system already knows; entering the page scans again (wol-login).
        let nets = self.nets.borrow().clone();
        self.show_networks(nets);
        let card = Card::new(
            Art::Icon("network-wireless-symbolic"),
            Step::Wifi.title(),
            Step::Wifi.subtitle(),
            &body,
        );
        let me = Rc::downgrade(self);
        card.add_action(&w::button(tr("Not Now"), w::Kind::Plain, move || {
            if let Some(s) = me.upgrade()
                && !s.busy.get()
            {
                let n = s.flow.borrow_mut().skip_wifi();
                s.apply(n);
            }
        }));
        card
    }

    /// Scans through wol-login (a few seconds, spinner meanwhile), then lists what it heard.
    fn scan(self: &Rc<Self>) {
        if self.scanning.replace(true) {
            return;
        }
        if let Some(ui) = &*self.wifi_ui.borrow() {
            replace_child(&ui.holder, &a::waiting(tr("Looking for networks…")));
            ui.pw_row.set_visible(false);
        }
        let me = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let found = gtk::gio::spawn_blocking(|| login::wifi_scan(login::LOGIN_SOCKET))
                .await
                .ok()
                .flatten()
                .unwrap_or_default();
            let Some(s) = me.upgrade() else {
                return;
            };
            s.scanning.set(false);
            s.show_networks(
                found
                    .into_iter()
                    .map(|n| WifiNetwork {
                        ssid: n.ssid,
                        signal: n.signal,
                        secure: n.secure,
                        connected: n.connected,
                    })
                    .collect(),
            );
        });
    }

    /// The network list (or how to find one); the joined network is the pick.
    fn show_networks(self: &Rc<Self>, nets: Vec<WifiNetwork>) {
        self.nets.borrow_mut().clone_from(&nets);
        let Some(ui) = self.wifi_ui.borrow().as_ref().map(WifiUi::clone) else {
            return;
        };
        self.flow.borrow_mut().wifi = nets.iter().find(|n| n.connected).map(|n| WifiPick {
            ssid: n.ssid.clone(),
            secure: n.secure,
            connected: true,
            password: String::new(),
        });
        ui.pw_row.set_visible(false);
        if nets.is_empty() {
            let me = Rc::downgrade(self);
            let again = w::button(tr("Look Again"), w::Kind::Default, move || {
                if let Some(s) = me.upgrade() {
                    s.scan()
                }
            });
            replace_child(
                &ui.holder,
                &w::empty_state(
                    "network-wireless-offline-symbolic",
                    tr("No networks found"),
                    tr("Move closer to your Wi-Fi and look again, or choose Not Now."),
                    Some(again),
                ),
            );
            return;
        }
        let rows = nets
            .iter()
            .map(|n| Choice {
                icon: Some(
                    crate::status::network(crate::status::Network {
                        online: true,
                        wifi: true,
                        signal: Some(n.signal),
                        limited: false,
                    })
                    .into(),
                ),
                title: n.ssid.clone(),
                detail: String::new(),
                trail: n
                    .secure
                    .then(|| "network-wireless-encrypted-symbolic".into()),
            })
            .collect();
        let selected = nets.iter().position(|n| n.connected);
        let me = Rc::downgrade(self);
        let list = ChoiceList::new(rows, selected, move |i| {
            let Some(s) = me.upgrade() else {
                return;
            };
            let n = &nets[i];
            let pw = ui.pw.clone().dynamic_cast::<gtk::Editable>().ok();
            s.flow.borrow_mut().wifi = Some(WifiPick {
                ssid: n.ssid.clone(),
                secure: n.secure,
                connected: n.connected,
                password: pw
                    .as_ref()
                    .map(|p| p.text().to_string())
                    .unwrap_or_default(),
            });
            ui.pw_row.set_visible(n.secure && !n.connected);
            if ui.pw_row.is_visible() {
                ui.pw.grab_focus();
            }
        });
        if let Some(ui) = &*self.wifi_ui.borrow() {
            replace_child(&ui.holder, &list.root);
        }
    }

    fn account(self: &Rc<Self>) -> Card {
        let me = Rc::downgrade(self);
        let picker = a::AvatarPicker::new(0, move |i| {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().draft.avatar = i;
            }
        });
        picker.avatar.set_label(&initials_or_placeholder(""));
        *self.avatar.borrow_mut() = Some(picker.avatar.clone());
        let form = w::vbox(w::S2);
        for (f, title, secret) in fields() {
            let e: gtk::Widget = if secret {
                w::password_field("").upcast()
            } else {
                w::text_field("").upcast()
            };
            if f == Field::Hint {
                e.set_property("placeholder-text", tr(crate::login::HINT_PLACEHOLDER));
            }
            e.add_css_class("assistant-field");
            form.append(&a::form_row(title, &e));
            self.editors.borrow_mut().push((f, e));
        }
        // Under Verify, the last password field.
        let caps = w::caps_lock_note();
        caps.set_halign(gtk::Align::Start);
        let caps_row = a::form_row("", &caps);
        // The row follows the label: hidden with it, so no empty gap.
        caps.bind_property("visible", &caps_row, "visible")
            .sync_create()
            .build();
        form.insert_child_after(
            &caps_row,
            form.observe_children()
                .item(3)
                .and_downcast_ref::<gtk::Widget>(),
        );
        *self.caps.borrow_mut() = Some(caps);
        let note = a::caption(tr(
            "This account is the administrator: it can install apps and change any setting.",
        ));
        note.set_wrap(true);
        form.append(&note);
        for (f, e) in self.editors.borrow().iter() {
            let Ok(e) = e.clone().dynamic_cast::<gtk::Editable>() else {
                continue;
            };
            let (f, me) = (*f, Rc::downgrade(self));
            e.connect_changed(move |e| {
                let Some(s) = me.upgrade() else {
                    return;
                };
                let t = e.text().to_string();
                let derived = {
                    let d = &mut s.flow.borrow_mut().draft;
                    match f {
                        Field::FullName => d.set_full_name(&t),
                        Field::Name => d.set_name(&t),
                        Field::Password => d.password = t.clone(),
                        Field::Verify => d.verify = t.clone(),
                        Field::Hint => d.hint = t.clone(),
                        Field::Hostname => {}
                    }
                    d.name.clone()
                };
                s.show_typing_problem();
                if f == Field::FullName {
                    if let Some(av) = &*s.avatar.borrow() {
                        av.set_label(&initials_or_placeholder(&t));
                    }
                    if let Some(nf) = s.editor(Field::Name)
                        && nf.text() != derived
                    {
                        nf.set_text(&derived);
                    }
                }
            });
        }
        let card = Card::new(
            Art::Widget(picker.root.upcast()),
            Step::Account.title(),
            Step::Account.subtitle(),
            &form,
        );
        card.next.set_sensitive(false);
        card
    }

    /// Account card while typing: the field that is already wrong is outlined and its sentence
    /// shows; Continue works only once the whole form is valid.
    fn show_typing_problem(&self) {
        let (live, ready) = {
            let d = &self.flow.borrow().draft;
            (d.typing_problem(), d.account_problem().is_none())
        };
        for (g, e) in self.editors.borrow().iter() {
            if live.is_some_and(|(f, _)| f == *g) {
                e.add_css_class("problem");
            } else {
                e.remove_css_class("problem");
            }
        }
        self.with_card(Step::Account, |c| {
            c.say(live.map_or("", |p| p.1), live.is_some());
            c.next.set_sensitive(ready);
        });
    }

    fn appearance(self: &Rc<Self>) -> Card {
        let body = w::vbox(w::S4);
        let light = crate::theme::resolve(Some("wol-light"), false);
        let dark = crate::theme::resolve(Some("wol-dark"), true);
        let selected = self.flow.borrow().draft.appearance;
        let modes: Vec<gtk::ToggleButton> = APPEARANCES
            .iter()
            .map(|ap| {
                let slices = match ap {
                    Appearance::Light => vec![w::Palette::of(&light)],
                    Appearance::Dark => vec![w::Palette::of(&dark)],
                    Appearance::Auto => vec![w::Palette::of(&light), w::Palette::of(&dark)],
                };
                let t = w::theme_preview(slices, ap.title(), *ap == selected);
                let (me, ap) = (Rc::downgrade(self), *ap);
                t.connect_toggled(move |t| {
                    if let Some(s) = me.upgrade()
                        && t.is_active()
                    {
                        s.flow.borrow_mut().draft.appearance = ap;
                        s.preview_look();
                    }
                });
                t
            })
            .collect();
        body.append(&a::tile_row(&modes));
        let dark_now = super::lock::prefers_dark();
        let looks: Vec<gtk::ToggleButton> = THEMES
            .iter()
            .enumerate()
            .map(|(i, (accent, title))| {
                let id = wol_core::theme::accent::theme_id(
                    Some(if dark_now {
                        shared::theme::Variant::Dark
                    } else {
                        shared::theme::Variant::Light
                    }),
                    accent,
                );
                let t = crate::theme::resolve(id.as_deref(), dark_now);
                let tile = w::theme_preview(vec![w::Palette::of(&t)], title, i == 0);
                let me = Rc::downgrade(self);
                tile.connect_toggled(move |t| {
                    if let Some(s) = me.upgrade()
                        && t.is_active()
                    {
                        s.flow.borrow_mut().draft.theme = i;
                        s.preview_look();
                    }
                });
                tile
            })
            .collect();
        body.append(&a::tile_row(&looks));
        Card::new(
            Art::Icon("preferences-desktop-appearance-symbolic"),
            Step::Appearance.title(),
            Step::Appearance.subtitle(),
            &body,
        )
    }

    /// The assistant itself takes the chosen look.
    fn preview_look(&self) {
        let d = self.flow.borrow().draft.clone();
        let variant = match d.appearance {
            Appearance::Light => Some(shared::theme::Variant::Light),
            Appearance::Dark => Some(shared::theme::Variant::Dark),
            Appearance::Auto => None,
        };
        let dark = match d.appearance {
            Appearance::Light => false,
            Appearance::Dark => true,
            Appearance::Auto => super::lock::prefers_dark(),
        };
        let id = wol_core::theme::accent::theme_id(variant, d.accent());
        kit::style::apply(&crate::theme::resolve(id.as_deref(), dark), "");
    }

    /// What to do on the firmware's blue key screen at the next restart: the steps, numbered, and
    /// the password in large type. The same words the installer's Done page showed.
    fn key_screen(self: &Rc<Self>) -> Card {
        let body = w::vbox(w::S2);
        for (i, step) in crate::copy::installer::key_screen_steps()
            .iter()
            .enumerate()
        {
            let l = a::wrapped(&format!("{}. {step}", i + 1), "assistant-subtitle");
            l.set_xalign(0.0);
            l.set_justify(gtk::Justification::Left);
            l.set_halign(gtk::Align::Start);
            body.append(&l);
        }
        body.append(&w::centered(wol_core::boot::MOK_PASSWORD, "large-title"));
        body.append(&a::caption(tr(
            "You can also start this later from System Settings > Privacy & Security.",
        )));
        Card::moment(
            "keyhole",
            Step::KeyScreen.title(),
            Step::KeyScreen.subtitle(),
            &body,
        )
    }

    /// No Back: the account exists once this page shows.
    fn done(self: &Rc<Self>) -> Card {
        let c = Card::moment(
            "keyhole",
            Step::Done.title(),
            Step::Done.subtitle(),
            &a::blank(),
        );
        c.next.set_label(tr("Get Started"));
        c
    }
}

pub fn run() -> glib::ExitCode {
    a::run("org.wolos.setup", "setup", |win| {
        let s = Setup::new(&*crate::system::default_system());
        // Fades in from the compositor's black; Get Started fades out to it (ADR 0012).
        let shown = reveal::Reveal::new(&s.root, true);
        win.set_child(Some(&shown.root));
        shown.show_when(super::lock::backdrop_ready, || {
            reveal::show_pointer();
            reveal::boot_mark("setup shown");
        });
        let k = s.flow.borrow().draft.keyboard;
        if KEYBOARDS[k].0 != KEYBOARDS[0].0 {
            kit::spawn(&crate::setup::keyboard_command(KEYBOARDS[k].0));
        }
        // The key controller keeps the assistant alive with its window.
        let s2 = s.clone();
        a::keys(win, move || s.next(), move || s2.back());
    })
}
