// SPDX-License-Identifier: GPL-2.0-or-later
//! Wallpaper, OSD, session menu, window switcher (the clipboard panel is ui/clipboard.rs).
use crate::kit::{
    self,
    surface::{Keyboard, Layer, Spec},
    widgets as w,
};
use gtk::{gdk, gio, glib, prelude::*};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

/// Pixel width a wallpaper SVG is rendered at before the window knows its size.
const WALLPAPER_RENDER_PX: i32 = 1920;
/// Seconds between two looks at the clock for the Dynamic wallpaper.
const DYNAMIC_TICK: u32 = 60;

/// The desktop picture: the saved choice (`wol wallpaper set`, Settings > Wallpaper), followed
/// live through the shell reload and the choice file; Dynamic follows the clock.
/// One per display (`ui::displays`); it goes with its display.
pub fn wallpaper(app: &gtk::Application, display: &gdk::Monitor) -> gtk::ApplicationWindow {
    let win = super::window(app, "wallpaper", Spec::wallpaper());
    crate::kit::surface::set_monitor(&win, Some(display));
    dress_wallpaper(win.upcast_ref());
    win
}

/// The wallpaper picture into `win`; its reload hook, file watch, light/dark watch and Dynamic
/// tick end with it. A new picture cross-fades over the old one ([`CrossFade`], 400 ms on the
/// window's clock); from one Moon Gate picture to another (day and night, a theme's dye) the
/// gate's parameters move instead ([`CrossFade::show_morph`], `kit::gate`).
pub fn dress_wallpaper(win: &gtk::Window) {
    let life = crate::kit::lifetime::Lifetime::of_window(win);
    let fade = CrossFade::new(&kit::anim::Clock::of(win));
    win.set_child(Some(&fade.root));
    let shown: RefCell<Option<std::path::PathBuf>> = Default::default();
    let weak = win.downgrade();
    let show = Rc::new(move || {
        let size = weak
            .upgrade()
            .filter(|w| w.width() > 0 && w.height() > 0)
            .map(|w| (w.width(), w.height(), w.scale_factor()));
        let aspect = size.map_or(16.0 / 9.0, |(w, h, _)| f64::from(w) / f64::from(h));
        // The picture at the display's pixel width (1920 until the window has a size).
        let width = size.map_or(WALLPAPER_RENDER_PX, |(w, _, s)| w * s);
        let now = crate::kit::pickers::desktop_wallpaper_for(aspect);
        if *shown.borrow() == now {
            return;
        }
        let was = shown.replace(now.clone());
        let Some(p) = now else { return };
        // Gate to gate: the old picture's pixels are the parameters' start.
        let old = (was.as_deref().is_some_and(kit::gate::is_gate) && kit::gate::is_gate(&p))
            .then(|| fade.current()?.downcast::<gdk::Texture>().ok())
            .flatten();
        // Decoded off the main thread: the CSS gradient shows until the picture is ready.
        let fade = fade.clone();
        glib::spawn_future_local(async move {
            let p2 = p.clone();
            let Some(Some(t)) = kit::render::on(kit::render::Lane::Desktop, move || {
                wallpaper_texture(&p2, width)
            })
            .await
            else {
                return;
            };
            // The other side (day or night) rendered now, so a light/dark change does not wait.
            if let Some(other) = kit::pickers::other_side(&p) {
                glib::spawn_future_local(kit::render::on(kit::render::Lane::Desktop, move || {
                    wallpaper_texture(&other, width)
                }));
            }
            if let Some(old) = old.filter(|_| !fade.is_reduced()) {
                let (from, to) = (old.clone(), t.clone());
                if let Some(Some(m)) = kit::render::on(kit::render::Lane::Desktop, move || {
                    morph_between(&from, &to)
                })
                .await
                {
                    return fade.show_morph(m, &old, &t);
                }
            }
            fade.show(&t);
        });
    });
    {
        let s = show.clone();
        super::ipc::on_reload_while(&life, move |_| s());
    }
    let choice = gtk::gio::File::for_path(
        std::path::Path::new(&crate::kit::config_home()).join(shared::theme::WALLPAPER_FILE),
    );
    if let Ok(m) = choice.monitor_file(
        gtk::gio::FileMonitorFlags::NONE,
        gtk::gio::Cancellable::NONE,
    ) {
        let s = show.clone();
        m.connect_changed(move |_, _, _, _| s());
        life.on_end(move || {
            m.cancel();
        });
    }
    if let Some(settings) = gtk::Settings::default() {
        let s = show.clone();
        let id = settings.connect_gtk_application_prefer_dark_theme_notify(move |_| s());
        life.on_end(move || settings.disconnect(id));
    }
    life.own(glib::timeout_add_seconds_local(DYNAMIC_TICK, move || {
        show();
        glib::ControlFlow::Continue
    }));
}

