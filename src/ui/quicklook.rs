// SPDX-License-Identifier: GPL-2.0-or-later
//! wol-quicklook (docs/modules/quicklook.md): the window Files shows for Space on a file. No toolbar;
//! the compositor's strip carries the file name. Space or Esc closes, the arrow keys ask Files for the
//! next file. [`content`] draws one preview from what [`crate::quicklook`] read (goldens use it).
use crate::copy::quicklook as copy;
use crate::kit::{self, surface, widgets as w};
use crate::quicklook::{self as ql, Action, Bus, Event, Facts, FolderInfo, Kind, Loaded, Shown};
use gtk::{gdk, gio, glib, prelude::*};
use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;

/// What the window shows.
pub enum Body {
    Picture(gdk::Texture),
    /// First page of a PDF.
    Page(gdk::Texture),
    Text {
        text: String,
        cut: bool,
    },
    Audio(gtk::MediaStream),
    Video(gtk::MediaStream),
    Folder(FolderInfo),
    /// Icon, name, facts and one sentence.
    Card(&'static str),
}

pub struct View {
    pub facts: Facts,
    pub body: Body,
}

fn icon_of(kind: Kind) -> &'static str {
    match kind {
        Kind::Image => "image-x-generic-symbolic",
        Kind::Audio => "audio-x-generic-symbolic",
        Kind::Video => "video-x-generic-symbolic",
        Kind::Pdf => "x-office-document-symbolic",
        Kind::Folder => "folder-symbolic",
        Kind::Text | Kind::Other => "text-x-generic-symbolic",
    }
}

fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Image => copy::KIND_PICTURE,
        Kind::Text => copy::KIND_TEXT,
        Kind::Audio => copy::KIND_AUDIO,
        Kind::Video => copy::KIND_VIDEO,
        Kind::Pdf => copy::KIND_PDF,
        Kind::Folder => copy::KIND_FOLDER,
        Kind::Other => copy::KIND_OTHER,
    }
}

/// "Picture · 4.2 MB · Modified 3 March 2026".
fn facts_line(f: &Facts) -> String {
    let mut parts = vec![kind_label(f.kind).to_owned()];
    parts.extend(f.size.clone());
    parts.extend(
        f.modified
            .as_ref()
            .map(|m| format!("{} {m}", copy::MODIFIED)),
    );
    parts.join(" · ")
}

/// The centred card: big icon, name, facts line, then the extra rows.
fn card(f: &Facts, extra: &[gtk::Widget]) -> gtk::Box {
    let b = w::vbox(w::S2);
    b.add_css_class("empty-state");
    b.set_valign(gtk::Align::Center);
    b.set_halign(gtk::Align::Center);
    b.set_vexpand(true);
    b.set_hexpand(true);
    b.set_margin_start(w::S6);
    b.set_margin_end(w::S6);
    let icon = w::icon(icon_of(f.kind), w::ICON_XL);
    icon.add_css_class("dim-icon");
    b.append(&icon);
    let name = w::centered(&f.name, "heading");
    name.set_wrap(true);
    name.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    name.set_lines(2);
    name.set_max_width_chars(w::TILE_CHARS * 3);
    b.append(&name);
    let line = w::centered(&facts_line(f), "secondary");
    line.set_wrap(true);
    b.append(&line);
    for x in extra {
        b.append(x);
    }
    b
}

fn note(text: &str) -> gtk::Widget {
    let l = w::centered(text, "secondary");
    l.set_wrap(true);
    l.set_max_width_chars(w::TILE_CHARS * 3);
    l.upcast()
}

/// A picture or page in a rounded frame (the kit's art tile) with room around it.
fn picture(t: &gdk::Texture, name: &str) -> gtk::Box {
    let p = gtk::Picture::for_paintable(t);
    p.set_content_fit(gtk::ContentFit::Contain);
    p.set_can_shrink(true);
    p.set_hexpand(true);
    p.set_vexpand(true);
    p.update_property(&[gtk::accessible::Property::Label(name)]);
    let frame = w::hbox(0);
    frame.add_css_class("art");
    frame.set_overflow(gtk::Overflow::Hidden);
    frame.set_hexpand(true);
    frame.set_vexpand(true);
    frame.set_margin_top(w::S4);
    frame.set_margin_bottom(w::S4);
    frame.set_margin_start(w::S4);
    frame.set_margin_end(w::S4);
    frame.append(&p);
    frame
}

