// SPDX-License-Identifier: GPL-2.0-or-later
//! Control Center (WolOS layout): a connectivity card (Wi-Fi, Bluetooth) next to Focus
//! and Dark Mode, the Display and Sound sliders, Now Playing, then a grid of square
//! tiles (Battery, Screen Mirroring, Keyboard Brightness, Night Shift, Screenshot, Record, accessibility, and the
//! tiles of [`tiles::TileSource`], ADR 0031). Every control goes through `System` (core backend
//! via `Host`; root actions through wol-admind, which may open the Authenticate sheet, IaraOS
//! ADR 0026). Built once as a resident wol-shell panel; refreshes only while visible; ends above
//! the Dock and narrows on a narrow display. Optional modules follow Settings > Control Center
//! (`ShellPrefs.control_center`). UI pieces are Kit components only.
mod display;
mod pages;
mod sound;
pub mod tiles;

use crate::focus::{self, Focus};
use crate::kit::{
    self,
    controls::{
        BatteryTile, NowPlayingCard, SliderModule, ToggleTile, cap_scroller, fit_scroller,
        page_stack, show_page_now,
    },
    surface::{Keyboard, Spec},
    widgets as w,
};
use crate::system::{Action, ActionError, PowerMode, System};
use crate::ui::settings::SoundState;
use crate::{copy, theme};
use gtk::{glib, prelude::*};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use wol_core::prefs::ControlCenterPrefs;

/// Every argv the Control Center spawns (tests/cc_spawns.rs checks each resolves).
pub const SPAWNS: &[&[&str]] = &[
    SCREENSHOT,
    RECORD,
    WIFI_SETTINGS,
    SOUND_SETTINGS,
    DISPLAY_SETTINGS,
    STAY_AWAKE,
];
/// Stay Awake tile (`wol_core::toggles`): the CLI flips it; `tiles::toggle_argv` builds it.
pub const STAY_AWAKE: &[&str] = &["wol", "toggle", "stay-awake"];
/// Screenshot tile: the capture toolbar, like Super+Shift+5.
const SCREENSHOT: &[&str] = &["wol", "shell", "show", "screenshot"];
/// Record tile: the capture toolbar in Record Entire Screen mode.
const RECORD: &[&str] = &["wol", "shell", "show", crate::ui::capture::RECORD];
/// "Wi-Fi Settings…" on the Wi-Fi page: the Wi-Fi pane (live it opened Network, the wired one).
pub const WIFI_SETTINGS: &[&str] = &["wol-settings", "Wi-Fi"];
/// "Sound Settings…" on the Sound page.
pub const SOUND_SETTINGS: &[&str] = &["wol-settings", "Sound"];
/// "Displays Settings…" on the Display page.
pub const DISPLAY_SETTINGS: &[&str] = &["wol-settings", "Displays"];

/// Stack page names.
pub const PAGES: [&str; 6] = ["main", "wifi", "bluetooth", "sound", "focus", "display"];

thread_local! {
    /// The page the next show opens (set by [`request_page`], taken once by the show handler).
    static NEXT_PAGE: Cell<Option<&'static str>> = const { Cell::new(None) };
    /// Extra tiles (ADR 0031 `[[tile]]`), set once by the plugin host ([`set_tile_source`]).
    static TILE_SOURCE: RefCell<Option<Rc<dyn tiles::TileSource>>> = const { RefCell::new(None) };
}

/// The tiles `source` lists join the Control Center's tile grid, after the built-in ones, on the
/// next refresh (the plugin host calls it once at start and again when plugins change); None
/// removes them.
pub fn set_tile_source(source: Option<Rc<dyn tiles::TileSource>>) {
    TILE_SOURCE.with_borrow_mut(|s| *s = source);
}

fn tile_source() -> Option<Rc<dyn tiles::TileSource>> {
    TILE_SOURCE.with_borrow(|s| s.clone())
}

/// The next time the panel shows, open `page` (one of [`PAGES`]) instead of the main page (audit2
/// A2-14: the menu bar's Wi-Fi, Sound and Focus icons). An unknown name is ignored: main opens.
pub fn request_page(page: &str) {
    NEXT_PAGE.set(PAGES.into_iter().find(|p| *p == page));
}

/// The page a show opens: the requested one once, else main.
fn take_page() -> &'static str {
    NEXT_PAGE.take().unwrap_or("main")
}

/// A request made for a show that never came (output lost) does not open a later one on its
/// page: dropped on every hide (audit3 R2A-2).
fn forget_page() {
    NEXT_PAGE.set(None);
}

