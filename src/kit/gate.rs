// SPDX-License-Identifier: GPL-2.0-or-later
//! The Moon Gate's parametric transition. On a day/night or theme change the desktop does not
//! cross-fade the two gate pictures: it moves the gate's three parameters, ring light, ribbon
//! dye and sun, from the old picture's values to the new one's over [`MS`] ms, each on its own
//! ease-out phase ([`RING`], [`DYE`], [`SUN`]): the ring lights up first, the ribbons take the
//! new dye, the sun settles last.
//!
//! Why it works on pixels: every surface of the gate is a flat colour
//! `calculus.derive(k, chroma, hue)` (design `tools/brand/walls_ribbon.py`), that is OKLCH with
//! the lightness affine in `k`. The geometry is the same in every mode and dye, so each pixel's
//! colour in the old and the new picture are the two ends of that surface's parameters, and
//! moving the pixel in OKLCH (lightness and chroma straight, hue round the short way) is moving
//! `k`, chroma and hue. Where each parameter lives (the ring band, the sun's disc) comes from the
//! gate's parameter set, `walls.py gate --params` ([`Params`]); a soft mask gives each pixel its
//! phase. Endpoints are exact: frame 0 is the old picture, frame 1 the new one.
//!
//! Cost: the pictures are converted once (off the GTK thread) at [`MAX_WORK`] or less; a frame
//! is one pass of arithmetic and table lookups per pixel, split over a few threads, made on
//! `Lane::Desktop` (`ui::panels::Frames`), never on the GTK thread. Measured on
//! the lane (wolos-lane, 32 cores at load 22-32, release, 2026-11-04) at 1920x1080: the whole
//! frame (morph step, snapshot, GSK's cairo renderer; `tests/gate_transition.rs`) 9.1-12.1 ms
//! mean, 10.3-20.9 ms worst, over 39 frames; the morph alone 10.9-12.1 ms mean (`frame_cost`
//! below, `--release -- --ignored`); preparing it 0.3-0.4 s once. A 4k display on the software
//! renderer is over budget: GSK scaling the 1920x1080 frame up costs 14-15 ms by itself.
use std::path::Path;

use super::motion::Curve;

/// Length of the whole transition.
pub const MS: u64 = 600;
/// Share of [`MS`] over which each parameter moves (start, end), each eased out.
pub const RING: (f32, f32) = (0.0, 0.75);
pub const DYE: (f32, f32) = (0.08, 0.92);
pub const SUN: (f32, f32) = (0.2, 1.0);
/// Largest working picture: bigger screens show it scaled for the 600 ms, then the real picture.
pub const MAX_WORK: (usize, usize) = (1920, 1200);
/// The parameter file `walls.py gate --params` writes next to the shipped gate pictures.
pub const PARAMS_FILE: &str = "wolos-gate.params.json";

/// The ring and the sun of the gate (design `walls_sig3d.GATE`): x a share of the width, y and
/// lengths a share of the height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub ring_cx: f32,
    pub ring_cy: f32,
    pub ring_r: f32,
    pub ring_squash: f32,
    pub ring_width: f32,
    pub sun_cx: f32,
    pub sun_cy: f32,
    pub sun_r: f32,
}

impl Default for Params {
    /// The values of `walls_sig3d.GATE` (test `params_match_walls_py`).
    fn default() -> Self {
        Params {
            ring_cx: 0.58,
            ring_cy: 0.50,
            ring_r: 0.23,
            ring_squash: 0.82,
            ring_width: 0.115,
            sun_cx: 0.84,
            sun_cy: 0.80,
            sun_r: 0.24,
        }
    }
}

impl Params {
    /// Reads `walls.py gate --params` JSON; None when a value is missing.
    pub fn parse(text: &str) -> Option<Params> {
        let v: serde_json::Value = serde_json::from_str(text).ok()?;
        let n = |o: &str, k: &str| v.get(o)?.get(k)?.as_f64().map(|x| x as f32);
        Some(Params {
            ring_cx: n("ring", "cx")?,
            ring_cy: n("ring", "cy")?,
            ring_r: n("ring", "r")?,
            ring_squash: n("ring", "squash")?,
            ring_width: n("ring", "width")?,
            sun_cx: n("sun", "cx")?,
            sun_cy: n("sun", "cy")?,
            sun_r: n("sun", "r")?,
        })
    }
}

