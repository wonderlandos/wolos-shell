//! Setup assistant (`wol-setup`, ADR 0004): one glass card per step over the blurred wallpaper.
//! Runs as `_wollogin` on first boot; Wi-Fi and the account go to wol-login over its socket.
use crate::kit::{self, widgets as w};
use crate::login::{self, Verdict};
use crate::setup::{
    Appearance, Draft, Field, KEYBOARDS, LANGUAGES, STEPS, Step, initials_or_placeholder,
};
use crate::system::System;
use gtk::{glib, prelude::*};
use std::cell::RefCell;
use std::rc::Rc;

/// Greetings the welcome screen cross-fades through.
const HELLOS: [&str; 6] = ["Hello", "Olá", "Hola", "Bonjour", "Hallo", "Ciao"];
const HELLO_EVERY: std::time::Duration = std::time::Duration::from_millis(2600);
const HELLO_FADE_MS: u32 = 900;
const FIELDS: [Field; 5] = [
    Field::FullName,
    Field::Name,
    Field::Password,
    Field::Verify,
    Field::Hint,
];

pub struct Setup {
    pub root: gtk::Widget,
    stack: gtk::Stack,
    draft: Rc<RefCell<Draft>>,
    /// Continue buttons; the account one follows the draft's rules.
    account_next: RefCell<Option<gtk::Button>>,
    /// Problem line under each account field; shown once that field was edited.
    problems: RefCell<Vec<(Field, gtk::Label)>>,
    touched: RefCell<Vec<Field>>,
    editors: RefCell<Vec<(Field, gtk::Editable)>>,
    done_message: gtk::Label,
}

impl Setup {
    pub fn new(sys: &dyn System) -> Rc<Setup> {
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .vhomogeneous(false)
            .hhomogeneous(false)
            .valign(gtk::Align::Center)
            .halign(gtk::Align::Center)
            .vexpand(true)
            .build();
        let s = Rc::new(Setup {
            root: super::lock::screen(&stack),
            stack,
            draft: Default::default(),
            account_next: Default::default(),
            problems: Default::default(),
            touched: Default::default(),
            editors: Default::default(),
            done_message: w::centered("", "setup-subtitle"),
        });
        let pages: [(Step, gtk::Widget); 7] = [
            (Step::Welcome, welcome(&s).upcast()),
            (Step::Language, s.language().upcast()),
            (Step::Keyboard, s.keyboard().upcast()),
            (Step::Wifi, s.wifi(sys).upcast()),
            (Step::Account, s.account().upcast()),
            (Step::Appearance, s.appearance().upcast()),
            (Step::Done, s.done().upcast()),
        ];
        for (st, p) in pages {
            s.stack.add_named(&p, Some(st.id()));
        }
        s.refresh();
        s
    }

    pub fn show(&self, step: Step) {
        self.stack.set_visible_child_name(step.id());
    }

    /// Types into an account field as the user would (goldens, tests).
    pub fn type_into(&self, f: Field, text: &str) {
        if let Some((_, e)) = self.editors.borrow().iter().find(|(g, _)| *g == f) {
            e.set_text(text);
        }
    }

    fn go(&self, step: Option<Step>) {
        if let Some(st) = step {
            self.show(st);
        }
    }

    fn current(&self) -> Step {
        let id = self.stack.visible_child_name().unwrap_or_default();
        STEPS
            .into_iter()
            .find(|s| s.id() == id)
            .unwrap_or(Step::Welcome)
    }

    /// Account rules -> Continue sensitivity and the problem line under the concerned field.
    fn refresh(&self) {
        let problem = self.draft.borrow().account_problem();
        if let Some(b) = &*self.account_next.borrow() {
            b.set_sensitive(problem.is_none());
        }
        let touched = self.touched.borrow();
        for (f, l) in self.problems.borrow().iter() {
            let text = match problem {
                Some((pf, why)) if pf == *f && touched.contains(f) => why,
                _ => "",
            };
            l.set_label(text);
            l.set_visible(!text.is_empty());
        }
    }

