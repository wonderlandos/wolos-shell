//! org.freedesktop.Notifications server (zbus) + banners (top right, 5 s) + Notification Center panel.
use crate::kit::{
    surface::{Keyboard, Spec},
    widgets as w,
};
use gtk::{glib, prelude::*};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use zbus::zvariant::OwnedValue;

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

struct Server {
    tx: async_channel::Sender<Note>,
    next: AtomicU32,
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
        let _ = self.tx.try_send(Note {
            id,
            app: app_name,
            icon: app_icon,
            summary,
            body,
            critical,
            timeout_ms: expire_timeout,
        });
        id
    }
    fn close_notification(&self, _id: u32) {}
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
    let c = w::glass_panel(2);
    c.set_margin_top(0);
    c.set_margin_bottom(8);
    c.set_margin_start(0);
    c.set_margin_end(0);
    c.add_css_class("notification");
    if n.critical {
        c.add_css_class("critical");
    }
    let row = w::hbox(10);
    let img = gtk::Image::from_icon_name(if n.icon.is_empty() {
        "dialog-information-symbolic"
    } else {
        &n.icon
    });
    img.set_pixel_size(32);
    row.append(&img);
    let text = w::vbox(2);
    text.append(&w::label(&n.app, "dim"));
    text.append(&w::label(&n.summary, "summary"));
    if !n.body.is_empty() {
        let b = w::label(&n.body, "body");
        b.set_wrap(true);
        b.set_max_width_chars(40);
        text.append(&b);
    }
    row.append(&text);
    c.append(&row);
    c
}

/// Starts the D-Bus server; returns the Notification Center panel.
pub fn start(app: &gtk::Application) -> gtk::Window {
    let (tx, rx) = async_channel::unbounded::<Note>();
    glib::spawn_future_local(async move {
        let server = Server {
            tx,
            next: AtomicU32::new(1),
        };
        let built = async {
            zbus::connection::Builder::session()?
                .name("org.freedesktop.Notifications")?
                .serve_at("/org/freedesktop/Notifications", server)?
                .build()
                .await
        }
        .await;
        match built {
            Ok(c) => std::mem::forget(c),
            Err(e) => eprintln!("wol-shell: notifications: {e}"),
        }
    });

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
    let g = w::glass_panel(8);
    g.append(&w::label("Notifications", "title"));
    g.append(
        &gtk::ScrolledWindow::builder()
            .child(&center_list)
            .min_content_height(400)
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
        while let Ok(n) = rx.recv().await {
            center_list.prepend(&card(&n));
            let c = card(&n);
            let click = gtk::GestureClick::new();
            let (s2, c2) = (stack.clone(), c.clone());
            click.connect_released(move |_, _, _, _| s2.remove(&c2));
            c.add_controller(click);
            stack.append(&c);
            banners.set_visible(true);
            if let Some(ms) = banner_ms(&n) {
                let (stack, banners) = (stack.clone(), banners.clone());
                glib::timeout_add_local_once(
                    std::time::Duration::from_millis(ms as u64),
                    move || {
                        if c.parent().is_some() {
                            stack.remove(&c);
                        }
                        if stack.first_child().is_none() {
                            banners.set_visible(false);
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