/// The panel's page stack (the one with every [`PAGES`] child) inside `win`.
fn pages_of(win: &gtk::Window) -> Option<gtk::Stack> {
    let mut todo = vec![win.child()?];
    while let Some(w) = todo.pop() {
        if let Some(s) = w.downcast_ref::<gtk::Stack>()
            && PAGES.iter().all(|p| s.child_by_name(p).is_some())
        {
            return Some(s.clone());
        }
        let mut c = w.first_child();
        while let Some(x) = c {
            c = x.next_sibling();
            todo.push(x);
        }
    }
    None
}

/// The page the open panel shows (None before it is built).
pub fn current_page(win: &gtk::Window) -> Option<String> {
    pages_of(win)?.visible_child_name().map(|n| n.to_string())
}

/// The open panel slides to `page` (a status icon clicked while it is open, audit3 R2A-2).
pub fn switch_page(win: &gtk::Window, page: &str) {
    if let (Some(s), Some(p)) = (pages_of(win), PAGES.into_iter().find(|p| *p == page)) {
        s.set_visible_child_name(p);
    }
}

fn spec() -> Spec {
    Spec::panel(
        "wol-panel-controlcenter",
        [true, false, false, true],
        Keyboard::OnDemand,
    )
}

/// Resident panel in wol-shell (`wol shell toggle control-center`): opening is one frame, no
/// process start. Built once (live perf: rebuilding per open leaked ~0.5 MB each); the snapshot
/// watcher (1 s refresh) only lives while visible, and every open starts on the main page.
pub fn panel(app: &gtk::Application, sys: Rc<dyn System>) -> gtk::Window {
    let win = super::window(app, "controlcenter", spec());
    win.set_hide_on_close(true);
    let cc = build(sys.clone(), ControlCenterPrefs::default());
    win.set_child(Some(&cc.root));
    {
        let cc = Rc::downgrade(&cc);
        super::ipc::on_reload(move |p| {
            if let Some(cc) = cc.upgrade() {
                cc.ui.prefs.set(p.control_center);
                cc.sync();
            }
        });
    }
    {
        // Plugin [[tile]]s (ADR 0031): read off the GTK thread, again when the plugin folders
        // change and on every show (a toggle may have moved from the command line).
        let tiles = crate::ui::plugins::TileCache::new(crate::ui::plugins::Paths::user());
        let weak = Rc::downgrade(&cc);
        tiles.on_changed(Rc::new(move || {
            if let Some(cc) = weak.upgrade() {
                cc.sync();
            }
        }));
        tiles.watch(&kit::lifetime::Lifetime::of_window(&win));
        let t = tiles.clone();
        win.connect_show(move |_| t.refresh());
        set_tile_source(Some(tiles));
    }
    let running: Rc<RefCell<Option<glib::JoinHandle<()>>>> = Default::default();
    // The sound service's state, looked at twice a second while the panel is open.
    let polling: Rc<RefCell<Option<glib::SourceId>>> = Default::default();
    {
        let (running, polling, cc) = (running.clone(), polling.clone(), cc.clone());
        win.connect_show(move |win| {
            if !cc.sound_page.has_source() {
                cc.sound_page.set_source(sound::live());
            }
            show_page_now(&cc.ui.pages, take_page());
            cc.sync();
            fit(&cc, win);
            *running.borrow_mut() = follow(&sys, cc.clone());
            let weak = Rc::downgrade(&cc);
            let id = glib::timeout_add_local(SOUND_POLL, move || {
                let Some(cc) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                if cc.sound_page.poll() {
                    cc.sync();
                }
                glib::ControlFlow::Continue
            });
            if let Some(old) = polling.borrow_mut().replace(id) {
                old.remove();
            }
        });
    }
    {
        // On show the surface may not exist yet (then `screen_height` falls back to the first
        // display); mapped, the panel knows the display it is on.
        let cc = cc.clone();
        win.connect_map(move |win| fit(&cc, win));
    }
    win.connect_hide(move |_| {
        if let Some(h) = running.borrow_mut().take() {
            h.abort();
        }
        if let Some(id) = polling.borrow_mut().take() {
            id.remove();
        }
        forget_page();
    });
    let w2 = win.clone();
    kit::surface::on_escape(&win, move || w2.set_visible(false));
    win.upcast()
}

/// How often the open panel looks for a new sound service state.
const SOUND_POLL: std::time::Duration = std::time::Duration::from_millis(500);

/// Screen height the panel may not use: menu bar, glass margins and padding.
const CHROME: i32 = w::MENUBAR_HEIGHT + 2 * 24 + 2 * w::INSET + 2 * 8;

/// Tallest the main page's scroller may be on a `screen` px tall display whose bottom `dock` px
/// are reserved (the Dock's shelf): the panel ends above the Dock (audit2 A2-11, it drew over the
/// shelf on 768 px screens with every module on).
fn cap(screen: i32, dock: i32) -> i32 {
    screen - CHROME - dock.max(0)
}

