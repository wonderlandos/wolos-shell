// SPDX-License-Identifier: GPL-2.0-or-later
//! The one way the shell shows a picture file it did not make (QA-A9-SHELL-05): a Lua desktop
//! widget's `image` node, a libwol app's view, a banner's thumbnail. The file is outside input,
//! so it has named bounds: a regular file of at most [`FILE_MAX`] bytes whose header says at most
//! [`PIXELS_MAX`] pixels, decoded straight to the size it is shown at (`from_file_at_scale`),
//! never the whole picture, and off the GTK thread with the image empty until it arrives. The
//! textures are kept by (file, size) and used again while the file's modification time is the
//! same, so a card that rebuilds on every refresh does not decode its picture again.
use gtk::{gio, glib, prelude::*};
use std::cell::RefCell;
use std::time::SystemTime;

/// Biggest picture file decoded (audit4 S3-3: any app can name any file).
pub const FILE_MAX: u64 = 64 * 1024 * 1024;
/// Most pixels a picture may claim in its header (a few-KB PNG can say 20000x20000, which
/// decodes to 1.6 GB before any scaling).
pub const PIXELS_MAX: i64 = 100_000_000;
/// Decoded pictures kept.
const KEPT: usize = 48;

/// Decodes the picture at `path` into the box `fit` picks for its real size `(w, h)`, aspect kept;
/// None for a file that is missing, not a regular file, over [`FILE_MAX`], claiming over
/// [`PIXELS_MAX`] pixels, or not a picture. Blocking: off the GTK thread, or for a golden render.
pub fn decode_with(
    path: &str,
    fit: impl FnOnce(i32, i32) -> (i32, i32),
) -> Option<gtk::gdk::Texture> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > FILE_MAX {
        return None;
    }
    let (_, w, h) = gtk::gdk_pixbuf::Pixbuf::file_info(path)?;
    if i64::from(w) * i64::from(h) > PIXELS_MAX {
        return None;
    }
    let (box_w, box_h) = fit(w, h);
    let pb = gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(path, box_w, box_h, true).ok()?;
    Some(gtk::gdk::Texture::for_pixbuf(&pb))
}

/// [`decode_with`] into a `px` x `px` box.
pub fn decode(path: &str, px: i32) -> Option<gtk::gdk::Texture> {
    decode_with(path, |_, _| (px, px))
}

/// What a worker found for a file: the texture and the modification time it was decoded at.
type Fresh = (gtk::gdk::Texture, Option<SystemTime>);

/// Decodes `path` for the box `px` unless `known` (the time of the texture on screen) is still
/// the file's.
fn refresh(path: &str, px: i32, known: Option<SystemTime>) -> Option<Fresh> {
    let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    if mtime.is_some() && mtime == known {
        return None;
    }
    decode(path, px).map(|t| (t, mtime))
}

type Key = (String, i32);

thread_local! {
    /// Decoded pictures by (file, shown size), the one used last at the end.
    static PICTURES: RefCell<Vec<(Key, Fresh)>> = const { RefCell::new(Vec::new()) };
}

fn recall(key: &Key) -> Option<Fresh> {
    PICTURES.with_borrow_mut(|c| {
        let i = c.iter().position(|(k, _)| k == key)?;
        let entry = c.remove(i);
        let fresh = entry.1.clone();
        c.push(entry);
        Some(fresh)
    })
}

fn keep(key: Key, fresh: Fresh) {
    PICTURES.with_borrow_mut(|c| {
        c.retain(|(k, _)| *k != key);
        if c.len() >= KEPT {
            c.remove(0);
        }
        c.push((key, fresh));
    });
}

/// An image of the picture file `path`, shown `size` px square (decoded at twice that for
/// HiDPI). Empty until the picture has been decoded off the GTK thread, or at once from a
/// picture kept earlier (and checked again against the file's time in the background). A golden
/// render (`WOL_FAKE_NOW`) decodes on the spot: it cannot wait for the thread.
pub fn file_image(path: &str, size: i32) -> gtk::Image {
    let img = gtk::Image::new();
    img.set_pixel_size(size);
    show_in(&img, path, size);
    img
}