/// A Moon Gate picture: the shipped default dye (`wolos-gate-<mode>-<size>.svg`) or a theme's
/// (`themes/gate-<theme>-<mode>-<size>.svg`).
pub fn is_gate(p: &Path) -> bool {
    p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
        n.ends_with(".svg") && (n.starts_with("wolos-gate-") || n.starts_with("gate-"))
    })
}

/// Progress of one parameter at overall progress `t` (0..1): its phase, eased out.
pub fn phase(t: f32, (a, b): (f32, f32)) -> f32 {
    Curve::Out.at(((t - a) / (b - a)).clamp(0.0, 1.0))
}

const Q: f32 = 65535.0;
/// OKLCH chroma above this is clipped (sRGB never needs more than ~0.33).
const C_MAX: f32 = 0.4;
const TAU: f32 = std::f32::consts::TAU;
const TRIG: usize = 1024;
const ENC: usize = 4096;

/// One pixel in both pictures, OKLCH quantised to 16 bits: the old lightness, chroma and hue,
/// the new lightness and chroma, the hue step round the short way; its share of the ring and of
/// the sun (the rest is ribbon and ground: the dye).
#[derive(Clone, Copy)]
struct Px {
    l0: u16,
    c0: u16,
    h0: u16,
    l1: u16,
    c1: u16,
    dh: i16,
    ring: u8,
    sun: u8,
}