    /// Card sized to its content: icon, title, subtitle, body, Back / Continue.
    fn card(
        self: &Rc<Self>,
        step: Step,
        icon: &str,
        subtitle: &str,
        body: &impl IsA<gtk::Widget>,
        extra: Option<gtk::Widget>,
    ) -> gtk::Box {
        let c = w::glass_panel(w::S6);
        c.add_css_class("setup-card");
        c.set_width_request(w::SETUP_CARD_WIDTH);
        let head = w::vbox(w::S2);
        let i = w::icon(icon, w::ICON_L);
        i.add_css_class("setup-icon");
        i.set_halign(gtk::Align::Center);
        head.append(&i);
        head.append(&w::centered(step.title(), "large-title"));
        let sub = w::centered(subtitle, "setup-subtitle");
        sub.set_wrap(true);
        head.append(&sub);
        c.append(&head);
        c.append(body);
        let foot = w::hbox(w::S2);
        let me = Rc::downgrade(self);
        foot.append(&w::button("Back", w::Kind::Default, move || {
            if let Some(s) = me.upgrade() {
                s.go(step.prev())
            }
        }));
        foot.append(&w::spacer());
        if let Some(x) = extra {
            foot.append(&x);
        }
        let me = Rc::downgrade(self);
        let next = w::button("Continue", w::Kind::Primary, move || {
            if let Some(s) = me.upgrade() {
                s.go(step.next())
            }
        });
        if step == Step::Account {
            *self.account_next.borrow_mut() = Some(next.clone());
        }
        foot.append(&next);
        c.append(&foot);
        c
    }

    /// One-choice list showing whole rows only (scrolls past `SETUP_ROWS`).
    fn choices(
        self: &Rc<Self>,
        rows: Vec<(String, Option<gtk::Widget>, Option<gtk::Widget>)>,
        selected: usize,
        picked: impl Fn(&Setup, usize) + 'static,
    ) -> gtk::ScrolledWindow {
        let picked = Rc::new(picked);
        let list = w::vbox(0);
        list.set_valign(gtk::Align::Start);
        let checks: Rc<RefCell<Vec<gtk::Image>>> = Default::default();
        let n = rows.len() as i32;
        for (i, (title, lead, trail)) in rows.into_iter().enumerate() {
            let row = w::hbox(w::S3);
            row.add_css_class("list-row");
            row.add_css_class("setup-row");
            if let Some(l) = lead {
                row.append(&l);
            }
            let t = w::label(&title, "");
            t.set_hexpand(true);
            row.append(&t);
            if let Some(x) = trail {
                row.append(&x);
            }
            let check = w::icon("object-select-symbolic", w::ICON);
            check.add_css_class("accent-icon");
            check.set_opacity(if i == selected { 1.0 } else { 0.0 });
            row.append(&check);
            checks.borrow_mut().push(check);
            let b = gtk::Button::builder().child(&row).build();
            b.add_css_class("row-button");
            b.add_css_class("setup-choice");
            let (me, cs, picked) = (Rc::downgrade(self), checks.clone(), picked.clone());
            b.connect_clicked(move |_| {
                for (j, c) in cs.borrow().iter().enumerate() {
                    c.set_opacity(if i == j { 1.0 } else { 0.0 });
                }
                if let Some(s) = me.upgrade() {
                    picked(&s, i);
                }
            });
            list.append(&b);
        }
        let h = w::SETUP_ROW * n.min(w::SETUP_ROWS);
        let sc = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_height(h)
            .max_content_height(h)
            .build();
        sc.add_css_class("setup-list");
        sc.add_css_class("whole-rows");
        sc
    }

    fn language(self: &Rc<Self>) -> gtk::Box {
        let rows = LANGUAGES
            .iter()
            .map(|l| {
                let r = w::label(l.region, "secondary");
                (l.name.to_string(), None, Some(r.upcast()))
            })
            .collect();
        let list = self.choices(rows, 0, |s, i| s.draft.borrow_mut().set_language(i));
        self.card(
            Step::Language,
            "preferences-desktop-locale-symbolic",
            "Choose your language and region.",
            &list,
            None,
        )
    }

    fn keyboard(self: &Rc<Self>) -> gtk::Box {
        let rows = KEYBOARDS
            .iter()
            .map(|k| {
                let i = w::icon("input-keyboard-symbolic", w::ICON);
                i.add_css_class("dim-icon");
                (k.1.to_string(), Some(i.upcast()), None)
            })
            .collect();
        let list = self.choices(rows, 0, |s, i| s.draft.borrow_mut().keyboard = i);
        self.card(
            Step::Keyboard,
            "input-keyboard-symbolic",
            "Choose the layout that matches your keyboard.",
            &list,
            None,
        )
    }