/// Narrowest the panel gets (a 360 px wide display, scaled phones and portrait panels).
pub const NARROW_WIDTH: i32 = 288;

/// Panel width on a `screen` px wide display: the standard width, less where the display is too
/// narrow for it and its glass margins, never under [`NARROW_WIDTH`].
fn width_for(screen: Option<i32>) -> i32 {
    match screen {
        Some(s) if s > 0 => {
            (s - 2 * 24 - 2 * w::INSET).clamp(NARROW_WIDTH, w::CONTROL_CENTER_WIDTH)
        }
        _ => w::CONTROL_CENTER_WIDTH,
    }
}

/// Fits the panel to its display: its width (narrow layout on a narrow display), and the main
/// page scrolls only when its modules are taller than the screen allows. Only a Dock on the
/// panel's own display counts (audit3 R2A-4: a second display without one scrolled early).
fn fit(cc: &ControlCenter, win: &impl IsA<gtk::Widget>) {
    let screen = kit::surface::screen_of(win);
    let width = width_for(screen.map(|s| s.0));
    if cc.ui.width.get() != width {
        cc.ui.width.set(width);
        cc.root.set_width_request(width);
        cc.sync();
    }
    let dock = kit::surface::reserved_bottom_on(win);
    cap_scroller(&cc.ui.scroll, width, screen.map(|s| cap(s.1, dock)));
}

/// Control Center content on its own (golden renders): follows the snapshot while the widget lives.
pub fn content(sys: Rc<dyn System>) -> gtk::Widget {
    content_with(sys, super::ipc::shell_prefs().control_center, "main")
}

/// Content with explicit module prefs, showing `page` (one of [`PAGES`], or `wifi:other` for the
/// Wi-Fi page with the Other Network sheet open; golden renders).
pub fn content_with(sys: Rc<dyn System>, prefs: ControlCenterPrefs, page: &str) -> gtk::Widget {
    content_sized(sys, prefs, page, w::CONTROL_CENTER_WIDTH)
}

/// [`content_with`] at a panel `width` (the narrow layout under [`tiles::NARROW_BELOW`]).
pub fn content_sized(
    sys: Rc<dyn System>,
    prefs: ControlCenterPrefs,
    page: &str,
    width: i32,
) -> gtk::Widget {
    content_sound(sys, prefs, page, width, Some(SoundState::demo()))
}

/// [`content_sized`] with the sound service's state (`SoundState::demo()` in the others); None
/// draws `System`'s devices only, as when the service is not running.
pub fn content_sound(
    sys: Rc<dyn System>,
    prefs: ControlCenterPrefs,
    page: &str,
    width: i32,
    sound: Option<SoundState>,
) -> gtk::Widget {
    let cc = build(sys.clone(), prefs);
    if let Some(s) = sound {
        cc.sound_page.set_source(sound::fixed(s));
    }
    cc.ui.width.set(width);
    cc.root.set_width_request(width);
    show_page_now(&cc.ui.pages, page.split(':').next().unwrap_or(page));
    cc.sync();
    if page == "wifi:other" {
        cc.pages.other_network();
    }
    // The watcher goes with the widget (no leaked task).
    let watching = RefCell::new(follow(&sys, cc.clone()));
    cc.root.connect_destroy(move |_| {
        if let Some(h) = watching.borrow_mut().take() {
            h.abort();
        }
    });
    cc.root.clone()
}

/// Follows the snapshot until the returned handle is aborted (dropping the watcher).
fn follow(sys: &Rc<dyn System>, cc: Rc<ControlCenter>) -> Option<glib::JoinHandle<()>> {
    let rx = sys.watch()?;
    Some(glib::spawn_future_local(async move {
        while rx.recv().await.is_ok() {
            cc.sync();
        }
    }))
}

pub(crate) struct Ui {
    pub sys: Rc<dyn System>,
    pub pages: gtk::Stack,
    scroll: gtk::ScrolledWindow,
    pub prefs: Cell<ControlCenterPrefs>,
    /// Panel width now (narrow on a narrow display); the tile grid's columns follow it.
    width: Cell<i32>,
    message: gtk::Label,
    auth: w::AuthSheet,
    /// Admin action waiting for the password.
    pending: RefCell<Option<Action>>,
    /// Slider actions: one in flight per kind, newest value wins (no fork storm while dragging).
    busy: RefCell<std::collections::HashMap<&'static str, Option<Action>>>,
    /// True while a snapshot pushes values into widgets (their handlers must not act).
    pub syncing: Cell<bool>,
    /// Re-reads every widget after an action settles (failed toggles snap back).
    resync: RefCell<Option<Rc<dyn Fn()>>>,
}

