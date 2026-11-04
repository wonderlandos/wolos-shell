// SPDX-License-Identifier: GPL-2.0-or-later
//! Screenshot toolbar (Super+Shift+5), the floating thumbnail that follows a capture, and
//! Markup (ADR 0031 W5): the thumbnail's pen button opens the picture with three pens, Undo,
//! Copy and Save (a marked-up copy next to the original; the original is never changed).
use crate::capture::{self, DELAYS, Dest, Mode, Options};
use crate::kit::{surface::Spec, widgets as w};
use gtk::{gdk, gio, glib, prelude::*};
use std::cell::Cell;
use std::rc::Rc;

/// `wol shell show screen-record`: the toolbar opens in Record Entire Screen mode.
pub const RECORD: &str = "screen-record";

/// Seconds the thumbnail stays.
const THUMB_SECS: u32 = 5;

pub struct Toolbar {
    pub root: gtk::Box,
    pub mode: Rc<Cell<Mode>>,
    pub options: Rc<Cell<Options>>,
    pub capture: gtk::Button,
    pub close: gtk::Button,
    /// One toggle per mode, in `Mode::ALL` order.
    pub modes: Vec<(Mode, gtk::ToggleButton)>,
}

impl Toolbar {
    pub fn new(mode: Mode, options: Options) -> Self {
        let root = w::glass_panel(0);
        root.add_css_class("capture-bar");
        let row = w::hbox(w::S1);
        root.append(&row);
        let (mode_c, opts_c) = (Rc::new(Cell::new(mode)), Rc::new(Cell::new(options)));

        let close = w::icon_button("window-close-symbolic", "Close", || {});
        close.add_css_class("capture-close");
        row.append(&close);
        let capture = gtk::Button::with_label(capture::action(mode));
        capture.add_css_class("btn");
        capture.add_css_class("btn-primary");
        capture.add_css_class("capture-go");

        let mut first: Option<gtk::ToggleButton> = None;
        let mut modes = Vec::new();
        for m in Mode::ALL {
            if matches!(m, Mode::Screen | Mode::RecordScreen) {
                row.append(&gtk::Separator::new(gtk::Orientation::Vertical));
            }
            let b = gtk::ToggleButton::builder()
                .child(&w::icon(m.icon(), w::ICON_TOOL))
                .tooltip_text(m.label())
                .active(m == mode)
                .build();
            b.add_css_class("capture-mode");
            if let Some(f) = &first {
                b.set_group(Some(f));
            } else {
                first = Some(b.clone());
            }
            let (mc, cap) = (mode_c.clone(), capture.clone());
            b.connect_toggled(move |b| {
                if b.is_active() {
                    mc.set(m);
                    cap.set_label(capture::action(m));
                }
            });
            row.append(&b);
            modes.push((m, b));
        }
        row.append(&gtk::Separator::new(gtk::Orientation::Vertical));

        let opts = gtk::MenuButton::builder()
            .label("Options")
            .popover(&options_menu(opts_c.clone()))
            .build();
        opts.add_css_class("capture-options");
        row.append(&opts);
        row.append(&capture);
        Toolbar {
            root,
            mode: mode_c,
            options: opts_c,
            capture,
            close,
            modes,
        }
    }

    /// Selects `m` (its toggle updates the mode and the capture button label).
    pub fn select(&self, m: Mode) {
        if let Some((_, b)) = self.modes.iter().find(|(x, _)| *x == m) {
            b.set_active(true);
        }
    }
}