/// Both gate pictures as one [`kit::gate::Morph`] at the working size; None when their sizes
/// differ. Blocking (the downloads, the parameter file, the conversion): off the GTK thread.
pub fn morph_between(from: &gdk::Texture, to: &gdk::Texture) -> Option<kit::gate::Morph> {
    use std::path::Path;
    let size = |t: &gdk::Texture| {
        Some((
            usize::try_from(t.width()).ok()?,
            usize::try_from(t.height()).ok()?,
        ))
    };
    let (w, h) = size(from)?;
    if (w, h) != size(to)? || w == 0 || h == 0 {
        return None;
    }
    let (ww, wh) = kit::gate::work_size(w, h);
    let px = |t: &gdk::Texture| {
        let mut buf = vec![0u8; w * h * 4];
        // B8g8r8a8 premultiplied (GDK's default download); the gate is opaque.
        t.download(&mut buf, w * 4);
        if (ww, wh) == (w, h) {
            buf
        } else {
            kit::gate::resample(&buf, w, h, w * 4, ww, wh)
        }
    };
    // The shipped parameter set (`walls.py gate --params`), else the built-in values.
    let params = kit::pickers::WALLPAPER_DIRS
        .iter()
        .find_map(|d| std::fs::read_to_string(Path::new(d).join(kit::gate::PARAMS_FILE)).ok())
        .and_then(|t| kit::gate::Params::parse(&t))
        .unwrap_or_default();
    Some(kit::gate::Morph::new(&px(from), &px(to), ww, wh, &params))
}

/// A frame of a morph (tightly packed BGRA, `w` x `h`) as a texture.
fn frame_texture(w: usize, h: usize, bytes: Vec<u8>) -> gdk::MemoryTexture {
    gdk::MemoryTexture::new(
        i32::try_from(w).unwrap_or(i32::MAX),
        i32::try_from(h).unwrap_or(i32::MAX),
        gdk::MemoryFormat::B8g8r8a8Premultiplied,
        &glib::Bytes::from_owned(bytes),
        w * 4,
    )
}

/// Makes the frames of a running morph on `Lane::Desktop` and puts the newest finished one on a
/// picture. A frame is a pass over up to 1920x1200 pixels (10 ms and more, tens on a slow VM):
/// made on the GTK thread by every tick it held up input for the whole 600 ms of a theme or
/// day/night change. A tick that finds the previous frame still being made keeps what is on
/// screen instead (QA-A7-9).
#[derive(Clone, Default)]
struct Frames {
    busy: Rc<Cell<bool>>,
    epoch: Rc<Cell<u32>>,
}

impl Frames {
    /// Starts the frame at `t` of `m` for `pic` unless one is still being made.
    fn request(&self, m: &Arc<kit::gate::Morph>, t: f32, pic: &gtk::Picture) {
        if self.busy.replace(true) {
            return;
        }
        let (m, pic) = (m.clone(), pic.clone());
        let (busy, epoch) = (self.busy.clone(), self.epoch.clone());
        let mine = epoch.get();
        glib::spawn_future_local(async move {
            let (w, h) = (m.w, m.h);
            let bytes = kit::render::on(kit::render::Lane::Desktop, move || m.frame(t)).await;
            busy.set(false);
            // The morph ended (or another began) while this frame was being made: it is late.
            if let (true, Some(bytes)) = (epoch.get() == mine, bytes) {
                pic.set_paintable(Some(&frame_texture(w, h, bytes)));
            }
        });
    }

    /// The morph is over: a frame still being made is dropped when it lands.
    fn end(&self) {
        self.epoch.set(self.epoch.get().wrapping_add(1));
    }
}

/// Runs `m` on `pic` (the lock, login and assistant backdrop): its frames over [`kit::gate::MS`],
/// then `to`. Keep the handle while it runs; under reduce motion `to` at once.
pub fn morph_picture(
    pic: &gtk::Picture,
    m: kit::gate::Morph,
    to: &gdk::Paintable,
) -> kit::anim::Animated {
    let (weak, m, frames) = (pic.downgrade(), Arc::new(m), Frames::default());
    let f = frames.clone();
    let run = kit::anim::Animated::new(pic, 0.0, move |v| {
        if let Some(p) = weak.upgrade() {
            f.request(&m, v, &p);
        }
    });
    let (weak, to) = (pic.downgrade(), to.clone());
    run.connect_settled(move |v| {
        if let (true, Some(p)) = (v >= 1.0, weak.upgrade()) {
            frames.end();
            p.set_paintable(Some(&to));
        }
    });
    run.retarget(
        1.0,
        kit::anim::Motion::Tween {
            curve: kit::motion::Curve::Linear,
            ms: kit::gate::MS,
        },
    );
    run
}

/// The picture of wallpaper `p` for a display `width` px wide: an SVG is rendered once at that
/// width and kept with the previews ([`kit::form::wallpaper_thumb`]: a 4k gate took ~8 s to decode
/// at its own size on every change); a photo as it is. Blocking: off the GTK thread.
pub fn wallpaper_texture(p: &std::path::Path, width: i32) -> Option<gdk::Texture> {
    if p.extension().is_some_and(|x| x == "svg") {
        kit::form::wallpaper_thumb(p, width)
    } else {
        gdk::Texture::from_filename(p).ok()
    }
}