impl Ui {
    /// Runs `a` off the UI thread; `NeedsAuth`/`Denied` open the Authenticate sheet and retry after Allow.
    pub fn act(self: &Rc<Self>, a: Action) {
        if self.syncing.get() {
            return;
        }
        let rx = self.sys.spawn(a.clone());
        let u = self.clone();
        glib::spawn_future_local(async move {
            let Ok(r) = rx.recv().await else { return };
            u.done(a, r);
        });
    }
    fn done(self: &Rc<Self>, a: Action, r: Result<(), ActionError>) {
        match r {
            Ok(()) => self.message.set_visible(false),
            Err(e @ (ActionError::NeedsAuth | ActionError::Denied)) if asks_password(&a) => {
                self.auth.message.set_label(if e == ActionError::Denied {
                    e.message()
                } else {
                    ""
                });
                *self.pending.borrow_mut() = Some(a);
                self.auth.revealer.set_reveal_child(true);
                self.auth.field.grab_focus();
            }
            Err(e) => {
                self.message.set_label(copy::action(e));
                self.message.set_visible(true);
            }
        }
        let f = self.resync.borrow().clone();
        if let Some(f) = f {
            f();
        }
    }
    /// Runs `a` off the UI thread and hands the result to `then` (a page that shows its own
    /// progress and outcome, like joining a network); never opens the Authenticate sheet.
    pub fn act_with(
        self: &Rc<Self>,
        a: Action,
        then: impl FnOnce(Result<(), ActionError>) + 'static,
    ) {
        let rx = self.sys.spawn(a);
        let u = self.clone();
        glib::spawn_future_local(async move {
            let r = rx.recv().await.unwrap_or(Err(ActionError::Failed));
            then(r);
            let f = u.resync.borrow().clone();
            if let Some(f) = f {
                f();
            }
        });
    }
    /// Coalesced variant of [`Ui::act`] for sliders.
    pub fn act_latest(self: &Rc<Self>, kind: &'static str, a: Action) {
        if self.syncing.get() {
            return;
        }
        if let Some(slot) = self.busy.borrow_mut().get_mut(kind) {
            *slot = Some(a);
            return;
        }
        self.busy.borrow_mut().insert(kind, None);
        let rx = self.sys.spawn(a.clone());
        let u = self.clone();
        glib::spawn_future_local(async move {
            let r = rx.recv().await.unwrap_or(Ok(()));
            let next = u.busy.borrow_mut().remove(kind).flatten();
            if r.is_err() {
                u.done(a, r);
            }
            if let Some(n) = next {
                u.act_latest(kind, n);
            }
        });
    }
    fn allow(self: &Rc<Self>) {
        let pw = self.auth.field.text();
        self.auth.field.set_text("");
        match self.sys.authorize(&pw) {
            Ok(()) => {
                self.auth.revealer.set_reveal_child(false);
                if let Some(a) = self.pending.borrow_mut().take() {
                    self.act(a);
                }
            }
            Err(e) => self.auth.message.set_label(e.message()),
        }
    }
    pub fn show(&self, page: &str) {
        self.pages.set_visible_child_name(page);
    }
    fn sliding(&self, kind: &str) -> bool {
        self.busy.borrow().contains_key(kind)
    }
}

/// Wi-Fi and Bluetooth are the active user's to change (no administrator password): their
/// refusals read as a sentence, never as the Authenticate sheet.
fn asks_password(a: &Action) -> bool {
    !matches!(
        a,
        Action::Wifi(_) | Action::Bluetooth(_) | Action::WifiJoin(..) | Action::BtConnect(..)
    )
}

/// Closes the Control Center window holding `wd`, then runs `argv` (capture toolbar, Settings).
pub(crate) fn launch(wd: &impl IsA<gtk::Widget>, argv: &[&str]) {
    if let Some(win) = wd.root().and_downcast::<gtk::Window>() {
        win.close();
    }
    kit::spawn(&argv.iter().map(|s| s.to_string()).collect::<Vec<_>>());
}

struct Accessibility {
    reduce_motion: ToggleTile,
    contrast: ToggleTile,
    zoom: ToggleTile,
}

pub(crate) struct ControlCenter {
    root: gtk::Widget,
    ui: Rc<Ui>,
    wifi: w::ControlTile,
    bluetooth: w::ControlTile,
    focus: w::ControlTile,
    appearance: w::ControlTile,
    display: SliderModule,
    sound: SliderModule,
    media: NowPlayingCard,
    grid: tiles::Grid,
    battery: BatteryTile,
    mirroring: ToggleTile,
    keyboard: ToggleTile,
    night: ToggleTile,
    screenshot: gtk::Button,
    record: gtk::Button,
    a11y: Accessibility,
    /// Desktop toggles with a tile (`tiles::toggle_tiles`).
    toggles: Vec<(wol_core::toggles::Toggle, ToggleTile)>,
    extra: tiles::Extra,
    pages: pages::Pages,
    sound_page: sound::SoundPage,
    display_page: display::DisplayPage,
}

