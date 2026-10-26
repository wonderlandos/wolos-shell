//! Setup assistant (`wol-setup`, ADR 0004): one glass card per step over the blurred wallpaper.
//! Runs as `_wollogin` on first boot; Wi-Fi and the account go to wol-login over its socket.
use crate::kit::{self, widgets as w};
use crate::login::{self, Verdict};
use crate::setup::{Appearance, Draft, KEYBOARDS, LANGUAGES, STEPS, Step};
use crate::system::System;
use gtk::{glib, prelude::*};
use std::cell::RefCell;
use std::rc::Rc;

/// Greetings the welcome card cycles through.
const HELLOS: [&str; 6] = ["Hello", "Olá", "Hola", "Bonjour", "Hallo", "Ciao"];
const HELLO_EVERY: std::time::Duration = std::time::Duration::from_millis(2400);

pub struct Setup {
    pub root: gtk::Widget,
    stack: gtk::Stack,
    draft: Rc<RefCell<Draft>>,
    /// Continue per step (sensitivity follows the step's rules).
    next: RefCell<Vec<(Step, gtk::Button)>>,
    account_problem: gtk::Label,
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
            next: Default::default(),
            account_problem: w::label("", "secondary"),
            done_message: w::centered("", "secondary"),
        });
        s.stack.add_named(&welcome(&s), Some(Step::Welcome.id()));
        s.stack.add_named(&s.language(), Some(Step::Language.id()));
        s.stack.add_named(&s.keyboard(), Some(Step::Keyboard.id()));
        s.stack.add_named(&s.wifi(sys), Some(Step::Wifi.id()));
        s.stack.add_named(&s.account(), Some(Step::Account.id()));
        s.stack
            .add_named(&s.appearance(), Some(Step::Appearance.id()));
        s.stack.add_named(&s.done(), Some(Step::Done.id()));
        s.refresh();
        s
    }

    pub fn show(&self, step: Step) {
        self.stack.set_visible_child_name(step.id());
    }

    fn go(self: &Rc<Self>, step: Option<Step>) {
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

    /// Continue follows the draft (only the account step has rules).
    fn refresh(&self) {
        let problem = self.draft.borrow().account_problem();
        self.account_problem.set_label(problem.unwrap_or(""));
        for (st, b) in self.next.borrow().iter() {
            b.set_sensitive(*st != Step::Account || problem.is_none());
        }
    }

    /// Card: icon, title, subtitle, body, Back / Continue.
    fn card(
        self: &Rc<Self>,
        step: Step,
        icon: &str,
        subtitle: &str,
        body: &impl IsA<gtk::Widget>,
        extra: Option<gtk::Widget>,
    ) -> gtk::Box {
        let c = w::glass_panel(w::S4);
        c.add_css_class("setup-card");
        c.set_size_request(w::SETUP_CARD.0, w::SETUP_CARD.1);
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
        body.set_vexpand(true);
        c.append(body);
        let foot = w::hbox(w::S2);
        let me = Rc::downgrade(self);
        let back = w::button("Back", w::Kind::Default, move || {
            if let Some(s) = me.upgrade() {
                s.go(step.prev())
            }
        });
        foot.append(&back);
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
        foot.append(&next);
        self.next.borrow_mut().push((step, next));
        c.append(&foot);
        c
    }

    /// One-choice list in a card (languages, keyboards).
    fn choices(
        self: &Rc<Self>,
        rows: Vec<(String, String)>,
        selected: usize,
        picked: impl Fn(&Setup, usize) + 'static,
    ) -> gtk::ScrolledWindow {
        let picked = Rc::new(picked);
        let list = w::vbox(0);
        list.add_css_class("card-list");
        list.set_valign(gtk::Align::Start);
        let buttons: Rc<RefCell<Vec<gtk::Button>>> = Default::default();
        for (i, (title, sub)) in rows.iter().enumerate() {
            let b = w::choice_row("", title, sub, i == selected);
            let (me, bs, picked) = (Rc::downgrade(self), buttons.clone(), picked.clone());
            b.connect_clicked(move |_| {
                for (j, b) in bs.borrow().iter().enumerate() {
                    set_checked(b, i == j);
                }
                if let Some(s) = me.upgrade() {
                    picked(&s, i);
                }
            });
            buttons.borrow_mut().push(b.clone());
            list.append(&b);
        }
        gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build()
    }

    fn language(self: &Rc<Self>) -> gtk::Box {
        let rows = LANGUAGES
            .iter()
            .map(|l| (l.name.to_string(), l.region.to_string()))
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
            .map(|k| (k.name.to_string(), String::new()))
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
        let message = w::label("", "secondary");
        let join = w::vbox(w::S2);
        let pw = w::password_field("Network password");
        join.append(&pw);
        let join_row = w::hbox(w::S2);
        join_row.append(&message);
        message.set_hexpand(true);
        join.append(&join_row);
        let join_slot = join_row.clone();
        join.set_visible(false);
        let chosen: Rc<RefCell<Option<(String, bool)>>> = Default::default();
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
                    let sub = if n.connected {
                        "Connected"
                    } else if n.secure {
                        "Secured"
                    } else {
                        "Open"
                    };
                    (n.ssid.clone(), sub.to_string())
                })
                .collect();
            let selected = nets.iter().position(|n| n.connected).unwrap_or(usize::MAX);
            let nets2 = nets.clone();
            let (j, c, p) = (join.clone(), chosen.clone(), pw.clone());
            body.append(&self.choices(rows, selected, move |_, i| {
                let n = &nets2[i];
                *c.borrow_mut() = Some((n.ssid.clone(), n.secure));
                j.set_visible(!n.connected);
                p.set_visible(n.secure);
                p.grab_focus();
            }));
        }
        body.append(&join);
        let me = Rc::downgrade(self);
        let (c, p, m) = (chosen.clone(), pw.clone(), message.clone());
        let join_btn = w::button("Join", w::Kind::Default, move || {
            let Some((ssid, _)) = c.borrow().clone() else {
                return;
            };
            let pass = p.text().to_string();
            m.set_label("Joining…");
            let (me, m) = (me.clone(), m.clone());
            glib::spawn_future_local(async move {
                let v = gtk::gio::spawn_blocking(move || {
                    let req = shared::login::encode_wifi(&ssid, &pass);
                    login::request(login::LOGIN_SOCKET, &req)
                })
                .await
                .unwrap_or(Verdict::Error);
                if v == Verdict::Ok {
                    m.set_label("Connected.");
                    if let Some(s) = me.upgrade() {
                        s.go(Step::Wifi.next());
                    }
                } else {
                    m.set_label("Couldn't join. Check the password, or skip for now.");
                }
            });
        });
        join_slot.append(&join_btn);
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
        let row = w::hbox(w::S6);
        let avatar = w::avatar("", w::AVATAR_XL);
        let col = w::vbox(0);
        col.append(&avatar);
        row.append(&col);
        let form = w::vbox(w::S2);
        form.set_hexpand(true);
        let full = w::text_field("Full name");
        let name = w::text_field("Account name");
        let pw = w::password_field("Password");
        let verify = w::password_field("Verify password");
        let hint = w::text_field("Hint (optional)");
        for f in [
            full.upcast_ref::<gtk::Widget>(),
            name.upcast_ref(),
            pw.upcast_ref(),
            verify.upcast_ref(),
            hint.upcast_ref(),
        ] {
            form.append(f);
        }
        self.account_problem.set_wrap(true);
        form.append(&self.account_problem);
        row.append(&form);
        let d = self.draft.clone();
        let me = Rc::downgrade(self);
        let (n2, a2) = (name.clone(), avatar.clone());
        full.connect_changed(move |f| {
            let t = f.text();
            d.borrow_mut().set_full_name(&t);
            a2.set_label(&login::initials(&t));
            let derived = d.borrow().name.clone();
            if n2.text() != derived {
                n2.set_text(&derived);
            }
            if let Some(s) = me.upgrade() {
                s.refresh()
            }
        });
        let bind = |e: &gtk::Widget, set: fn(&mut Draft, String)| {
            let (d, me) = (self.draft.clone(), Rc::downgrade(self));
            let e = e.clone().dynamic_cast::<gtk::Editable>().expect("editable");
            e.connect_changed(move |e| {
                set(&mut d.borrow_mut(), e.text().to_string());
                if let Some(s) = me.upgrade() {
                    s.refresh()
                }
            });
        };
        bind(name.upcast_ref(), |d, v| d.set_name(&v));
        bind(pw.upcast_ref(), |d, v| d.password = v);
        bind(verify.upcast_ref(), |d, v| d.verify = v);
        bind(hint.upcast_ref(), |d, v| d.hint = v);
        self.card(
            Step::Account,
            "system-users-symbolic",
            "This account can administer this computer.",
            &row,
            None,
        )
    }

    fn appearance(self: &Rc<Self>) -> gtk::Box {
        let light = crate::theme::resolve(Some("wol-light"), false);
        let dark = crate::theme::resolve(Some("wol-dark"), true);
        let tiles = w::hbox(w::S4);
        tiles.set_halign(gtk::Align::Center);
        tiles.set_valign(gtk::Align::Center);
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
            "Auto follows the time of day. Change it any time in System Settings.",
            &tiles,
            None,
        )
    }

    fn done(self: &Rc<Self>) -> gtk::Box {
        let c = w::glass_panel(w::S4);
        c.add_css_class("setup-card");
        c.set_size_request(w::SETUP_CARD.0, w::SETUP_CARD.1);
        let mid = w::vbox(w::S3);
        mid.set_valign(gtk::Align::Center);
        mid.set_vexpand(true);
        let i = w::icon("object-select-symbolic", w::ICON_M);
        i.add_css_class("setup-done-icon");
        i.set_halign(gtk::Align::Center);
        mid.append(&i);
        mid.append(&w::centered(Step::Done.title(), "large-title"));
        mid.append(&w::centered(
            "Your account is ready. WolOS starts right away.",
            "setup-subtitle",
        ));
        let me = Rc::downgrade(self);
        let start = w::button("Start Using WolOS", w::Kind::Primary, move || {
            if let Some(s) = me.upgrade() {
                s.finish()
            }
        });
        start.set_halign(gtk::Align::Center);
        start.add_css_class("setup-start");
        mid.append(&start);
        mid.append(&self.done_message);
        c.append(&mid);
        let foot = w::hbox(w::S2);
        let me = Rc::downgrade(self);
        foot.append(&w::button("Back", w::Kind::Default, move || {
            if let Some(s) = me.upgrade() {
                s.go(Step::Done.prev())
            }
        }));
        c.append(&foot);
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

fn set_checked(row: &gtk::Button, on: bool) {
    // choice_row: button > list-row > [title box, check icon]
    if let Some(check) = row.child().and_then(|r| r.last_child()) {
        check.set_opacity(if on { 1.0 } else { 0.0 });
    }
}

/// Welcome: logo, cycling "Hello", Get Started.
fn welcome(s: &Rc<Setup>) -> gtk::Box {
    let b = w::vbox(w::S6);
    b.set_valign(gtk::Align::Center);
    let logo = w::icon("wolos-mark", w::ICON_XL);
    logo.add_css_class("setup-logo");
    logo.set_halign(gtk::Align::Center);
    b.append(&logo);
    let hello = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .transition_duration(700)
        .hhomogeneous(false)
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
    go.add_css_class("lock-round");
    go.connect_clicked(move |_| {
        if let Some(s) = me.upgrade() {
            s.go(Step::Welcome.next())
        }
    });
    let v = w::vbox(w::S2);
    v.append(&go);
    v.append(&w::centered("Get Started", "lock-caption"));
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
            if k == gtk::gdk::Key::Escape && s2.current() != Step::Welcome {
                s2.go(s2.current().prev());
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