/// The wallpaper's two pictures: `back` is on screen, `front` fades the next one in over it
/// (`out`, [`kit::motion::Duration::Slow`]) and hands it to `back` when it is done. The first
/// picture appears at once (nothing to fade from); a change during a fade finishes that one
/// first. Between two gate pictures `front` instead shows the morph's frames (opaque, linear over
/// [`kit::gate::MS`], each parameter eased inside it) and hands `back` the real new picture at the
/// end. Reduce motion: the new picture at once.
#[derive(Clone)]
pub struct CrossFade {
    pub root: gtk::Overlay,
    back: gtk::Picture,
    front: gtk::Picture,
    fade: kit::anim::Animated,
    clock: kit::anim::Clock,
    morph: Rc<RefCell<Option<(Arc<kit::gate::Morph>, gdk::Texture)>>>,
    run: kit::anim::Animated,
    frames: Frames,
}

impl CrossFade {
    pub fn new(clock: &kit::anim::Clock) -> CrossFade {
        let pic = || {
            let p = gtk::Picture::new();
            p.set_content_fit(gtk::ContentFit::Cover);
            p
        };
        let (back, front) = (pic(), pic());
        front.set_can_target(false);
        front.set_opacity(0.0);
        let root = gtk::Overlay::new();
        root.set_child(Some(&back));
        root.add_overlay(&front);
        let fade = {
            let f = front.clone();
            kit::anim::Animated::on_clock(clock, 0.0, move |v| f.set_opacity(v.into()))
        };
        {
            let (b, f) = (back.clone(), front.clone());
            fade.connect_settled(move |v| {
                if v == 1.0 && f.paintable().is_some() {
                    b.set_paintable(f.paintable().as_ref());
                    f.set_paintable(None::<&gdk::Paintable>);
                }
            });
        }
        let morph: Rc<RefCell<Option<(Arc<kit::gate::Morph>, gdk::Texture)>>> = Default::default();
        let frames = Frames::default();
        let run = {
            let (m, f, fr) = (morph.clone(), front.clone(), frames.clone());
            kit::anim::Animated::on_clock(clock, 0.0, move |v| {
                if let Some((m, _)) = m.borrow().as_ref() {
                    fr.request(m, v, &f);
                }
            })
        };
        {
            let (m, b, f, fr) = (morph.clone(), back.clone(), front.clone(), frames.clone());
            run.connect_settled(move |v| {
                if v < 1.0 {
                    return;
                }
                fr.end();
                if let Some((_, to)) = m.take() {
                    b.set_paintable(Some(&to));
                    b.set_opacity(1.0);
                    f.set_paintable(None::<&gdk::Paintable>);
                    f.set_opacity(0.0);
                }
            });
        }
        CrossFade {
            root,
            back,
            front,
            fade,
            clock: clock.clone(),
            morph,
            run,
            frames,
        }
    }

    /// Reduce motion is on (no morph is prepared then).
    pub fn is_reduced(&self) -> bool {
        self.clock.is_reduced()
    }

    /// Ends a fade or a morph that is still moving.
    fn settle(&self) {
        if self.fade.is_moving() {
            self.fade.finish();
        }
        if self.morph.borrow().is_some() {
            self.run.finish();
        }
    }

    /// Shows `t`: over the picture on screen, or at once when there is none.
    pub fn show(&self, t: &impl IsA<gdk::Paintable>) {
        if self.back.paintable().is_none() {
            return self.back.set_paintable(Some(t));
        }
        self.settle();
        self.fade.set(0.0);
        self.front.set_paintable(Some(t));
        self.fade.retarget(
            1.0,
            kit::anim::Motion::tween(kit::motion::Curve::Out, kit::motion::Duration::Slow),
        );
    }

    /// Moves from `from` (the picture on screen) to `to` through `m` (made by [`morph_between`]
    /// from the two). At once when nothing is on screen or under reduce motion; a cross-fade when
    /// another picture came on screen while `m` was being made.
    pub fn show_morph(&self, m: kit::gate::Morph, from: &gdk::Texture, to: &gdk::Texture) {
        if self.back.paintable().is_none() || self.clock.is_reduced() {
            self.settle();
            return self.back.set_paintable(Some(to));
        }
        self.settle();
        if self.back.paintable().as_ref() != Some(from.upcast_ref::<gdk::Paintable>()) {
            return self.show(to);
        }
        self.frames.end();
        self.morph.replace(Some((Arc::new(m), to.clone())));
        // Frame 0 is the picture on screen: the front shows it until the first frame is made.
        self.front.set_paintable(Some(from));
        self.front.set_opacity(1.0);
        self.run.set(0.0);
        // The frames are opaque: the renderer need not draw the full-size picture under them.
        self.back.set_opacity(0.0);
        self.run.retarget(
            1.0,
            kit::anim::Motion::Tween {
                curve: kit::motion::Curve::Linear,
                ms: kit::gate::MS,
            },
        );
    }

    /// The picture on screen once the fade or morph is done (the one coming in, during it).
    pub fn current(&self) -> Option<gdk::Paintable> {
        if let Some((_, to)) = self.morph.borrow().as_ref() {
            return Some(to.clone().upcast());
        }
        self.front.paintable().or_else(|| self.back.paintable())
    }