fn dark_now() -> bool {
    gtk::Settings::default().is_some_and(|s| s.is_gtk_application_prefer_dark_theme())
}

/// Theme id for the Dark Mode tile: the appearance set explicitly (Auto ends), the accent kept.
fn dark_mode_id(saved: Option<&str>, dark: bool) -> Option<String> {
    use wol_core::theme::accent;
    let variant = if dark {
        theme::Variant::Dark
    } else {
        theme::Variant::Light
    };
    accent::theme_id(Some(variant), accent::split(saved).1)
}

fn focus_label(f: Focus) -> String {
    let t = kit::now();
    focus::label(f, focus::unix_now(), t.hour, t.minute)
}

fn build(sys: Rc<dyn System>, prefs: ControlCenterPrefs) -> Rc<ControlCenter> {
    let panel = w::glass_panel(w::S2);
    panel.set_width_request(w::CONTROL_CENTER_WIDTH);
    panel.set_valign(gtk::Align::Start);
    let pages = page_stack();
    let main = w::vbox(w::S2);
    // Every module on outgrows a 720 px screen: the main page scrolls past the screen height.
    let scroll = fit_scroller(&main);
    let ui = Rc::new(Ui {
        sys: sys.clone(),
        pages: pages.clone(),
        scroll: scroll.clone(),
        prefs: Cell::new(prefs),
        width: Cell::new(w::CONTROL_CENTER_WIDTH),
        message: w::label("", "secondary"),
        auth: w::authenticate_sheet("Control Center"),
        pending: Default::default(),
        busy: Default::default(),
        syncing: Cell::new(false),
        resync: Default::default(),
    });
    {
        let u = ui.clone();
        ui.auth.cancel.connect_clicked(move |_| {
            u.pending.borrow_mut().take();
            u.auth.revealer.set_reveal_child(false);
        });
    }
    {
        let u = ui.clone();
        ui.auth.allow.connect_clicked(move |_| u.allow());
    }
    {
        let u = ui.clone();
        ui.auth.field.connect_activate(move |_| u.allow());
    }
    panel.append(&ui.auth.revealer);
    pages.add_named(&scroll, Some("main"));
    let detail = pages::Pages::build(&ui);
    let sound_page = sound::SoundPage::build(&ui);
    let display_page = display::DisplayPage::build(&ui);
    panel.append(&pages);

    // Two cards of two controls each (Wi-Fi + Bluetooth, Focus + Dark Mode): equal heights by
    // construction, nothing stretched, so every card keeps its inset on all four sides.
    let top = w::hbox(w::S2);
    top.set_homogeneous(true);
    let conn = w::vbox(w::S2);
    let wifi = w::ControlTile::new("network-wireless-symbolic", "Wi-Fi", "Off", Some(false));
    {
        let (u, t) = (ui.clone(), wifi.circle.clone());
        wifi.circle
            .connect_clicked(move |_| u.act(Action::Wifi(!t.has_css_class("on"))));
        let u = ui.clone();
        wifi.detail.connect_clicked(move |_| u.show("wifi"));
    }
    conn.append(&wifi.root);
    let bluetooth =
        w::ControlTile::new("bluetooth-active-symbolic", "Bluetooth", "Off", Some(false));
    {
        let (u, t) = (ui.clone(), bluetooth.circle.clone());
        bluetooth
            .circle
            .connect_clicked(move |_| u.act(Action::Bluetooth(!t.has_css_class("on"))));
        let u = ui.clone();
        bluetooth
            .detail
            .connect_clicked(move |_| u.show("bluetooth"));
    }
    conn.append(&bluetooth.root);
    top.append(&w::module(&conn));
    let right = w::vbox(w::S2);
    let focus_tile =
        w::ControlTile::new("weather-clear-night-symbolic", "Focus", "Off", Some(false));
    {
        let (u, c) = (ui.clone(), focus_tile.circle.clone());
        focus_tile.circle.connect_clicked(move |_| {
            let on = !c.has_css_class("on");
            let _ = focus::write(if on { Focus::On } else { Focus::Off });
            if let Some(f) = u.resync.borrow().clone() {
                f()
            }
        });
        let u = ui.clone();
        focus_tile.detail.connect_clicked(move |_| u.show("focus"));
    }
    right.append(&focus_tile.root);
    let appearance =
        w::ControlTile::new("wolos-dark-mode-symbolic", "Dark Mode", "Off", Some(false));
    {
        let c = appearance.circle.clone();
        appearance.circle.connect_clicked(move |_| {
            let on = !c.has_css_class("on");
            if let Some(id) = dark_mode_id(kit::style::active_id().as_deref(), on) {
                let _ = kit::style::set_active(&id);
            }
            if on {
                c.add_css_class("on")
            } else {
                c.remove_css_class("on")
            }
        });
    }
    right.append(&appearance.root);
    top.append(&w::module(&right));
    main.append(&top);

    let display = {
        let u = ui.clone();
        SliderModule::new(
            "Display",
            "display-brightness-symbolic",
            0.0,
            move |v| u.act_latest("brightness", Action::Brightness(v as u8)),
            Some("Display"),
        )
    };
    if let Some(d) = &display.detail {
        let u = ui.clone();
        d.connect_clicked(move |_| u.show("display"));
    }
    main.append(&display.root);
    let sound = {
        let u = ui.clone();
        SliderModule::new(
            "Sound",
            "audio-volume-high-symbolic",
            0.0,
            move |v| u.act_latest("volume", Action::Volume(v as u8)),
            Some("Output"),
        )
    };
    if let Some(d) = &sound.detail {
        let u = ui.clone();
        d.connect_clicked(move |_| u.show("sound"));
    }
    main.append(&sound.root);

    let media = NowPlayingCard::compact();
    for (b, a) in [
        (&media.prev, Action::MediaPrevious),
        (&media.play, Action::MediaToggle),
        (&media.next, Action::MediaNext),
    ] {
        let u = ui.clone();
        b.connect_clicked(move |_| u.act(a.clone()));
    }
    main.append(&media.root);

    // Battery folds into the grid: charge at a glance, a click toggles Low Power Mode.
    let battery = BatteryTile::new();
    {
        let u = ui.clone();
        battery.button.connect_clicked(move |_| {
            let Some(pm) = u.sys.power_mode() else { return };
            let low = pm == PowerMode::LowPower;
            u.act(Action::Power(if low {
                PowerMode::Automatic
            } else {
                PowerMode::LowPower
            }))
        });
    }
    // Screen Mirroring opens the Display page, where each external display can mirror this one.
    let mirroring = ToggleTile::new("video-display-symbolic", "Screen Mirroring");
    {
        let u = ui.clone();
        mirroring.button.connect_clicked(move |_| u.show("display"));
    }
    let keyboard = ToggleTile::new("keyboard-brightness-symbolic", "Keyboard Brightness");
    {
        let u = ui.clone();
        keyboard.button.connect_clicked(move |_| u.show("display"));
    }
    let night = ToggleTile::new("night-light-symbolic", "Night Shift");
    {
        let u = ui.clone();
        night.button.connect_clicked(move |_| {
            u.act(Action::NightLight(!u.sys.night_light().unwrap_or(false)))
        });
    }
    let screenshot = ToggleTile::new("camera-photo-symbolic", "Screenshot").button;
    screenshot.connect_clicked(|b| launch(b, SCREENSHOT));
    let record = ToggleTile::new("media-record-symbolic", "Record").button;
    record.connect_clicked(|b| launch(b, RECORD));

    let a11y = {
        let reduce_motion = ToggleTile::new("wolos-reduce-motion-symbolic", "Reduce Motion");
        let contrast = ToggleTile::new("wolos-contrast-symbolic", "Increase Contrast");
        let zoom = ToggleTile::new("zoom-in-symbolic", "Zoom");
        let read = |f: fn(crate::system::Accessibility) -> Option<bool>| f;
        for (t, get, make) in [
            (
                &reduce_motion,
                read(|a| a.reduce_motion),
                Action::ReduceMotion as fn(bool) -> Action,
            ),
            (
                &contrast,
                read(|a| Some(a.increase_contrast)),
                Action::IncreaseContrast,
            ),
            (&zoom, read(|a| a.zoom), Action::Zoom),
        ] {
            let u = ui.clone();
            t.button.connect_clicked(move |_| {
                let on = get(u.sys.accessibility()).unwrap_or(false);
                u.act(make(!on));
            });
        }
        Accessibility {
            reduce_motion,
            contrast,
            zoom,
        }
    };
    let toggles: Vec<_> = tiles::toggle_tiles()
        .into_iter()
        .map(|t| {
            let tile = ToggleTile::new(t.icon(), t.label());
            let u = ui.clone();
            tile.button.connect_clicked(move |_| {
                kit::spawn(&tiles::toggle_argv(t));
                // The CLI moves the state file; look again once it had the time to.
                let u = u.clone();
                glib::timeout_add_local_once(std::time::Duration::from_millis(600), move || {
                    if let Some(f) = u.resync.borrow().clone() {
                        f()
                    }
                });
            });
            (t, tile)
        })
        .collect();
    let grid = tiles::Grid::new();
    main.append(&grid.root);
    ui.message.set_visible(false);
    main.append(&ui.message);

    let cc = Rc::new(ControlCenter {
        root: panel.upcast(),
        ui: ui.clone(),
        wifi,
        bluetooth,
        focus: focus_tile,
        appearance,
        display,
        sound,
        media,
        grid,
        battery,
        mirroring,
        keyboard,
        night,
        screenshot,
        record,
        a11y,
        toggles,
        extra: Default::default(),
        pages: detail,
        sound_page,
        display_page,
    });
    {
        let weak = Rc::downgrade(&cc);
        *ui.resync.borrow_mut() = Some(Rc::new(move || {
            if let Some(cc) = weak.upgrade() {
                cc.sync();
            }
        }));
    }
    cc
}