    fn wifi(self: &Rc<Self>, sys: &dyn System) -> gtk::Box {
        let nets = sys.wifi_networks();
        let body = w::vbox(w::S3);
        let join = w::vbox(w::S2);
        let pw = w::password_field("Network password");
        pw.add_css_class("setup-field");
        join.append(&pw);
        let row = w::hbox(w::S2);
        let message = w::label("", "setup-problem");
        message.set_hexpand(true);
        row.append(&message);
        join.append(&row);
        join.set_visible(false);
        let chosen: Rc<RefCell<Option<String>>> = Default::default();
        if nets.is_empty() {
            body.append(&w::empty_state(
                "network-wireless-offline-symbolic",
                "No networks found",
                "You can join a network later in Control Center.",
                None,
            ));
        } else {
            let rows = nets
                .iter()
                .map(|n| {
                    let sig = w::icon(
                        crate::status::network(crate::status::Network {
                            online: true,
                            wifi: true,
                            signal: Some(n.signal),
                        }),
                        w::ICON,
                    );
                    let lock = n.secure.then(|| {
                        let l = w::icon("network-wireless-encrypted-symbolic", w::ICON);
                        l.add_css_class("dim-icon");
                        l.set_tooltip_text(Some("Secured"));
                        l.upcast()
                    });
                    (n.ssid.clone(), Some(sig.upcast()), lock)
                })
                .collect();
            let selected = nets.iter().position(|n| n.connected).unwrap_or(usize::MAX);
            let nets2 = nets.clone();
            let (j, c, p) = (join.clone(), chosen.clone(), pw.clone());
            body.append(&self.choices(rows, selected, move |_, i| {
                let n = &nets2[i];
                *c.borrow_mut() = Some(n.ssid.clone());
                j.set_visible(!n.connected);
                p.set_visible(n.secure);
                p.grab_focus();
            }));
        }
        let me = Rc::downgrade(self);
        let (c, p, m) = (chosen.clone(), pw.clone(), message.clone());
        let join_btn = w::button("Join", w::Kind::Default, move || {
            let Some(ssid) = c.borrow().clone() else {
                return;
            };
            let pass = p.text().to_string();
            m.set_label("Joining…");
            let (me, m) = (me.clone(), m.clone());
            glib::spawn_future_local(async move {
                let v = gtk::gio::spawn_blocking(move || {
                    login::request(
                        login::LOGIN_SOCKET,
                        &shared::login::encode_wifi(&ssid, &pass),
                    )
                })
                .await
                .unwrap_or(Verdict::Error);
                if v == Verdict::Ok {
                    m.set_label("");
                    if let Some(s) = me.upgrade() {
                        s.go(Step::Wifi.next());
                    }
                } else {
                    m.set_label("Couldn't join. Check the password, or skip for now.");
                }
            });
        });
        row.append(&join_btn);
        body.append(&join);
        let me = Rc::downgrade(self);
        let skip = w::button("Not Now", w::Kind::Plain, move || {
            if let Some(s) = me.upgrade() {
                s.go(Step::Wifi.next())
            }
        });
        self.card(
            Step::Wifi,
            "network-wireless-symbolic",
            "Connect to the internet. You can skip this and join later.",
            &body,
            Some(skip.upcast()),
        )
    }

