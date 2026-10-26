//! Lock screen (ext-session-lock, ADR 0022/0023) and login window (ADR 0024): clock, avatar, password.
use crate::kit::{self, widgets as w};
use crate::login::{self, Power, Verdict};
use gtk::{glib, prelude::*};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const WALLPAPER_DIRS: [&str; 2] = [
    "/usr/local/share/wol/wallpapers",
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../design/wallpapers"),
];
/// The backdrop is decoded small, blurred on the CPU (edge-clamped: no dark rim) and scaled up.
const BACKDROP_PX: (i32, i32) = (480, 270);
const BLUR_RADIUS: usize = 6;
const BLUR_PASSES: usize = 3;

/// Separable box blur, edge clamped, in place; `c` channels per pixel, rows tightly packed.
/// Three passes approximate a Gaussian.
pub fn box_blur(px: &mut [u8], w: usize, h: usize, c: usize, r: usize) {
    let mut line = Vec::new();
    let mut pass = |px: &mut [u8], n: usize, lines: usize, step: usize, next: usize| {
        for l in 0..lines {
            let base = l * next;
            for ch in 0..c {
                line.clear();
                line.extend((0..n).map(|i| px[base + i * step + ch] as u32));
                let at = |i: isize| line[i.clamp(0, n as isize - 1) as usize];
                let d = (2 * r + 1) as u32;
                let mut sum: u32 = (-(r as isize)..=r as isize).map(at).sum();
                for i in 0..n {
                    px[base + i * step + ch] = ((sum + d / 2) / d) as u8;
                    sum += at(i as isize + r as isize + 1);
                    sum -= at(i as isize - r as isize);
                }
            }
        }
    };
    for _ in 0..BLUR_PASSES {
        pass(px, w, h, c, w * c);
        pass(px, h, w, w * c, c);
    }
}

/// Wallpaper -> small blurred texture. Blocking; run off the GTK thread.
fn blurred_wallpaper(path: &Path) -> Option<gtk::gdk::MemoryTexture> {
    let (bw, bh) = BACKDROP_PX;
    let pb = gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(path, bw, bh, false).ok()?;
    let (w, h, c) = (
        pb.width() as usize,
        pb.height() as usize,
        pb.n_channels() as usize,
    );
    let stride = pb.rowstride() as usize;
    let bytes = pb.read_pixel_bytes();
    let mut px: Vec<u8> = (0..h)
        .flat_map(|y| &bytes[y * stride..y * stride + w * c])
        .copied()
        .collect();
    box_blur(&mut px, w, h, c, BLUR_RADIUS);
    let format = if c == 4 {
        gtk::gdk::MemoryFormat::R8g8b8a8
    } else {
        gtk::gdk::MemoryFormat::R8g8b8
    };
    Some(gtk::gdk::MemoryTexture::new(
        w as i32,
        h as i32,
        format,
        &glib::Bytes::from_owned(px),
        w * c,
    ))
}

/// Lock/login wallpaper: night in dark, day in light; installed copy first.
pub fn wallpaper_file(dark: bool) -> Option<PathBuf> {
    let name = if dark {
        "wonderland-night.svg"
    } else {
        "wonderland-day.svg"
    };
    WALLPAPER_DIRS
        .iter()
        .map(|d| Path::new(d).join(name))
        .find(|p| p.exists())
}

fn prefers_dark() -> bool {
    gtk::Settings::default().is_some_and(|s| s.is_gtk_application_prefer_dark_theme())
}

/// Blurred wallpaper, decoded off the main thread; follows the light/dark preference.
fn backdrop() -> gtk::Picture {
    let pic = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .can_shrink(true)
        .build();
    pic.add_css_class("lock-backdrop");
    let load = {
        let pic = pic.clone();
        move || {
            let Some(path) = wallpaper_file(prefers_dark()) else {
                return;
            };
            let pic = pic.clone();
            glib::spawn_future_local(async move {
                let r = gtk::gio::spawn_blocking(move || blurred_wallpaper(&path)).await;
                if let Ok(Some(t)) = r {
                    pic.set_paintable(Some(&t));
                }
            });
        }
    };
    load();
    if let Some(s) = gtk::Settings::default() {
        s.connect_gtk_application_prefer_dark_theme_notify(move |_| load());
    }
    pic
}

