// SPDX-License-Identifier: GPL-2.0-or-later
//! First sign-in Setup (`wol-setup --first-login`, `crate::first_login`): one card per question
//! over the blurred wallpaper, the same glass and foot as the install-time Setup, with Later next
//! to Continue. Runs as the person, in their own session, before the shell starts (the
//! compositor's start line, `hyprland.lua`); Return continues, Escape goes back.
use crate::first_login::{
    Choices, Flow, Job, Marks, Next, OnlineAccount, PAGES, Page, keyboard_index, location_argv,
    location_of,
};
use crate::i18n::tr;
use crate::kit::assistant::{self as a, Art, Card};
use crate::kit::{self, reveal, widgets as w};
use crate::setup::{APPEARANCES, Appearance, KEYBOARDS};
use crate::system::{Action, ActionError, System};
use gtk::{glib, prelude::*};
use shared::theme::Variant;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

/// Opened by the person (Settings > General): runs even after the last Later.
pub const AGAIN: &str = "--again";

/// What the pages start from: the choices on this computer and the facts around them.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    pub choices: Choices,
    /// Error reports are sent now (shown read-only to an account that can't change them).
    pub reports_on: bool,
    /// The input method module is installed.
    pub ime: bool,
}

fn fake() -> bool {
    std::env::var_os("WOL_FAKE_NOW").is_some()
}

impl Facts {
    /// This account's choices as the computer holds them (defaults in golden renders).
    pub fn read(sys: &dyn System) -> Facts {
        if fake() {
            return Facts::default();
        }
        let reports_on = std::fs::read_to_string(wol_core::admin::REPORTS_CONF)
            .is_ok_and(|t| t.lines().any(|l| l.trim() == "send=1"));
        let admin = wol_core::admin::this_account_is_admin(crate::envv::var("USER").as_deref());
        let location = match std::process::Command::new("gsettings")
            .args(["get", "org.gnome.system.location", "enabled"])
            .output()
        {
            Ok(o) => location_of(&String::from_utf8_lossy(&o.stdout)),
            Err(e) => {
                crate::diag::say!("wol-setup: location is off, gsettings did not run: {e}");
                false
            }
        };
        let saved = kit::style::active_id();
        let (mode, accent) = wol_core::theme::accent::split(saved.as_deref());
        let prefs = wol_core::hyprprefs::load_desktop(std::path::Path::new(&kit::config_home()));
        Facts {
            choices: Choices {
                reports: admin.then_some(reports_on),
                location,
                mode,
                accent: accent.into(),
                wallpaper: kit::pickers::wallpaper_choice().unwrap_or_default(),
                keyboard: keyboard_index(prefs.layouts.first().map(String::as_str)),
                helpers: sys.helpers().enabled,
            },
            reports_on,
            ime: std::path::Path::new("/usr/bin/fcitx5").exists(),
        }
    }
}

/// Saves `job` in this session. Location goes first: it needs no password, so a reports change
/// that waits for one leaves it saved (`Flow::job_done` counts on it).
async fn save(sys: Arc<dyn System>, job: Job, choices: Choices) -> Result<(), ActionError> {
    match job {
        Job::Privacy {
            reports,
            location,
            password,
        } => {
            if let Some(on) = location {
                let ok = gtk::gio::spawn_blocking(move || {
                    let argv = location_argv(on);
                    match std::process::Command::new(&argv[0])
                        .args(&argv[1..])
                        .status()
                    {
                        Ok(s) => s.success(),
                        Err(e) => {
                            crate::diag::say!("wol-setup: {} did not run: {e}", argv[0]);
                            false
                        }
                    }
                })
                .await
                .unwrap_or(false);
                if !ok {
                    return Err(ActionError::Failed);
                }
            }
            if let Some(on) = reports {
                let req = shared::admin::AdminRequest {
                    action: shared::admin::AdminAction::Reports,
                    args: vec![if on { "on" } else { "off" }.into()],
                };
                return sys
                    .spawn_with(Action::Admin(req), password.as_deref())
                    .recv()
                    .await
                    .unwrap_or(Err(ActionError::Failed));
            }
            Ok(())
        }
        Job::Look { theme, wallpaper } => {
            let r = match theme {
                Some(Some(id)) => kit::style::set_active(&id),
                Some(None) => crate::theme::clear(),
                None => Ok(()),
            };
            r.map_err(|_| ActionError::Failed)?;
            if let Some(choice) = wallpaper {
                let text = (!choice.is_empty()).then(|| format!("{choice}\n"));
                wol_core::journal::Journal::for_config(std::path::Path::new(&kit::config_home()))
                    .save(
                        "wallpaper",
                        "setup",
                        &[(shared::theme::WALLPAPER_FILE, text.as_deref())],
                        wol_core::journal::now(),
                    )
                    .map_err(|_| ActionError::Failed)?;
            }
            Ok(())
        }
        Job::Keyboard(_) => {
            let mut prefs =
                wol_core::hyprprefs::load_desktop(std::path::Path::new(&kit::config_home()));
            prefs.layouts = choices.layouts(&prefs.layouts);
            sys.spawn(Action::Desktop(Box::new(prefs)))
                .recv()
                .await
                .unwrap_or(Err(ActionError::Failed))
        }
        Job::Helpers(on) => sys.set_helpers_enabled(on),
    }
}

