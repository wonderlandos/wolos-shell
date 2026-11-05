// SPDX-License-Identifier: GPL-2.0-or-later
//! Lock screen (ext-session-lock, ADR 0022/0023) and login window (ADR 0024): clock, avatar, password.
use crate::kit::anim::{Animated, Motion};
use crate::kit::motion::{Curve, Duration, Stagger};
use crate::kit::{self, reveal, widgets as w};
use crate::login::{self, Power, Verdict};
use gtk::{glib, prelude::*};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

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

/// Lock/login wallpaper: the Moon Gate the desktop follows (the theme's dye; night when the theme
/// for `dark` is dark), else the earlier day and night pictures; installed copy first.
pub fn wallpaper_file(dark: bool) -> Option<PathBuf> {
    let t = crate::theme::resolve(kit::style::active_id().as_deref(), dark);
    let night = t.variant == crate::theme::Variant::Dark;
    kit::pickers::gate_file(&t.id, night, 16.0 / 9.0).or_else(|| {
        kit::pickers::shipped_wallpaper(if night {
            "wonderland-night"
        } else {
            "wonderland-day"
        })
    })
}

/// A backdrop change moves the gate's parameters (`kit::gate`) instead of jumping: both pictures
/// are gates, something is on screen, motion is on and the clock is real (goldens jump).
fn backdrop_morphs(old: Option<&Path>, new: &Path, reduced: bool, fixed_time: bool) -> bool {
    !reduced && !fixed_time && old.is_some_and(kit::gate::is_gate) && kit::gate::is_gate(new)
}

pub(crate) fn prefers_dark() -> bool {
    gtk::Settings::default().is_some_and(|s| s.is_gtk_application_prefer_dark_theme())
}

thread_local! {
    /// Blurred wallpaper per file: decoded once per process.
    static BACKDROPS: RefCell<std::collections::HashMap<PathBuf, gtk::gdk::MemoryTexture>> =
        Default::default();
}

/// The blurred wallpaper is decoded: a first surface may fade in (`kit::reveal`).
pub(crate) fn backdrop_ready() -> bool {
    BACKDROPS.with_borrow(|c| !c.is_empty())
}

/// Runs `f` on light/dark changes while `owner` lives (the handler is dropped with it).
fn on_dark_change(owner: &impl IsA<gtk::Widget>, f: impl Fn() + 'static) {
    let Some(s) = gtk::Settings::default() else {
        return;
    };
    let id = s.connect_gtk_application_prefer_dark_theme_notify(move |_| f());
    let id = RefCell::new(Some(id));
    owner.connect_destroy(move |_| {
        if let Some(id) = id.take() {
            s.disconnect(id);
        }
    });
}

/// Blurred wallpaper, decoded off the main thread (synchronously in fixed-time golden mode) and cached;
/// follows the light/dark preference.
pub(crate) fn backdrop() -> gtk::Picture {
    let pic = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .can_shrink(true)
        .build();
    pic.add_css_class("lock-backdrop");
    let weak = pic.downgrade();
    // What is on screen, and a morph while it runs.
    let shown: Rc<RefCell<Option<(PathBuf, gtk::gdk::MemoryTexture)>>> = Default::default();
    let run: Rc<RefCell<Option<Animated>>> = Default::default();
    let load = move || {
        let (Some(pic), Some(path)) = (weak.upgrade(), wallpaper_file(prefers_dark())) else {
            return;
        };
        if shown.borrow().as_ref().is_some_and(|(p, _)| *p == path) {
            return;
        }
        let fixed = crate::envv::is_set("WOL_FAKE_NOW");
        let old = shown.borrow().clone();
        let morph = old
            .clone()
            .filter(|(p, _)| backdrop_morphs(Some(p), &path, kit::motion::reduced(), fixed));
        let show = {
            let (shown, run) = (shown.clone(), run.clone());
            move |path: PathBuf, t: gtk::gdk::MemoryTexture, pic: gtk::Picture| {
                shown.replace(Some((path, t.clone())));
                let Some((_, from)) = morph.clone() else {
                    run.take();
                    return pic.set_paintable(Some(&t));
                };
                let (f, to) = (
                    from.upcast::<gtk::gdk::Texture>(),
                    t.clone().upcast::<gtk::gdk::Texture>(),
                );
                let run = run.clone();
                glib::spawn_future_local(async move {
                    let m = kit::render::on(kit::render::Lane::Desktop, move || {
                        super::panels::morph_between(&f, &to)
                    })
                    .await
                    .flatten();
                    match m {
                        Some(m) => {
                            run.replace(Some(super::panels::morph_picture(
                                &pic,
                                m,
                                t.upcast_ref(),
                            )));
                        }
                        None => {
                            run.take();
                            pic.set_paintable(Some(&t));
                        }
                    }
                });
            }
        };
        if let Some(t) = BACKDROPS.with_borrow(|c| c.get(&path).cloned()) {
            return show(path, t, pic);
        }
        let cache = |path: &PathBuf, t: &gtk::gdk::MemoryTexture| {
            BACKDROPS.with_borrow_mut(|c| c.insert(path.clone(), t.clone()));
        };
        if fixed {
            if let Some(t) = blurred_wallpaper(&path) {
                cache(&path, &t);
                show(path, t, pic);
            }
            return;
        }
        glib::spawn_future_local(async move {
            let p2 = path.clone();
            if let Ok(Some(t)) = gtk::gio::spawn_blocking(move || blurred_wallpaper(&p2)).await {
                cache(&path, &t);
                show(path, t, pic);
            }
        });
    };
    load();
    on_dark_change(&pic, load);
    pic
}

/// Wallpaper + scrim under `content`.
pub(crate) fn screen(content: &impl IsA<gtk::Widget>) -> gtk::Widget {
    screen_with(content, false)
}

/// Narrowest screen that shows the door mark and the word at the upper left: the table says 640, but a card
/// (624 wide, centred) reaches the 60 + 40 + 12 + word column of the mark below 968, and the two would overlap.
pub(crate) const CHROME_MIN_WIDTH: i32 = 968;

/// [`screen`] for the installer and Setup: the door mark and the word at the upper left (60 from the
/// edges), the entry of the page.
pub(crate) fn assistant_screen(content: &impl IsA<gtk::Widget>) -> gtk::Widget {
    screen_with(content, true)
}

fn screen_with(content: &impl IsA<gtk::Widget>, chrome: bool) -> gtk::Widget {
    let o = gtk::Overlay::new();
    o.add_css_class("lock-screen");
    let tone = {
        let weak = o.downgrade();
        move || {
            let Some(o) = weak.upgrade() else {
                return;
            };
            if prefers_dark() {
                o.remove_css_class("light")
            } else {
                o.add_css_class("light")
            }
        }
    };
    tone();
    on_dark_change(&o, tone);
    o.set_child(Some(&backdrop()));
    let scrim = w::vbox(0);
    scrim.add_css_class("lock-scrim");
    // The card keeps its size; a screen smaller than it (a VM window, 1366x768 at scale 2)
    // scrolls instead of cutting off the foot with the buttons. Focus scrolls into view.
    content.set_vexpand(true);
    let fit = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .child(content)
        .build();
    fit.add_css_class("lock-fit");
    // Not a Tab stop of its own (QA-VIS2-3): the controls inside scroll into view by themselves.
    fit.set_focusable(false);
    scrim.append(&fit);
    o.add_overlay(&scrim);
    if chrome {
        let mark = w::hbox(w::S3);
        mark.set_halign(gtk::Align::Start);
        mark.set_valign(gtk::Align::Start);
        mark.set_margin_start(w::S15);
        mark.set_margin_top(w::S15);
        mark.set_can_target(false);
        mark.append(&w::app_icon("wolos-mark", w::S10));
        mark.append(&w::label("WolOS", "screen-brand"));
        o.add_overlay(&mark);
        // Below 640 px the card needs the room: the mark steps out of the way.
        let weak = mark.downgrade();
        let first = weak.clone();
        o.add_tick_callback(move |o, _| {
            // The first frame with a width decides; the surface's layout signal follows later resizes.
            if o.width() == 0 {
                return glib::ControlFlow::Continue;
            }
            if let Some(m) = first.upgrade() {
                m.set_visible(o.width() >= CHROME_MIN_WIDTH);
            }
            glib::ControlFlow::Break
        });
        o.connect_map(move |o| {
            let Some(surface) = o.native().and_then(|n| n.surface()) else {
                return;
            };
            let weak = weak.clone();
            surface.connect_layout(move |_, width, _| {
                if let Some(m) = weak.upgrade() {
                    m.set_visible(width >= CHROME_MIN_WIDTH);
                }
            });
        });
    }
    o.upcast()
}