    /// The front picture's opacity now (0 at rest).
    pub fn progress(&self) -> f32 {
        self.fade.value()
    }

    /// How far a morph is (0 at rest and at its start, 1 at its end).
    pub fn morph_progress(&self) -> f32 {
        self.run.value()
    }
}

/// Panel window: transparent overlay layer, glass content, Esc hides.
pub(super) fn panel(
    app: &gtk::Application,
    class: &str,
    ns: &'static str,
    anchor: [bool; 4],
    child: &impl IsA<gtk::Widget>,
) -> gtk::Window {
    panel_on(
        app,
        class,
        ns,
        anchor,
        Layer::Overlay,
        Keyboard::OnDemand,
        child,
    )
}

/// [`panel`] on a chosen layer and keyboard mode. Top, like the banners (`Spec::banners`): only
/// a fullscreen window mapped after it covers it; Overlay: nothing does.
pub(super) fn panel_on(
    app: &gtk::Application,
    class: &str,
    ns: &'static str,
    anchor: [bool; 4],
    layer: Layer,
    keyboard: Keyboard,
    child: &impl IsA<gtk::Widget>,
) -> gtk::Window {
    let spec = Spec {
        layer,
        ..Spec::panel(ns, anchor, keyboard)
    };
    let win = super::window(app, "panel", spec);
    crate::kit::surface::survive_output_loss(&win, spec);
    win.add_css_class(class);
    win.set_child(Some(child));
    let w2 = win.clone();
    kit::surface::on_escape(&win, move || super::transient::dismiss(&w2));
    win.upcast()
}

/// About <App> card: close light (top right, where the compositor puts close), 64 px icon, name,
/// app id. Glass, centred (HIG panel).
pub fn about_card(name: &str, icon: &str, class: &str) -> gtk::Box {
    let g = w::glass_panel(w::S6);
    g.set_size_request(w::ABOUT_WIDTH, -1);
    let close = gtk::Button::new();
    for c in ["light", "close"] {
        close.add_css_class(c);
    }
    close.set_tooltip_text(Some("Close"));
    close.connect_clicked(|b| {
        if let Some(win) = b.root().and_downcast::<gtk::Window>() {
            win.close();
        }
    });
    let lights = w::hbox(0);
    lights.add_css_class("traffic-lights");
    lights.set_halign(gtk::Align::End);
    lights.append(&close);
    g.append(&lights);
    let i = w::icon(icon, w::ICON_XL);
    i.set_halign(gtk::Align::Center);
    g.append(&i);
    g.append(&w::centered(name, "title"));
    g.append(&w::centered(class, "secondary"));
    g
}

/// Shell-drawn About panel: the frontmost app keeps focus and its name in the menu bar.
pub fn about(app: &gtk::Application, name: &str, icon: &str, class: &str) -> gtk::Window {
    let win = panel(
        app,
        "about",
        "wol-panel-about",
        [false; 4],
        &about_card(name, icon, class),
    );
    win.connect_hide(|w| w.destroy());
    // One at a time, and closed like the transient panels: another app in front, a press on the
    // Dock, a menu or a panel opening (audit2 A2-15; it stayed until Esc or its close light).
    if let Some(old) = ABOUT.with_borrow(|a| a.upgrade()) {
        old.set_visible(false);
    }
    ABOUT.with_borrow_mut(|a| a.set(Some(&win)));
    if !ABOUT_CLOSES.replace(true) {
        super::transient::also_close(Rc::new(|| {
            if let Some(w) = ABOUT.with_borrow(|a| a.upgrade()) {
                w.set_visible(false);
            }
        }));
    }
    win
}

thread_local! {
    /// The About panel on screen (destroyed on hide).
    static ABOUT: RefCell<glib::WeakRef<gtk::Window>> = Default::default();
    /// Its closer is in the transient set (added once, not per About).
    static ABOUT_CLOSES: Cell<bool> = const { Cell::new(false) };
}

/// The volume OSD never takes the keyboard (each volume key took it from the app in front).
pub const OSD_KEYBOARD: Keyboard = Keyboard::None;

pub struct Osd {
    pub window: gtk::Window,
    icon: gtk::Image,
    bar: gtk::ProgressBar,
    hide_gen: Cell<u32>,
}

impl Osd {
    pub fn build(app: &gtk::Application) -> Rc<Osd> {
        // The Kit's on-screen display pill (design/components/osd.toml).
        let (g, icon, bar) = kit::sheets::osd_pill("audio-volume-high-symbolic");
        // Top layer (kit::surface::Spec::banners), shown over full screen too (a HUD). No
        // keyboard: an on-demand layer takes the keys from the app on map.
        let window = panel_on(
            app,
            "osd",
            "wol-panel-osd",
            [false, true, false, false],
            Layer::Top,
            OSD_KEYBOARD,
            &g,
        );
        super::transient::fade_on_map(&window, kit::motion::Duration::Normal);
        Rc::new(Osd {
            window,
            icon,
            bar,
            hide_gen: Cell::new(0),
        })
    }