/// Save to (Desktop / Pictures) and Timer (None / 5 / 10 s), radio rows.
fn options_menu(opts: Rc<Cell<Options>>) -> gtk::Popover {
    let p = gtk::Popover::new();
    p.add_css_class("menu");
    p.set_has_arrow(false);
    let col = w::vbox(0);
    col.append(&w::label("Save to", "menu-heading"));
    let cur = opts.get();
    let radio = |text: &str, on: bool, group: Option<&gtk::CheckButton>| {
        let c = gtk::CheckButton::with_label(text);
        c.set_active(on);
        c.set_group(group);
        c.add_css_class("menu-check");
        col.append(&c);
        c
    };
    let mut g: Option<gtk::CheckButton> = None;
    for (name, d) in [
        ("Desktop", Dest::Desktop),
        ("Pictures", Dest::Pictures),
        ("Clipboard", Dest::Clipboard),
    ] {
        let c = radio(name, cur.dest == d, g.as_ref());
        let o = opts.clone();
        c.connect_toggled(move |c| {
            if c.is_active() {
                o.set(Options { dest: d, ..o.get() });
            }
        });
        g.get_or_insert(c);
    }
    col.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    col.append(&w::label("Timer", "menu-heading"));
    let mut g: Option<gtk::CheckButton> = None;
    for d in DELAYS {
        let name = if d == 0 {
            "None".to_string()
        } else {
            format!("{d} Seconds")
        };
        let c = radio(&name, cur.delay == d, g.as_ref());
        let o = opts.clone();
        c.connect_toggled(move |c| {
            if c.is_active() {
                o.set(Options {
                    delay: d,
                    ..o.get()
                });
            }
        });
        g.get_or_insert(c);
    }
    p.set_child(Some(&col));
    p
}

/// Floating thumbnail of a saved capture: click opens it, drag drops the file.
pub fn thumbnail(texture: &gdk::Texture, path: &std::path::Path) -> gtk::Widget {
    let pic = gtk::Picture::for_paintable(texture);
    pic.set_content_fit(gtk::ContentFit::Cover);
    pic.set_can_shrink(true);
    pic.set_size_request(w::CAPTURE_THUMB.0, w::CAPTURE_THUMB.1);
    let frame = gtk::Overlay::new();
    let sizer = w::vbox(0);
    sizer.set_size_request(w::CAPTURE_THUMB.0, w::CAPTURE_THUMB.1);
    frame.set_child(Some(&sizer));
    frame.add_overlay(&pic);
    frame.set_overflow(gtk::Overflow::Hidden);
    frame.add_css_class("capture-thumb");
    frame.set_halign(gtk::Align::End);
    frame.set_valign(gtk::Align::End);
    let file = gio::File::for_path(path);
    let click = gtk::GestureClick::new();
    let f2 = file.clone();
    click.connect_released(move |_, _, _, _| {
        let _ = gio::AppInfo::launch_default_for_uri(&f2.uri(), None::<&gio::AppLaunchContext>);
    });
    frame.add_controller(click);
    let drag = gtk::DragSource::new();
    drag.set_actions(gdk::DragAction::COPY);
    drag.set_content(Some(&gdk::ContentProvider::for_value(
        &gdk::FileList::from_array(&[file]).to_value(),
    )));
    let t2 = texture.clone();
    drag.connect_drag_begin(move |d, _| d.set_icon(Some(&t2), 0, 0));
    frame.add_controller(drag);
    let p2 = path.to_path_buf();
    let pen = w::icon_button("document-edit-symbolic", "Markup", || {});
    pen.connect_clicked(move |b| {
        let win = b.root().and_downcast::<gtk::Window>();
        if let Some(app) = win.as_ref().and_then(|w| w.application()) {
            markup(&app, &p2);
        }
        if let Some(w) = win {
            w.set_visible(false);
        }
    });
    pen.set_halign(gtk::Align::End);
    pen.set_valign(gtk::Align::Start);
    frame.add_overlay(&pen);
    frame.upcast()
}

/// Focused window as "x,y wxh" for Window mode. Asks the compositor: call it off the GTK thread.
fn active_window() -> Option<String> {
    let c = crate::hypr::active_window()?;
    Some(geometry(&c))
}

/// `grim -g` geometry of a client.
fn geometry(c: &crate::hypr::Client) -> String {
    format!("{},{} {}x{}", c.at[0], c.at[1], c.size[0], c.size[1])
}

/// Shell command handler for [`RECORD`]: show/toggle opens the toolbar in Record Entire Screen mode,
/// or stops the recording that runs (`capture::record_ask`).
pub type Handler = Rc<dyn Fn(&crate::protocol::Command) -> bool>;