/// Wallpaper + scrim under `content`.
fn screen(content: &impl IsA<gtk::Widget>) -> gtk::Widget {
    let o = gtk::Overlay::new();
    o.add_css_class("lock-screen");
    let tone = {
        let o = o.clone();
        move || {
            if prefers_dark() {
                o.remove_css_class("light")
            } else {
                o.add_css_class("light")
            }
        }
    };
    tone();
    if let Some(s) = gtk::Settings::default() {
        s.connect_gtk_application_prefer_dark_theme_notify(move |_| tone());
    }
    o.set_child(Some(&backdrop()));
    let scrim = w::vbox(0);
    scrim.add_css_class("lock-scrim");
    scrim.append(content);
    content.set_vexpand(true);
    o.add_overlay(&scrim);
    o.upcast()
}

/// Clock on top, `bottom` (user panel, power row) at the foot.
fn body(middle: &impl IsA<gtk::Widget>, footer: Option<&gtk::Widget>) -> gtk::Box {
    let b = w::vbox(0);
    b.add_css_class("lock-body");
    b.append(&w::clock_header());
    let s = w::vbox(0);
    s.set_vexpand(true);
    b.append(&s);
    b.append(middle);
    if let Some(f) = footer {
        b.append(f);
    }
    b
}

/// The lock screen for one user panel.
pub fn lock_screen(panel: &w::UserPanel) -> gtk::Widget {
    screen(&body(&panel.root, None))
}

/// Login window: user tiles, then the picked user's password pill; power row at the foot.
pub struct Login {
    pub root: gtk::Widget,
    pub stack: gtk::Stack,
    pub panel: Rc<w::UserPanel>,
    pub back: gtk::Button,
    selected: Rc<RefCell<String>>,
}

impl Login {
    pub fn new(users: &[String]) -> Rc<Login> {
        let panel = Rc::new(w::UserPanel::new(users.first().map_or("", |u| u.as_str())));
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .vhomogeneous(false)
            .hhomogeneous(false)
            .build();
        let tiles = w::hbox(w::S8);
        tiles.set_halign(gtk::Align::Center);
        tiles.add_css_class("login-tiles");
        stack.add_named(&tiles, Some("users"));
        let pw = w::vbox(w::S4);
        pw.append(&panel.root);
        let (back_box, back) = action("go-previous-symbolic", "Back", || {});
        pw.append(&back_box);
        stack.add_named(&pw, Some("password"));
        let foot = w::hbox(w::S8);
        foot.set_halign(gtk::Align::Center);
        foot.add_css_class("login-power");
        for p in Power::ALL {
            foot.append(&action(p.icon(), p.label(), move || p.run()).0);
        }
        let root = screen(&body(&stack, Some(foot.upcast_ref())));
        let l = Rc::new(Login {
            root,
            stack,
            panel,
            back,
            selected: Default::default(),
        });
        for u in users {
            let t = gtk::Button::builder().build();
            t.add_css_class("login-tile");
            let c = w::vbox(w::S3);
            c.append(&w::avatar(u, w::AVATAR_XL));
            c.append(&w::centered(u, "lock-name"));
            t.set_child(Some(&c));
            let (l2, u) = (Rc::downgrade(&l), u.clone());
            t.connect_clicked(move |_| {
                if let Some(l) = l2.upgrade() {
                    l.pick(&u)
                }
            });
            tiles.append(&t);
        }
        let l2 = Rc::downgrade(&l);
        l.back.connect_clicked(move |_| {
            if let Some(l) = l2.upgrade() {
                l.stack.set_visible_child_name("users")
            }
        });
        if let [only] = users {
            l.pick(only);
            if let Some(b) = l.back.parent() {
                b.set_visible(false);
            }
        }
        l
    }

    /// Shows `user`'s password pill.
    pub fn pick(&self, user: &str) {
        *self.selected.borrow_mut() = user.to_string();
        self.panel.set_user(user);
        self.stack.set_visible_child_name("password");
        self.panel.field.grab_focus();
    }

    pub fn selected(&self) -> String {
        self.selected.borrow().clone()
    }
}

/// Round glass icon with a caption (power row).
fn action(icon: &str, label: &str, run: impl Fn() + 'static) -> (gtk::Box, gtk::Button) {
    let v = w::vbox(w::S2);
    let b = gtk::Button::builder()
        .child(&w::icon(icon, w::ICON))
        .tooltip_text(label)
        .halign(gtk::Align::Center)
        .build();
    b.add_css_class("lock-round");
    b.connect_clicked(move |_| run());
    v.append(&b);
    v.append(&w::centered(label, "lock-caption"));
    (v, b)
}

pub fn user_name() -> String {
    std::env::var("USER").unwrap_or_else(|_| "user".into())
}

