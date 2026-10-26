//! Golden screenshots of every surface, light and dark, with the layout check on.
//! Runs under a private `gtk4-broadwayd` (headless). `WOL_GOLDEN_UPDATE=1` rewrites goldens.
//! Renders land in `$CARGO_TARGET_TMPDIR/golden/`.
use gtk::{gdk, glib, prelude::*};
use shell::kit::{self, widgets as w};
use shell::theme::{self, Variant};
use shell::ui;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const WALL: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../design/wallpapers/wonderland-dusk.svg"
);
/// Max share of pixels allowed to differ (any channel > 24) before a golden fails.
const TOLERANCE: f64 = 0.005;

struct Case {
    name: String,
    size: (i32, i32),
    class: &'static str,
    wallpaper: bool,
    build: Box<dyn Fn() -> gtk::Widget>,
}

fn cases() -> Vec<Case> {
    let mut v: Vec<Case> = vec![
        Case {
            name: "menubar".into(),
            size: (900, 28),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let b = ui::menubar::content(&shell::system::Fake);
                b.set_app("Files".into());
                b.root.clone().upcast()
            }),
        },
        Case {
            name: "dock".into(),
            size: (900, 120),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let running = ["foot".to_string(), "wol-files".into()]
                    .into_iter()
                    .collect();
                ui::dock::content(&running).root.clone().upcast()
            }),
        },
        Case {
            name: "controlcenter".into(),
            size: (368, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| ui::control_center::content(Rc::new(shell::system::Fake))),
        },
        Case {
            name: "spotlight".into(),
            size: (728, 1),
            class: "preview",
            wallpaper: true,
            build: Box::new(|| {
                let items = Rc::new(std::cell::RefCell::new(ui::spotlight::index(vec![])));
                ui::spotlight::content(items, "te").0
            }),
        },
        Case {
            name: "lock".into(),
            size: (900, 600),
            class: "lock-window",
            wallpaper: false,
            build: Box::new(|| {
                ui::lock::body(&w::UserPanel::new("Alice Liddell").root, None).upcast()
            }),
        },
    ];
    for (sw, sh, tag) in [(900, 600, "wide"), (480, 360, "narrow")] {
        for p in ui::settings::PAGES {
            v.push(Case {
                name: format!(
                    "settings-{}-{tag}",
                    p.to_lowercase().replace([' ', '&'], "")
                ),
                size: (sw, sh),
                class: "preview-app",
                wallpaper: false,
                build: Box::new(move || {
                    let a = ui::settings::content(
                        &shell::system::Fake,
                        &ui::settings::Host {
                            hostname: "wonderland".into(),
                            users: vec!["alice".into(), "dinah".into()],
                        },
                    );
                    a.select(p);
                    a.root.clone().upcast()
                }),
            });
        }
    }
    v
}