    fn account(self: &Rc<Self>) -> gtk::Box {
        let body = w::vbox(w::S4);
        let avatar = w::avatar("", w::AVATAR_XL);
        avatar.set_label(&initials_or_placeholder(""));
        avatar.add_css_class("setup-avatar");
        body.append(&avatar);
        let form = w::vbox(w::S2);
        let mk = |f: Field| -> gtk::Widget {
            let (e, ph): (gtk::Widget, &str) = match f {
                Field::FullName => (w::text_field("").upcast(), "Full name"),
                Field::Name => (w::text_field("").upcast(), "Account name"),
                Field::Password => (w::password_field("").upcast(), "Password"),
                Field::Verify => (w::password_field("").upcast(), "Verify password"),
                Field::Hint => (w::text_field("").upcast(), "Password hint (optional)"),
            };
            e.set_property("placeholder-text", ph);
            e.add_css_class("setup-field");
            e
        };
        let fields: Vec<(Field, gtk::Widget)> = FIELDS.iter().map(|f| (*f, mk(*f))).collect();
        for (f, e) in &fields {
            form.append(e);
            let p = w::label("", "setup-problem");
            p.set_wrap(true);
            p.set_visible(false);
            form.append(&p);
            self.problems.borrow_mut().push((*f, p));
            if *f == Field::Hint {
                let note = w::label(
                    "Shown on the login window after three wrong passwords. Don't use the password itself.",
                    "secondary",
                );
                note.set_wrap(true);
                form.append(&note);
            }
        }
        body.append(&form);
        let editable = |f: Field| -> gtk::Editable {
            fields
                .iter()
                .find(|(g, _)| *g == f)
                .and_then(|(_, e)| e.clone().dynamic_cast::<gtk::Editable>().ok())
                .expect("editable field")
        };
        let name_field = editable(Field::Name);
        for (f, e) in &fields {
            let Ok(e) = e.clone().dynamic_cast::<gtk::Editable>() else {
                continue;
            };
            self.editors.borrow_mut().push((*f, e.clone()));
            let (f, me, av, nf) = (*f, Rc::downgrade(self), avatar.clone(), name_field.clone());
            e.connect_changed(move |e| {
                let Some(s) = me.upgrade() else {
                    return;
                };
                let t = e.text().to_string();
                {
                    let mut d = s.draft.borrow_mut();
                    match f {
                        Field::FullName => d.set_full_name(&t),
                        Field::Name => d.set_name(&t),
                        Field::Password => d.password = t.clone(),
                        Field::Verify => d.verify = t.clone(),
                        Field::Hint => d.hint = t.clone(),
                    }
                }
                if f == Field::FullName {
                    av.set_label(&initials_or_placeholder(&t));
                    let derived = s.draft.borrow().name.clone();
                    if nf.text() != derived {
                        nf.set_text(&derived);
                    }
                }
                let mut touched = s.touched.borrow_mut();
                if !touched.contains(&f) {
                    touched.push(f);
                }
                drop(touched);
                s.refresh();
            });
        }
        self.card(
            Step::Account,
            "system-users-symbolic",
            "This account can administer this computer.",
            &body,
            None,
        )
    }

    fn appearance(self: &Rc<Self>) -> gtk::Box {
        let light = crate::theme::resolve(Some("wol-light"), false);
        let dark = crate::theme::resolve(Some("wol-dark"), true);
        let tiles = w::hbox(w::S4);
        tiles.set_halign(gtk::Align::Center);
        let choices = [
            (Appearance::Light, "Light", vec![w::Palette::of(&light)]),
            (Appearance::Dark, "Dark", vec![w::Palette::of(&dark)]),
            (
                Appearance::Auto,
                "Auto",
                vec![w::Palette::of(&light), w::Palette::of(&dark)],
            ),
        ];
        let mut first: Option<gtk::ToggleButton> = None;
        let selected = self.draft.borrow().appearance;
        for (a, title, slices) in choices {
            let t = w::theme_preview(slices, title, a == selected);
            match &first {
                Some(f) => t.set_group(Some(f)),
                None => first = Some(t.clone()),
            }
            let (d, light, dark) = (self.draft.clone(), light.clone(), dark.clone());
            t.connect_toggled(move |t| {
                if t.is_active() {
                    d.borrow_mut().appearance = a;
                    // Preview the choice on the assistant itself.
                    match a {
                        Appearance::Light => kit::style::apply(&light, ""),
                        Appearance::Dark => kit::style::apply(&dark, ""),
                        Appearance::Auto => {}
                    }
                }
            });
            tiles.append(&t);
        }
        self.card(
            Step::Appearance,
            "preferences-desktop-appearance-symbolic",
            "Auto is light by day and dark at night. Change it any time in System Settings.",
            &tiles,
            None,
        )
    }