/// The clock at the upper left (the entry), the user panel at the lower right (the landing), the power row
/// at the lower left; the wallpaper between them is the calm centre. `footer_at` is where the footer sits.
fn body_at(
    middle: &impl IsA<gtk::Widget>,
    footer: Option<&gtk::Widget>,
    footer_at: gtk::Align,
) -> gtk::Box {
    let b = w::vbox(0);
    b.add_css_class("lock-body");
    b.append(&w::clock_header());
    let s = w::vbox(0);
    s.set_vexpand(true);
    let stage = gtk::Overlay::new();
    stage.set_vexpand(true);
    stage.set_child(Some(&s));
    middle.set_halign(gtk::Align::End);
    middle.set_valign(gtk::Align::End);
    stage.add_overlay(middle);
    if let Some(f) = footer {
        f.set_halign(footer_at);
        f.set_valign(gtk::Align::End);
        stage.add_overlay(f);
    }
    b.append(&stage);
    b
}

/// [`body_at`] with the footer at the lower left (the power row).
fn body(middle: &impl IsA<gtk::Widget>, footer: Option<&gtk::Widget>) -> gtk::Box {
    body_at(middle, footer, gtk::Align::Start)
}

/// The lock screen for one user panel.
pub fn lock_screen(panel: &w::UserPanel) -> gtk::Widget {
    let b = body(&panel.root, None);
    Entrance::new(&b, panel, None);
    screen(&b)
}

/// Gap between the lock's entering groups: the clock, then the avatar and name, then the
/// password field (a [`Stagger`] of three).
pub const ENTER_GAP_MS: u64 = 60;

/// The lock's content entering: `ease` (kit.css `--ease-out`) over `--motion-normal`.
pub fn enter_motion() -> Motion {
    Motion::enter(Duration::Normal)
}

/// The lock's content leaving on a right password, before the unlock shows the desktop: `ease`
/// too (it eases out, the session is already on its way) over `--motion-fast`, the field first.
pub fn leave_motion() -> Motion {
    Motion::tween(Curve::Ease, Duration::Fast)
}

/// The entering order: clock, avatar and name, password field.
pub fn enter_stagger() -> Stagger {
    Stagger::new(3, ENTER_GAP_MS)
}

/// Milliseconds a [`Motion::Tween`] lasts (0 for a spring: the caller's grace covers it).
fn tween_ms(m: Motion) -> u64 {
    match m {
        Motion::Tween { ms, .. } => ms,
        Motion::Spring(_) => 0,
    }
}

/// The lock screen's content on the window's motion clock (`docs/modules/hig.md` section 7):
/// the clock, the avatar and the password field fade in one after another when the screen
/// maps (at start-up, once the [`reveal::Reveal`] starts fading in from black: the same clock),
/// and fade out the other way round on a right password. Opacity only: no relayout per frame.
/// Reduce motion snaps both (the clock jumps to the targets); golden renders show the end state.
pub(crate) struct Entrance {
    /// Entering order; each group moves together.
    groups: Vec<Vec<gtk::Widget>>,
    /// One handle per widget, made on first use (the widgets are in their window by then).
    fades: RefCell<Vec<(usize, Animated)>>,
}

impl Entrance {
    /// `extra` joins the field's group (the lock's Restart row).
    fn new(b: &gtk::Box, panel: &w::UserPanel, extra: Option<&gtk::Widget>) -> Rc<Entrance> {
        let mut field: Vec<gtk::Widget> = vec![
            panel.pill.clone().upcast(),
            panel.caps.clone().upcast(),
            panel.message.clone().upcast(),
        ];
        field.extend(extra.cloned());
        let e = Rc::new(Entrance {
            groups: vec![
                b.first_child().into_iter().collect(),
                panel
                    .root
                    .first_child()
                    .into_iter()
                    .chain([panel.name.clone().upcast()])
                    .collect(),
                field,
            ],
            fades: RefCell::new(Vec::new()),
        });
        if crate::envv::is_set("WOL_FAKE_NOW") {
            return e;
        }
        let (e2, started) = (e.clone(), std::cell::Cell::new(false));
        b.connect_map(move |b| {
            if started.replace(true) {
                return;
            }
            for w in e2.groups.iter().flatten() {
                w.set_opacity(0.0);
            }
            // As `Reveal::show_when`: after a frame is on screen (a first frame that takes long
            // would eat the whole move) and over the blurred wallpaper, not the bare window.
            let (e3, b2) = (e2.clone(), b.downgrade());
            reveal::when_shown(b, move || {
                let Some(b) = b2.upgrade() else { return };
                let frames = std::cell::Cell::new(0u32);
                b.add_tick_callback(move |_, _| {
                    frames.set(frames.get() + 1);
                    if frames.get() < 2
                        || (!backdrop_ready() && frames.get() < reveal::MAX_WAIT_FRAMES)
                    {
                        return glib::ControlFlow::Continue;
                    }
                    e3.enter();
                    glib::ControlFlow::Break
                });
            });
        });
        e
    }

    fn fades(&self) -> Vec<(usize, Animated)> {
        let mut f = self.fades.borrow_mut();
        if f.is_empty() {
            for (i, g) in self.groups.iter().enumerate() {
                for w in g {
                    let weak = w.downgrade();
                    f.push((
                        i,
                        Animated::new(w, w.opacity() as f32, move |v| {
                            if let Some(w) = weak.upgrade() {
                                w.set_opacity(v.into());
                            }
                        }),
                    ));
                }
            }
        }
        f.clone()
    }

    fn enter(&self) {
        let st = enter_stagger();
        for (i, a) in self.fades() {
            a.retarget_after(1.0, enter_motion(), st.delay_ms(i));
        }
    }

    /// Fades the content out (field first, clock last), then `then` once: when the clock lands
    /// at once (reduce motion, an unmapped screen) and after the fade's length plus a grace when
    /// the frame clock stalls, so the unlock is never held up by motion.
    fn leave_then(&self, then: impl FnOnce() + 'static) {
        let then = Rc::new(RefCell::new(Some(then)));
        let once = move || {
            // Taken before the call: `f` may land here again (the unlock unmaps the window and
            // its clock settles the same fade), which must find nothing, not a held borrow.
            let f = then.borrow_mut().take();
            if let Some(f) = f {
                f()
            }
        };
        let mapped = self.groups.iter().flatten().any(|w| w.is_mapped());
        let fades = if mapped { self.fades() } else { Vec::new() };
        let Some((_, last)) = fades.iter().find(|(i, _)| *i == 0).or(fades.first()) else {
            return once();
        };
        let st = enter_stagger();
        glib::timeout_add_local_once(
            std::time::Duration::from_millis(st.total_ms(tween_ms(leave_motion())) + 250),
            once.clone(),
        );
        last.connect_settled(move |v| {
            if v == 0.0 {
                once()
            }
        });
        for (i, a) in &fades {
            a.retarget_after(0.0, leave_motion(), st.delay_rev_ms(*i));
        }
    }
}

