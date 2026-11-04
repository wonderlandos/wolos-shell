// SPDX-License-Identifier: GPL-2.0-or-later
//! Bluetooth: the radio, paired devices (connect, disconnect, forget) and pairing new ones with a
//! bluetoothctl agent: the sheet asks for the PIN or shows and confirms the passkey the device
//! asks for (the manual promises "pair a device here"). Every sentence not already frozen in this
//! file lives in `wol_core::bluetooth::words`.
use super::*;
use crate::copy::settings as words;
use crate::kit::controls::DeviceRow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use wol_core::bluetooth::{
    self as bt, AgentAnswer, AgentEvent, BtDevice, BtKind, BtLink, BtState, PairOutcome,
};

pub(crate) fn kind_icon(k: BtKind) -> &'static str {
    match k {
        BtKind::Audio => "audio-headphones-symbolic",
        BtKind::Keyboard => "input-keyboard-symbolic",
        BtKind::Mouse => "input-mouse-symbolic",
        BtKind::Computer => "computer-symbolic",
        BtKind::Phone | BtKind::Other => "bluetooth-active-symbolic",
    }
}

/// What the page shows (QA-SET-10): "No Bluetooth Hardware" only when the computer has no adapter;
/// before the first read it waits, and with an adapter whose bluetoothd is down it offers Turn On.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum View {
    Unknown,
    NoHardware,
    Down,
    Ready(bool),
}

/// `snapshot`: the radio as the shell's snapshot has it (None: not read yet or no answer).
/// `probe`: the page's own read and whether bluetoothd runs (None: not back yet).
pub(crate) fn view(snapshot: Option<bool>, probe: Option<(&BtState, BtLink)>) -> View {
    match probe {
        None => snapshot.map_or(View::Unknown, View::Ready),
        Some((st, _)) if !st.present => View::NoHardware,
        Some((_, BtLink::Down)) => View::Down,
        Some((st, _)) => View::Ready(st.powered),
    }
}

/// What the pairing sheet shows for one request of the agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Prompt {
    pub title: String,
    pub hint: &'static str,
    /// The passkey to compare or to type on the device.
    pub code: Option<String>,
    /// A field for the PIN or passkey.
    pub input: bool,
    /// The main button; None when the sheet only shows something (Cancel stays).
    pub accept: Option<&'static str>,
}

pub(crate) fn prompt(name: &str, ev: &AgentEvent) -> Prompt {
    let (title, hint, code, input, accept) = match ev {
        AgentEvent::NeedPin => (
            words::bluetooth_pin_title(name),
            words::BLUETOOTH_PIN_HINT,
            None,
            true,
            Some("Pair"),
        ),
        AgentEvent::NeedPasskey => (
            words::bluetooth_pin_title(name),
            words::BLUETOOTH_PASSKEY_HINT,
            None,
            true,
            Some("Pair"),
        ),
        AgentEvent::Confirm(code) => (
            words::bluetooth_pair_title(name),
            if code.is_some() {
                words::BLUETOOTH_CONFIRM_HINT
            } else {
                words::BLUETOOTH_CONFIRM_BARE
            },
            code.clone(),
            false,
            Some("Pair"),
        ),
        AgentEvent::Show(code) => (
            words::bluetooth_show_title(name),
            words::BLUETOOTH_SHOW_HINT,
            Some(code.clone()),
            false,
            None,
        ),
        _ => (
            words::bluetooth_show_title(name),
            words::BLUETOOTH_WAITING,
            None,
            false,
            None,
        ),
    };
    Prompt {
        title,
        hint,
        code,
        input,
        accept,
    }
}

/// The status line after a pairing.
pub(crate) fn outcome_line(name: &str, o: &PairOutcome) -> String {
    match o {
        PairOutcome::Paired {
            connected,
            note: None,
        } => words::bluetooth_paired(name, *connected),
        PairOutcome::Paired { note: Some(n), .. } => {
            format!("{} {n}", words::bluetooth_paired(name, false))
        }
        PairOutcome::Cancelled => words::BLUETOOTH_CANCELLED.to_string(),
        PairOutcome::Failed(why) => (*why).to_string(),
    }
}