/// The toolbar surface (bottom centre) and its thumbnail surface (bottom right), plus the
/// [`RECORD`] handler (Control Center Record tile).
pub fn build(app: &gtk::Application) -> (gtk::Window, Handler) {
    let win = super::window(app, "panel", Spec::capture_bar());
    crate::kit::surface::survive_output_loss(&win, Spec::capture_bar());
    win.add_css_class("capture");
    let thumb_win = super::window(app, "panel", Spec::capture_thumb());
    crate::kit::surface::survive_output_loss(&thumb_win, Spec::capture_thumb());
    thumb_win.add_css_class("capture");
    let bar = Rc::new(Toolbar::new(Mode::Selection, Options::default()));
    win.set_child(Some(&bar.root));
    let record: Handler = {
        let (bar, win) = (bar.clone(), win.clone());
        Rc::new(move |c: &crate::protocol::Command| {
            if c.target != RECORD {
                return false;
            }
            let pid = capture::record_pid_file(&crate::kit::runtime_dir());
            let show = match capture::record_ask(pid.exists(), c.verb, win.is_visible()) {
                capture::RecordAsk::Stop => {
                    win.set_visible(false);
                    crate::kit::spawn(&capture::STOP_ARGV.map(String::from));
                    return true;
                }
                capture::RecordAsk::Toolbar(show) => show,
            };
            if show {
                bar.select(Mode::RecordScreen);
            }
            win.set_visible(show);
            true
        })
    };
    // Return captures; the button holds focus whenever the bar opens.
    let cap = bar.capture.clone();
    win.connect_show(move |_| {
        cap.grab_focus();
    });
    let w2 = win.clone();
    bar.close.connect_clicked(move |_| w2.set_visible(false));
    let w2 = win.clone();
    crate::kit::surface::on_escape(&win, move || w2.set_visible(false));
    let (mode, opts, w2) = (bar.mode.clone(), bar.options.clone(), win.clone());
    let shown = Rc::new(Cell::new(0u32));
    bar.capture.connect_clicked(move |_| {
        let (m, o) = (mode.get(), opts.get());
        let home = std::env::var("HOME").unwrap_or_default();
        // Hide first so the toolbar (and a previous thumbnail) is not in the shot.
        w2.set_visible(false);
        thumb_win.set_visible(false);
        let (tw, shown) = (thumb_win.clone(), shown.clone());
        glib::spawn_future_local(async move {
            glib::timeout_future(capture::SETTLE).await;
            // The compositor and the capture tool both off the GTK thread (R2A-21).
            let out = gio::spawn_blocking(move || {
                let geom = (m == Mode::Window).then(active_window).flatten();
                let argv = capture::argv(m, o, geom.as_deref(), &home);
                std::process::Command::new(&argv[0])
                    .args(&argv[1..])
                    .env("WOL_SCREENSHOT_QUIET", "1")
                    .output()
                    .ok()
            })
            .await;
            let Ok(Some(o)) = out else { return };
            let path = String::from_utf8_lossy(&o.stdout).trim().to_string();
            let p = std::path::PathBuf::from(&path);
            let Ok(t) = gdk::Texture::from_filename(&p) else {
                return;
            };
            tw.set_child(Some(&thumbnail(&t, &p)));
            tw.set_visible(true);
            // The script was quiet (the thumbnail is the banner): keep the notice and name the folder.
            crate::kit::spawn(&capture::saved_notice_argv(&p));
            let id = shown.get().wrapping_add(1);
            shown.set(id);
            // Give me more time (QA-ACC-6): up to two minutes, so it is not left on screen all day.
            let stay = crate::kit::patience::current().capped_secs(THUMB_SECS, 120);
            glib::timeout_add_seconds_local_once(stay, move || {
                if shown.get() == id {
                    tw.set_visible(false);
                }
            });
        });
    });
    (win.upcast(), record)
}

// ---- Markup (ADR 0031 W5): a pen over the capture, then Save or Copy ----

/// Picture as a cairo surface (ARGB32 premultiplied, the layout `Texture::download` writes).
fn surface_of(t: &gdk::Texture) -> Option<gtk::cairo::ImageSurface> {
    let (wd, ht) = (t.width(), t.height());
    let stride = gtk::cairo::Format::ARgb32
        .stride_for_width(u32::try_from(wd).ok()?)
        .ok()?;
    let (s, h) = (usize::try_from(stride).ok()?, usize::try_from(ht).ok()?);
    let mut data = vec![0u8; s.checked_mul(h)?];
    t.download(&mut data, s);
    gtk::cairo::ImageSurface::create_for_data(data, gtk::cairo::Format::ARgb32, wd, ht, stride).ok()
}

