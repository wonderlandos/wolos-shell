// SPDX-License-Identifier: GPL-2.0-or-later
//! The Moon Gate's day-to-night transition on the real pictures and the software renderer
//! (`kit::gate`, `ui::panels::CrossFade::show_morph`): the morph starts on the day picture and
//! ends on the night one, the ring lights up ahead of the ribbons, no frame is made on the GTK
//! thread (a worker makes them, QA-A7-9), and the GTK thread's part of one frame of the whole
//! wallpaper (the snapshot and GSK's cairo renderer at 1920x1080) is timed.
//! The numbers are printed; `WOL_GATE_BUDGET=1 cargo test --release --test gate_transition`
//! also asserts the 16 ms budget (a timing, so not by default). Runs under a private
//! `gtk4-broadwayd` like `tests/golden.rs`; skips without the design's brand wallpapers.
use gtk::{gdk, glib, prelude::*};
use shell::kit::{anim::Clock, gate};
use shell::ui::panels::{CrossFade, morph_between, wallpaper_texture};
use std::path::Path;
use std::time::{Duration, Instant};

const SIZE: (i32, i32) = (1920, 1080);
const MS: u64 = 1_000_000;

fn pump(ms: u64) {
    let end = Instant::now() + Duration::from_millis(ms);
    let ctx = glib::MainContext::default();
    while Instant::now() < end {
        while ctx.iteration(false) {}
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn gate_svg(mode: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../design/brand/wallpapers/wolos-gate-{mode}-4k.svg"
    ))
}

/// BGRA pixels of `t` at the morph's working size.
fn pixels(t: &gdk::Texture) -> Vec<u8> {
    let (w, h) = (t.width() as usize, t.height() as usize);
    let mut buf = vec![0u8; w * h * 4];
    t.download(&mut buf, w * 4);
    let (ww, wh) = gate::work_size(w, h);
    gate::resample(&buf, w, h, w * 4, ww, wh)
}

fn max_diff(a: &[u8], b: &[u8]) -> u8 {
    a.iter()
        .zip(b)
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0)
}

fn luma(px: &[u8], w: usize, fx: f32, fy: f32, h: usize) -> f32 {
    let (x, y) = ((fx * w as f32) as usize, (fy * h as f32) as usize);
    let i = (y * w + x) * 4;
    0.0722 * px[i] as f32 + 0.7152 * px[i + 1] as f32 + 0.2126 * px[i + 2] as f32
}

