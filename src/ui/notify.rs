//! org.freedesktop.Notifications server (zbus) + banners (top right, 5 s) + Notification Center panel.
use crate::kit::{
    surface::{Keyboard, Spec},
    widgets as w,
};
use gtk::{glib, prelude::*};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedValue;

const PATH: &str = "/org/freedesktop/Notifications";
const NAME: &str = "org.freedesktop.Notifications";

#[derive(Clone, Debug)]
pub struct Note {
    pub id: u32,
    pub app: String,
    pub icon: String,
    pub summary: String,
    pub body: String,
    pub critical: bool,
    pub timeout_ms: i32,
}

/// Server -> UI.
#[derive(Clone, Debug)]
pub enum Event {
    Show(Note),
    /// CloseNotification from a client: drop the banner (signal already sent).
    Close(u32),
}

/// NotificationClosed reasons (Desktop Notifications spec 1.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum CloseReason {
    Expired = 1,
    Dismissed = 2,
    Closed = 3,
}

type Open = Arc<Mutex<HashSet<u32>>>;

/// Takes `id` out of the open set; true when it was open (signal it exactly once).
fn take(open: &Open, id: u32) -> bool {
    open.lock().map(|mut o| o.remove(&id)).unwrap_or(false)
}

struct Server {
    tx: async_channel::Sender<Event>,
    next: AtomicU32,
    open: Open,
}

/// The live bus connection: kept for the process lifetime to emit NotificationClosed.
#[derive(Clone)]
pub struct Bus {
    conn: zbus::Connection,
    open: Open,
}

impl Bus {
    /// Owns `org.freedesktop.Notifications` on `address` (None: session bus).
    pub async fn serve(
        address: Option<&str>,
        tx: async_channel::Sender<Event>,
    ) -> zbus::Result<Bus> {
        let open = Open::default();
        let server = Server {
            tx,
            next: AtomicU32::new(1),
            open: open.clone(),
        };
        let b = match address {
            Some(a) => zbus::connection::Builder::address(a)?,
            None => zbus::connection::Builder::session()?,
        };
        let conn = b.name(NAME)?.serve_at(PATH, server)?.build().await?;
        Ok(Bus { conn, open })
    }

    /// Emits NotificationClosed once per notification; false when it was already closed.
    pub async fn close(&self, id: u32, reason: CloseReason) -> zbus::Result<bool> {
        if !take(&self.open, id) {
            return Ok(false);
        }
        let e = SignalEmitter::new(&self.conn, PATH)?;
        Server::notification_closed(&e, id, reason as u32).await?;
        Ok(true)
    }
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Server {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        _actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let id = if replaces_id != 0 {
            replaces_id
        } else {
            self.next.fetch_add(1, Ordering::Relaxed)
        };
        let critical = hints.get("urgency").and_then(|v| u8::try_from(v).ok()) == Some(2);
        if let Ok(mut o) = self.open.lock() {
            o.insert(id);
        }
        let _ = self.tx.try_send(Event::Show(Note {
            id,
            app: app_name,
            icon: app_icon,
            summary,
            body,
            critical,
            timeout_ms: expire_timeout,
        }));
        id
    }
    async fn close_notification(
        &self,
        id: u32,
        #[zbus(signal_emitter)] e: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        if take(&self.open, id) {
            Self::notification_closed(&e, id, CloseReason::Closed as u32).await?;
            let _ = self.tx.try_send(Event::Close(id));
        }
        Ok(())
    }
    #[zbus(signal)]
    async fn notification_closed(e: &SignalEmitter<'_>, id: u32, reason: u32) -> zbus::Result<()>;
    fn get_capabilities(&self) -> Vec<String> {
        vec!["body".into(), "persistence".into()]
    }
    fn get_server_information(&self) -> (String, String, String, String) {
        (
            "wol-shell".into(),
            "WolOS".into(),
            env!("CARGO_PKG_VERSION").into(),
            "1.2".into(),
        )
    }
}

/// Banner lifetime: server default 5 s, -1 = default, 0 = until clicked, critical stays.
pub fn banner_ms(n: &Note) -> Option<u32> {
    match (n.critical, n.timeout_ms) {
        (true, _) | (_, 0) => None,
        (_, t) if t > 0 => Some(t as u32),
        _ => Some(5000),
    }
}

fn card(n: &Note) -> gtk::Box {
    let c = w::glass_panel(w::S1);
    c.add_css_class("notification");
    if n.critical {
        c.add_css_class("critical");
    }
    let row = w::hbox(w::S3);
    let img = w::icon(
        if n.icon.is_empty() {
            "dialog-information-symbolic"
        } else {
            &n.icon
        },
        w::ICON_M,
    );
    row.append(&img);
    let text = w::vbox(0);
    text.append(&w::label(&n.app, "secondary"));
    text.append(&w::label(&n.summary, "title"));
    if !n.body.is_empty() {
        let b = w::label(&n.body, "");
        b.set_wrap(true);
        text.append(&b);
    }
    row.append(&text);
    c.append(&row);
    c
}