/// Draws the strokes (picture pixels) on `cr`, already scaled to the picture.
fn paint_strokes(cr: &gtk::cairo::Context, m: &capture::Markup) {
    cr.set_line_cap(gtk::cairo::LineCap::Round);
    cr.set_line_join(gtk::cairo::LineJoin::Round);
    for s in &m.strokes {
        let (r, g, b) = s.ink.rgb();
        cr.set_source_rgb(r, g, b);
        cr.set_line_width(s.width);
        let mut pts = s.points.iter();
        if let Some(p) = pts.next() {
            cr.move_to(p.0, p.1);
            // A dot (one point) still shows.
            cr.line_to(p.0 + 0.01, p.1);
        }
        for p in pts {
            cr.line_to(p.0, p.1);
        }
        let _ = cr.stroke();
    }
}

/// The picture with its strokes at full size, as a texture (for Save and Copy).
pub fn flatten(img: &gtk::cairo::ImageSurface, m: &capture::Markup) -> Option<gdk::Texture> {
    let (wd, ht) = (img.width(), img.height());
    let mut out = gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, wd, ht).ok()?;
    {
        let cr = gtk::cairo::Context::new(&out).ok()?;
        cr.set_source_surface(img, 0.0, 0.0).ok()?;
        cr.paint().ok()?;
        paint_strokes(&cr, m);
    }
    out.flush();
    let stride = usize::try_from(out.stride()).ok()?;
    let bytes = glib::Bytes::from(&*out.data().ok()?);
    Some(
        gdk::MemoryTexture::new(
            wd,
            ht,
            gdk::MemoryFormat::B8g8r8a8Premultiplied,
            &bytes,
            stride,
        )
        .upcast(),
    )
}

/// The Markup view: pens, Undo, Copy and Save over the picture. `done` gets the flattened
/// picture and whether it was saved (true) or copied (false).
pub struct MarkupView {
    pub root: gtk::Box,
    pub area: gtk::DrawingArea,
    pub markup: Rc<std::cell::RefCell<capture::Markup>>,
    pub save: gtk::Button,
    pub copy: gtk::Button,
    pub undo: gtk::Button,
    image: gtk::cairo::ImageSurface,
}