    /// Shows for 1.5 s; a newer show extends it. Also over a window in full screen, like the
    /// Volume HUD (brief; banners are the ones held, `notify::held`).
    pub fn show(self: &Rc<Self>, icon: &str, level: u8) {
        self.icon.set_icon_name(Some(icon));
        self.bar.set_fraction(level as f64 / 100.0);
        if self.window.is_visible() {
            // A show during the fade-out brings it back (its map fades a new one in).
            if let Some(p) = super::transient::presence(&self.window) {
                p.keep();
            }
        }
        self.window.set_visible(true);
        let seq = self.hide_gen.get() + 1;
        self.hide_gen.set(seq);
        let me = self.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(1500), move || {
            if me.hide_gen.get() == seq {
                super::transient::dismiss(&me.window);
            }
        });
    }
}

/// Restart / Shut Down / Log Out ask first: the question, a 60 s countdown, Cancel and
/// the action. Doing nothing runs it; Cancel, Esc or closing the panel drops it.
struct Confirm {
    view: gtk::Box,
    question: gtk::Label,
    line: gtk::Label,
    go: gtk::Button,
    action: Cell<&'static str>,
    left: Cell<u32>,
    timer: RefCell<Option<glib::SourceId>>,
}

impl Confirm {
    fn new(row: &gtk::Box) -> Rc<Confirm> {
        let view = w::vbox(w::S2);
        // The question sets the width; the countdown line wraps under it instead of widening
        // the panel (asks nothing of its own width: one character).
        let question = w::label("", "title");
        let line = w::label("", "secondary");
        line.set_wrap(true);
        line.set_wrap_mode(gtk::pango::WrapMode::Word);
        line.set_max_width_chars(1);
        let buttons = w::hbox(w::S2);
        buttons.set_halign(gtk::Align::End);
        let go = w::button("", w::Kind::Primary, || {});
        let c = Rc::new(Confirm {
            view,
            question,
            line,
            go,
            action: Cell::new(""),
            left: Cell::new(0),
            timer: RefCell::new(None),
        });
        let (c2, r2) = (Rc::downgrade(&c), row.clone());
        buttons.append(&w::button("Cancel", w::Kind::Default, move || {
            if let Some(c) = c2.upgrade() {
                c.stop();
                c.view.set_visible(false);
                r2.set_visible(true);
            }
        }));
        let c3 = Rc::downgrade(&c);
        c.go.connect_clicked(move |_| {
            if let Some(c) = c3.upgrade() {
                c.run();
            }
        });
        buttons.append(&c.go);
        c.view.append(&c.question);
        c.view.append(&c.line);
        c.view.append(&buttons);
        c.view.set_visible(false);
        c
    }

    fn show(self: &Rc<Self>, action: &'static str, row: &gtk::Box, countdown: bool) {
        self.stop();
        self.action.set(action);
        self.left.set(crate::clipboard::CONFIRM_SECS);
        self.render();
        row.set_visible(false);
        self.view.set_visible(true);
        self.go.grab_focus();
        if !countdown {
            return;
        }
        let c = Rc::downgrade(self);
        let id = glib::timeout_add_seconds_local(1, move || {
            let Some(c) = c.upgrade() else {
                return glib::ControlFlow::Break;
            };
            c.left.set(c.left.get().saturating_sub(1));
            if c.left.get() == 0 {
                c.timer.take();
                c.run();
                return glib::ControlFlow::Break;
            }
            c.render();
            glib::ControlFlow::Continue
        });
        self.timer.replace(Some(id));
    }

    fn render(&self) {
        if let Some((q, line, button)) =
            crate::copy::session_confirm(self.action.get(), self.left.get())
        {
            self.question.set_label(q);
            // The count keeps its unit on its line ("60 seconds.") when the line wraps.
            let line = match line.rsplit_once(' ') {
                Some((head, unit)) => format!("{head}\u{a0}{unit}"),
                None => line,
            };
            self.line.set_label(&line);
            self.go.set_label(button);
        }
    }

    fn stop(&self) {
        if let Some(id) = self.timer.take() {
            id.remove();
        }
    }

    fn run(&self) {
        self.stop();
        if let Some(win) = self.view.root().and_downcast::<gtk::Window>() {
            win.set_visible(false);
        }
        end_session(self.action.get());
    }
}

/// Wait between two looks at the windows still open.
const CLOSE_POLL: std::time::Duration = std::time::Duration::from_millis(250);
/// Longest wait for the open apps to answer a close request (a "Save changes?" sheet is an
/// answer: the window stays, and Restart / Shut Down / Log Out wait for the person).
pub const CLOSE_WAIT: std::time::Duration = std::time::Duration::from_secs(15);
/// A second Restart / Shut Down / Log Out this soon after one that found windows still open
/// goes ahead (the way out when an app never answers).
pub const FORCE_WITHIN: std::time::Duration = std::time::Duration::from_secs(120);

/// When the last session end stopped for open windows (one per process, not per UI thread).
static HELD: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

fn held() -> std::sync::MutexGuard<'static, Option<std::time::Instant>> {
    HELD.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// One request that asks every window to close (`closewindow`: the app's own close, so a
