// SPDX-License-Identifier: GPL-2.0-or-later
//! Pictures render on `kit::render`'s own threads, not on GIO's shared pool. With a gate's 4k
//! render on `gio::spawn_blocking`, a Wallpaper page of tiles held every pool thread for seconds
//! and the style load and Settings' Modules page queued behind it: the golden run's setup check
//! found no Install row, then no focus ring (golden_cases/focus.rs). Own process: the slow jobs
//! below fill the pool of the process they run in. No display needed.
use gtk::glib;
use shell::kit::render::{self, Lane};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SLOW: Duration = Duration::from_millis(400);
/// More jobs than GIO's pool has threads (16).
const JOBS: usize = 40;

/// One test fills the pool at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// `JOBS` slow jobs started with `start`, then one short read on the pool; how long the read took.
fn short_read_behind_pictures(
    start: impl Fn(&glib::MainContext, Box<dyn FnOnce() + Send>),
) -> Duration {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let ctx = glib::MainContext::new();
    for _ in 0..JOBS {
        start(&ctx, Box::new(|| std::thread::sleep(SLOW)));
    }
    let t = Instant::now();
    let read = ctx.block_on(async { gtk::gio::spawn_blocking(|| 7).await });
    assert_eq!(read.ok(), Some(7));
    t.elapsed()
}

/// The premise, so the test below means something: on the shared pool the short read waits.
#[test]
fn on_the_shared_pool_a_short_read_waits_behind_pictures() {
    let waited = short_read_behind_pictures(|ctx, job| {
        ctx.spawn(async move {
            let _ = gtk::gio::spawn_blocking(job).await;
        });
    });
    assert!(waited >= SLOW, "the pool was free after {waited:?}");
}

/// The same load on a lane leaves the pool free, and the lane renders one picture at a time.
#[test]
fn pictures_on_the_lane_leave_the_shared_pool_free() {
    let (now, most) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let waited = short_read_behind_pictures(|ctx, job| {
        let (now, most) = (now.clone(), most.clone());
        ctx.spawn(async move {
            render::on(Lane::Previews, move || {
                most.fetch_max(now.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
                job();
                now.fetch_sub(1, Ordering::SeqCst);
            })
            .await;
        });
    });
    assert!(
        waited < SLOW / 2,
        "a short read waited {waited:?} behind the pictures"
    );
    assert!(most.load(Ordering::SeqCst) <= 1, "one picture at a time");
}