thread_local! {
    /// How many [`Morph::frame`]s this thread has made.
    static MADE_HERE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

static MADE_ALL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// How many [`Morph::frame`]s every thread together has made so far (tests).
pub fn frames_made() -> u32 {
    MADE_ALL.load(std::sync::atomic::Ordering::Relaxed)
}

/// How many [`Morph::frame`]s the calling thread has made so far. A test reads it on the GTK
/// thread before and after a transition: the difference must be 0 (QA-A7-9).
pub fn frames_made_here() -> u32 {
    MADE_HERE.with(std::cell::Cell::get)
}

/// Two gate pictures ready to move between.
pub struct Morph {
    pub w: usize,
    pub h: usize,
    px: Vec<Px>,
    sin: Vec<f32>,
    enc: Vec<u8>,
}

fn srgb_to_linear(u: u8) -> f32 {
    let c = u as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Linear sRGB -> OKLab (Björn Ottosson's matrices).
fn oklab(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    (
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    )
}

/// OKLab -> linear sRGB.
fn linear_of(l: f32, a: f32, b: f32) -> (f32, f32, f32) {
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    (
        4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3,
        -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3,
        -0.004_196_086_3 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3,
    )
}

/// Box-filters a BGRA picture (`stride` bytes per row) to `w` x `h` tightly packed BGRA.
pub fn resample(src: &[u8], sw: usize, sh: usize, stride: usize, w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        let (y0, y1) = (y * sh / h, ((y + 1) * sh / h).max(y * sh / h + 1).min(sh));
        for x in 0..w {
            let (x0, x1) = (x * sw / w, ((x + 1) * sw / w).max(x * sw / w + 1).min(sw));
            let mut acc = [0u32; 4];
            for yy in y0..y1 {
                let row = &src[yy * stride + x0 * 4..yy * stride + x1 * 4];
                for px in row.as_chunks::<4>().0 {
                    for (a, v) in acc.iter_mut().zip(px) {
                        *a += *v as u32;
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            let o = (y * w + x) * 4;
            for k in 0..4 {
                out[o + k] = ((acc[k] + n / 2) / n) as u8;
            }
        }
    }
    out
}

/// The working size for a `w` x `h` picture: itself, or scaled into [`MAX_WORK`].
pub fn work_size(w: usize, h: usize) -> (usize, usize) {
    let s = (MAX_WORK.0 as f32 / w as f32)
        .min(MAX_WORK.1 as f32 / h as f32)
        .min(1.0);
    (
        ((w as f32 * s).round() as usize).max(1),
        ((h as f32 * s).round() as usize).max(1),
    )
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// OKLCH of one BGRA pixel, quantised: (lightness, chroma, hue in turns).
fn lch_of(px: &[u8], lin: &[f32; 256]) -> (u16, u16, u16) {
    let (l, a, b) = oklab(
        lin[px[2] as usize],
        lin[px[1] as usize],
        lin[px[0] as usize],
    );
    let c = (a * a + b * b).sqrt();
    let hue = b.atan2(a).rem_euclid(TAU) / TAU;
    (
        (l.clamp(0.0, 1.0) * Q).round() as u16,
        ((c / C_MAX).clamp(0.0, 1.0) * Q).round() as u16,
        (hue.clamp(0.0, 1.0) * Q).round() as u16,
    )
}

impl Morph {
    /// `from` and `to`: tightly packed BGRA of the same `w` x `h` (opaque). Blocking (tens of ms
    /// at [`MAX_WORK`]): run it off the GTK thread.
    pub fn new(from: &[u8], to: &[u8], w: usize, h: usize, p: &Params) -> Morph {
        assert_eq!(from.len(), w * h * 4, "from is w x h BGRA");
        assert_eq!(to.len(), w * h * 4, "to is w x h BGRA");
        let mut lin = [0f32; 256];
        for (i, v) in lin.iter_mut().enumerate() {
            *v = srgb_to_linear(i as u8);
        }
        let (fw, fh) = (w as f32, h as f32);
        let (rx, ry) = (p.ring_r * fh, p.ring_r * p.ring_squash * fh);
        let half = 0.5 * p.ring_width * fh;
        let (rcx, rcy) = (p.ring_cx * fw, p.ring_cy * fh);
        let (scx, scy, sr) = (p.sun_cx * fw, p.sun_cy * fh, p.sun_r * fh);
        let mut px = Vec::with_capacity(w * h);
        let (mut fa, mut fb) = (from.as_chunks::<4>().0.iter(), to.as_chunks::<4>().0.iter());
        for y in 0..h {
            for x in 0..w {
                let (qx, qy) = (x as f32 + 0.5, y as f32 + 0.5);
                // Distance from the ring's centre line, in px (the ellipse's radial scale).
                let (dx, dy) = ((qx - rcx) / rx, (qy - rcy) / ry);
                let d = ((dx * dx + dy * dy).sqrt() - 1.0).abs() * rx.min(ry);
                let wr = 1.0 - smooth(0.6 * half, 1.6 * half, d);
                let ds = ((qx - scx).powi(2) + (qy - scy).powi(2)).sqrt() / sr;
                let ws = (1.0 - smooth(0.2, 1.0, ds)) * (1.0 - wr);
                let (l0, c0, h0) = lch_of(fa.next().expect("w x h"), &lin);
                let (l1, c1, h1) = lch_of(fb.next().expect("w x h"), &lin);
                // The hue step in [-1/2, 1/2] of a turn (Q/2 = half a turn fits an i16).
                let mut dh = h1 as i32 - h0 as i32;
                if dh > Q as i32 / 2 {
                    dh -= Q as i32;
                } else if dh < -(Q as i32) / 2 {
                    dh += Q as i32;
                }
                px.push(Px {
                    l0,
                    c0,
                    h0,
                    l1,
                    c1,
                    dh: dh as i16,
                    ring: (wr * 255.0).round() as u8,
                    sun: (ws * 255.0).round() as u8,
                });
            }
        }
        let sin = (0..TRIG + TRIG / 4)
            .map(|i| (i as f32 * TAU / TRIG as f32).sin())
            .collect();
        let enc = (0..ENC)
            .map(|i| (linear_to_srgb(i as f32 / (ENC - 1) as f32) * 255.0).round() as u8)
            .collect();
        Morph { w, h, px, sin, enc }
    }

    /// The picture at overall progress `t` (0 = old, 1 = new): tightly packed BGRA, opaque.
    /// Blocking (about 10 ms at [`MAX_WORK`] over a few threads): never on the GTK thread while a
    /// transition runs (`ui::panels` makes the frames on a worker; QA-A7-9).
    pub fn frame(&self, t: f32) -> Vec<u8> {
        MADE_HERE.with(|n| n.set(n.get() + 1));
        MADE_ALL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut out = vec![0u8; self.w * self.h * 4];
        let (tr, td, ts) = (phase(t, RING), phase(t, DYE), phase(t, SUN));
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .clamp(1, 6);
        let rows = self.h.div_ceil(threads);
        std::thread::scope(|s| {
            for (out, px) in out
                .chunks_mut(rows * self.w * 4)
                .zip(self.px.chunks(rows * self.w))
            {
                s.spawn(move || self.paint(out, px, tr, td, ts));
            }
        });
        out
    }

    fn paint(&self, out: &mut [u8], px: &[Px], tr: f32, td: f32, ts: f32) {
        let (sin, enc) = (&self.sin[..], &self.enc[..]);
        let (kr, ks) = ((tr - td) / 255.0, (ts - td) / 255.0);
        let enc_max = (ENC - 1) as f32;
        let e = |v: f32| enc[(v.clamp(0.0, 1.0) * enc_max) as usize];
        let (lq, cq, hq) = (1.0 / Q, C_MAX / Q, TRIG as f32 / Q);
        for (o, p) in out.as_chunks_mut::<4>().0.iter_mut().zip(px) {
            let t = td + kr * p.ring as f32 + ks * p.sun as f32;
            let l = (p.l0 as f32 + (p.l1 as f32 - p.l0 as f32) * t) * lq;
            let c = (p.c0 as f32 + (p.c1 as f32 - p.c0 as f32) * t) * cq;
            // Hue in table steps; the table wraps once (a step is never more than half a turn).
            let k = ((p.h0 as f32 + p.dh as f32 * t) * hq + TRIG as f32) as usize % TRIG;
            let (sn, cs) = (sin[k], sin[k + TRIG / 4]);
            let (r, g, b) = linear_of(l, c * cs, c * sn);
            o[0] = e(b);
            o[1] = e(g);
            o[2] = e(r);
            o[3] = 255;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: usize, h: usize, rgb: [u8; 3]) -> Vec<u8> {
        (0..w * h)
            .flat_map(|_| [rgb[2], rgb[1], rgb[0], 255])
            .collect()
    }

    fn close(a: &[u8], b: &[u8], tol: i32) -> bool {
        a.iter()
            .zip(b)
            .all(|(x, y)| (*x as i32 - *y as i32).abs() <= tol)
    }

    #[test]
    fn ends_are_the_two_pictures() {
        let (w, h) = (64, 36);
        let (from, to) = (solid(w, h, [124, 92, 255]), solid(w, h, [15, 9, 40]));
        let m = Morph::new(&from, &to, w, h, &Params::default());
        assert!(close(&m.frame(0.0), &from, 2), "frame 0 is the old picture");
        assert!(close(&m.frame(1.0), &to, 2), "frame 1 is the new picture");
    }

    /// The dye moves through the hue wheel, not through grey: half way between a violet and a
    /// teal of the same lightness and chroma keeps that chroma (a cross-fade loses most of it).
    #[test]
    fn the_dye_keeps_its_colour_half_way() {
        let (w, h) = (8, 8);
        let lin = |u: u8| srgb_to_linear(u);
        let chroma = |px: &[u8]| {
            let (_, a, b) = oklab(lin(px[2]), lin(px[1]), lin(px[0]));
            (a * a + b * b).sqrt()
        };
        let (va, vb) = ([133, 131, 172], [96, 150, 140]);
        let m = Morph::new(&solid(w, h, va), &solid(w, h, vb), w, h, &Params::default());
        let mid = m.frame(0.5);
        let fade: Vec<u8> = (0..3)
            .map(|k| ((va[k] as u16 + vb[k] as u16) / 2) as u8)
            .collect();
        let c_mid = chroma(&mid[..4]);
        let c_fade = chroma(&[fade[2], fade[1], fade[0], 255]);
        let c_ends = (chroma(&solid(1, 1, va)) + chroma(&solid(1, 1, vb))) / 2.0;
        assert!(
            c_mid > 0.9 * c_ends,
            "chroma kept: {c_mid} vs ends {c_ends}"
        );
        assert!(
            c_mid > c_fade + 0.015,
            "more colour than a cross-fade: {c_mid} vs {c_fade}"
        );
    }

    /// The ring leads, the sun follows: at a third of the run the ring is further along than
    /// the dye, and the dye than the sun.
    #[test]
    fn ring_leads_and_sun_follows() {
        let t = 1.0 / 3.0;
        assert!(phase(t, RING) > phase(t, DYE) && phase(t, DYE) > phase(t, SUN));
        for p in [RING, DYE, SUN] {
            assert_eq!(phase(0.0, p), 0.0);
            assert_eq!(phase(1.0, p), 1.0);
        }
        let (w, h) = (192, 108);
        let m = Morph::new(
            &solid(w, h, [20, 20, 40]),
            &solid(w, h, [230, 230, 250]),
            w,
            h,
            &Params::default(),
        );
        let f = m.frame(t);
        let at =
            |fx: f32, fy: f32| f[((fy * h as f32) as usize * w + (fx * w as f32) as usize) * 4 + 1];
        let p = Params::default();
        let on_ring = at(p.ring_cx + p.ring_r * h as f32 / w as f32, p.ring_cy);
        let ground = at(0.05, 0.05);
        let sun = at(p.sun_cx, p.sun_cy);
        assert!(
            on_ring > ground && ground > sun,
            "ring {on_ring} > dye {ground} > sun {sun}"
        );
    }

    #[test]
    fn parses_walls_py_params() {
        let j = r#"{"ring": {"cx": 0.58, "cy": 0.5, "r": 0.23, "squash": 0.82, "width": 0.115},
                   "sun": {"cx": 0.84, "cy": 0.8, "r": 0.24, "stretch": 1.5}}"#;
        assert_eq!(Params::parse(j), Some(Params::default()));
        assert_eq!(Params::parse(r#"{"ring": {}}"#), None);
    }

    /// The built-in values are the design's (`walls.py gate --params`), when the design tree and
    /// python3 are here.
    #[test]
    fn params_match_walls_py() {
        let walls = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../design/tools/brand/walls.py"
        );
        if !Path::new(walls).exists() {
            eprintln!("skip: no design tree with tools/brand");
            return;
        }
        let Ok(out) = std::process::Command::new("python3")
            .args(["-I", walls, "gate", "--params"])
            .output()
        else {
            eprintln!("skip: no python3");
            return;
        };
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let p = Params::parse(&String::from_utf8_lossy(&out.stdout)).expect("ring and sun");
        assert_eq!(
            p,
            Params::default(),
            "Params::default follows walls_sig3d.GATE"
        );
    }

    #[test]
    fn gate_pictures_by_name() {
        assert!(is_gate(Path::new(
            "/usr/share/wolos/wallpapers/wolos-gate-night-4k.svg"
        )));
        assert!(is_gate(Path::new("/x/themes/gate-nord-day-16x10.svg")));
        assert!(!is_gate(Path::new("/x/cachoeira-day-4k.svg")));
        assert!(!is_gate(Path::new("/home/a/Pictures/gate-photo.jpg")));
    }

    #[test]
    fn resample_averages() {
        let src: Vec<u8> = [[0u8, 0, 0, 255], [200, 100, 50, 255]]
            .iter()
            .flatten()
            .copied()
            .collect();
        assert_eq!(resample(&src, 2, 1, 8, 1, 1), vec![100, 50, 25, 255]);
        assert_eq!(work_size(3840, 2160), (1920, 1080));
        assert_eq!(work_size(1280, 800), (1280, 800));
    }

    /// Frame cost at 1920x1080 (the lane: `cargo test --release --lib gate -- --ignored
    /// --nocapture`). Prints the mean and worst of 36 frames (600 ms at 60 Hz) and the one-time
    /// preparation; fails over the 16 ms budget.
    #[test]
    #[ignore]
    fn frame_cost() {
        let (w, h) = (1920, 1080);
        let grad = |k: u8| -> Vec<u8> {
            (0..w * h)
                .flat_map(|i| {
                    let (x, y) = ((i % w) as u32, (i / w) as u32);
                    [
                        (x * 255 / w as u32) as u8,
                        (y * 255 / h as u32) as u8,
                        k,
                        255,
                    ]
                })
                .collect()
        };
        let (a, b) = (grad(40), grad(220));
        let t0 = std::time::Instant::now();
        let m = Morph::new(&a, &b, w, h, &Params::default());
        let prep = t0.elapsed();
        let mut worst = std::time::Duration::ZERO;
        let mut sum = std::time::Duration::ZERO;
        for f in 0..36 {
            let t = std::time::Instant::now();
            let px = m.frame(f as f32 / 35.0);
            std::hint::black_box(&px);
            let d = t.elapsed();
            worst = worst.max(d);
            sum += d;
        }
        // What the software renderer (GSK's cairo renderer) does with the frame: paint it as an
        // image at 1:1 (a 1080p display) and scaled 2x with the bilinear filter (a 4k display).
        use gtk::cairo;
        let px = m.frame(0.5);
        let src = cairo::ImageSurface::create_for_data(
            px,
            cairo::Format::ARgb32,
            w as i32,
            h as i32,
            (w * 4) as i32,
        )
        .expect("frame surface");
        let paint = |scale: f64, filter: cairo::Filter| {
            let dst = cairo::ImageSurface::create(
                cairo::Format::ARgb32,
                (w as f64 * scale) as i32,
                (h as f64 * scale) as i32,
            )
            .expect("target");
            let cr = cairo::Context::new(&dst).expect("context");
            let t = std::time::Instant::now();
            for _ in 0..10 {
                cr.save().expect("save");
                cr.scale(scale, scale);
                cr.set_source_surface(&src, 0.0, 0.0).expect("source");
                cr.source().set_filter(filter);
                cr.paint().expect("paint");
                cr.restore().expect("restore");
            }
            dst.flush();
            t.elapsed().as_secs_f64() * 100.0
        };
        let (p1, p2) = (
            paint(1.0, cairo::Filter::Bilinear),
            paint(2.0, cairo::Filter::Bilinear),
        );
        let p2n = paint(2.0, cairo::Filter::Fast);
        // For comparison, the cross-fade it replaces: two full pictures per frame, one with alpha.
        let fade = {
            let dst = cairo::ImageSurface::create(cairo::Format::ARgb32, w as i32, h as i32)
                .expect("target");
            let cr = cairo::Context::new(&dst).expect("context");
            let t = std::time::Instant::now();
            for _ in 0..10 {
                cr.set_source_surface(&src, 0.0, 0.0).expect("source");
                cr.paint().expect("paint");
                cr.paint_with_alpha(0.5).expect("paint");
            }
            t.elapsed().as_secs_f64() * 100.0
        };
        // The one-time 4k -> working size box filter (before the conversion).
        let big = vec![128u8; 3840 * 2160 * 4];
        let t = std::time::Instant::now();
        std::hint::black_box(resample(&big, 3840, 2160, 3840 * 4, w, h));
        let shrink = t.elapsed().as_secs_f64() * 1000.0;
        let mean = sum.as_secs_f64() * 1000.0 / 36.0;
        eprintln!(
            "gate frame {w}x{h}: compute mean {mean:.2} ms, worst {:.2} ms; cairo paint 1:1 {p1:.2} ms, 2x {p2:.2} ms (nearest {p2n:.2} ms); cross-fade 1:1 {fade:.2} ms; prepare {:.1} ms + 4k shrink {shrink:.1} ms; threads {}",
            worst.as_secs_f64() * 1000.0,
            prep.as_secs_f64() * 1000.0,
            std::thread::available_parallelism()
                .map(|n| n.get().min(6))
                .unwrap_or(1),
        );
        // The budget holds where the working size covers the display 1:1 (up to 1920x1200).
        assert!(
            worst.as_secs_f64() * 1000.0 + p1 < 16.0,
            "over the 16 ms frame budget"
        );
    }
}