/// One sheet for pairing prompts and for "Forget …?".
struct PairSheet {
    revealer: gtk::Revealer,
    title: gtk::Label,
    hint: gtk::Label,
    code: gtk::Label,
    entry: gtk::Entry,
    accept: gtk::Button,
    danger: gtk::Button,
    cancel: gtk::Button,
    busy: gtk::Spinner,
    on_accept: RefCell<Option<Box<dyn Fn(String)>>>,
    on_cancel: RefCell<Option<Box<dyn Fn()>>>,
}

impl PairSheet {
    fn new() -> Rc<Self> {
        let title = w::label("", "title");
        title.set_wrap(true);
        title.set_hexpand(true);
        let busy = w::busy_spinner();
        let head = w::hbox(w::S3);
        head.append(&title);
        head.append(&busy);
        let hint = w::label("", "secondary");
        hint.set_wrap(true);
        let code = w::label("", "large-title");
        code.set_selectable(true);
        let entry = w::text_field("PIN or code");
        let buttons = w::hbox(w::S2);
        buttons.append(&w::spacer());
        let this = Rc::new_cyclic(|me: &std::rc::Weak<PairSheet>| {
            let go = |me: std::rc::Weak<PairSheet>| {
                move || {
                    if let Some(s) = me.upgrade() {
                        let text = s.entry.text().to_string();
                        s.entry.set_text("");
                        if let Some(f) = s.on_accept.borrow().as_ref() {
                            f(text)
                        }
                    }
                }
            };
            let cancel = {
                let me = me.clone();
                w::button("Cancel", w::Kind::Default, move || {
                    if let Some(s) = me.upgrade() {
                        let f = s.on_cancel.borrow_mut().take();
                        s.on_accept.borrow_mut().take();
                        s.hide();
                        if let Some(f) = f {
                            f()
                        }
                    }
                })
            };
            let accept = w::button("Pair", w::Kind::Primary, go(me.clone()));
            let danger = w::button("Forget", w::Kind::Destructive, go(me.clone()));
            let g = go(me.clone());
            entry.connect_activate(move |_| g());
            buttons.append(&cancel);
            buttons.append(&accept);
            buttons.append(&danger);
            let body = w::vbox(w::S3);
            body.append(&head);
            body.append(&hint);
            body.append(&code);
            body.append(&entry);
            body.append(&buttons);
            PairSheet {
                revealer: w::sheet(&body),
                title: title.clone(),
                hint: hint.clone(),
                code: code.clone(),
                entry: entry.clone(),
                accept,
                danger,
                cancel,
                busy: busy.clone(),
                on_accept: RefCell::new(None),
                on_cancel: RefCell::new(None),
            }
        });
        this
    }

    #[allow(clippy::too_many_arguments)]
    fn show(
        &self,
        title: &str,
        hint: &str,
        code: Option<&str>,
        input: bool,
        accept: Option<(&str, bool)>,
        busy: bool,
        on_accept: Option<Box<dyn Fn(String)>>,
        on_cancel: Option<Box<dyn Fn()>>,
    ) {
        self.title.set_label(title);
        self.hint.set_label(hint);
        self.hint.set_visible(!hint.is_empty());
        self.code.set_label(code.unwrap_or(""));
        self.code.set_visible(code.is_some());
        self.entry.set_visible(input);
        self.accept.set_visible(matches!(accept, Some((_, false))));
        self.danger.set_visible(matches!(accept, Some((_, true))));
        if let Some((label, danger)) = accept {
            let b = if danger { &self.danger } else { &self.accept };
            b.set_label(label);
        }
        self.busy.set_visible(busy);
        self.busy.set_spinning(busy);
        *self.on_accept.borrow_mut() = on_accept;
        *self.on_cancel.borrow_mut() = on_cancel;
        self.revealer.set_reveal_child(true);
        if input {
            self.entry.grab_focus();
        } else if accept.is_some() {
            let b = if matches!(accept, Some((_, true))) {
                &self.danger
            } else {
                &self.accept
            };
            b.grab_focus();
        } else {
            self.cancel.grab_focus();
        }
    }