/// Starts the D-Bus server; returns the Notification Center panel.
pub fn start(app: &gtk::Application) -> gtk::Window {
    let (tx, rx) = async_channel::unbounded::<Event>();
    let bus: Rc<std::cell::OnceCell<Bus>> = Default::default();
    let b2 = bus.clone();
    glib::spawn_future_local(async move {
        match Bus::serve(None, tx).await {
            Ok(b) => {
                let _ = b2.set(b);
            }
            Err(e) => eprintln!("wol-shell: notifications: {e}"),
        }
    });
    let closed = move |id: u32, reason: CloseReason| {
        if let Some(b) = bus.get().cloned() {
            glib::spawn_future_local(async move {
                if let Err(e) = b.close(id, reason).await {
                    eprintln!("wol-shell: NotificationClosed: {e}");
                }
            });
        }
    };

    let banners = super::window(
        app,
        "banners",
        Spec::panel(
            "wol-panel-banners",
            [true, false, false, true],
            Keyboard::None,
        ),
    );
    let stack = w::vbox(0);
    banners.set_child(Some(&stack));
    let center_list = w::vbox(0);
    let g = w::glass_panel(w::S2);
    g.append(&w::label("Notifications", "title"));
    g.append(
        &gtk::ScrolledWindow::builder()
            .child(&center_list)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build(),
    );
    let center = super::window(
        app,
        "panel",
        Spec::panel(
            "wol-panel-notifications",
            [true, true, false, true],
            Keyboard::OnDemand,
        ),
    );
    center.add_css_class("notifications");
    center.set_child(Some(&g));

    glib::spawn_future_local(async move {
        // Banners on screen by id; entries leave on dismiss, expiry, close or replacement.
        let shown: Rc<RefCell<HashMap<u32, gtk::Box>>> = Default::default();
        let forget = |shown: &RefCell<HashMap<u32, gtk::Box>>, id: u32, c: &gtk::Box| {
            let mut m = shown.borrow_mut();
            if m.get(&id) == Some(c) {
                m.remove(&id);
            }
        };
        let hide = |stack: &gtk::Box, banners: &gtk::ApplicationWindow, c: &gtk::Box| {
            if c.parent().is_some() {
                stack.remove(c);
            }
            if stack.first_child().is_none() {
                banners.set_visible(false);
            }
        };
        while let Ok(ev) = rx.recv().await {
            let n = match ev {
                Event::Show(n) => n,
                Event::Close(id) => {
                    if let Some(c) = shown.borrow_mut().remove(&id) {
                        hide(&stack, &banners, &c);
                    }
                    continue;
                }
            };
            let old = shown.borrow_mut().remove(&n.id);
            if let Some(old) = old {
                hide(&stack, &banners, &old);
            }
            center_list.prepend(&card(&n));
            let c = card(&n);
            let click = gtk::GestureClick::new();
            let (s2, b2, c2, closed2, id, sh) = (
                stack.clone(),
                banners.clone(),
                c.clone(),
                closed.clone(),
                n.id,
                shown.clone(),
            );
            click.connect_released(move |_, _, _, _| {
                forget(&sh, id, &c2);
                hide(&s2, &b2, &c2);
                closed2(id, CloseReason::Dismissed);
            });
            c.add_controller(click);
            stack.append(&c);
            shown.borrow_mut().insert(n.id, c.clone());
            banners.set_visible(true);
            if let Some(ms) = banner_ms(&n) {
                let (stack, banners, closed, id, sh) = (
                    stack.clone(),
                    banners.clone(),
                    closed.clone(),
                    n.id,
                    shown.clone(),
                );
                glib::timeout_add_local_once(
                    std::time::Duration::from_millis(ms as u64),
                    move || {
                        // Only the banner that is still showing expires (a replacement has its own timer).
                        if c.parent().is_some() {
                            forget(&sh, id, &c);
                            hide(&stack, &banners, &c);
                            closed(id, CloseReason::Expired);
                        }
                    },
                );
            }
        }
    });
    center.upcast()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn n(critical: bool, t: i32) -> Note {
        Note {
            id: 1,
            app: "".into(),
            icon: "".into(),
            summary: "".into(),
            body: "".into(),
            critical,
            timeout_ms: t,
        }
    }

    #[test]
    fn banner_lifetimes() {
        for (c, t, want) in [
            (false, -1, Some(5000)),
            (false, 2000, Some(2000)),
            (false, 0, None),
            (true, 3000, None),
        ] {
            assert_eq!(banner_ms(&n(c, t)), want);
        }
    }
}