/// Restart the way the menu does (`wol power restart`, through wol-admind); true when it was
/// taken. Blocking: handed to `gio::spawn_blocking`. Waits at most [`restart_wait`].
fn restart_now() -> bool {
    finished_ok("wol", &["power", "restart"], restart_wait())
}

/// As long as `wol` itself waits for wol-admind's answer to a restart (`admin::reply_secs`;
/// stdin is closed, so it never asks for a password), plus a little.
fn restart_wait() -> std::time::Duration {
    let req = shared::admin::AdminRequest {
        action: shared::admin::AdminAction::SessionPower,
        args: vec!["restart".into()],
    };
    std::time::Duration::from_secs(u64::from(wol_core::admin::reply_secs(&req)) + 5)
}

/// True when `program args` exits successfully within `limit`. One still running then is killed
/// (by `timeout`, which waits for it: nothing here polls) and counts as not taken, so the lock
/// screen's Restart always answers (SP-5: no stuck state).
fn finished_ok(program: &str, args: &[&str], limit: std::time::Duration) -> bool {
    std::process::Command::new("timeout")
        .arg("--signal=KILL")
        .arg(format!("{:.3}s", limit.as_secs_f32()))
        .arg(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .spawn()
        .and_then(|mut c| c.wait())
        .is_ok_and(|s| s.success())
}

/// Lock screen whose password can't be checked (wol-authcheck missing or broken): Restart
/// appears under it, so nobody is stuck behind it. It goes through `wol power restart` like the
/// menu's, and says so when the restart was not taken (audit4 S3-9: a BSD `shutdown -r now` as
/// the session user failed silently); nothing of the session is shown or kept.
fn lock_screen_with_restart(panel: &w::UserPanel) -> (gtk::Widget, gtk::Box, Rc<Entrance>) {
    let message = panel.message.clone();
    let (restart, button) = action(
        login::power_icon(login::Power::Restart),
        login::power_label(login::Power::Restart),
        || {},
    );
    // One request at a time (pressing again while one is out started another `wol`); the wait
    // is bounded ([`finished_ok`]), so the button always comes back for a later try.
    button.connect_clicked(move |b| {
        b.set_sensitive(false);
        let (m, b) = (message.clone(), b.downgrade());
        glib::spawn_future_local(async move {
            let taken = gtk::gio::spawn_blocking(restart_now).await.unwrap_or(false);
            m.set_label(restart_caption(taken).0);
            if let Some(b) = b.upgrade() {
                b.set_sensitive(true);
            }
        });
    });
    let foot = w::hbox(0);
    foot.add_css_class("login-power");
    foot.append(&restart);
    foot.set_visible(false);
    // Under the message, on the panel's right edge.
    let b = body_at(&panel.root, Some(foot.upcast_ref()), gtk::Align::End);
    let entrance = Entrance::new(&b, panel, Some(foot.upcast_ref()));
    (screen(&b), foot, entrance)
}

/// The avatar in `tone`, or in no tone at all (the plain avatar).
fn show_tone(av: &gtk::Widget, tone: Option<&str>) {
    match tone {
        Some(t) => kit::assistant::set_tone(av, t),
        None => {
            for t in shared::login::AVATAR_TONES {
                av.remove_css_class(&kit::assistant::tone_class(t));
            }
        }
    }
}

/// `wol power <verb>` (through wol-admind, like the menu's); true when it was taken. Blocking:
/// handed to `gio::spawn_blocking`.
fn power_now(p: Power) -> bool {
    std::process::Command::new("wol")
        .args(["power", p.verb()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// What a power button of the lock screen says once `wol power` answered: Restart names the way
/// out of its refusal (someone else signed in), the others say they were not taken.
pub fn lock_power_caption(p: Power, taken: bool) -> (&'static str, bool) {
    if p == Power::Restart {
        restart_caption(taken)
    } else {
        caption((!taken).then(|| crate::copy::power_refused(p)))
    }
}

/// The login window of a computer with one account: that account's lock screen at boot (ADR
/// 0004), with the login window's Sleep, Restart and Shut Down row at the foot (parity with the
/// login window of several accounts). The buttons go through `wol power` as the session's
/// account; a refusal is said under the row.
pub fn startup_lock_screen(panel: &w::UserPanel) -> gtk::Widget {
    startup_screen(panel).0
}

/// [`startup_lock_screen`] with its [`Entrance`]: the power row enters and leaves with the
/// password field.
fn startup_screen(panel: &w::UserPanel) -> (gtk::Widget, Rc<Entrance>) {
    let foot = w::hbox(w::S10);
    foot.add_css_class("login-power");
    let refused = w::label("", "lock-caption");
    refused.set_visible(false);
    for p in Power::ALL {
        let r = refused.clone();
        let run = move || {
            let r = r.clone();
            glib::spawn_future_local(async move {
                let taken = gtk::gio::spawn_blocking(move || power_now(p))
                    .await
                    .unwrap_or(false);
                let (text, shown) = lock_power_caption(p, taken);
                r.set_label(text);
                r.set_visible(shown);
            });
        };
        foot.append(&action(login::power_icon(p), login::power_label(p), run).0);
    }
    let footer = w::vbox(w::S2);
    footer.append(&foot);
    footer.append(&refused);
    let b = body(&panel.root, Some(footer.upcast_ref()));
    let entrance = Entrance::new(&b, panel, Some(footer.upcast_ref()));
    (screen(&b), entrance)
}

/// wol-login's sentence for a session that ended early (`controller::login::NOTICE_FILE`, root,
/// 0644, one sentence; it removes the file itself). Read once per greeter start, never polled.
pub const LOGIN_NOTICE: &str = "/run/wol-login-notice";

/// The notice as shown: first line, trimmed, at most 300 chars; None for absent/empty.
pub fn login_notice(text: Option<String>) -> Option<String> {
    let t = text?;
    let line = t.lines().next().unwrap_or_default().trim();
    (!line.is_empty()).then(|| line.chars().take(300).collect())
}

/// Login window: user tiles, then the picked user's password pill; power row at the foot.
pub struct Login {
    pub root: gtk::Widget,
    /// The stack of screens, with the notice above it.
    middle: gtk::Box,
    pub stack: gtk::Stack,
    pub panel: Rc<w::UserPanel>,
    pub back: gtk::Button,
    selected: Rc<RefCell<String>>,
    failures: std::cell::Cell<u32>,
}

impl Login {
    /// `users`: (login name, name shown), see [`login::login_users`].
    pub fn new(users: &[(String, String)]) -> Rc<Login> {
        let panel = Rc::new(w::UserPanel::new(
            users.first().map_or("", |u| u.1.as_str()),
        ));
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .vhomogeneous(false)
            .hhomogeneous(false)
            .build();
        let tiles = w::hbox(w::S6);
        tiles.set_halign(gtk::Align::End);
        tiles.add_css_class("login-tiles");
        stack.add_named(&tiles, Some("users"));
        let pw = w::vbox(w::S4);
        pw.append(&panel.root);
        let (back_box, back) = action("go-previous-symbolic", crate::i18n::tr("Back"), || {});
        pw.append(&back_box);
        stack.add_named(&pw, Some("password"));
        // No account can sign in: say so instead of an empty row (audit4 S3-7).
        if users.is_empty() {
            let (title, hint) = crate::copy::LOGIN_NO_ACCOUNTS;
            let (title, hint) = (crate::i18n::tr(title), crate::i18n::tr(hint));
            stack.add_named(
                &w::empty_state("system-users-symbolic", title, hint, None),
                Some("none"),
            );
            stack.set_visible_child_name("none");
        }
        let foot = w::hbox(w::S10);
        foot.add_css_class("login-power");
        // A power request wol-login did not take is said under the row (it was dropped).
        let refused = w::label("", "lock-caption");
        refused.set_visible(false);
        for p in Power::ALL {
            let r = refused.clone();
            let run = move || {
                let r = r.clone();
                glib::spawn_future_local(async move {
                    let v = gtk::gio::spawn_blocking(move || login::power(login::LOGIN_SOCKET, p))
                        .await
                        .unwrap_or(Verdict::Error);
                    // A later answer replaces an earlier refusal: a taken request clears it.
                    let (text, shown) = caption(power_note(p, v));
                    r.set_label(text);
                    r.set_visible(shown);
                });
            };
            foot.append(&action(login::power_icon(p), login::power_label(p), run).0);
        }
        let footer = w::vbox(w::S2);
        footer.append(&foot);
        footer.append(&refused);
        // Above the accounts: why a session ended early ([`Login::set_notice`]).
        let middle = w::vbox(w::S4);
        middle.append(&stack);
        let root = screen(&body(&middle, Some(footer.upcast_ref())));
        let l = Rc::new(Login {
            root,
            middle,
            stack,
            panel,
            back,
            selected: Default::default(),
            failures: Default::default(),
        });
        for (u, shown) in users {
            let t = gtk::Button::builder().build();
            t.add_css_class("login-tile");
            let c = w::vbox(w::S3);
            let av = w::avatar(shown, w::AVATAR_XL);
            if let Some(tone) = login::read_avatar(u) {
                kit::assistant::set_tone(&av, tone);
            }
            c.append(&av);
            c.append(&w::centered(shown, "lock-name"));
            t.set_child(Some(&c));
            let (l2, u, shown) = (Rc::downgrade(&l), u.clone(), shown.clone());
            t.connect_clicked(move |_| {
                if let Some(l) = l2.upgrade() {
                    l.pick(&u, &shown)
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
        if let [(only, shown)] = users {
            l.pick(only, shown);
            if let Some(b) = l.back.parent() {
                b.set_visible(false);
            }
        }
        l
    }

    /// A session that ended early says why, once, above the accounts (QA-LIVE-33/DATA-3);
    /// `notice` is [`login_notice`]'s answer. The greeter reads the file off the UI thread.
    pub fn set_notice(&self, notice: Option<String>) {
        let Some(n) = notice else { return };
        let note = w::centered(&n, "lock-caption");
        note.set_wrap(true);
        self.middle.prepend(&note);
    }

    /// Shows `user`'s password pill, titled with `shown` (full name).
    pub fn pick(&self, user: &str, shown: &str) {
        *self.selected.borrow_mut() = user.to_string();
        self.failures.set(0);
        self.panel.set_user(shown);
        // The account's colour, as on its tile and on its lock screen (parity, vt-lock journey):
        // the one picked in Setup, else the one its name gives (w::avatar), never the colour of
        // the first account the panel was made for.
        if let Some(av) = self.panel.root.first_child() {
            show_tone(
                &av,
                login::read_avatar(user).or_else(|| w::name_tone(shown)),
            );
        }
        self.stack.set_visible_child_name("password");
        self.panel.field.grab_focus();
    }

    /// Wrong password: after three, the account's hint replaces the message.
    pub fn failed(&self, v: Verdict) {
        let user = self.selected();
        if let Some(h) = login::wrong_password(&self.failures, v, || login::read_hint(&user)) {
            self.panel.message.set_label(&h);
        }
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

/// The caption under a power row for `note` (what [`power_note`] returned): its text and whether
/// it is shown. No note clears the caption (audit6 S6-5: a refusal stayed after a later
/// request was taken).
pub fn caption(note: Option<&'static str>) -> (&'static str, bool) {
    (note.unwrap_or(""), note.is_some())
}

/// What the lock screen's Restart says once `wol power restart` answered. The restart is
/// refused while another account is signed in (wol-admind's rule, `user_level_ok`), and the lock
/// screen cannot ask for more rights, so the caption names that way out (audit6 S6-5).
pub fn restart_caption(taken: bool) -> (&'static str, bool) {
    caption((!taken).then_some(crate::i18n::tr(crate::copy::LOCK_RESTART_REFUSED)))
}

/// What a power button's answer shows: nothing when it was taken.
pub fn power_note(p: Power, v: Verdict) -> Option<&'static str> {
    (v != Verdict::Ok).then(|| crate::copy::power_refused(p))
}

/// The session's login name: `$USER`, else the `passwd` entry of this process's uid (audit4
/// S3-9: a lock screen started without `USER` showed "user", no avatar tone and no hint).
pub fn user_name(passwd: &str) -> String {
    user_name_from(crate::envv::var("USER"), shared::sys::euid(), passwd)
}

/// The account the lock screen is for: the one that owns the session (`$USER`, else its uid in
/// `passwd`), never the list of accounts the login window offers (QA-LIVE3-35: decided, a lock
/// screen unlocks one session).
pub fn lock_owner(env: Option<String>, uid: u32, passwd: &str) -> String {
    user_name_from(env, uid, passwd)
}

fn user_name_from(env: Option<String>, uid: u32, passwd: &str) -> String {
    env.filter(|u| !u.is_empty())
        .or_else(|| {
            passwd.lines().find_map(|l| {
                let f: Vec<&str> = l.split(':').collect();
                let id: u32 = f.get(2)?.parse().ok()?;
                (id == uid).then(|| f[0].to_string())
            })
        })
        .unwrap_or_else(|| "user".into())
}

/// The accounts the login window offers: [`login::login_users`], which uses the same rule
/// wol-login accepts (shared `login_names`, with the no-`/etc/shells` fallback; audit5 S9-1).
pub fn login_accounts(passwd: &str, shells: &str) -> Vec<(String, String)> {
    login::login_users(passwd, shells)
}

/// Wires Enter -> blocking check off the main thread -> `ok(password)` or shake.
fn wire<C: FnOnce(String) -> Verdict + Send + 'static>(
    panel: Rc<w::UserPanel>,
    check: impl Fn() -> C + 'static,
    ok: impl Fn(String) + Clone + 'static,
    failed: impl Fn(Verdict) + Clone + 'static,
) {
    let check = Rc::new(check);
    let p2 = panel.clone();
    panel.field.connect_activate(move |f| {
        let pw = f.text().to_string();
        let typed = pw.clone();
        p2.set_busy(true);
        let (p, check, ok, failed) = (p2.clone(), check(), ok.clone(), failed.clone());
        glib::spawn_future_local(async move {
            let v = gtk::gio::spawn_blocking(move || check(pw))
                .await
                .unwrap_or(Verdict::Error);
            p.set_busy(false);
            if v == Verdict::Ok {
                ok(typed)
            } else {
                p.reject(v.message());
                failed(v);
            }
        });
    });
}

/// The keyring daemon's own unlock: it reads the password on stdin.
pub const KEYRING_UNLOCK: [&str; 2] = ["gnome-keyring-daemon", "--unlock"];
/// Longest the unlock may take before it is given up (the lock screen is gone by then anyway).
const KEYRING_UNLOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// Runs `program --unlock` with `password` and a newline on its stdin; true when it exited 0. The
/// password goes on stdin only (never in an argument, never in the environment). A failure says
/// why; a daemon that is not installed is nothing to unlock.
pub fn run_keyring_unlock(program: &str, args: &[&str], password: &str) -> bool {
    use wol_core::system::{RunFail, run_full};
    let argv: Vec<&str> = std::iter::once(program)
        .chain(args.iter().copied())
        .collect();
    match run_full(&argv, Some(&format!("{password}\n")), KEYRING_UNLOCK_WAIT) {
        Ok((0, ..)) => true,
        Ok((code, _, err)) => {
            crate::diag::say!("wol-lock: {program} exited {code}: {}", err.trim());
            false
        }
        Err(RunFail::Start(_)) => false,
        Err(RunFail::TimedOut) => {
            crate::diag::say!("wol-lock: {program} did not answer within {KEYRING_UNLOCK_WAIT:?}");
            false
        }
    }
}

/// After a good unlock: the login keyring opens with the password just checked (QA-LIVE1-2 and
/// A2's hand-over: a keyring the lock screen found locked made the next Browser/Store call ask for
/// a password the person never chose). At boot the session bus call (nobody had typed a password
/// yet), else, and after a lock, the daemon's own `--unlock`.
fn unlock_keyring(password: &str, startup: bool) {
    if startup {
        let r = zbus::blocking::Connection::session()
            .and_then(|c| login::unlock_login_keyring(&c, password));
        match r {
            Ok(()) => return,
            Err(e) => crate::diag::say!("wol-lock: login keyring: {e}"),
        }
    }
    let [prog, arg] = KEYRING_UNLOCK;
    if !run_keyring_unlock(prog, &[arg], password) {
        crate::diag::say!(
            "wol-lock: the login keyring was not unlocked (no daemon, or already open)"
        );
    }
}

/// A lock screen on the live system (`apps::LIVE_MARK`) is refused: the live account's password
/// is locked, so nothing could ever unlock it and the only way out was the power button
/// (QA-FDAY-7). Every way in ends here: the menu, Ctrl+Super+Q, the sleep hook, `loginctl
/// lock-session`. `--startup` is the login window of an installed system, never live.
pub fn refuses_lock(live: bool, startup: bool) -> bool {
    live && !startup
}

/// Exit code of a lock screen on a compositor without ext-session-lock (EX_UNAVAILABLE): the
/// supervisor does not start it again (QA-FILES-13).
pub const LOCK_UNSUPPORTED: i32 = 69;

/// The exit code when the compositor cannot lock the session; None when it can.
pub fn no_lock_exit(supported: bool) -> Option<i32> {
    (!supported).then_some(LOCK_UNSUPPORTED)
}

/// logind's `SetLockedHint` on this session: bus name, object, interface, method.
pub const LOCKED_HINT: [&str; 4] = [
    "org.freedesktop.login1",
    "/org/freedesktop/login1/session/auto",
    "org.freedesktop.login1.Session",
    "SetLockedHint",
];

/// Tells logind the session is (not) locked, so wold and the doctor can see a session whose lock
/// screen died (QA-LIVE-41). A failure is said and never stops the lock.
fn set_locked_hint(on: bool) {
    let [name, path, iface, method] = LOCKED_HINT;
    let r = zbus::blocking::Connection::system().and_then(|c| {
        c.call_method(Some(name), path, Some(iface), method, &(on,))
            .map(|_| ())
    });
    if let Err(e) = r {
        crate::diag::say!("wol-lock: logind locked hint {on}: {e}");
    }
}

/// The user unit that supervises the lock screen (files/usr/lib/systemd/user/wolos-lock.service).
pub const LOCK_UNIT: &str = "wolos-lock.service";

/// A `wol-lock` started by hand (Super+L, the menu, hypridle, the sleep hook) hands the lock to
/// [`LOCK_UNIT`]; one already running under systemd (`INVOCATION_ID`) or the boot lock does not.
pub fn hands_off_to_unit(startup: bool, under_systemd: bool) -> bool {
    !startup && !under_systemd
}

/// The commands that start [`LOCK_UNIT`] on this session's display: the user manager first learns
/// the display variables of the caller (it may have started before the compositor exported them).
pub fn lock_unit_argvs() -> [Vec<&'static str>; 2] {
    [
        vec![
            "systemctl",
            "--user",
            "import-environment",
            "WAYLAND_DISPLAY",
            "HYPRLAND_INSTANCE_SIGNATURE",
            "XDG_RUNTIME_DIR",
        ],
        vec!["systemctl", "--user", "start", LOCK_UNIT],
    ]
}

/// How long one `systemctl --user` call of [`start_lock_unit`] may take.
const UNIT_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// True when the unit took the lock; false (no user manager, the unit refused: said) and this
/// process supervises the lock itself, as before.
fn start_lock_unit() -> bool {
    lock_unit_argvs()
        .iter()
        .all(|a| match wol_core::system::run_full(a, None, UNIT_WAIT) {
            Ok((0, ..)) => true,
            Ok((code, _, err)) => {
                crate::diag::say!("wol-lock: {} exited {code}: {}", a.join(" "), err.trim());
                false
            }
            Err(e) => {
                crate::diag::say!("wol-lock: {}: {e:?}", a.join(" "));
                false
            }
        })
}

/// `wol-lock [--startup]`: a supervisor that keeps a lock screen up until one reports a real
/// unlock (ADR 0012). A crashed lock screen leaves the session locked (ext-session-lock) and the next
/// one takes it over (`misc:allow_session_lock_restore`). Nothing here ever unlocks. `args`: the
/// binary's arguments (without its name).
pub fn run_lock(args: &[String]) -> glib::ExitCode {
    let startup = args.iter().any(|a| a == login::STARTUP);
    if refuses_lock(crate::apps::is_live(), startup) {
        crate::diag::say!("wol-lock: the live session has no password to unlock with; not locking");
        return glib::ExitCode::SUCCESS;
    }
    if crate::envv::is_set(login::LOCK_UI_ENV) {
        return run_lock_screen(startup);
    }
    // QA-LIVE-41: the supervisor runs as the user unit wolos-lock.service, which the user manager
    // starts again when it is killed (Restart=on-failure), so a session locked by
    // ext-session-lock always gets its lock screen back. At boot (--startup) the compositor's
    // --locked-cmd supervises it instead.
    if hands_off_to_unit(startup, crate::envv::is_set("INVOCATION_ID")) && start_lock_unit() {
        return glib::ExitCode::SUCCESS;
    }
    let ui_args = login::lock_ui_args(args);
    let exe = std::env::current_exe().unwrap_or_else(|_| "wol-lock".into());
    let var = |k: &str| crate::envv::var(k).unwrap_or_default();
    let runtime = var("XDG_RUNTIME_DIR");
    let socket = login::wayland_socket(&runtime, &var("WAYLAND_DISPLAY"));
    // One supervisor per session: a second Super+L, or the restarted compositor's --locked-cmd
    // while the previous supervisor already brings a lock screen back.
    // Without a guard (no runtime directory, no permission) the lock screen still comes up: a
    // second lock screen is better than none, so this one goes on, and says so.
    let _guard = match crate::supervise::take_guard(&runtime, login::LOCK_GUARD) {
        crate::supervise::Guard::Held(g) => Some(g),
        crate::supervise::Guard::Busy => return glib::ExitCode::SUCCESS,
        crate::supervise::Guard::Unavailable(e) => {
            crate::diag::say!("wol-lock: no session guard in '{runtime}' ({e}); locking anyway");
            None
        }
    };
    let mut attempt = 0;
    loop {
        let started = std::time::Instant::now();
        let out = std::process::Command::new(&exe)
            .args(&ui_args)
            .env(login::LOCK_UI_ENV, "1")
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::inherit())
            .output();
        let unlocked = out
            .as_ref()
            .is_ok_and(|o| login::reported_unlock(&o.stdout));
        if out
            .as_ref()
            .is_ok_and(|o| o.status.code() == Some(LOCK_UNSUPPORTED))
        {
            // wolos-lock.service does not restart on this code (RestartPreventExitStatus).
            return glib::ExitCode::from(u8::try_from(LOCK_UNSUPPORTED).unwrap_or(1));
        }
        match login::lock_exit(unlocked, socket.exists(), attempt) {
            login::LockExit::Done => {
                return if unlocked {
                    glib::ExitCode::SUCCESS
                } else {
                    glib::ExitCode::FAILURE
                };
            }
            login::LockExit::Respawn(delay) => {
                let st = out
                    .map(|o| o.status.to_string())
                    .unwrap_or_else(|e| format!("not started: {:?}", e.kind()));
                let note = login::respawn_note(&st, started.elapsed(), attempt);
                crate::diag::say!("wol-lock: {note}");
                // To the journal too: the next crash shows its cause (`journalctl -t wol-lock`).
                let _ = std::process::Command::new("logger")
                    .args(["-t", "wol-lock", "-p", "user.warning", "--", &note])
                    .status();
                attempt = login::next_attempt(attempt, started.elapsed());
                std::thread::sleep(delay);
            }
        }
    }
}

/// The lock screen itself. `startup`: fades in from the compositor's black, and the password
/// typed here also opens the login keyring (nobody typed one at boot).
/// What gets the keyboard back after an unlock: the window that was in front when the lock came
/// up. Locked from the WolOS menu, the menu's popup had the keyboard; it was gone at unlock, so
/// nothing had it and the open windows ignored every key (QA-FILES-20). None: no window was in
/// front (the compositor picks, as before).
pub(crate) fn refocus_after_unlock(
    before: Option<&crate::hypr::Client>,
) -> Option<crate::hypr::Dispatch> {
    before
        .filter(|c| !c.address.is_empty())
        .map(|c| crate::hypr::Dispatch::FocusWindow(format!("address:{}", c.address)))
}

fn run_lock_screen(startup: bool) -> glib::ExitCode {
    // NON_UNIQUE: a hung earlier instance holding the bus name must not turn every new lock
    // screen into a remote call that exits at once (the supervisor would respawn in vain).
    let app = gtk::Application::builder()
        .application_id("org.wolos.lock")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    crate::kit::fence::survive_bus_loss(&app);
    app.connect_activate(move |app| {
        kit::style::install("");
        let win = gtk::ApplicationWindow::new(app);
        win.add_css_class("lock-window");
        let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
        let user = user_name(&passwd);
        // The window in front now, read off the UI thread (a compositor request).
        let front =
            std::thread::spawn(|| refocus_after_unlock(crate::hypr::active_window().as_ref()));
        let front = Rc::new(RefCell::new(Some(front)));
        let panel = Rc::new(w::UserPanel::new(&login::display_name(&user, &passwd)));
        // The avatar in the colour picked in Setup, as on the login window.
        if let (Some(tone), Some(av)) = (login::read_avatar(&user), panel.root.first_child()) {
            kit::assistant::set_tone(&av, tone);
        }
        // At boot this is the login window: the power row is always there (Restart included),
        // so the Restart that only appears when the password can't be checked is not needed.
        // Both screens enter and leave on the same motion (`Entrance`, DT-7).
        let (screen, restart, entrance) = if startup {
            let (s, e) = startup_screen(&panel);
            (s, None, e)
        } else {
            let (s, r, e) = lock_screen_with_restart(&panel);
            (s, Some(r), e)
        };
        let shown = reveal::Reveal::new(&screen, startup);
        win.set_child(Some(&shown.root));
        if startup {
            shown.show_when(backdrop_ready, || reveal::boot_mark("lock shown"));
        }
        kit::snapshot_on_map(&win, "lock");
        let unlock = lock_surface(app, &win);
        let (app2, p_ok) = (app.clone(), panel.clone());
        wire(
            panel.clone(),
            || login::authcheck_owned,
            move |password: String| {
                // The content eases out over the blurred wallpaper, then the desktop shows; no
                // second Enter while it does.
                p_ok.field.set_sensitive(false);
                let (app2, unlock, front) = (app2.clone(), unlock.clone(), front.clone());
                entrance.leave_then(move || {
                    unlock();
                    println!("{}", login::UNLOCKED);
                    // The login keyring opens with the password just typed: at boot (nobody typed
                    // one yet) and after any lock (the daemon may have restarted, B2).
                    let app3 = app2.clone();
                    let front = front.borrow_mut().take();
                    glib::spawn_future_local(async move {
                        let _ = gtk::gio::spawn_blocking(move || {
                            // The window that had the keyboard before the lock gets it back.
                            if let Some(d) = front.and_then(|t| t.join().ok()).flatten() {
                                let _ = crate::hypr::dispatch(&d);
                            }
                            set_locked_hint(false);
                            unlock_keyring(&password, startup);
                        })
                        .await;
                        app3.quit();
                    });
                });
            },
            {
                let (failures, p) = (std::cell::Cell::new(0), panel.clone());
                move |v| {
                    if let Some(r) = &restart {
                        r.set_visible(v == Verdict::Error);
                    }
                    if let Some(h) = login::wrong_password(&failures, v, || login::read_hint(&user))
                    {
                        p.message.set_label(&h);
                    }
                }
            },
        );
        panel.field.grab_focus();
    });
    let code = app.run_with_args::<&str>(&[]);
    // A lock surface that could not lock ended the app with its own code (`lock_surface`).
    match LOCK_EXIT.load(std::sync::atomic::Ordering::Relaxed) {
        0 => code,
        c => glib::ExitCode::from(u8::try_from(c).unwrap_or(1)),
    }
}

/// The exit code `lock_surface` ends the lock screen with when no lock can be had (0: none): it
/// quits the app instead of exiting the process under GTK, and [`run_lock_screen`] returns it.
static LOCK_EXIT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// Ends the lock screen with `code` (see [`LOCK_EXIT`]).
#[cfg(feature = "layer-shell")]
fn end_lock_screen(app: &gtk::Application, code: i32) {
    LOCK_EXIT.store(code, std::sync::atomic::Ordering::Relaxed);
    app.quit();
}

/// Session lock on every monitor (this window on the first); returns the unlock action.
#[cfg(feature = "layer-shell")]
fn lock_surface(app: &gtk::Application, win: &gtk::ApplicationWindow) -> Rc<dyn Fn()> {
    use gtk::gdk;
    if let Some(code) = no_lock_exit(gtk4_session_lock::is_supported()) {
        // A plain fullscreen window is not a lock (any window or key can move past it): say so,
        // and leave the session unlocked rather than pretend (QA-FILES-13).
        let line =
            "wol-lock: this compositor has no session lock (ext-session-lock-v1): NOT locked";
        crate::diag::say!("{line}");
        crate::kit::spawn(&[
            "logger".into(),
            "-t".into(),
            "wol-lock".into(),
            "-p".into(),
            "user.err".into(),
            "--".into(),
            line.into(),
        ]);
        end_lock_screen(app, code);
        return Rc::new(|| {});
    }
    let lock = gtk4_session_lock::Instance::new();
    let ender = app.clone();
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
    // Refused or ended by the compositor: no lock surface will ever show. Exit non-zero so the
    // supervisor starts a new one instead of waiting on a window nobody sees.
    let refused = ender.clone();
    lock.connect_failed(move |_| {
        crate::diag::say!("wol-lock: the compositor refused the session lock");
        end_lock_screen(&refused, 1);
    });
    if !lock.lock() {
        crate::diag::say!("wol-lock: the session lock could not start");
        end_lock_screen(&ender, 1);
        return Rc::new(|| {});
    }
    // Off the UI thread (a bus connect): the lock is up whatever logind says.
    let hint = std::thread::Builder::new().name("locked-hint".into());
    if let Err(e) = hint.spawn(|| set_locked_hint(true)) {
        crate::diag::say!("wol-lock: no thread for logind's locked hint: {e}");
    }
    Rc::new(move || lock.unlock())
}

#[cfg(not(feature = "layer-shell"))]
fn lock_surface(_app: &gtk::Application, win: &gtk::ApplicationWindow) -> Rc<dyn Fn()> {
    crate::diag::say!("wol-lock: built without layer-shell: NOT a secure lock (dev window)");
    win.present();
    Rc::new(|| {})
}

pub fn run_login() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("org.wolos.loginwindow")
        .build();
    crate::kit::fence::survive_bus_loss(&app);
    app.connect_activate(|app| {
        kit::style::install("");
        let win = gtk::ApplicationWindow::new(app);
        win.add_css_class("lock-window");
        let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
        let shells = std::fs::read_to_string("/etc/shells").unwrap_or_default();
        let l = Login::new(&login_accounts(&passwd, &shells));
        {
            let l = l.clone();
            glib::spawn_future_local(async move {
                let text = gtk::gio::spawn_blocking(|| std::fs::read_to_string(LOGIN_NOTICE).ok())
                    .await
                    .unwrap_or_default();
                l.set_notice(login_notice(text));
            });
        }
        // Fades in from the compositor's black, and out to black for the session (ADR 0012).
        let shown = reveal::Reveal::new(&l.root, true);
        win.set_child(Some(&shown.root));
        shown.show_when(backdrop_ready, || {
            reveal::show_pointer();
            reveal::boot_mark("login window shown");
        });
        let (l2, l4) = (l.clone(), l.clone());
        wire(
            l.panel.clone(),
            move || {
                let user = l2.selected();
                move |pw: String| login::login(login::LOGIN_SOCKET, &user, &pw)
            },
            |_| reveal::exit_faded(),
            move |v| l4.failed(v),
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

    /// A good unlock hands the password to the keyring daemon on stdin, once, and nowhere else.
    #[test]
    fn keyring_unlock_takes_the_password_on_stdin() {
        let dir = std::env::temp_dir().join(format!("wol-lock-kr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (out, args) = (dir.join("stdin"), dir.join("args"));
        let script = dir.join("gnome-keyring-daemon");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\ncat > {}\necho \"$@\" > {}\n",
                out.display(),
                args.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let [_, arg] = KEYRING_UNLOCK;
        assert!(run_keyring_unlock(
            script.to_str().unwrap(),
            &[arg],
            "s3cret pass"
        ));
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "s3cret pass\n");
        assert_eq!(std::fs::read_to_string(&args).unwrap().trim(), "--unlock");
        assert!(!run_keyring_unlock(
            "/nonexistent/gnome-keyring-daemon",
            &[arg],
            "x"
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// QA-FILES-13: a compositor without a session lock gets no lock screen and no respawn loop.
    #[test]
    fn fallback_exits_nonzero() {
        assert_eq!(no_lock_exit(false), Some(69));
        assert_eq!(no_lock_exit(true), None);
        assert_eq!(LOCK_UNSUPPORTED, 69);
    }

    /// QA-LIVE-41: a lock started by hand runs under wolos-lock.service (restarted when killed);
    /// the unit's own process and the boot lock supervise themselves.
    #[test]
    fn lock_hands_off_to_the_user_unit() {
        assert!(hands_off_to_unit(false, false));
        assert!(!hands_off_to_unit(false, true));
        assert!(!hands_off_to_unit(true, false));
        let [import, start] = lock_unit_argvs();
        assert_eq!(import[..3], ["systemctl", "--user", "import-environment"]);
        assert!(import.contains(&"WAYLAND_DISPLAY"));
        assert_eq!(
            start,
            ["systemctl", "--user", "start", "wolos-lock.service"]
        );
    }

    /// QA-FILES-20: after an unlock the window that was in front gets the keyboard back (locked
    /// from the WolOS menu, nothing had it: the popup that held it was gone).
    #[test]
    fn unlock_refocuses_the_window_in_front() {
        let c =
            crate::hypr::parse_active_client(r#"{"address":"0x5d2a","class":"foot","title":"~"}"#)
                .unwrap();
        assert_eq!(
            refocus_after_unlock(Some(&c)).map(|d| d.to_request()),
            Some(crate::hypr::Dispatch::FocusWindow("address:0x5d2a".into()).to_request())
        );
        assert!(
            refocus_after_unlock(Some(&c))
                .unwrap()
                .to_request()
                .contains("focuswindow address:0x5d2a")
        );
        assert_eq!(refocus_after_unlock(None).map(|d| d.to_request()), None);
        let src = include_str!("lock.rs");
        let code = &src[..src.find("#[cfg(test)]").unwrap()];
        assert!(code.contains("refocus_after_unlock(crate::hypr::active_window().as_ref())"));
        assert!(code.contains("crate::hypr::dispatch(&d)"));
    }

    /// QA-LIVE-41: the lock tells logind the session is locked, and unlocked again.
    #[test]
    fn sets_locked_hint() {
        assert_eq!(
            LOCKED_HINT,
            [
                "org.freedesktop.login1",
                "/org/freedesktop/login1/session/auto",
                "org.freedesktop.login1.Session",
                "SetLockedHint"
            ]
        );
        let src = include_str!("lock.rs");
        let code = &src[..src.find("#[cfg(test)]").unwrap()];
        let (on, off) = (
            code.find("set_locked_hint(true)").unwrap(),
            code.find("set_locked_hint(false);").unwrap(),
        );
        assert!(on > 0 && off > 0, "both ends tell logind");
    }

    /// QA-LIVE-33 / DATA-3: wol-login's one sentence shows above the accounts.
    #[test]
    fn login_notice_is_the_first_line() {
        assert_eq!(login_notice(None), None);
        assert_eq!(login_notice(Some("".into())), None);
        assert_eq!(login_notice(Some("  \n".into())), None);
        assert_eq!(
            login_notice(Some("Your disk is full.\nx".into())),
            Some("Your disk is full.".into())
        );
        assert_eq!(
            login_notice(Some("a".repeat(400))).unwrap().chars().count(),
            300
        );
    }

    /// QA-LIVE3-35: the lock screen names the session's owner, from the environment or the uid,
    /// whatever other accounts exist.
    #[test]
    fn lock_owner_is_the_session_user() {
        let pw = "root:x:0:0::/root:/bin/sh\nkim:x:1001:1001:Kim:/home/kim:/bin/sh\nbo:x:1002:1002:Bo:/home/bo:/bin/sh\n";
        assert_eq!(lock_owner(Some("bo".into()), 1001, pw), "bo");
        assert_eq!(lock_owner(None, 1001, pw), "kim");
        assert_eq!(lock_owner(Some("".into()), 1002, pw), "bo");
    }

    /// QA-FDAY-7: wol-lock on the live stick locked a session nobody could unlock.
    #[test]
    fn live_refuses_lock() {
        assert!(super::refuses_lock(true, false));
        assert!(
            !super::refuses_lock(false, false),
            "an installed system locks"
        );
        assert!(
            !super::refuses_lock(false, true),
            "the login window of an installed system"
        );
    }

    #[test]
    fn lock_shows_the_full_name() {
        let pw = "root:*:0:0:Charlie &:/root:/bin/sh\nkim:*:1001:1001:Kim Lee,,,:/home/kim:/bin/sh\nbo:*:1002:1002::/home/bo:/bin/sh\n";
        for (user, want) in [("kim", "Kim Lee"), ("bo", "bo"), ("ghost", "ghost")] {
            assert_eq!(login::display_name(user, pw), want);
        }
    }

    /// Audit4 S3-9: without `$USER` the lock screen looked up "user"; the uid finds the account.
    #[test]
    fn the_lock_screen_finds_its_user_without_the_environment() {
        let pw = "root:*:0:0::/root:/bin/sh\nkim:*:1001:1001:Kim Lee:/home/kim:/bin/sh\n";
        assert_eq!(user_name_from(None, 1001, pw), "kim");
        assert_eq!(user_name_from(Some(String::new()), 1001, pw), "kim");
        assert_eq!(user_name_from(Some("bo".into()), 1001, pw), "bo");
        assert_eq!(
            user_name_from(None, 4242, pw),
            "user",
            "no entry: still a name"
        );
        assert_eq!(
            login::display_name(&user_name_from(None, 1001, pw), pw),
            "Kim Lee"
        );
    }

    /// Audit4 S3-7: with `/etc/shells` missing or a login shell not listed, nobody could sign
    /// in; the fallback keeps service accounts out. Power answers that were dropped are said.
    #[test]
    fn the_login_window_lists_accounts_without_etc_shells() {
        let pw = "root:*:0:0::/root:/bin/sh\n\
                  bo:*:1002:1002:Bo Peep:/home/bo:/usr/bin/fish\n\
                  svc:*:1003:1003::/var/svc:/usr/sbin/nologin\n\
                  off:*:1004:1004::/home/off:/bin/false\n\
                  nobody:*:65534:65534::/:/bin/sh\n";
        assert_eq!(
            login_accounts(pw, ""),
            [("bo".to_string(), "Bo Peep".to_string())]
        );
        assert_eq!(
            login_accounts(pw, "/bin/sh\n/usr/bin/fish\n"),
            login::login_users(pw, "/bin/sh\n/usr/bin/fish\n"),
            "listed shells: the normal rule"
        );
        assert!(login_accounts("root:*:0:0::/root:/bin/sh\n", "").is_empty());
        // The one-account login window's power row (lock_power_caption): Restart names the way
        // out, Sleep and Shut Down say they were not taken; a taken request says nothing.
        for p in Power::ALL {
            assert_eq!(lock_power_caption(p, true), ("", false), "{p:?}");
        }
        assert_eq!(
            lock_power_caption(Power::Restart, false),
            (crate::copy::LOCK_RESTART_REFUSED, true)
        );
        assert_eq!(
            lock_power_caption(Power::Sleep, false),
            (crate::copy::power_refused(Power::Sleep), true)
        );
        assert_eq!(power_note(Power::ShutDown, Verdict::Ok), None);
        // Audit6 S6-5: a refusal and then a taken request: the caption goes away.
        let refused = caption(power_note(Power::ShutDown, Verdict::Error));
        assert_eq!(refused, (crate::copy::power_refused(Power::ShutDown), true));
        assert_eq!(caption(power_note(Power::Sleep, Verdict::Ok)), ("", false));
        // The lock screen's Restart names the other signed-in account when it is refused,
        // and clears the text when taken.
        assert_eq!(
            restart_caption(false),
            (crate::copy::LOCK_RESTART_REFUSED, true)
        );
        assert!(crate::copy::LOCK_RESTART_REFUSED.contains("signed in"));
        assert_eq!(restart_caption(true), ("", false));
        assert_eq!(
            power_note(Power::ShutDown, Verdict::Error),
            Some(crate::copy::power_refused(Power::ShutDown))
        );
    }

    /// SP-5: the lock screen's Restart waits a bounded time for `wol power restart`; a `wol`
    /// that hangs is killed and counts as not taken (the refusal caption shows, the button
    /// comes back), a quick answer is read as it is.
    #[test]
    fn the_restart_wait_is_bounded() {
        fn run(sh: &str) -> (&'static str, [&str; 2]) {
            ("sh", ["-c", sh])
        }
        let t = std::time::Instant::now();
        let (p, a) = run("sleep 30");
        assert!(!finished_ok(p, &a, std::time::Duration::from_millis(200)));
        assert!(
            t.elapsed() < std::time::Duration::from_secs(5),
            "killed, not waited out"
        );
        let (p, a) = run("exit 0");
        assert!(finished_ok(p, &a, std::time::Duration::from_secs(5)));
        let (p, a) = run("exit 1");
        assert!(!finished_ok(p, &a, std::time::Duration::from_secs(5)));
        assert_eq!(
            caption(Some(crate::copy::LOCK_RESTART_REFUSED)),
            restart_caption(false)
        );
        let w = restart_wait();
        assert!(w > std::time::Duration::from_secs(5) && w < std::time::Duration::from_secs(600));
    }

    /// DT-7 (hig.md section 7): the lock's content enters with a staggered `ease` on the
    /// contract's durations, the whole entrance inside the 400 ms ceiling, first group first;
    /// it leaves the other way round, faster, and the unlock waits only for that.
    #[test]
    fn the_lock_enters_staggered_and_leaves_quickly() {
        use crate::kit::anim::Value;
        let st = enter_stagger();
        assert_eq!(st.len(), 3, "clock, avatar and name, password field");
        assert_eq!(
            (0..3).map(|i| st.delay_ms(i)).collect::<Vec<_>>(),
            [0, ENTER_GAP_MS, 2 * ENTER_GAP_MS]
        );
        assert_eq!(
            (0..3).map(|i| st.delay_rev_ms(i)).collect::<Vec<_>>(),
            [2 * ENTER_GAP_MS, ENTER_GAP_MS, 0],
            "the field leaves first"
        );
        assert_eq!(enter_motion(), Motion::tween(Curve::Ease, Duration::Normal));
        assert_eq!(leave_motion(), Motion::tween(Curve::Ease, Duration::Fast));
        assert!(st.total_ms(tween_ms(enter_motion())) <= Duration::Slow.ms());
        let leave = st.total_ms(tween_ms(leave_motion()));
        assert!(leave < st.total_ms(tween_ms(enter_motion())) && leave <= 300);
        // The field's opacity on the pure clock: nothing before its delay, eased (ahead of
        // linear) on the way, exactly 1 at the end.
        const MS: u64 = 1_000_000;
        let mut v = Value::new(0.0);
        assert!(v.retarget(1.0, enter_motion(), st.delay_ms(2)));
        v.tick(0);
        v.tick(100 * MS);
        assert_eq!(v.value(), 0.0, "still waiting its turn");
        v.tick(120 * MS + 125 * MS);
        assert!(v.value() > 0.5, "ease-out is past half at half time");
        assert!(v.tick(120 * MS + 250 * MS));
        assert_eq!(v.value(), 1.0);
        // The unlock path goes through the fade, and only the lock screen's own code calls it.
        let src = include_str!("lock.rs");
        let run =
            &src[src.find("fn run_lock_screen").unwrap()..src.find("fn lock_surface").unwrap()];
        let (fade, unlock) = (
            run.find("leave_then").unwrap(),
            run.find("unlock();").unwrap(),
        );
        assert!(fade < unlock, "the content fades, then the session unlocks");
    }

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

    /// Lock and login show the desktop's Moon Gate (day in Light, night in Dark), else the
    /// earlier pictures on an image without it.
    #[test]
    fn wallpaper_follows_variant() {
        let gate = kit::pickers::shipped_wallpaper("wolos-gate-day-4k").is_some();
        for (dark, name) in [
            (
                true,
                if gate {
                    "wolos-gate-night-4k.svg"
                } else {
                    "wonderland-night.svg"
                },
            ),
            (
                false,
                if gate {
                    "wolos-gate-day-4k.svg"
                } else {
                    "wonderland-day.svg"
                },
            ),
        ] {
            let f = wallpaper_file(dark).unwrap();
            assert!(f.ends_with(name), "{f:?}");
        }
    }

    #[test]
    fn the_backdrop_morphs_between_gates_only() {
        let (day, night) = (
            Path::new("/w/wolos-gate-day-4k.svg"),
            Path::new("/w/themes/gate-nord-night-4k.svg"),
        );
        assert!(backdrop_morphs(Some(day), night, false, false));
        assert!(
            !backdrop_morphs(None, night, false, false),
            "nothing on screen yet"
        );
        assert!(
            !backdrop_morphs(Some(day), night, true, false),
            "reduce motion"
        );
        assert!(!backdrop_morphs(Some(day), night, false, true), "goldens");
        let place = Path::new("/w/cachoeira-night-4k.svg");
        assert!(
            !backdrop_morphs(Some(day), place, false, false),
            "not a gate"
        );
    }
}
