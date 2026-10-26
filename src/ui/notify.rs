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

/// Where a new banner goes, given the banners on screen as (app, stacked count), top first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// New group at the top.
    Top,
    /// Same app already showing: replace that banner, stack count becomes `count`.
    Stack { index: usize, count: u32 },
}

pub fn place(shown: &[(String, u32)], app: &str) -> Place {
    match shown
        .iter()
        .position(|(a, _)| !app.is_empty() && a.eq_ignore_ascii_case(app))
    {
        Some(index) => Place::Stack {
            index,
            count: shown[index].1 + 1,
        },
        None => Place::Top,
    }
}

/// Notification Center row (same layout as a banner, no stack).
fn card(n: &Note) -> gtk::Box {
    banner(n, 1)
}

/// macOS banner: 344 px glass card, app icon, app name + "now", bold title, body up to 2 lines.
/// `count` > 1 draws the group's stacked edges under the card.
pub fn banner(n: &Note, count: u32) -> gtk::Box {
    let outer = w::vbox(0);
    outer.add_css_class("banner-group");
    let c = w::glass_panel(0);
    c.add_css_class("notification");
    if n.critical {
        c.add_css_class("critical");
    }
    let row = w::hbox(w::S3);
    let icon = if n.icon.is_empty() {
        crate::apps::icon_for(&n.app, crate::apps::PINS, &[])
    } else {
        n.icon.clone()
    };
    let img = if icon.starts_with('/') {
        let i = gtk::Image::from_file(&icon);
        i.set_pixel_size(w::ICON_M);
        i
    } else {
        w::app_icon(&icon, w::ICON_M)
    };
    row.append(&img);
    let text = w::vbox(0);
    let head = w::hbox(w::S2);
    let app = w::label(&n.app.to_uppercase(), "banner-app");
    app.set_hexpand(true);
    app.set_max_width_chars(1);
    app.set_ellipsize(gtk::pango::EllipsizeMode::End);
    app.add_css_class("may-ellipsize");
    head.append(&app);
    head.append(&w::label("now", "banner-time"));
    text.append(&head);
    let title = w::label(&n.summary, "banner-title");
    title.add_css_class("may-ellipsize");
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_max_width_chars(1);
    text.append(&title);
    if !n.body.is_empty() {
        let b = w::label(&n.body, "banner-body");
        b.set_wrap(true);
        b.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        b.set_lines(2);
        b.set_ellipsize(gtk::pango::EllipsizeMode::End);
        b.add_css_class("may-ellipsize");
        b.set_max_width_chars(1);
        text.append(&b);
    }
    if count > 1 {
        text.append(&w::label(
            &format!("{} more notifications", count - 1),
            "banner-more",
        ));
    }
    row.append(&text);
    c.append(&row);
    outer.append(&c);
    for edge in ["banner-edge-1", "banner-edge-2"]
        .iter()
        .take((count as usize).saturating_sub(1).min(2))
    {
        let e = w::vbox(0);
        e.add_css_class("banner-edge");
        e.add_css_class(edge);
        outer.append(&e);
    }
    outer
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
    // Width first: a Fill-height stack would ask the 2-line bodies for width-for-height and widen.
    stack.set_valign(gtk::Align::Start);
    stack.set_halign(gtk::Align::End);
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
        // One banner per app, newest group on top; a group's ids leave together.
        struct Group {
            app: String,
            ids: Vec<u32>,
            rev: gtk::Revealer,
            generation: u32,
        }
        let groups: Rc<RefCell<Vec<Group>>> = Default::default();
        let next_gen = Rc::new(std::cell::Cell::new(0u32));
        // Removes the group holding `rev`; returns its ids.
        let drop_group = {
            let (groups, stack, banners) = (groups.clone(), stack.clone(), banners.clone());
            move |rev: &gtk::Revealer| -> Vec<u32> {
                let mut g = groups.borrow_mut();
                let Some(i) = g.iter().position(|x| &x.rev == rev) else {
                    return vec![];
                };
                let gr = g.remove(i);
                stack.remove(&gr.rev);
                if g.is_empty() {
                    banners.set_visible(false);
                }
                gr.ids
            }
        };
        while let Ok(ev) = rx.recv().await {
            let n = match ev {
                Event::Show(n) => n,
                Event::Close(id) => {
                    let rev = {
                        let mut g = groups.borrow_mut();
                        let Some(gr) = g.iter_mut().find(|x| x.ids.contains(&id)) else {
                            continue;
                        };
                        gr.ids.retain(|&x| x != id);
                        gr.ids.is_empty().then(|| gr.rev.clone())
                    };
                    if let Some(rev) = rev {
                        drop_group(&rev);
                    }
                    continue;
                }
            };
            center_list.prepend(&card(&n));
            let generation = next_gen.get().wrapping_add(1);
            next_gen.set(generation);
            let shown: Vec<(String, u32)> = groups
                .borrow()
                .iter()
                .map(|g| (g.app.clone(), g.ids.len() as u32))
                .collect();
            let rev = match place(&shown, &n.app) {
                Place::Stack { index, count } => {
                    let mut g = groups.borrow_mut();
                    let gr = &mut g[index];
                    if !gr.ids.contains(&n.id) {
                        gr.ids.push(n.id);
                    }
                    gr.generation = generation;
                    debug_assert!(count >= gr.ids.len() as u32);
                    gr.rev.set_child(Some(&banner(&n, gr.ids.len() as u32)));
                    gr.rev.clone()
                }
                Place::Top => {
                    let rev = gtk::Revealer::builder()
                        .transition_type(gtk::RevealerTransitionType::SlideLeft)
                        .child(&banner(&n, 1))
                        .build();
                    let click = gtk::GestureClick::new();
                    let (r2, drop2, closed2) = (rev.clone(), drop_group.clone(), closed.clone());
                    click.connect_released(move |_, _, _, _| {
                        for id in drop2(&r2) {
                            closed2(id, CloseReason::Dismissed);
                        }
                    });
                    rev.add_controller(click);
                    stack.prepend(&rev);
                    groups.borrow_mut().insert(
                        0,
                        Group {
                            app: n.app.clone(),
                            ids: vec![n.id],
                            rev: rev.clone(),
                            generation,
                        },
                    );
                    banners.set_visible(true);
                    let r2 = rev.clone();
                    glib::idle_add_local_once(move || r2.set_reveal_child(true));
                    rev
                }
            };
            if let Some(ms) = banner_ms(&n) {
                let (groups, drop_group, closed) =
                    (groups.clone(), drop_group.clone(), closed.clone());
                glib::timeout_add_local_once(
                    std::time::Duration::from_millis(ms as u64),
                    move || {
                        // Only the group's latest note expires it (a newer note has its own timer).
                        let live = groups
                            .borrow()
                            .iter()
                            .any(|g| g.rev == rev && g.generation == generation);
                        if live {
                            for id in drop_group(&rev) {
                                closed(id, CloseReason::Expired);
                            }
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
    fn banners_group_per_app() {
        let shown = vec![("Mail".to_string(), 1), ("Music".to_string(), 3)];
        for (app, want) in [
            ("Mail", Place::Stack { index: 0, count: 2 }),
            ("music", Place::Stack { index: 1, count: 4 }),
            ("Calendar", Place::Top),
            ("", Place::Top),
        ] {
            assert_eq!(place(&shown, app), want, "{app}");
        }
        assert_eq!(place(&[], "Mail"), Place::Top);
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