/// Play or pause, a position slider and the time, for audio. (GTK's own media controls carry a
/// volume button with no tooltip, which the kit does not allow.)
fn player(stream: &gtk::MediaStream) -> gtk::Box {
    let bar = w::hbox(w::S2);
    bar.set_width_request(w::CONTROL_CENTER_WIDTH);
    let play = w::icon_button("media-playback-start-symbolic", copy::PLAY, || {});
    let slider = w::row_slider(0.0, |_| {});
    let time = w::label("0:00 / 0:00", "secondary");
    bar.append(&play);
    bar.append(&slider);
    bar.append(&time);
    // Follows the stream; `by_stream` tells its own moves of the slider from the person's.
    let by_stream = Rc::new(Cell::new(false));
    let refresh = {
        let (s, slider, time, by_stream) = (
            stream.clone(),
            slider.clone(),
            time.clone(),
            by_stream.clone(),
        );
        move || {
            let (at, len) = (s.timestamp(), s.duration());
            time.set_label(&format!("{} / {}", ql::clock(at), ql::clock(len)));
            slider.set_sensitive(s.is_seekable() && len > 0);
            by_stream.set(true);
            slider.set_value(if len > 0 {
                at as f64 * 100.0 / len as f64
            } else {
                0.0
            });
            by_stream.set(false);
        }
    };
    refresh();
    {
        let r = refresh.clone();
        stream.connect_timestamp_notify(move |_| r());
        let r = refresh.clone();
        stream.connect_duration_notify(move |_| r());
    }
    {
        let (s, by_stream) = (stream.clone(), by_stream.clone());
        slider.connect_value_changed(move |sl| {
            if !by_stream.get() && s.duration() > 0 {
                s.seek((sl.value() / 100.0 * s.duration() as f64) as i64);
            }
        });
    }
    {
        let s = stream.clone();
        play.connect_clicked(move |_| s.set_playing(!s.is_playing()));
        let p = play.clone();
        let face = move |s: &gtk::MediaStream| {
            let playing = s.is_playing();
            p.set_child(Some(&w::icon(
                if playing {
                    "media-playback-pause-symbolic"
                } else {
                    "media-playback-start-symbolic"
                },
                w::ICON,
            )));
            p.set_tooltip_text(Some(if playing { copy::PAUSE } else { copy::PLAY }));
        };
        face(stream);
        stream.connect_playing_notify(face);
    }
    bar
}

/// One preview, filling its parent. No toolbar, no sidebar.
pub fn content(v: &View) -> gtk::Widget {
    let root = w::vbox(0);
    root.add_css_class("app-window");
    root.set_hexpand(true);
    root.set_vexpand(true);
    match &v.body {
        Body::Picture(t) | Body::Page(t) => root.append(&picture(t, &v.facts.name)),
        Body::Text { text, cut } => {
            let tv = gtk::TextView::builder()
                .editable(false)
                .cursor_visible(false)
                .monospace(true)
                .wrap_mode(gtk::WrapMode::WordChar)
                .focusable(false)
                .top_margin(w::S3)
                .bottom_margin(w::S3)
                .left_margin(w::S4)
                .right_margin(w::S4)
                .build();
            tv.buffer().set_text(text);
            tv.update_property(&[gtk::accessible::Property::Label(&v.facts.name)]);
            let sc = gtk::ScrolledWindow::builder()
                .child(&tv)
                .hexpand(true)
                .vexpand(true)
                .focusable(false)
                .build();
            root.append(&sc);
            if *cut {
                let n = w::footnote(copy::TEXT_CUT);
                n.set_margin_start(w::S4);
                n.set_margin_end(w::S4);
                n.set_margin_bottom(w::S2);
                root.append(&n);
            }
        }
        Body::Audio(stream) => {
            let controls = player(stream);
            controls.set_margin_top(w::S3);
            root.append(&card(&v.facts, &[controls.upcast()]));
        }
        Body::Video(stream) => {
            let video = gtk::Video::for_media_stream(Some(stream));
            video.set_hexpand(true);
            video.set_vexpand(true);
            root.append(&video);
        }
        Body::Folder(info) => {
            let mut rows: Vec<gtk::Widget> = Vec::new();
            let mut f = v.facts.clone();
            if info.items() == 0 {
                rows.push(note(copy::EMPTY_FOLDER));
            } else {
                rows.push(w::centered(&copy::items(info.items()), "title").upcast());
                rows.push(note(&copy::folder_parts(info.folders, info.files)));
                if info.files > 0 {
                    rows.push(note(&copy::folder_size(&wol_core::storage::human(
                        info.bytes,
                    ))));
                }
                if info.partial {
                    rows.push(note(copy::FOLDER_PARTIAL));
                }
            }
            f.size = None;
            root.append(&card(&f, &rows));
        }
        Body::Card(sentence) => root.append(&card(&v.facts, &[note(sentence)])),
    }
    root.upcast()
}