    fn hide(&self) {
        self.busy.set_spinning(false);
        self.revealer.set_reveal_child(false);
    }
}

/// One bluetoothctl at a time: a pairing (its sheet) or a Forget question holds the gate, and the
/// other rows' buttons do nothing until it is let go (two agents at once answer each other's
/// prompts).
#[derive(Default)]
pub(crate) struct Gate(Cell<bool>);

impl Gate {
    /// True when the caller now holds the gate.
    pub(crate) fn enter(&self) -> bool {
        !self.0.replace(true)
    }
    pub(crate) fn leave(&self) {
        self.0.set(false)
    }
    pub(crate) fn held(&self) -> bool {
        self.0.get()
    }
}

/// What the page's closures share.
struct Ui {
    gate: Gate,
    /// The page shows [`View::Down`]: Turn On starts bluetoothd first.
    down: Rc<Cell<bool>>,
    status: gtk::Label,
    sheet: Rc<PairSheet>,
    /// Paired devices and the search, rebuilt by [`load`].
    list: gtk::Box,
    toggle: gtk::Switch,
    hint: gtk::Label,
    content: gtk::Box,
    empty: gtk::Box,
}

impl Ui {
    fn apply(&self, v: View) {
        self.down.set(v == View::Down);
        self.empty.set_visible(v == View::NoHardware);
        self.content.set_visible(v != View::NoHardware);
        let (sensitive, on, hint, list) = match v {
            View::Unknown => (false, false, words::BLUETOOTH_LOOKING, false),
            View::NoHardware => (false, false, "", false),
            View::Down => (true, false, words::BLUETOOTH_DOWN, false),
            View::Ready(on) => (true, on, "", on),
        };
        self.toggle.set_state(on);
        self.toggle.set_active(on);
        self.toggle.set_sensitive(sensitive);
        say(&self.hint, hint);
        self.list.set_visible(list);
    }
}

fn os() -> wol_core::system::PosixHost {
    wol_core::system::PosixHost
}

/// Reads Bluetooth off the UI thread, then sets the switch and fills the device list.
fn load(e: &Rc<Env>, ui: &Rc<Ui>) {
    clear(&ui.list);
    let snapshot = e.sys().bluetooth();
    let m = e.machine();
    let (e2, ui2) = (e.clone(), ui.clone());
    later(
        &ui.list,
        move || (m.bluetooth(), bt::link(&os())),
        move |into, (st, link)| {
            let v = view(snapshot, Some((&st, link)));
            ui2.apply(v);
            if v == View::Ready(true) {
                devices(&e2, into, st, &ui2);
            }
        },
    );
}

fn devices(e: &Rc<Env>, into: &gtk::Box, st: BtState, ui: &Rc<Ui>) {
    let mut sections: Vec<gtk::Widget> = Vec::new();
    if st.devices.is_empty() {
        sections
            .push(w::list_row("", "No devices yet", words::BLUETOOTH_EMPTY_HINT, None).upcast());
    }
    for d in &st.devices {
        sections.push(device_row(e, d, ui));
    }
    into.append(&w::section("My Devices", &sections));
    let nearby = w::vbox(w::S6);
    let add = {
        let (e, ui, nearby) = (e.clone(), ui.clone(), nearby.clone());
        w::button("Add Device…", w::Kind::Default, move || {
            search(&e, &ui, &nearby)
        })
    };
    add.set_halign(gtk::Align::Start);
    add.set_widget_name("bt-add-device");
    into.append(&add);
    into.append(&nearby);
}