/// document asks to be saved). None without windows (QA-LIVE1-22: Shut Down with 8 apps open and
/// an unsaved note took the machine off in 12 s, asking nothing).
pub fn close_requests(clients: &[crate::hypr::Client]) -> Option<String> {
    let steps: Vec<crate::hypr::Dispatch> = clients
        .iter()
        .map(|c| crate::hypr::Dispatch::CloseWindow(format!("address:{}", c.address)))
        .collect();
    crate::hypr::batch(&steps)
}

/// Whether the session ends now: nothing is open any more, or this is the second try.
pub fn session_may_end(open_left: usize, forced: bool) -> bool {
    open_left == 0 || forced
}

/// The banner when the session stays: who is still open and what the person can do.
pub fn held_notice(action: &str, names: &[String]) -> Vec<String> {
    let what = match action {
        "restart" => "Restart",
        "logout" => "Log Out",
        _ => "Shut Down",
    };
    let mut apps = names.to_vec();
    apps.sort();
    apps.dedup();
    let body = format!(
        "{} still open. Save your work, then choose {what} again; choosing it twice does not wait.",
        apps.join(", ")
    );
    ["notify-send", "-a", "Session", "-u", "critical"]
        .iter()
        .map(|s| s.to_string())
        .chain([format!("{what} is waiting for your apps"), body])
        .collect()
}

/// Saves the window set (the autosave is every 60 s; QA-FILES-5) and asks every window to close;
/// false when there is nothing to wait for (no compositor answer, no window). Blocking.
fn ask_windows_to_close() -> bool {
    crate::sessions::save_now();
    let Some(clients) = crate::hypr::read_clients() else {
        return false;
    };
    let Some(req) = close_requests(&clients) else {
        return false;
    };
    let _ = crate::hypr::request(&req);
    true
}

/// [`ask_windows_to_close`] off the UI thread, then a look every [`CLOSE_POLL`] (a GLib timer,
/// nothing sleeps) for up to [`CLOSE_WAIT`]. The names of the windows still open.
async fn close_open_windows() -> Vec<String> {
    if !gio::spawn_blocking(ask_windows_to_close)
        .await
        .unwrap_or(false)
    {
        return Vec::new();
    }
    let t0 = std::time::Instant::now();
    loop {
        glib::timeout_future(CLOSE_POLL).await;
        let left = gio::spawn_blocking(|| crate::hypr::read_clients().unwrap_or_default())
            .await
            .unwrap_or_default();
        if left.is_empty() || t0.elapsed() >= CLOSE_WAIT {
            return left.into_iter().map(|c| c.class).collect();
        }
    }
}

/// How long the step before an action's command ([`crate::clipboard::before_end`]) may take.
const BEFORE_END_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Runs `argv` and waits for it (off the UI thread); a failure is said, the session still ends.
async fn run_before_end(argv: &'static [&'static str]) {
    let r =
        gio::spawn_blocking(move || wol_core::system::run_full(argv, None, BEFORE_END_WAIT)).await;
    match r {
        // pkill: 1 = nothing to stop.
        Ok(Ok((0 | 1, ..))) => {}
        Ok(Ok((code, _, err))) => {
            crate::diag::say!(
                "wol-shell: {} exited {code}: {}",
                argv.join(" "),
                err.trim()
            );
        }
        Ok(Err(e)) => crate::diag::say!("wol-shell: {}: {e:?}", argv.join(" ")),
        Err(_) => crate::diag::say!("wol-shell: {} did not run", argv.join(" ")),
    }
}

/// Runs a session action. Restart, Shut Down and Log Out ask the open apps to close first and
/// go on only when they have (or on the second try); Lock and Sleep end nothing.
pub fn end_session(action: &'static str) {
    let Some(argv) = crate::clipboard::session_command(action) else {
        return;
    };
    let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
    if !crate::clipboard::ends_apps(action) {
        return kit::spawn(&argv);
    }
    let forced = held().is_some_and(|t| t.elapsed() < FORCE_WITHIN);
    glib::spawn_future_local(async move {
        let left = if forced {
            Vec::new()
        } else {
            close_open_windows().await
        };
        // (A second try saved on the first; the set is not saved again over a half-closed one.)
        if session_may_end(left.len(), forced) {
            *held() = None;
            if let Some(pre) = crate::clipboard::before_end(action) {
                run_before_end(pre).await;
            }
            kit::spawn(&argv);
        } else {
            *held() = Some(std::time::Instant::now());
            kit::spawn(&held_notice(action, &left));
        }
    });
}

/// The session panel's buttons: icon, label, action ([`crate::clipboard::session_command`]).
const SESSION_ITEMS: [(&str, &str, &str); 5] = [
    ("system-lock-screen-symbolic", "Lock", "lock"),
    ("weather-clear-night-symbolic", "Sleep", "sleep"),
    ("system-reboot-symbolic", "Restart", "restart"),
    ("system-shutdown-symbolic", "Shut Down", "shutdown"),
    ("system-log-out-symbolic", "Log Out", "logout"),
];

/// The buttons on this kind of system: live (`apps::LIVE_MARK`) has no Lock, the live account
/// has no password to unlock with (QA-FDAY-7, as the WolOS menu).
pub fn session_items(live: bool) -> Vec<(&'static str, &'static str, &'static str)> {
    SESSION_ITEMS
        .into_iter()
        .filter(|i| !(live && i.2 == "lock"))
        .collect()
}