/// The window size the content asks for before [`ql::fit`] limits it.
pub fn natural(b: &Body) -> (i32, i32) {
    match b {
        Body::Picture(t) | Body::Page(t) => (t.width(), t.height()),
        Body::Text { .. } => w::APP_SIZE,
        Body::Video(_) => (w::APP_SIZE.0, w::APP_SIZE.0 * 9 / 16),
        Body::Audio(_) | Body::Folder(_) | Body::Card(_) => w::APP_NARROW,
    }
}

/// The widgets for what was read; the pieces that need the GTK thread are made here.
pub fn view_of(facts: Facts, loaded: Loaded) -> View {
    let card = |s| View {
        facts: facts.clone(),
        body: Body::Card(s),
    };
    let body = match loaded {
        Loaded::Picture(r) => {
            let format = if r.alpha {
                gdk::MemoryFormat::R8g8b8a8
            } else {
                gdk::MemoryFormat::R8g8b8
            };
            Body::Picture(
                gdk::MemoryTexture::new(r.width, r.height, format, &r.bytes, r.stride).upcast(),
            )
        }
        Loaded::Page(png) => match gdk::Texture::from_bytes(&glib::Bytes::from_owned(png)) {
            Ok(t) => Body::Page(t),
            Err(_) => return card(copy::PDF_NOT_SHOWN),
        },
        Loaded::Text { text, cut } => Body::Text { text, cut },
        Loaded::Media { video, path } => {
            let s: gtk::MediaStream = gtk::MediaFile::for_filename(path).upcast();
            if video {
                Body::Video(s)
            } else {
                Body::Audio(s)
            }
        }
        Loaded::Folder(i) => Body::Folder(i),
        Loaded::Card(s) => Body::Card(s),
    };
    View { facts, body }
}

/// Arrow keys to the `SelectionEvent` direction.
fn direction(key: gdk::Key) -> Option<u32> {
    match key {
        gdk::Key::Up | gdk::Key::KP_Up => Some(ql::DIR_UP),
        gdk::Key::Down | gdk::Key::KP_Down => Some(ql::DIR_DOWN),
        gdk::Key::Left | gdk::Key::KP_Left => Some(ql::DIR_LEFT),
        gdk::Key::Right | gdk::Key::KP_Right => Some(ql::DIR_RIGHT),
        _ => None,
    }
}

/// The running previewer.
struct Q {
    app: gtk::Application,
    win: gtk::ApplicationWindow,
    shown: RefCell<Shown>,
    bus: OnceCell<Bus>,
    /// One load at a time; a hide or a newer ShowFile drops the one in flight (its result is not shown).
    loads: RefCell<ql::Loads>,
    /// Bumped on every request from Files and on every hide: an older idle timer finds it changed.
    idle: Cell<u64>,
    uri: RefCell<Option<String>>,
    kind: Cell<Option<Kind>>,
    media: RefCell<Option<gtk::MediaStream>>,
    /// Standalone (`wol-quicklook FILE`): no bus name, quits when closed.
    standalone: bool,
}