fn device_row(e: &Rc<Env>, d: &BtDevice, ui: &Rc<Ui>) -> gtk::Widget {
    let r = DeviceRow::new(
        kind_icon(d.kind),
        &d.name,
        if d.connected {
            "Connected"
        } else {
            "Not connected"
        },
        d.connected,
        None,
    );
    let row = Rc::new(r);
    let on = Rc::new(Cell::new(d.connected));
    {
        let (ctx, addr, st, r2, on2) = (
            e.ctx.clone(),
            d.addr.clone(),
            ui.status.clone(),
            row.clone(),
            on.clone(),
        );
        row.button.connect_clicked(move |_| {
            let want = !on2.get();
            r2.set_subtitle(if want {
                "Connecting…"
            } else {
                "Disconnecting…"
            });
            let (st, r3, on3) = (st.clone(), r2.clone(), on2.clone());
            ctx.act(
                Action::BtConnect(addr.clone(), want),
                Rc::new(move |res| {
                    let now = if res.is_ok() { want } else { on3.get() };
                    on3.set(now);
                    r3.set_active(now);
                    r3.set_subtitle(if now { "Connected" } else { "Not connected" });
                    say(&st, error_line(res));
                }),
            );
        });
    }
    row.button.set_hexpand(true);
    let forget = {
        let (e, ui, addr, name) = (e.clone(), ui.clone(), d.addr.clone(), d.name.clone());
        w::button("Forget", w::Kind::Plain, move || {
            confirm_forget(&e, &ui, &addr, &name)
        })
    };
    forget.set_valign(gtk::Align::Center);
    forget.set_tooltip_text(Some(&format!("Forget {}", d.name)));
    let line = w::hbox(w::S2);
    line.append(&row.button);
    line.append(&forget);
    line.upcast()
}

fn confirm_forget(e: &Rc<Env>, ui: &Rc<Ui>, addr: &str, name: &str) {
    if !ui.gate.enter() {
        say(&ui.status, words::BLUETOOTH_BUSY);
        return;
    }
    let (e2, ui2, ui_cancel, addr) = (e.clone(), ui.clone(), ui.clone(), addr.to_string());
    ui.sheet.show(
        &words::bluetooth_forget_title(name),
        words::BLUETOOTH_FORGET_BODY,
        None,
        false,
        Some(("Forget", true)),
        false,
        Some(Box::new(move |_| {
            ui2.sheet.hide();
            ui2.gate.leave();
            let (e3, ui3, a) = (e2.clone(), ui2.clone(), addr.clone());
            off_thread(
                move || bt::forget(&os(), &a),
                move |ok| {
                    say(
                        &ui3.status,
                        if ok {
                            words::BLUETOOTH_FORGOT
                        } else {
                            words::BLUETOOTH_FORGET_FAILED
                        },
                    );
                    load(&e3, &ui3);
                },
            );
        })),
        Some(Box::new(move || ui_cancel.gate.leave())),
    );
}