/// `bt` -> Bluetooth tile subtitle.
fn bluetooth_label(bt: Option<bool>) -> &'static str {
    match bt {
        Some(true) => "On",
        Some(false) => "Off",
        None => "Unavailable",
    }
}

impl ControlCenter {
    /// Pushes the current snapshot into every widget; modules hide when their hardware or pref is off.
    fn sync(&self) {
        let u = &self.ui;
        let s = &*u.sys;
        let p = u.prefs.get();
        u.syncing.set(true);

        let nets = s.wifi_networks();
        self.wifi
            .set_subtitle(&crate::system::wifi_subtitle(s.network(), &nets));
        self.wifi.set_on(s.wifi_enabled().unwrap_or(false));
        let bt = s.bluetooth();
        self.bluetooth.set_subtitle(bluetooth_label(bt));
        self.bluetooth.set_on(bt.unwrap_or(false));
        self.bluetooth.circle.set_sensitive(bt.is_some());
        let f = focus::read();
        self.focus.set_on(f.active());
        self.focus.set_subtitle(&match f {
            Focus::Off => "Off".to_string(),
            Focus::On => "Do Not Disturb".to_string(),
            Focus::Until(_) => focus_label(f),
        });
        let dark = dark_now();
        self.appearance.set_on(dark);
        self.appearance
            .set_subtitle(if dark { "On" } else { "Off" });

        let b = s.brightness();
        self.display.root.set_visible(b.is_some());
        if let Some(b) = b
            && !u.sliding("brightness")
        {
            self.display.set_value(b as f64);
        }
        let a = s.audio();
        self.sound.set_icon(crate::status::volume(a));
        if let Some(v) = a.volume
            && !u.sliding("volume")
        {
            self.sound.set_value(v as f64);
        }

        let np = s.now_playing().filter(|_| p.now_playing);
        self.media.root.set_visible(np.is_some());
        if let Some(np) = &np {
            self.media.set(np);
            self.media.set_app_icon(s.now_playing_icon().as_deref());
        }

        // The grid: visible tiles in a fixed order, then the extra ones.
        let mut shown: Vec<gtk::Widget> = Vec::new();
        let bat = s.battery().filter(|_| p.battery);
        let pm = s.power_mode();
        if let Some(bt) = bat {
            self.battery.set(
                bt.percent,
                bt.status,
                s.battery_time(),
                pm == Some(PowerMode::LowPower),
            );
            shown.push(self.battery.button.clone().upcast());
        }
        let monitors = s.monitors();
        let mirrored = self.display_page.mirrored.borrow().clone();
        let mirroring = display::externals(&monitors)
            .into_iter()
            .find(|m| Some(&m.name) == mirrored.as_ref());
        self.mirroring.set_on(mirroring.is_some());
        self.mirroring.button.set_tooltip_text(
            mirroring
                .as_ref()
                .map(|m| display::display_name(m).to_string())
                .as_deref(),
        );
        shown.push(self.mirroring.button.clone().upcast());
        let k = s.keyboard_brightness().filter(|_| p.keyboard_brightness);
        if let Some(k) = k {
            self.keyboard
                .button
                .set_tooltip_text(Some(&kit::controls::percent(f64::from(k))));
            shown.push(self.keyboard.button.clone().upcast());
        }
        let night = s.night_light().filter(|_| p.night_shift);
        self.night.set_on(night.unwrap_or(false));
        if night.is_some() {
            shown.push(self.night.button.clone().upcast());
        }
        if p.screenshot {
            shown.push(self.screenshot.clone().upcast());
        }
        if p.record {
            shown.push(self.record.clone().upcast());
        }
        let acc = s.accessibility();
        self.a11y
            .reduce_motion
            .set_on(acc.reduce_motion.unwrap_or(false));
        self.a11y.contrast.set_on(acc.increase_contrast);
        self.a11y.zoom.set_on(acc.zoom.unwrap_or(false));
        if p.accessibility {
            if acc.reduce_motion.is_some() {
                shown.push(self.a11y.reduce_motion.button.clone().upcast());
            }
            shown.push(self.a11y.contrast.button.clone().upcast());
            if acc.zoom.is_some() {
                shown.push(self.a11y.zoom.button.clone().upcast());
            }
        }
        for (t, tile) in &self.toggles {
            tile.set_on(tiles::toggle_on(*t));
            shown.push(tile.button.clone().upcast());
        }
        if let Some(src) = tile_source() {
            let weak = Rc::downgrade(u);
            let toggle_src = src.clone();
            shown.extend(self.extra.buttons(src.tiles(), move |id, on| {
                toggle_src.toggle(id, on);
                if let Some(f) = weak.upgrade().and_then(|u| u.resync.borrow().clone()) {
                    f();
                }
            }));
        }
        self.grid.set(shown, tiles::columns(u.width.get()));
        if let (Some(rm), Some(g)) = (acc.reduce_motion, gtk::Settings::default())
            && g.is_gtk_enable_animations() == rm
        {
            g.set_gtk_enable_animations(!rm);
        }

        self.pages.sync(u, f, &focus_label(f));
        self.sound_page.sync(u);
        self.display_page.sync(u);
        u.syncing.set(false);
        super::menubar::refresh_now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Audit2 A2-14: a requested page opens once; the next show is main again.
    #[test]
    fn a_requested_page_opens_once() {
        request_page("wifi");
        assert_eq!(take_page(), "wifi");
        assert_eq!(take_page(), "main");
        request_page("nowhere");
        assert_eq!(take_page(), "main", "unknown pages fall back to main");
        request_page("sound");
        forget_page();
        assert_eq!(take_page(), "main", "a hide drops a request no show took");
    }

    /// Audit2 A2-11: on 1366x768 with every module on the panel crossed the Dock (768 - 76).
    /// The scroller's cap plus the menu bar and the glass margins ends above the shelf.
    #[test]
    fn the_panel_ends_above_the_dock() {
        let (screen, dock) = (768, 76);
        assert!(cap(screen, dock) + w::MENUBAR_HEIGHT + 2 * 24 <= screen - dock);
        assert_eq!(
            cap(screen, 0) - cap(screen, dock),
            dock,
            "autohide frees the space"
        );
        assert_eq!(
            cap(screen, -1),
            cap(screen, 0),
            "a bad zone reserves nothing"
        );
    }

    /// The panel keeps its width on every usual display and narrows (three tile columns) on a
    /// narrow one, never below the narrow width.
    #[test]
    fn the_panel_narrows_on_a_narrow_display() {
        assert_eq!(width_for(None), w::CONTROL_CENTER_WIDTH);
        assert_eq!(width_for(Some(1366)), w::CONTROL_CENTER_WIDTH);
        assert_eq!(
            width_for(Some(0)),
            w::CONTROL_CENTER_WIDTH,
            "a bad size changes nothing"
        );
        let narrow = width_for(Some(360));
        assert!(
            (NARROW_WIDTH..w::CONTROL_CENTER_WIDTH).contains(&narrow),
            "{narrow}"
        );
        assert_eq!(tiles::columns(narrow), 3);
        assert_eq!(width_for(Some(200)), NARROW_WIDTH);
    }

    /// Every page a status icon or a control opens is a page of the stack; the Display page
    /// (Display chevron, Keyboard Brightness, Screen Mirroring) is one of them.
    #[test]
    fn display_is_a_page_and_links_its_settings_pane() {
        assert!(PAGES.contains(&"display"));
        request_page("display");
        assert_eq!(take_page(), "display");
        assert!(SPAWNS.contains(&DISPLAY_SETTINGS));
    }

    /// UX review: the Dark Mode tile switched to the plain dark theme, dropping the accent and
    /// leaving Auto on in Settings. It sets the appearance explicitly and keeps the accent.
    #[test]
    fn dark_mode_keeps_the_accent_and_ends_auto() {
        for (saved, on, want) in [
            (None, true, Some("wol-dark")),
            (Some("wol-auto-blue"), true, Some("wol-dark-blue")),
            (Some("wol-auto-blue"), false, Some("wol-light-blue")),
            (Some("wol-light-pink"), true, Some("wol-dark-pink")),
            (Some("wol-dark-green"), false, Some("wol-light-green")),
            (Some("wol-dark"), false, Some("wol-light")),
        ] {
            assert_eq!(dark_mode_id(saved, on).as_deref(), want, "{saved:?} {on}");
        }
    }
}