/// The session menu: Lock, Sleep, Restart, Shut Down, Log Out, and the confirm view the last
/// three open.
pub struct SessionMenu {
    pub root: gtk::Box,
    row: gtk::Box,
    confirm: Rc<Confirm>,
}

impl SessionMenu {
    pub fn new() -> SessionMenu {
        let root = w::glass_panel(w::S2);
        let row = w::hbox(w::S2);
        let confirm = Confirm::new(&row);
        for (icon, label, action) in session_items(crate::apps::is_live()) {
            let b = w::vbox(w::S1);
            let i = w::icon(icon, w::ICON_M);
            i.set_halign(gtk::Align::Center);
            b.append(&i);
            b.append(&w::centered(label, ""));
            let btn = gtk::Button::builder().child(&b).build();
            btn.add_css_class("session-item");
            let (c, r) = (confirm.clone(), row.clone());
            btn.connect_clicked(move |b| {
                if crate::clipboard::needs_confirm(action) {
                    c.show(action, &r, true);
                    return;
                }
                if let Some(win) = b.root().and_downcast::<gtk::Window>() {
                    win.set_visible(false);
                }
                end_session(action);
            });
            row.append(&btn);
        }
        root.append(&row);
        root.append(&confirm.view);
        SessionMenu { root, row, confirm }
    }

    /// Ask before `action` (restart, shutdown, logout); the countdown starts.
    pub fn ask(&self, action: &'static str) {
        self.confirm.show(action, &self.row, true);
    }

    /// The question as it first shows, without a countdown running (goldens).
    pub fn preview(&self, action: &'static str) {
        self.confirm.show(action, &self.row, false);
    }

    /// Back to the five actions; any countdown stops.
    pub fn reset(&self) {
        self.confirm.stop();
        self.confirm.view.set_visible(false);
        self.row.set_visible(true);
    }
}

impl Default for SessionMenu {
    fn default() -> Self {
        Self::new()
    }
}

thread_local! {
    /// The shell's session menu, for [`ask_session`].
    static SESSION: RefCell<Option<Rc<SessionMenu>>> = const { RefCell::new(None) };
}

/// The question a `wol shell show <target>` opens the session panel on: `session-logout` ->
/// `logout`, `session-restart` -> `restart`, `session-shutdown` -> `shutdown` (the WolOS menu's
/// Log Out…, Restart… and Shut Down…).
pub fn session_action(target: &str) -> Option<&'static str> {
    Some(match target {
        "session-logout" => "logout",
        "session-restart" => "restart",
        "session-shutdown" => "shutdown",
        _ => return None,
    })
}

/// The session panel is asking Log Out / Restart / Shut Down right now (its countdown runs).
pub fn session_sheet_up() -> bool {
    SESSION.with_borrow(|s| s.as_ref().is_some_and(|m| m.confirm.view.is_visible()))
}

/// The session panel asks `action`'s question (countdown running); nothing without a panel.
pub fn ask_session(action: &'static str) {
    SESSION.with_borrow(|s| {
        if let Some(m) = s {
            m.ask(action);
        }
    });
}

pub fn session(app: &gtk::Application) -> gtk::Window {
    let menu = Rc::new(SessionMenu::new());
    SESSION.set(Some(menu.clone()));
    let win = panel(app, "session", "wol-panel-session", [false; 4], &menu.root);
    // Hidden (Esc, toggled away, clicked outside): the countdown stops and the icons come back.
    win.connect_visible_notify(move |win| {
        if !win.is_visible() {
            menu.reset();
        }
    });
    win
}

#[cfg(test)]
mod tests {
    /// Log Out… and Shut Down… in the WolOS menu ran the same `toggle session`: each now opens
    /// the panel on its own question, through a target the CLI accepts.
    #[test]
    fn the_wolos_menu_opens_the_session_panel_on_its_question() {
        use super::session_action;
        assert_eq!(session_action("session-logout"), Some("logout"));
        assert_eq!(session_action("session-shutdown"), Some("shutdown"));
        assert_eq!(session_action("session-restart"), Some("restart"));
        assert_eq!(session_action("session"), None, "the plain panel");
        for t in ["session-logout", "session-restart", "session-shutdown"] {
            assert!(
                shared::shell::PANELS.contains(&t),
                "{t}: the wol CLI rejects it"
            );
            let action = session_action(t).unwrap();
            assert!(
                crate::clipboard::needs_confirm(action),
                "{action} asks first"
            );
            assert!(crate::clipboard::session_command(action).is_some());
        }
    }

    /// QA-FDAY-7: the live session panel offers no Lock (nothing unlocks the live account).
    #[test]
    fn live_session_panel_has_no_lock() {
        let actions = |live| {
            super::session_items(live)
                .iter()
                .map(|i| i.2)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            actions(false),
            ["lock", "sleep", "restart", "shutdown", "logout"]
        );
        assert_eq!(actions(true), ["sleep", "restart", "shutdown", "logout"]);
    }