/// "Add Device…": looks around for ~8 s and lists what is not paired yet.
fn search(e: &Rc<Env>, ui: &Rc<Ui>, nearby: &gtk::Box) {
    if ui.gate.held() {
        say(&ui.status, words::BLUETOOTH_BUSY);
        return;
    }
    clear(nearby);
    let spin = w::busy_spinner();
    let wait = w::hbox(w::S2);
    wait.append(&spin);
    wait.append(&w::label(words::BLUETOOTH_SEARCHING, "secondary"));
    spin.set_visible(true);
    spin.set_spinning(true);
    nearby.append(&wait);
    let (e2, ui2, nearby2) = (e.clone(), ui.clone(), nearby.clone());
    off_thread(
        move || bt::scan(&os()),
        move |found| {
            clear(&nearby2);
            if found.is_empty() {
                nearby2.append(&w::label(words::BLUETOOTH_NONE_FOUND, "secondary"));
            } else {
                let rows: Vec<gtk::Widget> = found
                    .iter()
                    .map(|d| {
                        let r = DeviceRow::new(kind_icon(d.kind), &d.name, "", false, None);
                        let (e3, ui3, addr, name) =
                            (e2.clone(), ui2.clone(), d.addr.clone(), d.name.clone());
                        r.button
                            .connect_clicked(move |_| pair(&e3, &ui3, &addr, &name));
                        r.button.upcast()
                    })
                    .collect();
                nearby2.append(&w::section("Nearby Devices", &rows));
            }
            let again = {
                let (e, ui, n) = (e2.clone(), ui2.clone(), nearby2.clone());
                w::button("Search Again", w::Kind::Plain, move || search(&e, &ui, &n))
            };
            again.set_halign(gtk::Align::Start);
            nearby2.append(&again);
        },
    );
}

/// What the pairing thread tells the page.
enum Msg {
    Ask(AgentEvent),
    Done(PairOutcome),
}

/// The agent's `bluetoothctl`, closed when the person cancels while the device is silent.
struct Cancellable {
    proc: bt::BluetoothctlProc,
    cancelled: Arc<AtomicBool>,
}

impl bt::Console for Cancellable {
    fn send(&mut self, line: &str) -> bool {
        self.proc.send(line)
    }
    fn recv(&mut self, wait: std::time::Duration) -> bt::Recv {
        if self.cancelled.load(Ordering::Relaxed) {
            return bt::Recv::Closed;
        }
        self.proc.recv(wait)
    }
}