impl MarkupView {
    pub fn new(texture: &gdk::Texture) -> Option<Self> {
        let image = surface_of(texture)?;
        let markup = Rc::new(std::cell::RefCell::new(capture::Markup::new(
            f64::from(image.width()),
            f64::from(image.height()),
        )));
        let root = w::vbox(w::S2);
        let bar = w::hbox(w::S1);
        let ink = Rc::new(Cell::new(capture::Ink::Red));
        let mut first: Option<gtk::ToggleButton> = None;
        for i in capture::Ink::ALL {
            let (r, g, b) = i.rgb();
            let swatch = gtk::DrawingArea::builder()
                .content_width(w::ICON_TOOL)
                .content_height(w::ICON_TOOL)
                .build();
            swatch.set_draw_func(move |_, cr, wd, ht| {
                let d = f64::from(wd.min(ht));
                cr.set_source_rgb(r, g, b);
                cr.arc(
                    f64::from(wd) / 2.0,
                    f64::from(ht) / 2.0,
                    d / 2.0,
                    0.0,
                    std::f64::consts::TAU,
                );
                let _ = cr.fill();
            });
            let t = gtk::ToggleButton::builder()
                .child(&swatch)
                .tooltip_text(i.label())
                .active(i == capture::Ink::Red)
                .build();
            t.add_css_class("capture-mode");
            if let Some(f) = &first {
                t.set_group(Some(f));
            } else {
                first = Some(t.clone());
            }
            let ink = ink.clone();
            t.connect_toggled(move |t| {
                if t.is_active() {
                    ink.set(i);
                }
            });
            bar.append(&t);
        }
        bar.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        let undo = w::icon_button("edit-undo-symbolic", "Undo", || {});
        bar.append(&undo);
        bar.append(&w::spacer());
        let copy = w::button("Copy", w::Kind::Default, || {});
        let save = w::button("Save", w::Kind::Primary, || {});
        bar.append(&copy);
        bar.append(&save);
        let area = gtk::DrawingArea::builder()
            .hexpand(true)
            .vexpand(true)
            .content_width(640)
            .content_height(400)
            .build();
        {
            let (img, m) = (image.clone(), markup.clone());
            area.set_draw_func(move |_, cr, wd, ht| {
                let m = m.borrow();
                let (s, x, y) = m.fit((f64::from(wd), f64::from(ht)));
                cr.translate(x, y);
                cr.scale(s, s);
                if cr.set_source_surface(&img, 0.0, 0.0).is_ok() {
                    cr.source().set_filter(gtk::cairo::Filter::Good);
                    let _ = cr.paint();
                }
                paint_strokes(cr, &m);
            });
        }
        let drag = gtk::GestureDrag::new();
        {
            let (m, a, ink) = (markup.clone(), area.clone(), ink.clone());
            drag.connect_drag_begin(move |_, x, y| {
                let size = (f64::from(a.width()), f64::from(a.height()));
                let mut mk = m.borrow_mut();
                let p = mk.to_picture(size, (x, y));
                if mk.begin(ink.get(), p) {
                    a.queue_draw();
                }
            });
        }
        {
            let (m, a) = (markup.clone(), area.clone());
            drag.connect_drag_update(move |g, dx, dy| {
                let Some((x, y)) = g.start_point() else {
                    return;
                };
                let size = (f64::from(a.width()), f64::from(a.height()));
                let mut mk = m.borrow_mut();
                let p = mk.to_picture(size, (x + dx, y + dy));
                if mk.extend(p) {
                    a.queue_draw();
                }
            });
        }
        area.add_controller(drag);
        {
            let (m, a) = (markup.clone(), area.clone());
            undo.connect_clicked(move |_| {
                if m.borrow_mut().undo() {
                    a.queue_draw();
                }
            });
        }
        root.append(&bar);
        root.append(&area);
        Some(MarkupView {
            root,
            area,
            markup,
            save,
            copy,
            undo,
            image,
        })
    }

    /// The picture with the strokes, full size.
    pub fn flattened(&self) -> Option<gdk::Texture> {
        flatten(&self.image, &self.markup.borrow())
    }
}

/// Opens Markup for the capture at `path`: Save writes "<name> (Markup).png" next to it and
/// closes; Copy puts the marked-up picture on the clipboard and closes.
pub fn markup(app: &gtk::Application, path: &std::path::Path) {
    let Ok(texture) = gdk::Texture::from_filename(path) else {
        return;
    };
    let Some(view) = MarkupView::new(&texture) else {
        return;
    };
    let win = gtk::Window::builder()
        .application(app)
        .title("Markup")
        .default_width(960)
        .default_height(640)
        .child(&view.root)
        .build();
    let view = Rc::new(view);
    let out = capture::markup_path(path);
    {
        let (v, win) = (view.clone(), win.clone());
        view.save.connect_clicked(move |_| {
            let Some(t) = v.flattened() else { return };
            let (out, win) = (out.clone(), win.clone());
            // The PNG is encoded here (one click, then the window closes); the disk write goes
            // off the GTK thread.
            let bytes = t.save_to_png_bytes();
            glib::spawn_future_local(async move {
                let r = gio::spawn_blocking(move || std::fs::write(&out, &bytes)).await;
                if let Ok(Err(e)) = r {
                    eprintln!("wol-shell: markup not saved: {e}");
                }
                win.close();
            });
        });
    }
    {
        let (v, win) = (view.clone(), win.clone());
        view.copy.connect_clicked(move |b| {
            if let Some(t) = v.flattened() {
                b.display().clipboard().set_texture(&t);
            }
            win.close();
        });
    }
    let w2 = win.clone();
    crate::kit::surface::on_escape(&win, move || w2.close());
    win.present();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_mode_captures_the_client_box() {
        let mut c = crate::hypr::Client::new("0x1", "foot", "~");
        c.at = [10, 40];
        c.size = [800, 600];
        assert_eq!(geometry(&c), "10,40 800x600");
    }
}