    /// No Back: the account exists once this card's button is pressed.
    fn done(self: &Rc<Self>) -> gtk::Box {
        let c = w::glass_panel(w::S4);
        c.add_css_class("setup-card");
        c.set_width_request(w::SETUP_CARD_WIDTH);
        let i = w::icon("object-select-symbolic", w::ICON_M);
        i.add_css_class("setup-done-icon");
        // Gentle scale-in when the card appears (not in fixed-time golden renders).
        if std::env::var_os("WOL_FAKE_NOW").is_none() {
            let (i2, i3) = (i.clone(), i.clone());
            c.connect_map(move |_| i2.add_css_class("pop"));
            c.connect_unmap(move |_| i3.remove_css_class("pop"));
        }
        i.set_halign(gtk::Align::Center);
        c.append(&i);
        c.append(&w::centered(Step::Done.title(), "large-title"));
        let sub = w::centered(
            "Your account is ready. WolOS starts right away.",
            "setup-subtitle",
        );
        sub.set_wrap(true);
        c.append(&sub);
        let me = Rc::downgrade(self);
        let start = w::button("Start Using WolOS", w::Kind::Primary, move || {
            if let Some(s) = me.upgrade() {
                s.finish()
            }
        });
        start.set_halign(gtk::Align::Center);
        start.add_css_class("setup-start");
        c.append(&start);
        self.done_message.set_wrap(true);
        c.append(&self.done_message);
        c
    }

    /// Creates the account through wol-login; on `ok` wol-login starts the session and ends this greeter.
    fn finish(self: &Rc<Self>) {
        let req = self.draft.borrow().account().encode();
        self.done_message.set_label("Creating your account…");
        let m = self.done_message.clone();
        glib::spawn_future_local(async move {
            let v = gtk::gio::spawn_blocking(move || login::request(login::LOGIN_SOCKET, &req))
                .await
                .unwrap_or(Verdict::Error);
            match v {
                Verdict::Ok => std::process::exit(0),
                Verdict::Denied => {
                    m.set_label("The account couldn't be created. Go back and check it.")
                }
                Verdict::Error => m.set_label("Setup is unavailable. Restart and try again."),
            }
        });
    }
}

/// Welcome: logo, cross-fading "Hello", accent arrow with "Get Started".
fn welcome(s: &Rc<Setup>) -> gtk::Box {
    let b = w::vbox(w::S6);
    b.set_valign(gtk::Align::Center);
    let logo = w::icon("wolos-mark", w::ICON_XL);
    logo.set_halign(gtk::Align::Center);
    b.append(&logo);
    let hello = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .transition_duration(HELLO_FADE_MS)
        .hhomogeneous(true)
        .vhomogeneous(true)
        .build();
    for h in HELLOS {
        hello.add_named(&w::centered(h, "setup-hello"), Some(h));
    }
    let h2 = hello.downgrade();
    let i = std::cell::Cell::new(0usize);
    glib::timeout_add_local(HELLO_EVERY, move || {
        let Some(h) = h2.upgrade() else {
            return glib::ControlFlow::Break;
        };
        i.set((i.get() + 1) % HELLOS.len());
        h.set_visible_child_name(HELLOS[i.get()]);
        glib::ControlFlow::Continue
    });
    b.append(&hello);
    let me = Rc::downgrade(s);
    let go = gtk::Button::builder()
        .child(&w::icon("go-next-symbolic", w::ICON))
        .tooltip_text("Get Started")
        .halign(gtk::Align::Center)
        .build();
    go.add_css_class("setup-go");
    go.connect_clicked(move |_| {
        if let Some(s) = me.upgrade() {
            s.go(Step::Welcome.next())
        }
    });
    let v = w::vbox(w::S2);
    v.append(&go);
    let cap = w::centered("Get Started", "lock-caption");
    cap.add_css_class("setup-caption");
    v.append(&cap);
    b.append(&v);
    b
}

pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("org.wolos.setup")
        .build();
    app.connect_activate(|app| {
        kit::style::install("");
        let win = gtk::ApplicationWindow::new(app);
        win.add_css_class("lock-window");
        let s = Setup::new(&*crate::system::default_system());
        win.set_child(Some(&s.root));
        let keys = gtk::EventControllerKey::new();
        let s2 = s.clone();
        keys.connect_key_pressed(move |_, k, _, _| {
            let cur = s2.current();
            if k == gtk::gdk::Key::Escape && cur != Step::Welcome && cur != Step::Done {
                s2.go(cur.prev());
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        win.add_controller(keys);
        kit::snapshot_on_map(&win, "setup");
        win.fullscreen();
        win.present();
    });
    app.run_with_args::<&str>(&[])
}