/// Pairs `addr`: a thread runs [`bt::run_pair`]; its requests come to the sheet, the answers go
/// back through a channel. The page polls the thread's messages (no blocking on the UI thread).
fn pair(e: &Rc<Env>, ui: &Rc<Ui>, addr: &str, name: &str) {
    if !ui.gate.enter() {
        say(&ui.status, words::BLUETOOTH_BUSY);
        return;
    }
    let (tx, rx) = mpsc::channel::<Msg>();
    let (ans_tx, ans_rx) = mpsc::channel::<AgentAnswer>();
    let cancelled = Arc::new(AtomicBool::new(false));
    say(&ui.status, "");
    {
        let (cancelled, ans_tx) = (cancelled.clone(), ans_tx.clone());
        ui.sheet.show(
            &words::bluetooth_show_title(name),
            words::BLUETOOTH_WAITING,
            None,
            false,
            None,
            true,
            None,
            Some(Box::new(move || {
                cancelled.store(true, Ordering::Relaxed);
                let _ = ans_tx.send(AgentAnswer::Cancel);
            })),
        );
    }
    let addr = addr.to_string();
    let flag = cancelled.clone();
    std::thread::spawn(move || {
        let outcome = match bt::BluetoothctlProc::spawn() {
            None => PairOutcome::Failed(words::BLUETOOTH_NO_CLIENT),
            Some(proc) => {
                let mut c = Cancellable {
                    proc,
                    cancelled: flag,
                };
                let ask_tx = tx.clone();
                bt::run_pair(&mut c, &addr, bt::PAIR_WAIT, &mut |ev| {
                    let _ = ask_tx.send(Msg::Ask(ev.clone()));
                    if matches!(ev, AgentEvent::Show(_)) {
                        return AgentAnswer::Ack;
                    }
                    ans_rx
                        .recv_timeout(std::time::Duration::from_secs(120))
                        .unwrap_or(AgentAnswer::Cancel)
                })
            }
        };
        let _ = tx.send(Msg::Done(outcome));
    });
    let (e, ui, name) = (e.clone(), ui.clone(), name.to_string());
    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::Ask(ev) => {
                    let p = prompt(&name, &ev);
                    let (tx_yes, tx_no, cancelled) =
                        (ans_tx.clone(), ans_tx.clone(), cancelled.clone());
                    let (input, entry_ev) = (p.input, ev.clone());
                    let on_accept: Option<Box<dyn Fn(String)>> = p.accept.map(|_| {
                        Box::new(move |text: String| {
                            let a = match (&entry_ev, input) {
                                (_, true) => AgentAnswer::Text(text),
                                _ => AgentAnswer::Yes,
                            };
                            let _ = tx_yes.send(a);
                        }) as Box<dyn Fn(String)>
                    });
                    ui.sheet.show(
                        &p.title,
                        p.hint,
                        p.code.as_deref(),
                        p.input,
                        p.accept.map(|l| (l, false)),
                        p.accept.is_none(),
                        on_accept,
                        Some(Box::new(move || {
                            cancelled.store(true, Ordering::Relaxed);
                            let _ = tx_no.send(AgentAnswer::Cancel);
                        })),
                    );
                }
                Msg::Done(o) => {
                    ui.sheet.hide();
                    ui.gate.leave();
                    let o = if cancelled.load(Ordering::Relaxed) {
                        PairOutcome::Cancelled
                    } else {
                        o
                    };
                    say(&ui.status, &outcome_line(&name, &o));
                    load(&e, &ui);
                    return glib::ControlFlow::Break;
                }
            }
        }
        glib::ControlFlow::Continue
    });
}

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let p = w::page("Bluetooth");
    let status = status_label();
    let sheet = PairSheet::new();
    let snapshot = e.sys().bluetooth();
    let first = view(snapshot, None);
    let list = w::vbox(w::S6);
    let hint = w::label("", "secondary");
    hint.set_wrap(true);
    hint.set_visible(false);
    let ui: Rc<RefCell<Option<Rc<Ui>>>> = Default::default();
    let down: Rc<Cell<bool>> = Default::default();
    // Waits for its change and flips back when it fails or is cancelled; the device list
    // follows the radio's real state (hunt 1 G1). With bluetoothd down (crashed, start limit
    // hit) Turn On starts the service first: `bluetoothctl power on` as the person cannot, it
    // waits for org.bluez for ever.
    let toggle = {
        let (ctx, st, list, e2, ui, down) = (
            e.ctx.clone(),
            status.clone(),
            list.clone(),
            e.clone(),
            ui.clone(),
            down.clone(),
        );
        w::pending_toggle(first == View::Ready(true), move |v, done| {
            let was_down = v && down.get();
            let (list, e3, ui) = (list.clone(), e2.clone(), ui.clone());
            let finish: Box<dyn FnOnce(bool)> = Box::new(move |ok| {
                list.set_visible(v == ok);
                done(ok);
                // The devices come with the radio; a started service changes the page either way.
                if let (true, Some(ui)) = ((ok && v) || was_down, ui.borrow().clone()) {
                    load(&e3, &ui);
                }
            });
            if !was_down {
                return ctx.act_switch(Action::Bluetooth(v), &st, finish);
            }
            let (ctx2, st2) = (ctx.clone(), st.clone());
            let start = Action::Admin(shared::admin::AdminRequest {
                action: shared::admin::AdminAction::Bluetooth,
                args: vec!["on".into()],
            });
            ctx.act_switch(
                start,
                &st,
                Box::new(move |ok| {
                    if ok {
                        ctx2.act_switch(Action::Bluetooth(true), &st2, finish)
                    } else {
                        finish(false)
                    }
                }),
            )
        })
    };
    let content = w::vbox(w::S6);
    content.append(&w::section(
        "",
        &[w::list_row(
            "bluetooth-active-symbolic",
            "Bluetooth",
            &words::bluetooth_appears_as(&e.host.hostname),
            Some(toggle.upcast_ref()),
        )
        .upcast()],
    ));
    content.append(&hint);
    content.append(&list);
    let empty = w::empty_state(
        "bluetooth-disabled-symbolic",
        "No Bluetooth Hardware",
        words::BLUETOOTH_NO_ADAPTER,
        None,
    );
    empty.set_visible(false);
    p.append(&sheet.revealer);
    p.append(&content);
    p.append(&empty);
    p.append(&status);
    let this = Rc::new(Ui {
        gate: Gate::default(),
        down,
        status,
        sheet,
        list,
        toggle,
        hint,
        content,
        empty,
    });
    this.apply(first);
    *ui.borrow_mut() = Some(this.clone());
    load(e, &this);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(present: bool, powered: bool) -> BtState {
        BtState {
            present,
            powered,
            devices: Vec::new(),
        }
    }

    /// QA-SET-10: unknown, down and none are three different pages.
    #[test]
    fn three_states_unknown_down_none() {
        // Not read yet and no snapshot: wait, do not claim "no hardware".
        assert_eq!(view(None, None), View::Unknown);
        // The snapshot knows the radio: show it at once.
        assert_eq!(view(Some(true), None), View::Ready(true));
        assert_eq!(view(Some(false), None), View::Ready(false));
        // The page's own read: no adapter at all.
        assert_eq!(
            view(None, Some((&st(false, false), BtLink::None))),
            View::NoHardware
        );
        assert_eq!(
            view(Some(true), Some((&st(false, false), BtLink::None))),
            View::NoHardware,
            "a stale snapshot does not invent hardware"
        );
        // An adapter with bluetoothd down (crashed): Turn On, never "no hardware".
        assert_eq!(
            view(None, Some((&st(true, false), BtLink::Down))),
            View::Down
        );
        assert_eq!(
            view(Some(true), Some((&st(true, true), BtLink::Down))),
            View::Down
        );
        // Everything answers.
        assert_eq!(
            view(None, Some((&st(true, true), BtLink::Up))),
            View::Ready(true)
        );
        assert_eq!(
            view(None, Some((&st(true, false), BtLink::Up))),
            View::Ready(false)
        );
    }

    /// One bluetoothctl at a time.
    #[test]
    fn the_gate_lets_one_pairing_in() {
        let g = Gate::default();
        assert!(!g.held());
        assert!(g.enter(), "the first pairing gets in");
        assert!(g.held());
        assert!(!g.enter(), "a second one waits for the first");
        g.leave();
        assert!(g.enter(), "free again after Cancel or Done");
    }

    #[test]
    fn prompts_name_the_device_and_the_code() {
        let p = prompt("MX Keys", &AgentEvent::Confirm(Some("123456".into())));
        assert_eq!(p.title, "Pair with “MX Keys”?");
        assert_eq!(p.code.as_deref(), Some("123456"));
        assert_eq!((p.input, p.accept), (false, Some("Pair")));
        let p = prompt("Old Keyboard", &AgentEvent::NeedPin);
        assert!(p.input && p.code.is_none() && p.hint.contains("0000"));
        let p = prompt("Old Keyboard", &AgentEvent::Show("042371".into()));
        assert_eq!(p.code.as_deref(), Some("042371"));
        assert_eq!(p.accept, None, "a code to type needs no button");
    }

    #[test]
    fn outcomes_become_status_lines() {
        let ok = PairOutcome::Paired {
            connected: true,
            note: None,
        };
        assert_eq!(outcome_line("Buds", &ok), "“Buds” is paired and connected.");
        let half = PairOutcome::Paired {
            connected: false,
            note: Some("Couldn’t connect. Try again."),
        };
        assert!(outcome_line("Buds", &half).starts_with("“Buds” is paired."));
        assert_eq!(
            outcome_line("Buds", &PairOutcome::Cancelled),
            words::BLUETOOTH_CANCELLED
        );
        assert_eq!(
            outcome_line(
                "Buds",
                &PairOutcome::Failed("Pairing didn’t work. Try again.")
            ),
            "Pairing didn’t work. Try again."
        );
    }
}