fn start_broadway() -> impl Drop {
    struct Kill(std::process::Child);
    // Drop: kills the private broadwayd the test started.
    impl Drop for Kill {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let n = (0..200)
        .map(|i| 40 + (std::process::id() + i) % 200)
        .find(|n| {
            !Path::new(&runtime)
                .join(format!("broadway{}.socket", n + 1))
                .exists()
                && std::net::TcpListener::bind(("0.0.0.0", (18000 + n) as u16)).is_ok()
        })
        .expect("a free Broadway display");
    let (display, port) = (format!(":{n}"), (18000 + n).to_string());
    let child = std::process::Command::new("gtk4-broadwayd")
        .args([display.as_str(), "--port", port.as_str()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("gtk4-broadwayd (GTK 4 package) is required");
    let sock = Path::new(&runtime).join(format!("broadway{}.socket", n + 1));
    for _ in 0..100 {
        if std::os::unix::net::UnixStream::connect(&sock).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    unsafe {
        std::env::set_var("GDK_BACKEND", "broadway");
        std::env::set_var("BROADWAY_DISPLAY", &display);
    }
    Kill(child)
}

fn main() {
    if !gate_svg("day").exists() {
        eprintln!("skip: no design/brand/wallpapers (the design tree predates the Moon Gate)");
        return;
    }
    let _broadway = start_broadway();
    gtk::init().expect("gtk init");

    // The desktop's pictures: rendered once at the display's width and cached
    // (`panels::wallpaper_texture`); the second ask is what a day/night change waits for.
    let cache = std::env::temp_dir().join(format!("wol-gate-transition-{}", std::process::id()));
    unsafe { std::env::set_var("XDG_CACHE_HOME", &cache) };
    let t = Instant::now();
    let full = gdk::Texture::from_filename(gate_svg("day")).expect("day picture at 4k");
    let decode = t.elapsed();
    assert_eq!(full.width(), 3840);
    let t = Instant::now();
    let day = wallpaper_texture(&gate_svg("day"), SIZE.0).expect("day picture");
    let night = wallpaper_texture(&gate_svg("night"), SIZE.0).expect("night picture");
    let first = t.elapsed() / 2;
    let t = Instant::now();
    let again = wallpaper_texture(&gate_svg("night"), SIZE.0).expect("night again");
    let cached = t.elapsed();
    assert_eq!((day.width(), day.height()), (SIZE.0, SIZE.1));
    assert_eq!(
        (again.width(), again.height()),
        (night.width(), night.height())
    );
    assert!(
        cached < first,
        "the cached picture is quicker than a render"
    );
    let t = Instant::now();
    let m = morph_between(&day, &night).expect("same size");
    let prepare = t.elapsed();

    // Ends: the two pictures (at the working size); the ring leads the dye.
    let (a, b) = (pixels(&day), pixels(&night));
    assert!(
        max_diff(&m.frame(0.0), &a) <= 3,
        "frame 0 is the day picture"
    );
    assert!(
        max_diff(&m.frame(1.0), &b) <= 3,
        "frame 1 is the night picture"
    );
    let p = gate::Params::default();
    let third = m.frame(1.0 / 3.0);
    let share = |px: &[u8], fx: f32, fy: f32| {
        let (l0, l1) = (luma(&a, m.w, fx, fy, m.h), luma(&b, m.w, fx, fy, m.h));
        (luma(px, m.w, fx, fy, m.h) - l0) / (l1 - l0)
    };
    // The ring's top, and the ground at the upper left (ribbon dye and backdrop).
    let ring_top = p.ring_cy - p.ring_r * p.ring_squash;
    let (ring, ground) = (share(&third, p.ring_cx, ring_top), share(&third, 0.04, 0.5));
    println!("at a third: ring {ring:.2} of the way, ground {ground:.2}");
    assert!(ring > ground, "the ring lights up ahead of the dye");

    // The whole wallpaper, frame by frame, as the desktop draws it.
    let clock = Clock::manual();
    let fade = CrossFade::new(&clock);
    let win = gtk::Window::new();
    win.set_default_size(SIZE.0, SIZE.1);
    win.set_child(Some(&fade.root));
    win.present();
    pump(300);
    fade.show(&day);
    // QA-A7-9: the frames of the transition are made off the GTK thread, so none is made here
    // while it runs (the test's own calls above are counted before this point).
    let (made_before, all_before) = (gate::frames_made_here(), gate::frames_made());
    fade.show_morph(m, &day, &night);
    let paintable = gtk::WidgetPaintable::new(Some(&win));
    let renderer = gtk::gsk::CairoRenderer::new();
    renderer
        .realize(None::<&gdk::Surface>)
        .expect("cairo renderer");
    let area = gtk::graphene::Rect::new(0.0, 0.0, SIZE.0 as f32, SIZE.1 as f32);
    let (mut worst, mut sum, mut frames) = (Duration::ZERO, Duration::ZERO, 0u32);
    let mut step_ms = 0u64;
    while step_ms <= gate::MS + 16 {
        let t = Instant::now();
        clock.step(step_ms * MS);
        // The frames are made by a worker and land through the main loop; the time given to it
        // here is not part of the frame's cost on the GTK thread.
        let spent = t.elapsed();
        pump(12);
        let t = Instant::now() - spent;
        let snap = gtk::Snapshot::new();
        paintable.snapshot(&snap, SIZE.0 as f64, SIZE.1 as f64);
        if let Some(node) = snap.to_node() {
            std::hint::black_box(renderer.render_texture(node, Some(&area)));
        }
        let d = t.elapsed();
        worst = worst.max(d);
        sum += d;
        frames += 1;
        step_ms += 16;
    }
    renderer.unrealize();
    assert_eq!(
        gate::frames_made_here(),
        made_before,
        "a transition frame was made on the GTK thread"
    );
    assert!(
        gate::frames_made() > all_before,
        "the worker made frames for the transition"
    );
    assert_eq!(fade.morph_progress(), 1.0, "the morph ran to its end");
    assert_eq!(
        fade.current().as_ref(),
        Some(night.upcast_ref::<gdk::Paintable>()),
        "the real night picture is on screen at the end"
    );
    let mean = sum.as_secs_f64() * 1000.0 / frames as f64;
    println!(
        "gate transition {}x{}: {frames} frames, mean {mean:.2} ms, worst {:.2} ms per frame \
         (morph step + snapshot + GSK cairo renderer); once: prepare {:.0} ms",
        SIZE.0,
        SIZE.1,
        worst.as_secs_f64() * 1000.0,
        prepare.as_secs_f64() * 1000.0,
    );
    println!(
        "gate picture: 4k SVG at its own size {:.0} ms; at {} px first render {:.0} ms, from the cache {:.0} ms",
        decode.as_secs_f64() * 1000.0,
        SIZE.0,
        first.as_secs_f64() * 1000.0,
        cached.as_secs_f64() * 1000.0,
    );
    let _ = std::fs::remove_dir_all(&cache);
    // Timing depends on the machine and its load: the budget is asserted on request only.
    if std::env::var_os("WOL_GATE_BUDGET").is_some() {
        assert!(
            mean < 16.0,
            "mean frame over the 16 ms budget: {mean:.2} ms"
        );
    }
    println!("gate_transition: ok");
}