/// [`file_image`] into an image that exists (a Dock tile's, a row's): it keeps what it shows
/// (a theme icon) until the picture arrives, and keeps it for good when the file is refused.
pub fn show_in(img: &gtk::Image, path: &str, size: i32) {
    let (key, px) = ((path.to_owned(), size), size.saturating_mul(2));
    let known = recall(&key).map(|(t, m)| {
        img.set_paintable(Some(&t));
        m
    });
    if std::env::var_os("WOL_FAKE_NOW").is_some() {
        if known.is_none()
            && let Some(fresh) = refresh(path, px, None)
        {
            img.set_paintable(Some(&fresh.0));
            keep(key, fresh);
        }
        return;
    }
    let weak = img.downgrade();
    glib::spawn_future_local(async move {
        let (p, known) = (key.0.clone(), known.flatten());
        if let Ok(Some(fresh)) = gio::spawn_blocking(move || refresh(&p, px, known)).await
            && let Some(img) = weak.upgrade()
        {
            img.set_paintable(Some(&fresh.0));
            keep(key, fresh);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("wol-picture-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d.join(name)
    }

    /// A PNG signature, an IHDR for 20000x20000 and the start of the image data: 53 bytes that
    /// would decode to 1.2 GB (the bytes `quicklook`'s own bomb test uses).
    const HUGE_PNG: [u8; 53] = [
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x4e, 0x20, 0x00, 0x00, 0x4e, 0x20, 0x08, 0x02, 0x00, 0x00, 0x00, 0x6c,
        0x12, 0xd1, 0x6e, 0x00, 0x00, 0x00, 0x08, 0x49, 0x44, 0x41, 0x54, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0xee, 0x48, 0x5d, 0x87,
    ];

    /// QA-A9-SHELL-05: a picture claiming more pixels than [`PIXELS_MAX`] is refused from its
    /// header, a picture file over [`FILE_MAX`] before that, and a big real picture comes back at
    /// the size it is shown at, not its own.
    #[test]
    fn a_huge_picture_is_decoded_small_or_not_at_all() {
        let huge = temp("huge.png");
        std::fs::write(&huge, HUGE_PNG).unwrap();
        let (w, h) = {
            let (_, w, h) = gtk::gdk_pixbuf::Pixbuf::file_info(&huge).expect("a header");
            (w, h)
        };
        assert_eq!((w, h), (20000, 20000), "the file does claim that");
        let t0 = std::time::Instant::now();
        assert!(decode(huge.to_str().unwrap(), 64).is_none(), "refused");
        assert!(
            t0.elapsed() < std::time::Duration::from_millis(500),
            "refused from the header: {:?}",
            t0.elapsed()
        );

        let big = temp("big.png");
        let pb =
            gtk::gdk_pixbuf::Pixbuf::new(gtk::gdk_pixbuf::Colorspace::Rgb, false, 8, 3000, 2000)
                .expect("a pixbuf");
        pb.fill(0x336699ff);
        pb.savev(&big, "png", &[]).expect("saved");
        let t = decode(big.to_str().unwrap(), 64).expect("a real picture decodes");
        assert!(
            t.width() <= 64 && t.height() <= 64 && t.width().max(t.height()) == 64,
            "{}x{}",
            t.width(),
            t.height()
        );
        assert!(decode("/nonexistent/wol-a9.png", 64).is_none());
        assert!(
            decode(env!("CARGO_MANIFEST_DIR"), 64).is_none(),
            "a directory"
        );
    }

    /// A kept picture is used again while the file's time is the same, and decoded again when
    /// the file changed; the cache is bounded.
    #[test]
    fn a_kept_picture_is_checked_against_the_files_time() {
        let t = || {
            gtk::gdk::MemoryTexture::new(
                1,
                1,
                gtk::gdk::MemoryFormat::R8g8b8,
                &glib::Bytes::from_static(&[0, 0, 0]),
                3,
            )
            .upcast::<gtk::gdk::Texture>()
        };
        let file = temp("same.png");
        let pb = gtk::gdk_pixbuf::Pixbuf::new(gtk::gdk_pixbuf::Colorspace::Rgb, false, 8, 40, 40)
            .expect("a pixbuf");
        pb.savev(&file, "png", &[]).expect("saved");
        let path = file.to_str().unwrap();
        let (_, mtime) = refresh(path, 64, None).expect("first time: decoded");
        assert!(mtime.is_some());
        assert!(refresh(path, 64, mtime).is_none(), "same time: kept as is");
        assert!(
            refresh(path, 64, Some(SystemTime::UNIX_EPOCH)).is_some(),
            "another time: decoded again"
        );
        for i in 0..KEPT + 3 {
            keep((format!("/{i}.png"), 8), (t(), None));
        }
        assert!(
            recall(&("/0.png".into(), 8)).is_none(),
            "bounded: the oldest goes"
        );
        assert!(recall(&(format!("/{}.png", KEPT + 2), 8)).is_some());
    }
}
