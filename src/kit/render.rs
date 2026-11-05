// SPDX-License-Identifier: GPL-2.0-or-later
//! Picture work on threads of its own. A 4k gate is ~3000 shapes and takes seconds to render, and
//! on `gio::spawn_blocking` a Wallpaper page of tiles (or a gate change) held every thread of
//! GIO's shared pool: the CSS, the module list and every other page's read waited behind pictures.
//! Here the pictures wait for each other instead, on one worker per [`Lane`], and the pool is
//! left to the short reads it is for.
use std::sync::OnceLock;

/// What a picture is for: lanes do not wait for each other, so a desktop picture is not queued
/// behind the previews of a page that is open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lane {
    /// Wallpaper tiles and other previews.
    Previews,
    /// The desktop and lock pictures, the other side of day/night, the gate morphs.
    Desktop,
}

type Job = Box<dyn FnOnce() + Send>;

impl Lane {
    fn name(self) -> &'static str {
        match self {
            Lane::Previews => "wol-render-previews",
            Lane::Desktop => "wol-render-desktop",
        }
    }

    /// The lane's queue; its worker thread starts with the first job and runs for the process.
    fn queue(self) -> &'static async_channel::Sender<Job> {
        static PREVIEWS: OnceLock<async_channel::Sender<Job>> = OnceLock::new();
        static DESKTOP: OnceLock<async_channel::Sender<Job>> = OnceLock::new();
        let cell = match self {
            Lane::Previews => &PREVIEWS,
            Lane::Desktop => &DESKTOP,
        };
        cell.get_or_init(|| {
            let (tx, rx) = async_channel::unbounded::<Job>();
            let started = std::thread::Builder::new()
                .name(self.name().into())
                .spawn(move || {
                    while let Ok(job) = rx.recv_blocking() {
                        job();
                    }
                });
            if let Err(e) = started {
                crate::diag::say!("wol: {}: {e}", self.name());
            }
            tx
        })
    }
}

/// Runs `work` on `lane`'s thread and waits for it without blocking the caller: None when the
/// work panicked or its thread could not start. Await it in `glib::spawn_future_local`.
pub async fn on<T: Send + 'static>(
    lane: Lane,
    work: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = async_channel::bounded::<T>(1);
    let job: Job = Box::new(move || {
        // A panicking picture must not take the lane's thread with it.
        if let Ok(v) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
            let _ = tx.send_blocking(v);
        }
    });
    lane.queue().send(job).await.ok()?;
    rx.recv().await.ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::glib;
    use std::time::{Duration, Instant};

    fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
        glib::MainContext::new().block_on(f)
    }

    #[test]
    fn one_thread_per_lane_in_order_and_lanes_apart() {
        let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let o = order.clone();
        let (a, names) = block_on(async move {
            let first = on(Lane::Previews, {
                let o = o.clone();
                move || {
                    std::thread::sleep(Duration::from_millis(150));
                    o.lock().unwrap().push(1);
                    std::thread::current().name().map(str::to_owned)
                }
            });
            let second = on(Lane::Previews, {
                let o = o.clone();
                move || {
                    o.lock().unwrap().push(2);
                    std::thread::current().name().map(str::to_owned)
                }
            });
            // The desktop lane is not behind the previews.
            let desk = on(Lane::Desktop, move || {
                std::thread::current().name().map(str::to_owned)
            });
            let t = Instant::now();
            let d = desk.await;
            let desk_wait = t.elapsed();
            let (f, s) = (first.await, second.await);
            (desk_wait, (f, s, d))
        });
        assert!(a < Duration::from_millis(150), "desktop waited {a:?}");
        assert_eq!(*order.lock().unwrap(), [1, 2]);
        assert_eq!(names.0.flatten().as_deref(), Some("wol-render-previews"));
        assert_eq!(names.1.flatten().as_deref(), Some("wol-render-previews"));
        assert_eq!(names.2.flatten().as_deref(), Some("wol-render-desktop"));
    }

    #[test]
    fn a_panic_costs_one_picture_not_the_lane() {
        let (bad, good) = block_on(async {
            let bad = on(Lane::Desktop, || -> u8 { panic!("a broken picture") }).await;
            (bad, on(Lane::Desktop, || 5u8).await)
        });
        assert_eq!(bad, None);
        assert_eq!(good, Some(5));
    }
}