fn pump(ms: u64) {
    let until = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    let ctx = glib::MainContext::default();
    while std::time::Instant::now() < until {
        while ctx.iteration(false) {}
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn render(win: &gtk::Window) -> gdk::Texture {
    let (wd, ht) = (win.width(), win.height());
    let p = gtk::WidgetPaintable::new(Some(win));
    let mut node = None;
    for _ in 0..20 {
        let snap = gtk::Snapshot::new();
        p.snapshot(&snap, wd as f64, ht as f64);
        node = snap.to_node();
        if node.is_some() {
            break;
        }
        win.queue_draw();
        pump(100);
    }
    let node =
        node.unwrap_or_else(|| panic!("render node: window {wd}x{ht} mapped={}", win.is_mapped()));
    let r = gtk::gsk::CairoRenderer::new();
    r.realize(None::<&gdk::Surface>).expect("cairo renderer");
    let t = r.render_texture(
        node,
        Some(&gtk::graphene::Rect::new(0.0, 0.0, wd as f32, ht as f32)),
    );
    r.unrealize();
    t
}

fn pixels(t: &gdk::Texture) -> (i32, i32, Vec<u8>) {
    let mut d = gdk::TextureDownloader::new(t);
    d.set_format(gdk::MemoryFormat::R8g8b8a8);
    let (bytes, stride) = d.download_bytes();
    let (w, h) = (t.width(), t.height());
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h as usize {
        out.extend_from_slice(&bytes[y * stride..y * stride + w as usize * 4]);
    }
    (w, h, out)
}

/// Share of differing pixels, or 1.0 on size mismatch.
fn diff(a: &gdk::Texture, b: &gdk::Texture) -> f64 {
    let ((aw, ah, pa), (bw, bh, pb)) = (pixels(a), pixels(b));
    if (aw, ah) != (bw, bh) {
        return 1.0;
    }
    let bad = pa
        .chunks(4)
        .zip(pb.chunks(4))
        .filter(|(x, y)| x.iter().zip(*y).any(|(p, q)| p.abs_diff(*q) > 24))
        .count();
    bad as f64 / (aw * ah) as f64
}

fn main() {
    let n = 40 + std::process::id() % 200;
    let display = format!(":{n}");
    let port = (18000 + n).to_string();
    /// Kills the private broadwayd on every exit path (panics included).
    struct Kill(std::process::Child);
    impl Drop for Kill {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let broadway = Kill(
        std::process::Command::new("gtk4-broadwayd")
            .args([display.as_str(), "--port", port.as_str()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("gtk4-broadwayd (GTK 4 package) is required"),
    );
    let sock = Path::new(&std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into()))
        .join(format!("broadway{}.socket", n + 1));
    for _ in 0..100 {
        if sock.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    let cfg = std::env::temp_dir().join(format!("wol-golden-{}", std::process::id()));
    unsafe {
        std::env::set_var("GDK_BACKEND", "broadway");
        std::env::set_var("BROADWAY_DISPLAY", &display);
        std::env::set_var("WOL_FAKE_NOW", "1");
        std::env::set_var("XDG_CONFIG_HOME", &cfg);
        std::env::set_var("GTK_THEME", "Adwaita");
    }
    gtk::init().expect("gtk init");
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("golden");
    std::fs::create_dir_all(&out).unwrap();
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let update = std::env::var_os("WOL_GOLDEN_UPDATE").is_some();
    let only = std::env::var("WOL_GOLDEN_ONLY").ok();
    let mut failures = Vec::new();
    for variant in [Variant::Light, Variant::Dark] {
        let t = theme::resolve(
            Some(if variant == Variant::Light {
                "wol-light"
            } else {
                "wol-dark"
            }),
            false,
        );
        let extra = format!(
            "window.preview {{ background: url(\"file://{WALL}\") center / cover; }} window.preview-app {{ background: none; }}"
        );
        kit::style::apply(&t, &extra);
        for c in cases() {
            let name = format!(
                "{}-{}",
                c.name,
                if variant == Variant::Light {
                    "light"
                } else {
                    "dark"
                }
            );
            if only.as_deref().is_some_and(|o| !name.contains(o)) {
                continue;
            }
            let win = gtk::Window::builder()
                .decorated(false)
                .default_width(c.size.0)
                .default_height(c.size.1)
                .build();
            win.add_css_class(c.class);
            if !c.wallpaper && c.class == "preview-app" {
                win.add_css_class("app");
            }
            let child = (c.build)();
            win.set_child(Some(&child));
            win.present();
            pump(900);
            fn count(n: &shell::layout::Node) -> usize {
                1 + n.children.iter().map(count).sum::<usize>()
            }
            let nodes = win
                .child()
                .map(|c| count(&kit::check::tree(&c, win.upcast_ref(), "")))
                .unwrap_or(0);
            assert!(nodes > 3, "{name}: layout tree has {nodes} nodes");
            let mut issues: Vec<String> = kit::check::issues(&win)
                .into_iter()
                .map(|i| i.message)
                .collect();
            if c.size.1 > 1 && (win.width() > c.size.0 || win.height() > c.size.1) {
                issues.push(format!(
                    "content needs {}x{}, surface is {}x{}",
                    win.width(),
                    win.height(),
                    c.size.0,
                    c.size.1
                ));
            }
            let tex = render(&win);
            let png = out.join(format!("{name}.png"));
            tex.save_to_png(&png).unwrap();
            let gold = golden.join(format!("{name}.png"));
            if update {
                std::fs::copy(&png, &gold).unwrap();
            } else if let Ok(g) = gdk::Texture::from_filename(&gold) {
                let d = diff(&tex, &g);
                if d > TOLERANCE {
                    issues.push(format!(
                        "differs from golden by {:.2}% of pixels",
                        d * 100.0
                    ));
                }
            } else {
                issues.push("no golden (run with WOL_GOLDEN_UPDATE=1)".into());
            }
            for i in &issues {
                eprintln!("{name}: {i}");
            }
            if !issues.is_empty() {
                failures.push(name.clone());
            }
            println!(
                "{name}: {} ({nodes} widgets checked)",
                if issues.is_empty() { "ok" } else { "FAIL" }
            );
            win.destroy();
        }
    }
    drop(broadway);
    let _ = std::fs::remove_dir_all(cfg);
    println!("renders: {}", out.display());
    if !failures.is_empty() {
        eprintln!("golden failures: {failures:?}");
        std::process::exit(1);
    }
}