/// The accounts GNOME Online Accounts holds for this person (Mail and Calendar read them).
/// None when the service did not answer.
fn online_accounts() -> Option<Vec<OnlineAccount>> {
    let conn = zbus::blocking::Connection::session().ok()?;
    let om = zbus::blocking::fdo::ObjectManagerProxy::builder(&conn)
        .destination("org.gnome.OnlineAccounts")
        .ok()?
        .path("/org/gnome/OnlineAccounts")
        .ok()?
        .build()
        .ok()?;
    let objects = om.get_managed_objects().ok()?;
    let text = |props: &std::collections::HashMap<String, zbus::zvariant::OwnedValue>, k: &str| {
        props
            .get(k)
            .and_then(|v| String::try_from(v.clone()).ok())
            .unwrap_or_default()
    };
    let mut v: Vec<OnlineAccount> = objects
        .values()
        .filter_map(|ifaces| {
            let props = ifaces
                .iter()
                .find(|(n, _)| n.as_str() == "org.gnome.OnlineAccounts.Account")?
                .1;
            Some(OnlineAccount {
                provider: text(props, "ProviderName"),
                identity: text(props, "PresentationIdentity"),
            })
        })
        .collect();
    v.sort_by(|a, b| (&a.provider, &a.identity).cmp(&(&b.provider, &b.identity)));
    Some(v)
}

pub struct FirstLogin {
    pub root: gtk::Widget,
    pager: gtk::Stack,
    sys: Arc<dyn System>,
    marks: Marks,
    flow: RefCell<Flow>,
    cards: RefCell<Vec<(Page, Card)>>,
    /// The Privacy page's password row (hidden until error reports ask), its field and the line
    /// under it.
    report_row: RefCell<Option<(gtk::Box, gtk::PasswordEntry, gtk::Label)>>,
    accounts: gtk::Box,
    busy: Cell<bool>,
}

impl FirstLogin {
    pub fn new(sys: Arc<dyn System>) -> Rc<FirstLogin> {
        let facts = Facts::read(&*sys);
        FirstLogin::with(sys, &facts)
    }

    pub fn with(sys: Arc<dyn System>, facts: &Facts) -> Rc<FirstLogin> {
        a::set_header_icon("wolos-mark");
        let pager = a::pager();
        let s = Rc::new(FirstLogin {
            root: super::lock::assistant_screen(&pager),
            pager,
            sys,
            marks: Marks::here(),
            flow: RefCell::new(Flow::new(facts.choices.clone())),
            cards: Default::default(),
            report_row: Default::default(),
            accounts: w::vbox(w::S2),
            busy: Cell::new(false),
        });
        for p in PAGES {
            let card = match p {
                Page::Privacy => s.privacy(facts),
                Page::Look => s.look(),
                Page::Accounts => s.accounts(),
                Page::Keyboard => s.keyboard(facts),
                Page::Helper => s.helper(),
                Page::Done => s.done(),
            };
            s.wire(p, &card);
            s.pager.add_named(&card.root, Some(p.id()));
            s.cards.borrow_mut().push((p, card));
        }
        s.fill_accounts();
        s
    }

    /// Shows `page` at once (golden renders, tests).
    pub fn show(&self, page: Page) {
        self.flow.borrow_mut().page = page;
        self.pager
            .set_transition_type(gtk::StackTransitionType::None);
        self.pager.set_visible_child_name(page.id());
        self.pager
            .set_transition_type(gtk::StackTransitionType::Crossfade);
    }