/// Wires Enter -> blocking check off the main thread -> `ok` or shake.
fn wire<C: FnOnce(String) -> Verdict + Send + 'static>(
    panel: Rc<w::UserPanel>,
    check: impl Fn() -> C + 'static,
    ok: impl Fn() + Clone + 'static,
) {
    let check = Rc::new(check);
    let p2 = panel.clone();
    panel.field.connect_activate(move |f| {
        let pw = f.text().to_string();
        f.set_sensitive(false);
        p2.message.set_label("");
        let (p, check, ok) = (p2.clone(), check(), ok.clone());
        glib::spawn_future_local(async move {
            let v = gtk::gio::spawn_blocking(move || check(pw))
                .await
                .unwrap_or(Verdict::Error);
            p.field.set_sensitive(true);
            if v == Verdict::Ok {
                ok()
            } else {
                p.reject(v.message());
                p.field.grab_focus();
            }
        });
    });
}

pub fn run_lock() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("org.wolos.lock")
        .build();
    app.connect_activate(|app| {
        kit::style::install("");
        let win = gtk::ApplicationWindow::new(app);
        win.add_css_class("lock-window");
        let panel = Rc::new(w::UserPanel::new(&user_name()));
        win.set_child(Some(&lock_screen(&panel)));
        kit::snapshot_on_map(&win, "lock");
        let unlock = lock_surface(app, &win);
        let app2 = app.clone();
        wire(
            panel.clone(),
            || login::authcheck_owned,
            move || {
                unlock();
                app2.quit();
            },
        );
        panel.field.grab_focus();
    });
    app.run_with_args::<&str>(&[])
}

/// Session lock on every monitor (this window on the first); returns the unlock action.
#[cfg(feature = "layer-shell")]
fn lock_surface(app: &gtk::Application, win: &gtk::ApplicationWindow) -> Rc<dyn Fn()> {
    use gtk::gdk;
    if !gtk4_session_lock::is_supported() {
        win.fullscreen();
        win.present();
        return Rc::new(|| {});
    }
    let lock = gtk4_session_lock::Instance::new();
    let (app, first) = (app.clone(), win.clone());
    let used = std::cell::Cell::new(false);
    lock.connect_monitor(move |l, m: &gdk::Monitor| {
        let w = if !used.replace(true) {
            first.clone()
        } else {
            let w = gtk::ApplicationWindow::new(&app);
            w.add_css_class("lock-window");
            w
        };
        l.assign_window_to_monitor(&w, m);
    });
    lock.lock();
    Rc::new(move || lock.unlock())
}

#[cfg(not(feature = "layer-shell"))]
fn lock_surface(_app: &gtk::Application, win: &gtk::ApplicationWindow) -> Rc<dyn Fn()> {
    eprintln!("wol-lock: built without layer-shell: NOT a secure lock (dev window)");
    win.present();
    Rc::new(|| {})
}

pub fn run_login() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("org.wolos.loginwindow")
        .build();
    app.connect_activate(|app| {
        kit::style::install("");
        let win = gtk::ApplicationWindow::new(app);
        win.add_css_class("lock-window");
        let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
        let shells = std::fs::read_to_string("/etc/shells").unwrap_or_default();
        let l = Login::new(&login::users(&passwd, &shells));
        win.set_child(Some(&l.root));
        let l2 = l.clone();
        wire(
            l.panel.clone(),
            move || {
                let user = l2.selected();
                move |pw: String| login::login(login::LOGIN_SOCKET, &user, &pw)
            },
            || std::process::exit(0),
        );
        let keys = gtk::EventControllerKey::new();
        let l3 = l.clone();
        keys.connect_key_pressed(move |_, k, _, _| {
            if k == gtk::gdk::Key::Escape && l3.back.is_mapped() {
                l3.stack.set_visible_child_name("users");
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        win.add_controller(keys);
        kit::snapshot_on_map(&win, "loginwindow");
        win.fullscreen();
        win.present();
    });
    app.run_with_args::<&str>(&[])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blur_spreads_and_keeps_flat_areas() {
        let (w, h, c) = (9, 9, 4);
        let mut flat = vec![100u8; w * h * c];
        box_blur(&mut flat, w, h, c, 2);
        assert!(flat.iter().all(|&v| v == 100));
        let mut dot = vec![0u8; w * h * c];
        dot[(4 * w + 4) * c] = 255;
        box_blur(&mut dot, w, h, c, 1);
        let at = |x: usize, y: usize| dot[(y * w + x) * c];
        assert!(at(4, 4) < 255 && at(4, 4) > 0);
        assert_eq!(at(3, 4), at(5, 4));
        assert_eq!(at(4, 3), at(4, 5));
        assert_eq!(at(0, 0), 0);
    }

    #[test]
    fn wallpaper_follows_variant() {
        for (dark, name) in [
            (true, "wonderland-night.svg"),
            (false, "wonderland-day.svg"),
        ] {
            assert!(wallpaper_file(dark).unwrap().ends_with(name));
        }
    }
}
