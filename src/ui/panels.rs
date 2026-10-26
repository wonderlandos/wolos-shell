//! Wallpaper, OSD, clipboard, session menu, window switcher.
use crate::kit::{
    self,
    surface::{Keyboard, Spec},
    widgets as w,
};
use gtk::{gdk, glib, prelude::*};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const WALLPAPERS: [&str; 2] = [
    "/usr/local/share/wol/wallpapers/wonderland-dusk.svg",
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../design/wallpapers/wonderland-dusk.svg"
    ),
];

pub fn wallpaper(app: &gtk::Application) -> gtk::ApplicationWindow {
    let win = super::window(app, "wallpaper", Spec::wallpaper());
    // Decoded off the main thread: the CSS gradient shows until the picture is ready (fast first frame).
    if let Some(p) = WALLPAPERS.iter().find(|p| std::path::Path::new(p).exists()) {
        let pic = gtk::Picture::new();
        pic.set_content_fit(gtk::ContentFit::Cover);
        win.set_child(Some(&pic));
        let p = p.to_string();
        glib::spawn_future_local(async move {
            if let Ok(Ok(t)) =
                gtk::gio::spawn_blocking(move || gdk::Texture::from_filename(p)).await
            {
                pic.set_paintable(Some(&t));
            }
        });
    }
    win
}