    /// QA-FILES-5: Log Out ran `hyprctl dispatch exit` alone, every app died and the window set
    /// was never saved (ADR 0032 promises a save at logout; the autosave is every 60 s). Log
    /// Out, Restart and Shut Down save first (bounded), before any window is asked to close.
    #[test]
    fn logout_saves_sessions_first() {
        let src = include_str!("panels.rs");
        let code = &src[..src.find("#[cfg(test)]").unwrap_or(src.len())];
        let job = &code[code.find("fn ask_windows_to_close").unwrap()..];
        let (save, close) = (
            job.find("crate::sessions::save_now()").expect("saves"),
            job.find("close_requests(&clients)").expect("closes"),
        );
        assert!(
            save < close,
            "the set is saved before any window is asked to close"
        );
        let end = &code[code.find("pub fn end_session").unwrap()..];
        assert!(
            end.contains("close_open_windows"),
            "every ending action goes through it"
        );
        assert_eq!(
            crate::sessions::save_argv().join(" "),
            "wol sessions save",
            "the one command the autosave runs"
        );
        assert_eq!(
            crate::sessions::SAVE_AT_END,
            10,
            "a hung save never holds the logout"
        );
        for (a, then) in [
            ("restart", "wol power restart"),
            ("shutdown", "wol power shutdown"),
            ("logout", "hyprctl dispatch exit"),
        ] {
            assert!(crate::clipboard::ends_apps(a), "{a}");
            assert!(
                crate::clipboard::session_command(a)
                    .unwrap()
                    .join(" ")
                    .ends_with(then),
                "{a}"
            );
        }
        assert!(!crate::clipboard::ends_apps("sleep"), "sleep ends no app");
    }

    /// QA-LIVE-32: a click outside cancelled the Log Out question silently (it worked on the
    /// third try). While the sheet asks, a click outside or another app in front leaves the
    /// session panel; Esc closes every panel still.
    #[test]
    fn logout_sheet_outside_click_keeps_countdown() {
        use crate::ui::transient::{PANELS, panels_to_close};
        assert!(!panels_to_close(true).contains(&"session"));
        assert_eq!(panels_to_close(true).len(), PANELS.len() - 1);
        assert_eq!(
            panels_to_close(false),
            PANELS,
            "no question up: it closes like the others"
        );
        let src = include_str!("transient.rs");
        let esc = &src[src.find("pub fn close_on_escape").unwrap()..];
        let esc = &esc[..esc.find("\n}\n").unwrap()];
        assert!(
            esc.contains("hide(panels, PANELS)"),
            "Esc ends the question: {esc}"
        );
    }

    /// QA-LIVE1-22: every window gets a close request before the machine goes; a window that
    /// stays (a save sheet) holds the session until a second try.
    #[test]
    fn close_request_sent_to_every_window_before_poweroff() {
        use crate::hypr::Client;
        let cs = [
            Client::new("0x1", "foot", "a"),
            Client::new("0x2", "wol-files", "b"),
        ];
        let req = super::close_requests(&cs).unwrap();
        assert_eq!(
            req,
            "[[BATCH]]dispatch closewindow address:0x1;dispatch closewindow address:0x2"
        );
        assert_eq!(super::close_requests(&[]), None);
        assert!(super::session_may_end(0, false));
        assert!(!super::session_may_end(1, false), "a save sheet holds it");
        assert!(super::session_may_end(1, true), "the second try goes");
        for (a, yes) in [
            ("restart", true),
            ("shutdown", true),
            ("logout", true),
            ("sleep", false),
            ("lock", false),
        ] {
            assert_eq!(crate::clipboard::ends_apps(a), yes, "{a}");
        }
        let n = super::held_notice("shutdown", &["foot".into(), "foot".into(), "gedit".into()]);
        assert_eq!(n[n.len() - 2], "Shut Down is waiting for your apps");
        assert!(
            n[n.len() - 1].starts_with("foot, gedit still open."),
            "{n:?}"
        );
    }

    /// Audit2 A2-15: About ignored everything but Esc and its close light. (The clipboard half
    /// of this test moved to ui/clipboard.rs with the panel.)
    #[test]
    fn about_closes_like_a_menu() {
        let src = include_str!("panels.rs");
        let a = src.find("pub fn about(").unwrap();
        let about = &src[a..a + src[a..].find("pub struct Osd").unwrap()];
        assert!(
            about.contains("transient::also_close"),
            "About closes with the set"
        );
        assert!(
            about.contains("old.set_visible(false)"),
            "one About at a time"
        );
    }

    /// `wol wallpaper set` and Settings save the choice; the desktop showed one hard-coded picture.
    #[test]
    fn desktop_follows_the_saved_wallpaper() {
        let src = include_str!("panels.rs");
        let body =
            &src[src.find("pub fn wallpaper(").unwrap()..src.find("/// Panel window").unwrap()];
        for want in [
            "pickers::desktop_wallpaper_for(",
            "ipc::on_reload",
            "monitor_file",
        ] {
            assert!(body.contains(want), "{want}");
        }
        assert!(
            !src.contains(concat!("wonderland-dusk", ".svg")),
            "a hard-coded picture"
        );
    }
}