/// Runs `f` with the bus connection when it is up.
fn tell<F: std::future::Future<Output = ()> + 'static>(q: &Rc<Q>, f: impl FnOnce(Bus) -> F) {
    if let Some(b) = q.bus.get().cloned() {
        glib::spawn_future_local(f(b));
    }
}

fn visible(q: &Rc<Q>, on: bool) {
    tell(q, move |b| async move {
        if let Err(e) = b.set_visible(on).await {
            crate::diag::say!("wol-quicklook: Visible: {e}");
        }
    });
}

fn show(q: &Rc<Q>, uri: &str) {
    if q.win.is_visible() && q.uri.borrow().as_deref() == Some(uri) {
        q.win.present();
        return;
    }
    let start = q.loads.borrow_mut().request(uri);
    if let Some((token, uri)) = start {
        load(q.clone(), token, uri);
    }
}

/// Reads `uri` off the GTK thread, then shows it if nothing newer was asked for or closed meanwhile.
fn load(q: Rc<Q>, token: u64, uri: String) {
    glib::spawn_future_local(async move {
        let (facts, loaded) = ql::load_async(uri.clone()).await;
        let (wanted, next) = q.loads.borrow_mut().finished(token);
        if let Some((t, u)) = next {
            load(q.clone(), t, u);
        }
        if !wanted {
            return;
        }
        let v = view_of(facts, loaded);
        q.win.set_title(Some(&v.facts.name));
        q.win.set_child(Some(&content(&v)));
        let screen = surface::screen_of(&q.win).unwrap_or(ql::FALLBACK_SCREEN);
        let (width, height) = ql::fit(natural(&v.body), screen, w::APP_NARROW);
        q.win.set_default_size(width, height);
        let stream = match &v.body {
            Body::Audio(s) | Body::Video(s) => Some(s.clone()),
            _ => None,
        };
        if let Some(s) = &stream {
            let (qq, f) = (q.clone(), v.facts.clone());
            s.connect_error_notify(move |s| {
                if s.error().is_some() {
                    let sorry = View {
                        facts: f.clone(),
                        body: Body::Card(copy::NOT_PLAYED),
                    };
                    qq.win.set_child(Some(&content(&sorry)));
                }
            });
            s.set_playing(true);
        }
        *q.media.borrow_mut() = stream;
        *q.uri.borrow_mut() = Some(uri);
        q.kind.set(Some(v.facts.kind));
        q.win.present();
        visible(&q, true);
    });
}

/// Hidden, either way (Space, Esc, the close button, Files asking): stop what plays, let go of the
/// content, tell Files, and in service mode quit after a long idle.
fn hidden(q: &Rc<Q>) {
    q.loads.borrow_mut().cancel();
    q.idle.set(q.idle.get() + 1);
    if let Some(m) = q.media.take() {
        m.set_playing(false);
    }
    q.win.set_child(None::<&gtk::Widget>);
    *q.uri.borrow_mut() = None;
    q.shown.borrow_mut().closed();
    visible(q, false);
    if q.standalone {
        q.app.quit();
        return;
    }
    let (at, qq) = (q.idle.get(), q.clone());
    glib::timeout_add_seconds_local_once(ql::IDLE_EXIT_SECS, move || {
        if qq.idle.get() == at && !qq.win.is_visible() {
            qq.app.quit();
        }
    });
}