/// Panel window: transparent overlay layer, glass content, Esc hides.
fn panel(
    app: &gtk::Application,
    class: &str,
    ns: &'static str,
    anchor: [bool; 4],
    child: &impl IsA<gtk::Widget>,
) -> gtk::Window {
    let win = super::window(app, "panel", Spec::panel(ns, anchor, Keyboard::OnDemand));
    win.add_css_class(class);
    win.set_child(Some(child));
    let keys = gtk::EventControllerKey::new();
    let w2 = win.clone();
    keys.connect_key_pressed(move |_, k, _, _| {
        if k == gdk::Key::Escape {
            w2.set_visible(false);
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    win.add_controller(keys);
    win.upcast()
}

pub struct Osd {
    pub window: gtk::Window,
    icon: gtk::Image,
    bar: gtk::ProgressBar,
    hide_gen: Cell<u32>,
}

impl Osd {
    pub fn build(app: &gtk::Application) -> Rc<Osd> {
        let g = w::glass_panel(10);
        let row = w::hbox(12);
        let icon = gtk::Image::new();
        icon.set_pixel_size(24);
        let bar = gtk::ProgressBar::new();
        bar.set_hexpand(true);
        bar.set_valign(gtk::Align::Center);
        row.append(&icon);
        row.append(&bar);
        g.append(&row);
        let window = panel(app, "osd", "wol-panel-osd", [false, true, false, false], &g);
        Rc::new(Osd {
            window,
            icon,
            bar,
            hide_gen: Cell::new(0),
        })
    }

    /// Shows for 1.5 s; a newer show extends it.
    pub fn show(self: &Rc<Self>, icon: &str, level: u8) {
        self.icon.set_icon_name(Some(icon));
        self.bar.set_fraction(level as f64 / 100.0);
        self.window.set_visible(true);
        let seq = self.hide_gen.get() + 1;
        self.hide_gen.set(seq);
        let me = self.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(1500), move || {
            if me.hide_gen.get() == seq {
                me.window.set_visible(false);
            }
        });
    }
}

/// Clipboard history: watches the display clipboard (event-driven), click copies back.
pub fn clipboard(app: &gtk::Application) -> gtk::Window {
    let g = w::glass_panel(10);
    let head = w::hbox(8);
    head.append(&w::label("Clipboard", "strong"));
    head.append(&w::spacer());
    let list = w::vbox(2);
    let scroll = gtk::ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    let history: Rc<RefCell<Vec<String>>> = Default::default();
    let render = {
        let (list, history) = (list.clone(), history.clone());
        Rc::new(move || {
            while let Some(c) = list.first_child() {
                list.remove(&c);
            }
            let h = history.borrow();
            if h.is_empty() {
                list.append(&w::label("Copied text shows up here", "dim"));
            }
            for e in h.iter() {
                let row = w::list_row("edit-paste-symbolic", &crate::clipboard::preview(e), "");
                let e = e.clone();
                row.connect_clicked(move |b| b.display().clipboard().set_text(&e));
                list.append(&row);
            }
        })
    };
    let (h2, r2) = (history.clone(), render.clone());
    head.append(&w::button("Clear", "secondary", move || {
        h2.borrow_mut().clear();
        r2();
    }));
    g.append(&head);
    g.append(&scroll);
    render();
    let cb = gdk::Display::default().expect("display").clipboard();
    cb.connect_changed(move |cb| {
        let (history, render) = (history.clone(), render.clone());
        cb.read_text_async(gio_none(), move |r| {
            if let Ok(Some(t)) = r {
                if crate::clipboard::push(&mut history.borrow_mut(), &t) {
                    render();
                }
            }
        });
    });
    panel(app, "clipboard", "wol-panel-clipboard", [false; 4], &g)
}

fn gio_none() -> Option<&'static gtk::gio::Cancellable> {
    None
}

pub fn session(app: &gtk::Application) -> gtk::Window {
    let g = w::glass_panel(10);
    let row = w::hbox(8);
    for (icon, label, action) in [
        ("system-lock-screen-symbolic", "Lock", "lock"),
        ("weather-clear-night-symbolic", "Sleep", "sleep"),
        ("system-reboot-symbolic", "Restart", "restart"),
        ("system-shutdown-symbolic", "Shut Down", "shutdown"),
        ("system-log-out-symbolic", "Log Out", "logout"),
    ] {
        let b = w::vbox(6);
        b.append(&gtk::Image::from_icon_name(icon));
        let l = w::label(label, "");
        l.set_xalign(0.5);
        b.append(&l);
        let btn = gtk::Button::builder().child(&b).build();
        btn.add_css_class("session-item");
        btn.connect_clicked(move |b| {
            if let Some(win) = b.root().and_downcast::<gtk::Window>() {
                win.set_visible(false);
            }
            if let Some(argv) = crate::clipboard::session_command(action) {
                kit::spawn(&argv.iter().map(|s| s.to_string()).collect::<Vec<_>>());
            }
        });
        row.append(&btn);
    }
    g.append(&row);
    panel(app, "session", "wol-panel-session", [false; 4], &g)
}

/// Window switcher: one tile per Hyprland client, rebuilt each time it is shown.
pub fn switcher(app: &gtk::Application) -> gtk::Window {
    let g = w::glass_panel(10);
    let row = w::hbox(8);
    g.append(&row);
    let win = panel(app, "switcher", "wol-panel-switcher", [false; 4], &g);
    win.connect_show(move |_| {
        while let Some(c) = row.first_child() {
            row.remove(&c);
        }
        let clients = crate::hypr::clients();
        if clients.is_empty() {
            row.append(&w::label("No open windows", "dim"));
        }
        for c in clients {
            let b = w::vbox(6);
            let img = gtk::Image::from_icon_name(&c.class.to_lowercase());
            img.set_pixel_size(64);
            b.append(&img);
            let l = w::label(&crate::clipboard::preview(&c.title), "dim");
            l.set_max_width_chars(14);
            l.set_ellipsize(gtk::pango::EllipsizeMode::End);
            b.append(&l);
            let btn = gtk::Button::builder().child(&b).build();
            btn.add_css_class("session-item");
            let addr = c.address;
            btn.connect_clicked(move |b| {
                let _ = crate::hypr::request(&format!("dispatch focuswindow address:{addr}"));
                if let Some(w) = b.root().and_downcast::<gtk::Window>() {
                    w.set_visible(false);
                }
            });
            row.append(&btn);
        }
    });
    win
}