    /// The Privacy page asking for the password error reports need, as after a Continue that
    /// answered `NeedsAuth` (goldens).
    pub fn ask_password(self: &Rc<Self>) {
        self.flow.borrow_mut().asking = true;
        self.apply(Next::Password(
            crate::copy::first_login::REPORTS_PASSWORD.into(),
        ));
    }

    /// The page the flow is on.
    pub fn page(&self) -> Page {
        self.flow.borrow().page
    }

    fn with_card(&self, page: Page, f: impl FnOnce(&Card)) {
        if let Some((_, c)) = self.cards.borrow().iter().find(|(p, _)| *p == page) {
            f(c)
        }
    }

    /// Back, Later and Continue of a card, and its "N of M".
    fn wire(self: &Rc<Self>, page: Page, card: &Card) {
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
        card.back.set_visible(page != Page::Privacy);
        if page != Page::Done {
            let me = Rc::downgrade(self);
            card.add_action(&w::button(tr("Later"), w::Kind::Plain, move || {
                if let Some(s) = me.upgrade()
                    && !s.busy.get()
                {
                    let n = s.flow.borrow().later();
                    s.apply(n);
                }
            }));
        }
        card.set_step(Some(page.position()));
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
        let from = self.page();
        if let Some(p) = self.flow.borrow_mut().back() {
            self.with_card(from, |c| c.say("", false));
            self.pager.set_visible_child_name(p.id());
        }
    }

    fn apply(self: &Rc<Self>, n: Next) {
        let page = self.page();
        match n {
            Next::Go(p) => {
                self.with_card(page, |c| c.say("", false));
                self.pager.set_visible_child_name(p.id());
            }
            Next::Stay(why) => self.with_card(page, |c| c.say(tr(&why), true)),
            Next::Password(why) => {
                // Under the field, not at the foot: the foot holds Later and Continue.
                self.with_card(page, |c| c.say("", false));
                if let Some((row, e, line)) = &*self.report_row.borrow() {
                    let wrong = why != crate::copy::first_login::REPORTS_PASSWORD;
                    line.set_label(tr(&why));
                    line.set_visible(true);
                    if wrong {
                        line.add_css_class("problem");
                    } else {
                        line.remove_css_class("problem");
                    }
                    row.set_visible(true);
                    e.set_text("");
                    e.grab_focus();
                }
            }
            Next::Run(job) => self.run(job),
            Next::Finish => {
                if let Err(e) = self.marks.finish(wol_core::journal::now()) {
                    crate::diag::say!("wol-setup: first sign-in: the done mark: {e}");
                }
                reveal::exit_faded();
            }
            Next::Later => {
                if let Err(e) = self.marks.later() {
                    crate::diag::say!("wol-setup: first sign-in: the later count: {e}");
                }
                reveal::exit_faded();
            }
        }
    }

    /// Saves `job` without blocking the page, then lets the flow decide.
    fn run(self: &Rc<Self>, job: Job) {
        let page = self.page();
        self.busy.set(true);
        self.with_card(page, |c| {
            c.say("", false);
            c.set_busy(true);
        });
        let me = Rc::downgrade(self);
        let (sys, choices) = (self.sys.clone(), self.flow.borrow().draft.clone());
        glib::spawn_future_local(async move {
            let r = save(sys, job.clone(), choices).await;
            let Some(s) = me.upgrade() else {
                return;
            };
            s.busy.set(false);
            s.with_card(page, |c| c.set_busy(false));
            if r.is_ok()
                && let Some((row, ..)) = &*s.report_row.borrow()
            {
                row.set_visible(false);
            }
            let n = s.flow.borrow_mut().job_done(&job, r);
            s.apply(n);
        });
    }