fn build(app: &gtk::Application, standalone: bool) -> Rc<Q> {
    kit::style::install("");
    let win = gtk::ApplicationWindow::builder()
        .application(app)
        .title(copy::TITLE)
        .default_width(w::APP_NARROW.0)
        .default_height(w::APP_NARROW.1)
        .decorated(false)
        .hide_on_close(true)
        .build();
    let q = Rc::new(Q {
        app: app.clone(),
        win: win.clone(),
        shown: RefCell::default(),
        bus: OnceCell::new(),
        loads: RefCell::default(),
        idle: Cell::new(0),
        uri: RefCell::default(),
        kind: Cell::new(None),
        media: RefCell::default(),
        standalone,
    });
    {
        let qq = q.clone();
        win.connect_hide(move |_| hidden(&qq));
    }
    {
        let w2 = win.clone();
        surface::on_escape(&win, move || w2.set_visible(false));
    }
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let qq = q.clone();
        keys.connect_key_pressed(move |_, key, _, mods| {
            if mods.intersects(
                gdk::ModifierType::CONTROL_MASK
                    | gdk::ModifierType::ALT_MASK
                    | gdk::ModifierType::SUPER_MASK,
            ) {
                return glib::Propagation::Proceed;
            }
            if key == gdk::Key::space {
                qq.win.set_visible(false);
                return glib::Propagation::Stop;
            }
            if let Some(d) = direction(key) {
                tell(&qq, move |b| async move {
                    if let Err(e) = b.selection(d).await {
                        crate::diag::say!("wol-quicklook: SelectionEvent: {e}");
                    }
                });
                return glib::Propagation::Stop;
            }
            if matches!(key, gdk::Key::Return | gdk::Key::KP_Enter)
                && qq.kind.get() == Some(Kind::Folder)
            {
                let uri = qq.uri.borrow().clone();
                if let Some(uri) = uri {
                    tell(&qq, move |b| async move {
                        if let Err(e) = b.navigate(&uri).await {
                            crate::diag::say!("wol-quicklook: NavigateTo: {e}");
                        }
                    });
                    qq.win.set_visible(false);
                    return glib::Propagation::Stop;
                }
            }
            glib::Propagation::Proceed
        });
    }
    win.add_controller(keys);
    kit::snapshot_on_map(&win, "quicklook");
    q
}

/// `wol-quicklook` serves the previewer name for Files; `wol-quicklook FILE` shows one file and quits.
pub fn run() -> glib::ExitCode {
    let arg = std::env::args().nth(1);
    let standalone = arg.is_some();
    let flags = if standalone {
        gio::ApplicationFlags::NON_UNIQUE
    } else {
        gio::ApplicationFlags::empty()
    };
    let app = gtk::Application::builder()
        .application_id("org.wolos.quicklook")
        .flags(flags)
        .build();
    let started = Cell::new(false);
    app.connect_activate(move |app| {
        if started.replace(true) {
            return;
        }
        let q = build(app, standalone);
        match &arg {
            Some(a) => {
                let uri = gio::File::for_commandline_arg(a).uri().to_string();
                q.shown.borrow_mut().on(&Event::Show {
                    uri: uri.clone(),
                    handle: String::new(),
                    close_if_shown: false,
                });
                show(&q, &uri);
            }
            None => {
                // The window shows only when Files asks; the previewer lives until then.
                std::mem::forget(app.hold());
                let (tx, rx) = async_channel::unbounded();
                let (qs, app2) = (q.clone(), app.clone());
                glib::spawn_future_local(async move {
                    match Bus::serve(None, tx).await {
                        Ok(b) => {
                            let _ = qs.bus.set(b);
                        }
                        Err(e) => {
                            crate::diag::say!("wol-quicklook: {}: {e}", ql::NAME);
                            app2.quit();
                        }
                    }
                });
                glib::spawn_future_local(async move {
                    while let Ok(ev) = rx.recv().await {
                        // Anything from Files means the previewer is wanted: the idle quit starts over.
                        q.idle.set(q.idle.get() + 1);
                        let act = q.shown.borrow_mut().on(&ev);
                        match act {
                            Action::Present(uri) => show(&q, &uri),
                            // Not only the window: a load still running is dropped too, even when
                            // the window is not up yet (set_visible(false) alone would do nothing).
                            Action::Hide => {
                                hidden(&q);
                                q.win.set_visible(false);
                            }
                            Action::Nothing => {}
                        }
                    }
                });
            }
        }
    });
    app.run_with_args::<&str>(&[])
}