    fn privacy(self: &Rc<Self>, facts: &Facts) -> Card {
        use crate::copy::first_login as c;
        let d = self.flow.borrow().draft.clone();
        let mut rows: Vec<gtk::Widget> = Vec::new();
        let reports: gtk::Widget = match d.reports {
            Some(on) => {
                let me = Rc::downgrade(self);
                w::toggle(on, move |v| {
                    if let Some(s) = me.upgrade() {
                        s.flow.borrow_mut().draft.reports = Some(v);
                    }
                })
                .upcast()
            }
            None => w::label(tr(if facts.reports_on { "On" } else { "Off" }), "secondary").upcast(),
        };
        rows.push(
            w::list_row(
                "dialog-error-symbolic",
                tr("Send error reports"),
                tr(if d.reports.is_some() {
                    c::REPORTS_HINT
                } else {
                    c::REPORTS_NOT_YOURS
                }),
                Some(&reports),
            )
            .upcast(),
        );
        let me = Rc::downgrade(self);
        let location = w::toggle(d.location, move |v| {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().draft.location = v;
            }
        });
        rows.push(
            w::list_row(
                "find-location-symbolic",
                tr("Location Services"),
                tr(c::LOCATION_HINT),
                Some(location.upcast_ref()),
            )
            .upcast(),
        );
        let body = w::vbox(w::S4);
        body.append(&w::section("", &rows));
        let pw = w::password_field(tr("Your password"));
        pw.add_css_class("assistant-field");
        let me = Rc::downgrade(self);
        pw.connect_changed(move |e| {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().password = e.text().to_string();
            }
        });
        let pw_row = w::vbox(w::S1);
        pw_row.append(&a::form_row(tr("Password"), &pw));
        // Caps Lock is said in words under every password field (tests/password_peek.rs).
        let caps = w::caps_lock_note();
        caps.set_halign(gtk::Align::Start);
        pw_row.append(&a::form_row("", &caps));
        let line = a::caption("");
        line.set_halign(gtk::Align::Start);
        line.set_xalign(0.0);
        line.set_visible(false);
        pw_row.append(&a::form_row("", &line));
        pw_row.set_visible(false);
        body.append(&pw_row);
        *self.report_row.borrow_mut() = Some((pw_row, pw, line));
        Card::new(
            Art::Icon("preferences-system-privacy-symbolic"),
            Page::Privacy.title(),
            Page::Privacy.subtitle(),
            &body,
        )
    }

    fn look(self: &Rc<Self>) -> Card {
        let body = w::vbox(w::S4);
        let d = self.flow.borrow().draft.clone();
        let light = crate::theme::resolve(Some("wol-light"), false);
        let dark = crate::theme::resolve(Some("wol-dark"), true);
        let picked = match d.mode {
            Some(Variant::Light) => Appearance::Light,
            Some(Variant::Dark) => Appearance::Dark,
            None => Appearance::Auto,
        };
        let modes: Vec<gtk::ToggleButton> = APPEARANCES
            .iter()
            .map(|ap| {
                let slices = match ap {
                    Appearance::Light => vec![w::Palette::of(&light)],
                    Appearance::Dark => vec![w::Palette::of(&dark)],
                    Appearance::Auto => vec![w::Palette::of(&light), w::Palette::of(&dark)],
                };
                let t = w::theme_preview(slices, ap.title(), *ap == picked);
                let (me, ap) = (Rc::downgrade(self), *ap);
                t.connect_toggled(move |t| {
                    if let Some(s) = me.upgrade()
                        && t.is_active()
                    {
                        s.flow.borrow_mut().draft.mode = match ap {
                            Appearance::Light => Some(Variant::Light),
                            Appearance::Dark => Some(Variant::Dark),
                            Appearance::Auto => None,
                        };
                        s.preview_look();
                    }
                });
                t
            })
            .collect();
        body.append(&a::tile_row(&modes));
        // The accent colours, in the colours of the variant on screen.
        let variant = match d.mode {
            Some(v) => v,
            None if super::lock::prefers_dark() => Variant::Dark,
            None => Variant::Light,
        };
        let swatches = w::wrap_row(w::S2, 8);
        swatches.set_halign(gtk::Align::Center);
        let mut first: Option<gtk::ToggleButton> = None;
        for (id, name, ..) in wol_core::theme::accent::ACCENTS {
            let hex = wol_core::theme::accent::color(variant, id).unwrap_or_default();
            let c = shared::theme::Color::parse(hex).unwrap_or(shared::theme::Color::WHITE);
            let dot = shared::theme::Color::parse(wol_core::theme::accent::text_on(hex))
                .unwrap_or(shared::theme::Color::WHITE);
            let sw = w::accent_swatch(c, dot, name, id == d.accent);
            match &first {
                Some(f) => sw.set_group(Some(f)),
                None => first = Some(sw.clone()),
            }
            let me = Rc::downgrade(self);
            sw.connect_toggled(move |t| {
                if let Some(s) = me.upgrade()
                    && t.is_active()
                {
                    s.flow.borrow_mut().draft.accent = id.to_string();
                    s.preview_look();
                }
            });
            swatches.insert(&sw, -1);
        }
        body.append(&swatches);
        // The shipped pictures, the saved one checked (the default dusk when none). Three fit the
        // card; Dynamic and your own photo are in Settings > Wallpaper.
        use kit::pickers::{DYNAMIC, shipped_wallpaper};
        let pics: Vec<(String, std::path::PathBuf, &str)> = DYNAMIC
            .iter()
            .zip([tr("Day"), tr("Dusk"), tr("Night")])
            .filter_map(|(n, name)| {
                let f = shipped_wallpaper(n)?;
                Some((f.display().to_string(), f, name))
            })
            .collect();
        let saved = if d.wallpaper.is_empty() {
            pics.get(1).map(|p| p.0.clone()).unwrap_or_default()
        } else {
            d.wallpaper.clone()
        };
        let tiles: Vec<(gtk::ToggleButton, String)> = pics
            .iter()
            .map(|(choice, file, name)| {
                (
                    kit::form::wallpaper_tile(Some(file), name, saved == *choice),
                    choice.clone(),
                )
            })
            .collect();
        let first = tiles.first().map(|t| t.0.clone());
        let mut row = Vec::new();
        for (t, choice) in tiles {
            if let Some(f) = first.as_ref().filter(|f| **f != t) {
                t.set_group(Some(f));
            }
            let me = Rc::downgrade(self);
            t.connect_toggled(move |t| {
                if let Some(s) = me.upgrade()
                    && t.is_active()
                {
                    s.flow.borrow_mut().draft.wallpaper = choice.clone();
                }
            });
            row.push(t);
        }
        body.append(&a::tile_row(&row));
        body.append(&a::caption(tr(crate::copy::first_login::LOOK_MORE)));
        Card::new(
            Art::Icon("preferences-desktop-appearance-symbolic"),
            Page::Look.title(),
            Page::Look.subtitle(),
            &body,
        )
    }

    /// The setup itself takes the look being picked.
    fn preview_look(&self) {
        let d = self.flow.borrow().draft.clone();
        let dark = match d.mode {
            Some(v) => v == Variant::Dark,
            None => super::lock::prefers_dark(),
        };
        kit::style::apply(&crate::theme::resolve(d.theme_id().as_deref(), dark), "");
    }

    fn accounts(self: &Rc<Self>) -> Card {
        use crate::copy::first_login as c;
        let body = w::vbox(w::S4);
        body.append(&self.accounts);
        let mail = kit::form::button_row(tr("Mail"), tr(c::ACCOUNTS_MAIL), tr("Open Mail"), || {
            if !fake() {
                kit::spawn_app(&["geary".to_string()]);
            }
        });
        body.append(&w::section("", &[mail]));
        Card::new(
            Art::Icon("system-users-symbolic"),
            Page::Accounts.title(),
            Page::Accounts.subtitle(),
            &body,
        )
    }

    /// The account list, read off the UI thread (one sample account in golden renders).
    fn fill_accounts(self: &Rc<Self>) {
        let show = |holder: &gtk::Box, found: Option<Vec<OnlineAccount>>| {
            while let Some(c) = holder.first_child() {
                holder.remove(&c);
            }
            let rows: Vec<gtk::Widget> = match &found {
                Some(v) if !v.is_empty() => v
                    .iter()
                    .map(|acc| {
                        w::list_row(
                            "avatar-default-symbolic",
                            &acc.identity,
                            &acc.provider,
                            None,
                        )
                        .upcast()
                    })
                    .collect(),
                Some(_) => vec![
                    w::list_row(
                        "avatar-default-symbolic",
                        tr(crate::copy::first_login::ACCOUNTS_NONE),
                        "",
                        None,
                    )
                    .upcast(),
                ],
                None => vec![
                    w::list_row(
                        "dialog-error-symbolic",
                        tr(crate::copy::first_login::ACCOUNTS_UNREAD),
                        "",
                        None,
                    )
                    .upcast(),
                ],
            };
            holder.append(&w::section("", &rows));
        };
        if fake() {
            show(
                &self.accounts,
                Some(vec![OnlineAccount {
                    provider: "Google".into(),
                    identity: "alice@example.com".into(),
                }]),
            );
            return;
        }
        let holder = self.accounts.downgrade();
        glib::spawn_future_local(async move {
            let found = gtk::gio::spawn_blocking(online_accounts)
                .await
                .ok()
                .flatten();
            if let Some(h) = holder.upgrade() {
                show(&h, found);
            }
        });
    }

    fn keyboard(self: &Rc<Self>, facts: &Facts) -> Card {
        use crate::copy::first_login as c;
        let body = w::vbox(w::S4);
        let names: Vec<&str> = KEYBOARDS.iter().map(|k| k.1).collect();
        let kb = w::dropdown(&names, self.flow.borrow().draft.keyboard as u32);
        let me = Rc::downgrade(self);
        kb.connect_selected_notify(move |dd| {
            let i = (dd.selected() as usize).min(KEYBOARDS.len() - 1);
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().draft.keyboard = i;
            }
            // The keys type what the list says at once, before Continue saves it.
            if !fake() {
                kit::spawn(&crate::setup::keyboard_command(KEYBOARDS[i].0));
            }
        });
        body.append(&a::form_row(tr("Keyboard"), &kb));
        let test = w::text_field(tr("Type here to try the keyboard"));
        test.add_css_class("assistant-field");
        body.append(&a::form_row(tr("Try it"), &test));
        let ime: gtk::Widget = if facts.ime {
            w::list_row(
                "input-keyboard-symbolic",
                tr("Other scripts"),
                tr(c::IME_READY),
                None,
            )
            .upcast()
        } else {
            kit::form::button_row(
                tr("Other scripts"),
                tr(c::IME_MISSING),
                tr("Open Modules"),
                || {
                    if !fake() {
                        kit::spawn(&["wol-settings".to_string(), "Modules".to_string()]);
                    }
                },
            )
        };
        body.append(&w::section("", &[ime]));
        Card::new(
            Art::Icon("input-keyboard-symbolic"),
            Page::Keyboard.title(),
            Page::Keyboard.subtitle(),
            &body,
        )
    }

    fn helper(self: &Rc<Self>) -> Card {
        use crate::copy::first_login as c;
        let me = Rc::downgrade(self);
        let on = w::toggle(self.flow.borrow().draft.helpers, move |v| {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().draft.helpers = v;
            }
        });
        let body = w::vbox(w::S4);
        body.append(&w::section(
            "",
            &[w::list_row(
                "wolos-assistant-symbolic",
                tr("Allow assistants and scripts"),
                tr(c::HELPER_HINT),
                Some(on.upcast_ref()),
            )
            .upcast()],
        ));
        body.append(&a::caption(tr(c::HELPER_MANUAL)));
        Card::new(
            Art::Icon("wolos-assistant-symbolic"),
            Page::Helper.title(),
            Page::Helper.subtitle(),
            &body,
        )
    }

    fn done(self: &Rc<Self>) -> Card {
        let c = Card::moment(
            "keyhole",
            Page::Done.title(),
            Page::Done.subtitle(),
            &a::blank(),
        );
        c.next.set_label(tr("Get Started"));
        c
    }
}

/// `wol-setup --first-login [--again]`: shows the setup when this account's sign-in calls for it
/// (or always with `--again`, from Settings), else ends at once so the desktop starts.
pub fn run(again: bool) -> glib::ExitCode {
    // The person's language (LANG of their session), ADR 0045: without it every `tr` of the six
    // pages reads English after a translated install Setup (QA-A7-1).
    crate::i18n::init();
    let marks = Marks::here();
    if marks.done() || !(again || marks.due(crate::apps::is_live())) {
        return glib::ExitCode::SUCCESS;
    }
    a::run("org.wolos.firstlogin", "first-login", |win| {
        let s = FirstLogin::new(crate::system::default_system());
        let shown = reveal::Reveal::new(&s.root, true);
        win.set_child(Some(&shown.root));
        shown.show_when(super::lock::backdrop_ready, || {
            reveal::show_pointer();
            reveal::boot_mark("first sign-in shown");
        });
        let s2 = s.clone();
        a::keys(win, move || s.next(), move || s2.back());
    })
}
