// SPDX-License-Identifier: GPL-2.0-or-later
//! IaraKit component set (HIG, archicture/modules/hig.md). The ONLY place widget metrics live:
//! surfaces pass tokens from this module, never numbers.
use gtk::{glib, prelude::*};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Spacing scale (HIG §2).
pub const S1: i32 = 4;
pub const S2: i32 = 8;
pub const S3: i32 = 12;
pub const S4: i32 = 16;
pub const S6: i32 = 24;
/// 32 is retired (brand-screens): 40 or 24. The lengths past 24 are the fen's named ones: 40, 60, 84.
pub const S10: i32 = 40;
pub const S15: i32 = 60;
pub const S21: i32 = 84;
/// Padding of every glass panel and card, equal on all four sides (kit.css `--inset`).
pub const INSET: i32 = S3;
/// Icon sizes (HIG §2): rows/toolbar, file and app tiles.
pub const ICON: i32 = 16;
/// Toolbar glyphs (screenshot bar).
pub const ICON_TOOL: i32 = 18;
pub const ICON_M: i32 = 32;
pub const ICON_L: i32 = 48;
pub const ICON_XL: i32 = 64;
/// Shell metrics.
pub const DOCK_TILE: i32 = 60;
/// Menu bar strip height. kit.css (`.menubar-strip`) and the desktop hyprland.conf (hyprbars
/// `bar_height`) carry the same number; tests/css_classes.rs keeps them together.
pub const MENUBAR_HEIGHT: i32 = 28;
pub const CONTROL_CENTER_WIDTH: i32 = 336;
pub const SEARCHBAR_WIDTH: i32 = 576;
pub const PANEL_TOP: i32 = 160;
pub const SIDEBAR_WIDTH: i32 = 240;
/// Default app window and the narrow size every app must fit (HIG §5).
pub const APP_SIZE: (i32, i32) = (900, 600);
pub const APP_NARROW: (i32, i32) = (480, 360);
/// Below this window width the sidebar collapses: it shows only while the content keeps at least
/// the narrow window's width beside it (QA-DESIGN-19: at 640 the content had 400 px, less than a
/// 480 window gives it with the sidebar collapsed).
pub const NARROW: i32 = SIDEBAR_WIDTH + APP_NARROW.0;
pub const LOCK_FIELD_WIDTH: i32 = 240;
pub const LOCK_FIELD_HEIGHT: i32 = 40;
/// Gap between the password field's peek glyph and the submit disc inside the lock pill.
pub const LOCK_PILL_GAP: i32 = 8;
/// Caption width under icon tiles (switcher).
pub const TILE_CHARS: i32 = 14;
/// Overview desktop thumbnail and window title line.
pub const OVERVIEW_DESKTOP: (i32, i32) = (192, 108);
pub const OVERVIEW_TITLE: i32 = 28;
/// Screenshot thumbnail (bottom right after a capture).
pub const CAPTURE_THUMB: (i32, i32) = (200, 125);
/// About <App> panel width.
pub const ABOUT_WIDTH: i32 = 280;
/// Force Quit Applications panel: width, list height before it scrolls (about six apps).
pub const FORCE_QUIT: (i32, i32) = (336, 240);

pub fn vbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}
pub fn hbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Horizontal, spacing)
}

/// Text styles: "" body 13, "title" 13 semibold (row titles), "heading" 15 semibold (section headings),
/// "secondary" 11 dim, "large-title" 26 bold (page titles).
pub fn label(text: &str, style: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    l.set_valign(gtk::Align::Center);
    if !style.is_empty() {
        l.add_css_class(style);
    }
    l
}

pub fn centered(text: &str, style: &str) -> gtk::Label {
    let l = label(text, style);
    l.set_xalign(0.5);
    l.set_justify(gtk::Justification::Center);
    l
}

pub fn spacer() -> gtk::Box {
    let b = hbox(0);
    b.set_hexpand(true);
    b
}

pub fn icon(name: &str, size: i32) -> gtk::Image {
    let i = gtk::Image::from_icon_name(name);
    i.set_pixel_size(size);
    i.set_valign(gtk::Align::Center);
    i
}

thread_local! {
    /// Rendered app icons by (file, pixel size): each SVG is rasterised once per process.
    static APP_ICONS: std::cell::RefCell<std::collections::HashMap<(std::path::PathBuf, i32), gtk::gdk::Texture>> =
        Default::default();
}

/// Full-colour app icon rendered by librsvg (gdk-pixbuf) off the main thread at 2x for HiDPI,
/// then cached. GTK's own SVG path drops filters (shadows) to boxes, so app tiles never go through it.
pub fn app_icon(name: &str, size: i32) -> gtk::Image {
    match super::icon_file(name) {
        Some(path) => svg_icon(path, size),
        None => {
            let img = gtk::Image::new();
            img.set_pixel_size(size);
            img.set_icon_name(Some(name));
            img
        }
    }
}

/// `app_icon` for a known SVG file.
fn svg_icon(path: std::path::PathBuf, size: i32) -> gtk::Image {
    let img = gtk::Image::new();
    img.set_pixel_size(size);
    let px = size * 2;
    let key = (path, px);
    if let Some(t) = APP_ICONS.with_borrow(|c| c.get(&key).cloned()) {
        img.set_paintable(Some(&t));
        return img;
    }
    let load = move |p: &std::path::Path| {
        gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(p, px, px, true)
            .ok()
            .map(|pb| gtk::gdk::Texture::for_pixbuf(&pb))
    };
    // A golden render can't wait for the thread: the icon would race the screenshot.
    if std::env::var_os("WOL_FAKE_NOW").is_some() {
        if let Some(t) = load(&key.0) {
            img.set_paintable(Some(&t));
            APP_ICONS.with_borrow_mut(|c| c.insert(key, t));
        }
        return img;
    }
    let i2 = img.clone();
    glib::spawn_future_local(async move {
        let p = key.0.clone();
        let r = gtk::gio::spawn_blocking(move || load(&p)).await;
        if let Ok(Some(t)) = r {
            i2.set_paintable(Some(&t));
            APP_ICONS.with_borrow_mut(|c| c.insert(key, t));
        }
    });
    img
}

// ---- Buttons and controls ----

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Default,
    Primary,
    Destructive,
    Plain,
    /// The second choice beside a primary button: no fill, a hairline, the accent ink.
    Outlined,
}

pub fn button(text: &str, kind: Kind, clicked: impl Fn() + 'static) -> gtk::Button {
    let b = gtk::Button::with_label(text);
    b.add_css_class("btn");
    b.add_css_class(match kind {
        Kind::Default => "btn-default",
        Kind::Primary => "btn-primary",
        Kind::Destructive => "btn-destructive",
        Kind::Plain => "btn-plain",
        Kind::Outlined => "btn-outlined",
    });
    // A push button keeps its control height wherever it sits: a box child fills the box's height
    // by default, which stretched buttons to a tall neighbour (a wrapped installer status line, a
    // toolbar) and glued them to the edges (RU-700, RU-702).
    b.set_valign(gtk::Align::Center);
    b.connect_clicked(move |_| clicked());
    b
}

/// Icon-only button: needs a tooltip (HIG §1).
pub fn icon_button(name: &str, tooltip: &str, clicked: impl Fn() + 'static) -> gtk::Button {
    let b = gtk::Button::builder()
        .child(&icon(name, ICON))
        .tooltip_text(tooltip)
        .build();
    // A picture alone has no words: the tooltip is its name on the AT-SPI bus (QA-ACC-2).
    if !tooltip.is_empty() {
        b.update_property(&[gtk::accessible::Property::Label(tooltip)]);
    }
    b.add_css_class("icon-button");
    b.connect_clicked(move |_| clicked());
    b
}

pub fn toggle(on: bool, changed: impl Fn(bool) + 'static) -> gtk::Switch {
    let s = gtk::Switch::builder()
        .active(on)
        .valign(gtk::Align::Center)
        .build();
    s.add_css_class("toggle");
    s.connect_active_notify(move |s| changed(s.is_active()));
    s
}

/// A switch for a change that takes time or can fail: `run(on, done)` starts it; the switch waits
/// (insensitive) until `done(ok)`, and a failed change flips it back to the real state.
pub fn pending_toggle(
    on: bool,
    run: impl Fn(bool, Box<dyn FnOnce(bool)>) + 'static,
) -> gtk::Switch {
    pending_switch(on, run, None)
}

/// [`pending_toggle`] with a small spinner that shows while the change runs (from the flip until
/// `done`), so a wait reads as work and not as a greyed-out switch. Put both in the row's trailing
/// box, spinner first ([`super::form::pending_toggle_row`] does).
pub fn pending_toggle_spinner(
    on: bool,
    run: impl Fn(bool, Box<dyn FnOnce(bool)>) + 'static,
) -> (gtk::Switch, gtk::Spinner) {
    let spin = busy_spinner();
    (pending_switch(on, run, Some(spin.clone())), spin)
}

/// A hidden 16 px spinner for "working on it" beside a control (shown only while busy).
pub fn busy_spinner() -> gtk::Spinner {
    let s = gtk::Spinner::builder()
        .spinning(false)
        .visible(false)
        .valign(gtk::Align::Center)
        .build();
    s.set_size_request(ICON, ICON);
    s
}

/// Shows and spins `s` while `on`, hides it otherwise.
fn spin_while(s: &gtk::Spinner, on: bool) {
    s.set_spinning(on);
    s.set_visible(on);
}

fn pending_switch(
    on: bool,
    run: impl Fn(bool, Box<dyn FnOnce(bool)>) + 'static,
    spin: Option<gtk::Spinner>,
) -> gtk::Switch {
    let s = gtk::Switch::builder()
        .active(on)
        .state(on)
        .valign(gtk::Align::Center)
        .build();
    s.add_css_class("toggle");
    s.connect_state_set(move |s, want| {
        if s.state() == want {
            return glib::Propagation::Proceed;
        }
        s.set_sensitive(false);
        if let Some(sp) = &spin {
            spin_while(sp, true);
        }
        let (s2, sp2) = (s.clone(), spin.clone());
        run(
            want,
            Box::new(move |ok| {
                let now = if ok { want } else { !want };
                s2.set_state(now);
                s2.set_active(now);
                s2.set_sensitive(true);
                if let Some(sp) = &sp2 {
                    spin_while(sp, false);
                }
            }),
        );
        glib::Propagation::Stop
    });
    s
}

pub fn slider(value: f64, changed: impl Fn(f64) + 'static) -> gtk::Scale {
    let s = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    s.set_value(value);
    s.set_draw_value(false);
    s.set_hexpand(true);
    s.add_css_class("slider");
    s.connect_value_changed(move |s| changed(s.value()));
    s
}

pub fn dropdown(options: &[&str], selected: u32) -> gtk::DropDown {
    let d = gtk::DropDown::from_strings(options);
    d.set_selected(selected);
    d.set_valign(gtk::Align::Center);
    d.add_css_class("dropdown");
    d
}

/// Gives a [`dropdown`] a new list of options (the selection is the first until set again).
pub fn set_options(d: &gtk::DropDown, options: &[&str]) {
    d.set_model(Some(&gtk::StringList::new(options)));
}

pub fn segmented(options: &[&str], selected: usize, picked: impl Fn(usize) + 'static) -> gtk::Box {
    let row = hbox(0);
    row.add_css_class("segmented");
    row.set_valign(gtk::Align::Center);
    let picked = Rc::new(picked);
    let buttons: Vec<gtk::ToggleButton> = options
        .iter()
        .map(|o| gtk::ToggleButton::with_label(o))
        .collect();
    for (i, b) in buttons.iter().enumerate() {
        if i > 0 {
            b.set_group(Some(&buttons[0]));
        }
        b.set_active(i == selected);
        let picked = picked.clone();
        b.connect_toggled(move |b| {
            if b.is_active() {
                picked(i)
            }
        });
        row.append(b);
    }
    row
}

pub fn search_field(placeholder: &str) -> gtk::SearchEntry {
    let s = gtk::SearchEntry::builder()
        .placeholder_text(placeholder)
        .build();
    s.add_css_class("search-field");
    s.set_valign(gtk::Align::Center);
    s
}

/// A key handler on `w` that sees keys before its children do (capture phase); return `Stop` to
/// take the key. For fields whose arrow keys mean something beyond the field (a search field
/// walking its hits).
pub fn on_key_captured(
    w: &impl IsA<gtk::Widget>,
    f: impl Fn(gtk::gdk::Key, gtk::gdk::ModifierType) -> glib::Propagation + 'static,
) {
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed(move |_, k, _, m| f(k, m));
    w.add_controller(keys);
}

pub fn password_field(placeholder: &str) -> gtk::PasswordEntry {
    let p = gtk::PasswordEntry::builder()
        .placeholder_text(placeholder)
        .show_peek_icon(true)
        .build();
    p.add_css_class("text-field");
    p
}

/// Under every password field: shows while Caps Lock is on. GTK's own indicator is a small icon
/// inside the entry, only while focused; this one says it in words.
pub const CAPS_LOCK: &str = crate::i18n::n_("⇪ Caps Lock is on");

/// Follows the seat's keyboard, also after one is plugged in or out (audit4 S3-12: it bound the
/// first keyboard only); its handlers go with the label.
pub fn caps_lock_note() -> gtk::Label {
    use gtk::gdk;
    let l = label(crate::i18n::tr(CAPS_LOCK), "caps-lock");
    l.set_visible(false);
    let Some(seat) = gdk::Display::default().and_then(|d| d.default_seat()) else {
        return l;
    };
    type Bound = Rc<RefCell<Option<(gdk::Device, glib::SignalHandlerId)>>>;
    let bound: Bound = Default::default();
    let bind = {
        let (weak, bound) = (l.downgrade(), bound.clone());
        Rc::new(move |seat: &gdk::Seat| {
            let Some(kb) = seat.keyboard() else { return };
            if bound.borrow().as_ref().is_some_and(|(d, _)| d == &kb) {
                return;
            }
            if let Some(l) = weak.upgrade() {
                l.set_visible(kb.is_caps_locked());
            }
            let w = weak.clone();
            let id = kb.connect_caps_lock_state_notify(move |kb| {
                if let Some(l) = w.upgrade() {
                    l.set_visible(kb.is_caps_locked());
                }
            });
            if let Some((old, old_id)) = bound.borrow_mut().replace((kb, id)) {
                old.disconnect(old_id);
            }
        })
    };
    bind(&seat);
    let ids = {
        let (b1, b2) = (bind.clone(), bind);
        Cell::new(Some((
            seat.connect_device_added(move |s, _| b1(s)),
            seat.connect_device_removed(move |s, _| b2(s)),
        )))
    };
    l.connect_destroy(move |_| {
        if let Some((added, removed)) = ids.take() {
            seat.disconnect(added);
            seat.disconnect(removed);
        }
        if let Some((kb, id)) = bound.take() {
            kb.disconnect(id);
        }
    });
    l
}

/// A spinning ICON_M spinner, centred in its slot. It never expands: inside a row it keeps the row
/// at row height (QA-VIS2-41); a page that centres one in free space sets vexpand itself.
pub fn spinner() -> gtk::Spinner {
    let s = gtk::Spinner::builder()
        .spinning(true)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    s.set_size_request(ICON_M, ICON_M);
    s
}

pub fn empty_state(
    icon_name: &str,
    title: &str,
    hint: &str,
    action: Option<gtk::Button>,
) -> gtk::Box {
    let b = vbox(S2);
    b.add_css_class("empty-state");
    b.set_valign(gtk::Align::Center);
    b.set_halign(gtk::Align::Center);
    b.set_vexpand(true);
    let i = icon(icon_name, ICON_XL);
    i.add_css_class("dim-icon");
    b.append(&i);
    b.append(&centered(title, "large-title"));
    let h = centered(hint, "secondary");
    h.set_wrap(true);
    b.append(&h);
    if let Some(a) = action {
        a.set_halign(gtk::Align::Center);
        b.append(&a);
    }
    b
}

/// Context/app menu: (label, action). "" = separator.
pub type MenuItem<'a> = (&'a str, Box<dyn Fn()>);
/// Menu row with an optional shortcut shown right-aligned and dimmed. "" = separator.
pub type MenuRow = (String, Option<&'static str>, Box<dyn Fn()>);

pub fn menu(items: Vec<MenuItem>) -> gtk::Popover {
    menu_keys(
        items
            .into_iter()
            .map(|(t, a)| (t.to_string(), None, a))
            .collect(),
    )
}

pub fn menu_keys(rows: Vec<MenuRow>) -> gtk::Popover {
    let p = gtk::Popover::new();
    p.add_css_class("menu");
    p.set_has_arrow(false);
    let b = vbox(0);
    for (text, key, act) in rows {
        if text.is_empty() {
            b.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
            continue;
        }
        let row = hbox(S6);
        row.append(&label(&text, ""));
        if let Some(k) = key {
            row.append(&spacer());
            row.append(&label(k, "menu-key"));
        }
        let it = gtk::Button::builder().child(&row).build();
        it.add_css_class("menu-item");
        let p2 = p.clone();
        it.connect_clicked(move |_| {
            p2.popdown();
            act();
        });
        b.append(&it);
    }
    p.set_child(Some(&b));
    p
}

/// Menu bar title with a drop-down menu (app name, Window).
pub fn menubar_menu(child: &impl IsA<gtk::Widget>) -> gtk::MenuButton {
    let b = gtk::MenuButton::new();
    b.set_child(Some(child));
    b.set_valign(gtk::Align::Center);
    b
}

// ---- Lists ----

/// Card list: rows separated by hairlines (Settings-style).
pub fn section(title: &str, rows: &[gtk::Widget]) -> gtk::Box {
    let b = vbox(S2);
    if !title.is_empty() {
        b.append(&label(title, "heading"));
    }
    let card = vbox(0);
    card.add_css_class("card-list");
    for r in rows {
        card.append(r);
    }
    b.append(&card);
    b
}

/// Row: optional 16px icon, title (+ secondary subtitle), trailing control right-aligned.
pub fn list_row(
    icon_name: &str,
    title: &str,
    subtitle: &str,
    trailing: Option<&gtk::Widget>,
) -> gtk::Box {
    let row = hbox(S3);
    row.add_css_class("list-row");
    if !icon_name.is_empty() {
        row.append(&icon(icon_name, ICON));
    }
    let t = vbox(0);
    t.set_valign(gtk::Align::Center);
    t.set_hexpand(true);
    // Titles and subtitles wrap instead of forcing the window wider (narrow layouts).
    let tl = label(title, "");
    tl.set_wrap(true);
    tl.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    t.append(&tl);
    let sub = (!subtitle.is_empty()).then(|| row_subtitle(subtitle));
    if let Some(l) = &sub {
        t.append(l);
    }
    row.append(&t);
    if let Some(w) = trailing {
        w.set_valign(gtk::Align::Center);
        row.append(w);
        if !title.is_empty() {
            name_controls(w, &tl, sub.as_ref());
        }
    }
    row
}

/// Controls a row's title names for assistive technology: they have no text of their own.
/// Buttons and labels carry their own words and keep them.
fn is_nameless_control(w: &gtk::Widget) -> bool {
    w.is::<gtk::Switch>()
        || w.is::<gtk::Scale>()
        || w.is::<gtk::DropDown>()
        || w.is::<gtk::SpinButton>()
        || w.is::<gtk::Entry>()
        || w.is::<gtk::PasswordEntry>()
        || w.is::<gtk::Text>()
        || (w.is::<gtk::CheckButton>()
            && w.downcast_ref::<gtk::CheckButton>()
                .is_some_and(|c| c.label().is_none_or(|l| l.is_empty())))
}

/// QA-ACC-2: every switch, slider and menu of a row was nameless on the AT-SPI bus (a screen
/// reader said "switch, off"). The row's title labels each nameless control in `within` (and
/// the subtitle describes it), however deep the row's control box nests it.
pub fn name_controls(within: &gtk::Widget, title: &gtk::Label, subtitle: Option<&gtk::Label>) {
    use gtk::accessible::Relation;
    let mut stack = vec![within.clone()];
    while let Some(w) = stack.pop() {
        if is_nameless_control(&w) {
            w.update_relation(&[Relation::LabelledBy(&[title.upcast_ref()])]);
            if let Some(d) = subtitle {
                w.update_relation(&[Relation::DescribedBy(&[d.upcast_ref()])]);
            }
            // An entry's inner text, a spin button's own entry: one name is enough.
            continue;
        }
        let mut c = w.first_child();
        while let Some(x) = c {
            c = x.next_sibling();
            stack.push(x);
        }
    }
}

/// A row's secondary line: wraps at the row's width instead of widening it.
pub fn row_subtitle(text: &str) -> gtk::Label {
    let l = label(text, "secondary");
    l.set_wrap(true);
    l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    l
}

/// Clickable row (lists of choices, results).
pub fn list_button(icon_name: &str, title: &str, subtitle: &str) -> gtk::Button {
    let b = gtk::Button::builder()
        .child(&list_row(icon_name, title, subtitle, None))
        .build();
    b.add_css_class("row-button");
    b
}

// ---- Sheets ----

pub fn sheet(child: &impl IsA<gtk::Widget>) -> gtk::Revealer {
    let card = vbox(S3);
    card.add_css_class("sheet");
    card.append(child);
    let r = gtk::Revealer::builder()
        .child(&card)
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .visible(false)
        .build();
    // Collapsed, the sheet leaves the layout: a visible 0 px child still took its box's spacing
    // (Control Center card sat 8 px lower than the panel's side insets).
    r.connect_reveal_child_notify(|r| {
        if r.reveals_child() {
            r.set_visible(true)
        }
    });
    r.connect_child_revealed_notify(|r| {
        if !r.is_child_revealed() && !r.reveals_child() {
            r.set_visible(false)
        }
    });
    r
}

/// Admin authorization sheet (IaraOS ADR 0026).
pub struct AuthSheet {
    pub revealer: gtk::Revealer,
    pub icon: gtk::Image,
    /// "Authenticate"; helper approvals put the request here (wraps).
    pub title: gtk::Label,
    /// Who asks and why (helper approvals name the helper and the change).
    pub why: gtk::Label,
    pub field: gtk::PasswordEntry,
    pub cancel: gtk::Button,
    /// Hidden unless the caller offers a lasting yes (helper approvals: Always Allow).
    pub always: gtk::Button,
    pub allow: gtk::Button,
    pub message: gtk::Label,
    /// Beside the title, hidden: shown by [`AuthSheet::set_busy`] while the change is checked.
    pub busy: gtk::Spinner,
}

impl AuthSheet {
    /// `true` while the request is being checked (no answer yet): the spinner beside the title
    /// shows and spins; `false` hides it. Buttons and field are the caller's.
    pub fn set_busy(&self, on: bool) {
        spin_while(&self.busy, on);
    }
}

pub fn authenticate_sheet(who: &str) -> AuthSheet {
    let b = vbox(S2);
    let head = hbox(S3);
    let icon = icon("system-lock-screen-symbolic", ICON_M);
    head.append(&icon);
    let t = vbox(0);
    t.set_hexpand(true);
    let title = label("Authenticate", "title");
    title.set_wrap(true);
    t.append(&title);
    let why = label(
        &format!("{who} is trying to make changes. Enter your password to allow this."),
        "secondary",
    );
    why.set_wrap(true);
    t.append(&why);
    head.append(&t);
    let busy = busy_spinner();
    busy.set_valign(gtk::Align::Start);
    head.append(&busy);
    b.append(&head);
    let field = password_field("Password");
    b.append(&field);
    let caps = caps_lock_note();
    caps.set_halign(gtk::Align::Start);
    b.append(&caps);
    let message = label("", "secondary");
    b.append(&message);
    let row = hbox(S2);
    row.append(&spacer());
    let cancel = button("Cancel", Kind::Default, || {});
    let always = button("Always Allow", Kind::Default, || {});
    always.set_visible(false);
    let allow = button("Allow", Kind::Primary, || {});
    row.append(&cancel);
    row.append(&always);
    row.append(&allow);
    b.append(&row);
    let revealer = sheet(&b);
    // The window's sheet closer ([`toolbar_sheet`]) leaves this one to its owner: Esc is Cancel
    // there, and a cancel ends the changes waiting behind it (QA-A7-5).
    revealer.add_css_class(AUTH_SHEET);
    AuthSheet {
        revealer,
        icon,
        title,
        why,
        field,
        cancel,
        always,
        allow,
        message,
        busy,
    }
}

// ---- App scaffold ----

/// The narrow-window sidebar toggle's icon and tooltip: "Show Sidebar" (back arrow) over the
/// content, "Hide Sidebar" (forward arrow, back to the page) while the sidebar fills the window.
fn toggle_face(t: &gtk::Button, sidebar_shown: bool) {
    let (name, tip) = if sidebar_shown {
        ("go-next-symbolic", "Hide Sidebar")
    } else {
        ("go-previous-symbolic", "Show Sidebar")
    };
    t.set_child(Some(&icon(name, ICON)));
    t.set_tooltip_text(Some(tip));
}

/// HIG §1 AppWindow: 40px glass toolbar (actions, search), 240px sidebar, content, status bar.
/// The compositor draws the window buttons and the title, in a 28px strip above the toolbar.
pub struct AppWindow {
    pub root: gtk::Box,
    pub actions: gtk::Box,
    pub sidebar: gtk::ListBox,
    pub content: gtk::Stack,
    pub status: gtk::Label,
    /// Toolbar start, shown only while the window is narrower than [`NARROW`]: brings the
    /// collapsed sidebar back in place of the content (picking a row returns to the content).
    pub sidebar_toggle: gtk::Button,
    /// Narrow only: true shows the sidebar alone, false goes back to the content (no-op when wide).
    show_sidebar: Rc<dyn Fn(bool)>,
    /// Widest a page gets (0: the whole pane). Settings: one ruler of content, centred.
    column: Cell<(i32, (i32, i32))>,
    /// The window's one Esc / pane-change closer for its [`toolbar_sheet`]s is wired.
    sheets_wired: Cell<bool>,
}

impl AppWindow {
    pub fn new(with_sidebar: bool, with_status: bool) -> Self {
        let root = vbox(0);
        root.add_css_class("app-window");
        let bar = hbox(S2);
        bar.add_css_class("toolbar");
        let sidebar_toggle = icon_button("go-previous-symbolic", "Show Sidebar", || {});
        sidebar_toggle.set_visible(false);
        sidebar_toggle.set_valign(gtk::Align::Center);
        if with_sidebar {
            bar.append(&sidebar_toggle);
        }
        bar.append(&spacer());
        let actions = hbox(S2);
        // Whatever an app puts on the bar (and the sidebar toggle) keeps its own control height on
        // the bar's centre line, 8 px clear of its edges (RU-700, RU-703): a box child fills the
        // bar's height by default, which stretched System Monitor's End Process and Filter field to
        // the bar's top and bottom.
        actions.set_valign(gtk::Align::Center);
        bar.append(&actions);
        let handle = gtk::WindowHandle::builder().child(&bar).build();
        root.append(&handle);
        let body = hbox(0);
        body.set_vexpand(true);
        let sidebar = gtk::ListBox::new();
        sidebar.add_css_class("sidebar");
        sidebar.set_width_request(SIDEBAR_WIDTH);
        sidebar.set_vexpand(true);
        // Scrolls when the window is shorter than the page list (never forces the window taller).
        let side = gtk::ScrolledWindow::builder()
            .child(&sidebar)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_width(true)
            .build();
        if with_sidebar {
            // Shown on realize when the window is wide enough; hidden first so a narrow
            // window's minimum size never includes it.
            side.set_visible(false);
            body.append(&side);
        }
        let content = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .hexpand(true)
            .hhomogeneous(false)
            .build();
        content.add_css_class("content");
        body.append(&content);
        root.append(&body);
        // Selecting a source row shows its page (once; a handler per row ran every one per click).
        {
            let st = content.clone();
            sidebar.connect_row_selected(move |_, sel| {
                if let Some(s) = sel
                    && s.widget_name() != ""
                {
                    st.set_visible_child_name(&s.widget_name());
                }
            });
        }
        let status = label("", "secondary");
        status.add_css_class("status-bar");
        if with_status {
            root.append(&status);
        }
        // Narrow windows collapse the sidebar (HIG: collapsible): the content keeps the full width
        // and the toolbar's toggle shows the sidebar in its place, like a collapsed split view.
        // Driven by the surface's live width, so a user resize below NARROW collapses it too.
        let mut show_sidebar: Rc<dyn Fn(bool)> = Rc::new(|_| {});
        if with_sidebar {
            let narrow = Rc::new(Cell::new(false));
            // Weak: the closures hang off these widgets' own signals.
            let panes = {
                let (sb, ct, tg) = (
                    side.downgrade(),
                    content.downgrade(),
                    sidebar_toggle.downgrade(),
                );
                move || Some((sb.upgrade()?, ct.upgrade()?, tg.upgrade()?))
            };
            let fit = {
                let (panes, narrow) = (panes.clone(), narrow.clone());
                Rc::new(move |width: i32| {
                    let Some((sb, ct, tg)) = panes() else { return };
                    let n = width < NARROW;
                    // Already laid out for this width (narrow keeps whichever pane the user chose).
                    if n == narrow.get() && (n || sb.is_visible()) {
                        return;
                    }
                    narrow.set(n);
                    sb.set_hexpand(n);
                    sb.set_visible(!n);
                    ct.set_visible(true);
                    tg.set_visible(n);
                    toggle_face(&tg, false);
                })
            };
            // Narrow only: show the sidebar alone, or go back to the content.
            let collapse = {
                let (panes, narrow) = (panes.clone(), narrow.clone());
                Rc::new(move |show_sidebar: bool| {
                    let Some((sb, ct, tg)) = panes() else { return };
                    if !narrow.get() {
                        return;
                    }
                    sb.set_visible(show_sidebar);
                    ct.set_visible(!show_sidebar);
                    // The toggle stays in both states: it is the way back to the content too.
                    toggle_face(&tg, show_sidebar);
                })
            };
            show_sidebar = collapse.clone();
            {
                let (c, sb) = (collapse.clone(), side.downgrade());
                sidebar_toggle.connect_clicked(move |_| {
                    let shown = sb.upgrade().is_some_and(|s| s.is_visible());
                    c(!shown)
                });
            }
            {
                let c = collapse.clone();
                sidebar.connect_row_activated(move |_, _| c(false));
            }
            {
                // A page opened from elsewhere (search, `open`, IPC) shows; arrowing through the
                // rows (focus inside the sidebar) does not leave it.
                let (c, sb) = (collapse.clone(), side.downgrade());
                content.connect_visible_child_notify(move |_| {
                    let Some(sb) = sb.upgrade() else { return };
                    if sb.is_visible() && !sb.state_flags().contains(gtk::StateFlags::FOCUS_WITHIN)
                    {
                        c(false);
                    }
                });
            }
            root.connect_realize(move |r| {
                let Some(win) = r.root().and_downcast::<gtk::Window>() else {
                    return;
                };
                fit(win.default_width());
                {
                    let fit = fit.clone();
                    win.connect_default_width_notify(move |w| fit(w.default_width()));
                }
                // The surface reports the size the compositor gave it (a user resize).
                if let Some(s) = win.surface() {
                    let fit = fit.clone();
                    s.connect_width_notify(move |s| {
                        if s.width() > 0 {
                            fit(s.width());
                        }
                    });
                }
            });
        }
        AppWindow {
            root,
            actions,
            sidebar,
            content,
            status,
            sidebar_toggle,
            show_sidebar,
            column: Cell::new((0, (0, 0))),
            sheets_wired: Cell::new(false),
        }
    }

    /// Small-caps dim header in the sidebar (not selectable; the arrow keys pass it, hunt 1 B1:
    /// [`Self::sidebar_search`]'s cursor handler skips rows whose child is a `source-section`).
    pub fn source_section(&self, title: &str) {
        let r = gtk::ListBoxRow::builder()
            .child(&label(&title.to_uppercase(), "source-section"))
            .selectable(false)
            .activatable(false)
            .build();
        self.sidebar.append(&r);
    }

    /// 28px sidebar row with a 16px symbolic icon; `page` becomes the content.
    pub fn source_item(&self, icon_name: &str, title: &str, page: &impl IsA<gtk::Widget>) {
        let row = hbox(S2);
        let glyph = icon(icon_name, ICON);
        glyph.add_css_class(tint_of(icon_name));
        row.append(&glyph);
        row.append(&label(title, ""));
        let r = gtk::ListBoxRow::builder().child(&row).name(title).build();
        r.add_css_class("source-item");
        self.sidebar.append(&r);
        let scroll = self.scrolled(page);
        self.content.add_named(&scroll, Some(title));
        if self.sidebar.selected_row().is_none() {
            self.sidebar.select_row(Some(&r));
            self.content.set_visible_child_name(title);
        }
    }

    /// Pages stand in a centred column at most `max` px wide (0: the whole pane).
    pub fn set_column(&self, max: i32) {
        self.column.set((max, (0, 0)));
    }

    /// As [`set_column`](Self::set_column), and `wide` = (room needed, width then) for a wide window.
    pub fn set_column_wide(&self, max: i32, wide: (i32, i32)) {
        self.column.set((max, wide));
    }

    /// A page in its scroller, and in the column when there is one.
    fn scrolled(&self, page: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
        let b = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let (max, wide) = self.column.get();
        if max > 0 {
            b.set_child(Some(&super::column::column(page, max, wide)));
        } else {
            b.set_child(Some(page));
        }
        b
    }

    pub fn select(&self, title: &str) {
        let mut i = 0;
        while let Some(r) = self.sidebar.row_at_index(i) {
            if r.widget_name().eq_ignore_ascii_case(title) {
                self.sidebar.select_row(Some(&r));
                return;
            }
            i += 1;
        }
    }
}

/// Settings page body: large padding column.
pub fn page(title: &str) -> gtk::Box {
    let p = vbox(S6);
    p.add_css_class("page");
    p.append(&label(title, "large-title"));
    p
}

// ---- Shell pieces ----

pub fn glass_panel(spacing: i32) -> gtk::Box {
    let b = vbox(spacing);
    b.add_css_class("glass-panel");
    b
}

/// Menu bar item: flat, hover pill.
pub fn menubar_item(child: &impl IsA<gtk::Widget>) -> gtk::Button {
    let b = gtk::Button::builder().child(child).build();
    b.add_css_class("menubar-item");
    b.set_valign(gtk::Align::Center);
    b
}

/// Dock tile: 56px app icon + running dot underneath.
/// A Dock icon. It bounces while its app starts (`launching`, a CSS animation, so reduce motion
/// stills it).
#[derive(Clone)]
pub struct DockTile {
    pub button: gtk::Button,
    dot: gtk::Box,
    img: gtk::Image,
}

impl DockTile {
    pub fn new(icon_name: &str, name: &str, clicked: impl Fn() + 'static) -> Self {
        let col = vbox(0);
        let img = app_icon(icon_name, DOCK_TILE);
        col.append(&img);
        let dot = hbox(0);
        dot.add_css_class("running-dot");
        dot.set_halign(gtk::Align::Center);
        dot.set_opacity(0.0);
        col.append(&dot);
        let button = gtk::Button::builder()
            .child(&col)
            .tooltip_text(name)
            .build();
        button.add_css_class("dock-tile");
        button.connect_clicked(move |_| clicked());
        DockTile { button, dot, img }
    }
    /// Settings > Dock > Size: the icon's pixel size.
    pub fn set_icon_size(&self, px: i32) {
        self.img.set_pixel_size(px);
    }
    pub fn set_running(&self, on: bool) {
        if !on && self.is_launching() {
            return; // the starting app keeps its dot
        }
        let o = if on { 1.0 } else { 0.0 };
        if self.dot.opacity() != o {
            self.dot.set_opacity(o);
        }
        if on {
            self.set_launching(false);
        }
    }
    /// The app has the keys: its dot is the accent pill.
    pub fn set_focused(&self, on: bool) {
        if on {
            self.dot.add_css_class("focused");
        } else {
            self.dot.remove_css_class("focused");
        }
    }
    pub fn is_running(&self) -> bool {
        self.dot.opacity() > 0.0 && !self.is_launching()
    }
    pub fn is_launching(&self) -> bool {
        self.button.has_css_class("launching")
    }
    /// A handle that does not keep the tile alive.
    pub fn downgrade(&self) -> impl Fn() -> Option<DockTile> + 'static {
        let (b, d, i) = (
            self.button.downgrade(),
            self.dot.downgrade(),
            self.img.downgrade(),
        );
        move || {
            Some(DockTile {
                button: b.upgrade()?,
                dot: d.upgrade()?,
                img: i.upgrade()?,
            })
        }
    }
    pub fn is_attention(&self) -> bool {
        self.button.has_css_class("attention")
    }
    /// The app asks for attention: bounce (CSS, so reduce motion stills it) until turned off.
    pub fn set_attention(&self, on: bool) {
        if on {
            self.button.add_css_class("attention")
        } else {
            self.button.remove_css_class("attention")
        }
    }
    /// Bounce on (the dot shows at once, like a started app) or off.
    pub fn set_launching(&self, on: bool) {
        if on == self.is_launching() {
            return;
        }
        if on {
            self.button.add_css_class("launching");
            self.dot.set_opacity(1.0);
        } else {
            self.button.remove_css_class("launching");
        }
    }
}

/// Control Center toggle: 28px round icon (accent when on, grey when off, disabled when unavailable) + title/subtitle.
pub struct ControlTile {
    pub root: gtk::Box,
    pub circle: gtk::Button,
    pub detail: gtk::Button,
    subtitle: gtk::Label,
}

impl ControlTile {
    pub fn new(icon_name: &str, title: &str, subtitle: &str, state: Option<bool>) -> Self {
        let root = hbox(S2);
        root.add_css_class("control");
        let circle = gtk::Button::builder()
            .child(&icon(icon_name, ICON))
            .tooltip_text(title)
            .valign(gtk::Align::Center)
            .build();
        circle.add_css_class("control-circle");
        match state {
            Some(true) => circle.add_css_class("on"),
            None => circle.set_sensitive(false),
            _ => {}
        }
        root.append(&circle);
        let t = vbox(0);
        t.set_valign(gtk::Align::Center);
        t.append(&label(title, "title"));
        let sub = label(subtitle, "secondary");
        sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
        sub.set_max_width_chars(14);
        if !subtitle.is_empty() {
            t.append(&sub);
        }
        let detail = gtk::Button::builder().child(&t).hexpand(true).build();
        detail.add_css_class("control-detail");
        root.append(&detail);
        ControlTile {
            root,
            circle,
            detail,
            subtitle: sub,
        }
    }
    pub fn set_on(&self, on: bool) {
        if on {
            self.circle.add_css_class("on")
        } else {
            self.circle.remove_css_class("on")
        }
    }
    pub fn set_subtitle(&self, s: &str) {
        if self.subtitle.text() != s {
            self.subtitle.set_label(s);
        }
    }
}

/// Control Center module card.
pub fn module(child: &impl IsA<gtk::Widget>) -> gtk::Box {
    let b = vbox(S2);
    b.add_css_class("module");
    b.append(child);
    b
}

/// The four phases of the sidebar (kit.css `--tint-*`): a sidebar glyph carries the tint of its group, not of
/// its own: connect is water, alerts are earth, the system is the base violet, safety and power are wood.
/// Fire (`--tint-fire`) is for errors only and never a sidebar tint.
pub const TINTS: [&str; 4] = ["tint-water", "tint-earth", "tint-base", "tint-wood"];

/// Glyphs of the groups that are not the system: connect (water), alerts (earth), safety and power (wood).
const WATER: [&str; 4] = [
    "network-wireless-symbolic",
    "bluetooth-active-symbolic",
    "network-wired-symbolic",
    "package-x-generic-symbolic",
];
const EARTH: [&str; 7] = [
    "preferences-system-notifications-symbolic",
    "audio-volume-high-symbolic",
    // The Store's own phase: Discover and the categories.
    "starred-symbolic",
    "applications-graphics-symbolic",
    "applications-office-symbolic",
    "applications-games-symbolic",
    "applications-engineering-symbolic",
];
const WOOD: [&str; 5] = [
    "system-lock-screen-symbolic",
    "security-high-symbolic",
    "system-users-symbolic",
    "battery-full-symbolic",
    "software-update-available-symbolic",
];

/// FNV-1a: a hash that is the same on every run and every machine (std's is seeded).
fn stable_hash(text: &str) -> u32 {
    text.bytes().fold(0x811c_9dc5u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

/// `stable_hash(text)` as an index into a list of `len` (more than 0) items.
fn pick(text: &str, len: usize) -> usize {
    usize::try_from(stable_hash(text)).map_or(0, |h| h % len)
}

/// The phase colour class of `icon_name`'s group; the system violet for any glyph no group claims.
pub fn tint_of(icon_name: &str) -> &'static str {
    if WATER.contains(&icon_name) {
        TINTS[0]
    } else if EARTH.contains(&icon_name) {
        TINTS[1]
    } else if WOOD.contains(&icon_name) {
        TINTS[3]
    } else {
        TINTS[2]
    }
}

/// The avatar tone of a name nobody picked a colour for: the same name, the same colour, from the
/// tones Setup offers (graphite stays the grey of "no name").
pub fn name_tone(name: &str) -> Option<&'static str> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let colours = &shared::login::AVATAR_TONES[..shared::login::AVATAR_TONES.len() - 1];
    Some(colours[pick(&name.to_lowercase(), colours.len())])
}

pub fn avatar(name: &str, size: i32) -> gtk::Label {
    let l = gtk::Label::new(Some(&crate::login::initials(name)));
    l.add_css_class("avatar");
    if let Some(tone) = name_tone(name) {
        l.add_css_class(&super::assistant::tone_class(tone));
    }
    l.add_css_class(match size {
        96.. => "avatar-xl",
        48.. => "avatar-l",
        _ => "avatar-s",
    });
    // Fixed square, centred both ways: a stretched label draws a pill, not a circle.
    l.set_size_request(size, size);
    l.set_halign(gtk::Align::Center);
    l.set_valign(gtk::Align::Center);
    l
}
pub const AVATAR_XL: i32 = 84;

/// Lock/login clock: big time + long date, ticking on minute boundaries.
pub fn clock_header() -> gtk::Box {
    let b = vbox(0);
    b.set_halign(gtk::Align::Start);
    let (time, date) = (label("", "lock-time"), label("", "lock-date"));
    b.append(&time);
    b.append(&date);
    super::every_minute(move || {
        let (t, mut s) = (super::now(), String::with_capacity(32));
        crate::clock::lock_time(&mut s, t);
        time.set_label(&s);
        crate::clock::lock_date(&mut s, t);
        date.set_label(&s);
    });
    b
}

/// Avatar, name, glass password pill with a submit arrow, message line.
pub struct UserPanel {
    pub root: gtk::Box,
    pub name: gtk::Label,
    pub pill: gtk::Box,
    pub field: gtk::PasswordEntry,
    pub submit: gtk::Button,
    /// Shown in place of the arrow while the password is being checked.
    pub checking: gtk::Spinner,
    pub message: gtk::Label,
    /// [`CAPS_LOCK`], between the pill and the message.
    pub caps: gtk::Label,
}

impl UserPanel {
    pub fn new(name: &str) -> Self {
        let root = vbox(S2);
        root.set_halign(gtk::Align::Center);
        root.append(&avatar(name, AVATAR_XL));
        let n = centered(name, "lock-name");
        root.append(&n);
        let pill = hbox(LOCK_PILL_GAP);
        pill.add_css_class("lock-pill");
        pill.set_halign(gtk::Align::Center);
        pill.set_size_request(LOCK_FIELD_WIDTH, LOCK_FIELD_HEIGHT);
        let field = gtk::PasswordEntry::builder()
            .placeholder_text(crate::i18n::tr("Enter Password"))
            .show_peek_icon(true)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        field.add_css_class("lock-field");
        field.set_width_chars(1);
        pill.append(&field);
        let submit = gtk::Button::builder()
            .label("→")
            .tooltip_text(crate::i18n::tr("Unlock"))
            .valign(gtk::Align::Center)
            .sensitive(false)
            .build();
        submit.add_css_class("lock-submit");
        pill.append(&submit);
        let checking = gtk::Spinner::builder()
            .valign(gtk::Align::Center)
            .visible(false)
            .tooltip_text(crate::i18n::tr(crate::copy::CHECKING))
            .build();
        checking.add_css_class("lock-checking");
        pill.append(&checking);
        let s2 = submit.clone();
        field.connect_changed(move |f| s2.set_sensitive(!f.text().is_empty()));
        let f2 = field.clone();
        submit.connect_clicked(move |_| {
            f2.emit_by_name::<()>("activate", &[]);
        });
        root.append(&pill);
        let caps = caps_lock_note();
        caps.add_css_class("lock-message");
        caps.set_halign(gtk::Align::Center);
        root.append(&caps);
        let message = centered("", "lock-message");
        root.append(&message);
        UserPanel {
            root,
            name: n,
            pill,
            field,
            submit,
            checking,
            message,
            caps,
        }
    }

    /// Shown by the keyboard's state; tests set it.
    pub fn set_caps_lock(&self, on: bool) {
        self.caps.set_visible(on);
    }

    /// Checking the password: the field waits, a spinner replaces the arrow.
    pub fn set_busy(&self, on: bool) {
        self.field.set_sensitive(!on);
        self.submit.set_visible(!on);
        self.checking.set_visible(on);
        self.checking.set_spinning(on);
        if on {
            self.message.set_label("");
        }
    }

    pub fn set_user(&self, name: &str) {
        self.name.set_label(name);
        if let Some(a) = self.root.first_child().and_downcast::<gtk::Label>() {
            a.set_label(&crate::login::initials(name));
        }
        self.message.set_label("");
        self.field.set_text("");
    }

    /// Wrong password: shake once, clear, say why.
    pub fn reject(&self, why: &str) {
        self.set_busy(false);
        self.message.set_label(why);
        self.field.set_text("");
        self.field.grab_focus();
        self.pill.remove_css_class("shake");
        let p = self.pill.clone();
        glib::idle_add_local_once(move || p.add_css_class("shake"));
    }
}

// ---- Settings components (appended) ----

/// Appearance preview tile and accent swatch metrics.
pub const PREVIEW: (i32, i32) = (112, 72);
pub const SWATCH: i32 = 20;
/// Toolbar sheet width (Authenticate, Join, Add User).
pub const SHEET_WIDTH: i32 = 336;

/// Token colours one preview slice paints (from a resolved theme, never literals).
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub surface: shared::theme::Color,
    pub sidebar: shared::theme::Color,
    pub card: shared::theme::Color,
    pub line: shared::theme::Color,
    pub dim: shared::theme::Color,
    pub accent: shared::theme::Color,
}

impl Palette {
    pub fn of(t: &crate::theme::Theme) -> Self {
        let c = |k: &str| {
            t.tokens
                .get(k)
                .and_then(|v| shared::theme::Color::parse(v))
                .unwrap_or(shared::theme::Color::WHITE)
        };
        Palette {
            surface: c("surface"),
            sidebar: c("sidebar"),
            card: c("card"),
            line: c("line"),
            dim: c("dim"),
            accent: c("accent"),
        }
    }
}

pub(crate) fn rgba(cr: &gtk::cairo::Context, c: shared::theme::Color) {
    cr.set_source_rgba(
        c.r as f64 / 255.0,
        c.g as f64 / 255.0,
        c.b as f64 / 255.0,
        c.a,
    );
}

pub(crate) fn rounded(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    use std::f64::consts::{FRAC_PI_2, PI};
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, FRAC_PI_2, PI);
    cr.arc(x + r, y + r, r, PI, 3.0 * FRAC_PI_2);
    cr.close_path();
}

/// Miniature window (sidebar, card, accent pill) per palette; several palettes split the tile diagonally (Auto).
fn draw_preview(cr: &gtk::cairo::Context, w: f64, h: f64, slices: &[Palette]) {
    let n = slices.len().max(1) as f64;
    for (i, p) in slices.iter().enumerate() {
        cr.save().ok();
        if slices.len() > 1 {
            // Diagonal split: slice i covers its share of the width, slanted.
            let x0 = w * i as f64 / n;
            let x1 = w * (i + 1) as f64 / n;
            let slant = h * 0.35;
            cr.move_to(if i == 0 { -1.0 } else { x0 + slant / 2.0 }, 0.0);
            cr.line_to(
                if i + 1 == slices.len() {
                    w + 1.0
                } else {
                    x1 + slant / 2.0
                },
                0.0,
            );
            cr.line_to(
                if i + 1 == slices.len() {
                    w + 1.0
                } else {
                    x1 - slant / 2.0
                },
                h,
            );
            cr.line_to(if i == 0 { -1.0 } else { x0 - slant / 2.0 }, h);
            cr.close_path();
            cr.clip();
        }
        rgba(cr, p.surface);
        cr.paint().ok();
        let sw = w * 0.3;
        rgba(cr, p.sidebar.over(p.surface));
        cr.rectangle(0.0, 0.0, sw, h);
        cr.fill().ok();
        // sidebar rows: one selected (accent), two dim
        rgba(cr, p.accent);
        rounded(cr, 6.0, 14.0, sw - 12.0, 7.0, 3.5);
        cr.fill().ok();
        rgba(cr, p.dim.over(p.sidebar.over(p.surface)));
        for k in 0..2 {
            rounded(cr, 6.0, 27.0 + 11.0 * k as f64, sw * 0.6, 4.0, 2.0);
            cr.fill().ok();
        }
        // content card with text lines
        let (cx, cy, cw, ch) = (sw + 8.0, 12.0, w - sw - 16.0, h - 24.0);
        rgba(cr, p.card);
        rounded(cr, cx, cy, cw, ch, 4.0);
        cr.fill_preserve().ok();
        rgba(cr, p.line.over(p.card));
        cr.set_line_width(1.0);
        cr.stroke().ok();
        rgba(cr, p.dim.over(p.card));
        for k in 0..3 {
            let lw = cw * [0.7, 0.5, 0.6][k];
            rounded(cr, cx + 6.0, cy + 8.0 + 9.0 * k as f64, lw - 12.0, 4.0, 2.0);
            cr.fill().ok();
        }
        rgba(cr, p.accent);
        rounded(cr, cx + cw - 26.0, cy + ch - 12.0, 20.0, 7.0, 3.5);
        cr.fill().ok();
        cr.restore().ok();
    }
}

/// Appearance tile: drawn preview over a caption; checked = accent ring. Group tiles with `set_group`.
pub fn theme_preview(slices: Vec<Palette>, title: &str, selected: bool) -> gtk::ToggleButton {
    let area = gtk::DrawingArea::builder()
        .content_width(PREVIEW.0)
        .content_height(PREVIEW.1)
        .build();
    area.add_css_class("preview-frame");
    area.set_overflow(gtk::Overflow::Hidden);
    area.set_draw_func(move |_, cr, w, h| draw_preview(cr, w as f64, h as f64, &slices));
    let b = vbox(S2);
    area.set_halign(gtk::Align::Center);
    b.append(&area);
    b.append(&centered(title, ""));
    let t = gtk::ToggleButton::builder()
        .child(&b)
        .active(selected)
        .build();
    t.add_css_class("preview-tile");
    t
}

/// Accent circle; checked shows a centre dot in the accent's text colour.
pub fn accent_swatch(
    color: shared::theme::Color,
    dot: shared::theme::Color,
    name: &str,
    selected: bool,
) -> gtk::ToggleButton {
    let area = gtk::DrawingArea::builder()
        .content_width(SWATCH)
        .content_height(SWATCH)
        .build();
    let t = gtk::ToggleButton::builder()
        .child(&area)
        .active(selected)
        .tooltip_text(name)
        .valign(gtk::Align::Center)
        .build();
    t.add_css_class("swatch");
    let t2 = t.downgrade();
    area.set_draw_func(move |_, cr, w, h| {
        let (w, h) = (w as f64, h as f64);
        rgba(cr, color);
        cr.arc(w / 2.0, h / 2.0, w.min(h) / 2.0, 0.0, std::f64::consts::TAU);
        cr.fill().ok();
        if t2.upgrade().is_some_and(|t| t.is_active()) {
            rgba(cr, dot);
            cr.arc(w / 2.0, h / 2.0, w.min(h) / 6.0, 0.0, std::f64::consts::TAU);
            cr.fill().ok();
        }
    });
    let a2 = area.clone();
    t.connect_toggled(move |_| a2.queue_draw());
    t
}

pub fn text_field(placeholder: &str) -> gtk::Entry {
    let e = gtk::Entry::builder().placeholder_text(placeholder).build();
    e.add_css_class("text-field");
    e
}

/// Selectable list row: icon, title (+ subtitle), checkmark when selected.
pub fn choice_row(icon_name: &str, title: &str, subtitle: &str, selected: bool) -> gtk::Button {
    let check = icon("object-select-symbolic", ICON);
    check.add_css_class("accent-icon");
    check.set_opacity(if selected { 1.0 } else { 0.0 });
    let b = gtk::Button::builder()
        .child(&list_row(
            icon_name,
            title,
            subtitle,
            Some(check.upcast_ref()),
        ))
        .build();
    b.add_css_class("row-button");
    b.add_css_class("choice-row");
    b
}

/// Indeterminate or fractional progress (long work in place, HIG §3).
pub fn progress() -> gtk::ProgressBar {
    let p = gtk::ProgressBar::new();
    p.set_hexpand(true);
    p.set_valign(gtk::Align::Center);
    p
}

/// The class of the Authenticate sheet's revealer: its owner cancels it (Esc, a pane change);
/// the window's sheet closer never hides it, and yields Esc while it is up.
const AUTH_SHEET: &str = "auth-sheet";

/// Sheet that slides down from the AppWindow toolbar (HIG §1), centred, fixed width. It belongs
/// to the page it was opened from: Esc closes it, and so does going to another page (QA-VIS2-37:
/// the New account sheet stayed through Hardware, Keyboard and Printers, under a new
/// Authenticate sheet, and only its Cancel closed it). The Esc and the pane change are the
/// window's, wired once for every sheet of it ([`close_sheets_with_esc_and_pane_change`]); sheets
/// whose closing must end a change (the Authenticate sheet) are placed with [`toolbar_place`] and
/// handle Esc themselves.
pub fn toolbar_sheet(aw: &AppWindow, child: &impl IsA<gtk::Widget>) -> gtk::Revealer {
    let r = sheet(child);
    toolbar_place(aw, &r);
    if !aw.sheets_wired.replace(true) {
        close_sheets_with_esc_and_pane_change(aw);
    }
    r
}

/// The one closer of a window's toolbar sheets (QA-A7-5: Settings and `toolbar_sheet` each had
/// one, and they only agreed because of the order GTK ran them in): Esc closes the sheet on top
/// and goes on to the focused control when none is open, and a pane change closes them all.
/// Neither touches the Authenticate sheet ([`AUTH_SHEET`]): it is cancelled by its owner, and
/// while it is up Esc is Cancel and nobody else's.
fn close_sheets_with_esc_and_pane_change(aw: &AppWindow) {
    let over = content_overlay(aw).downgrade();
    let o = over.clone();
    crate::kit::surface::on_escape_first(&aw.root, move || {
        let Some(over) = o.upgrade() else {
            return false;
        };
        if sheets_in(&over, true).next().is_some() {
            return false;
        }
        let top = open_toolbar_sheets(&over).into_iter().next();
        top.inspect(|sheet| sheet.set_reveal_child(false)).is_some()
    });
    aw.content.connect_visible_child_notify(move |_| {
        if let Some(over) = over.upgrade() {
            for sheet in open_toolbar_sheets(&over) {
                sheet.set_reveal_child(false);
            }
        }
    });
}

/// The revealers over `overlay` that are open now and are (`auth`) or are not the Authenticate
/// sheet, the one placed last (on top) first.
fn sheets_in(overlay: &gtk::Overlay, auth: bool) -> impl Iterator<Item = gtk::Revealer> {
    let mut open = Vec::new();
    let mut child = overlay.first_child();
    while let Some(c) = child {
        if let Some(r) = c.downcast_ref::<gtk::Revealer>()
            && r.has_css_class("toolbar-sheet")
            && r.reveals_child()
            && r.has_css_class(AUTH_SHEET) == auth
        {
            open.push(r.clone());
        }
        child = c.next_sibling();
    }
    open.into_iter().rev()
}

/// The toolbar sheets over `overlay` ([`content_overlay`]) that are open now, the one on top
/// first: what Esc and a pane change close (not the Authenticate sheet).
pub fn open_toolbar_sheets(overlay: &gtk::Overlay) -> Vec<gtk::Revealer> {
    sheets_in(overlay, false).collect()
}

/// Places an existing sheet (e.g. [`AuthSheet::revealer`]) under the toolbar, over the content.
pub fn toolbar_place(aw: &AppWindow, r: &gtk::Revealer) {
    place_under_toolbar(aw, r);
    r.add_css_class("toolbar-sheet");
}

/// Places a status line that looks like a sheet but is not one (the shared "Checking…"): it is
/// under the toolbar like the sheets, and Esc and a pane change leave it alone (QA-A9-SHELL-03:
/// it carried `toolbar-sheet`, so Esc hid it instead of the sheet under it).
pub fn toolbar_status(aw: &AppWindow, r: &gtk::Revealer) {
    place_under_toolbar(aw, r);
    r.add_css_class("toolbar-status");
}

fn place_under_toolbar(aw: &AppWindow, r: &gtk::Revealer) {
    r.set_halign(gtk::Align::Center);
    r.set_valign(gtk::Align::Start);
    r.set_width_request(SHEET_WIDTH);
    // Overlay the content so opening a sheet never reflows the page.
    content_overlay(aw).add_overlay(r);
}

/// The overlay over `aw`'s body (sidebar + content), created the first time it is needed.
pub fn content_overlay(aw: &AppWindow) -> gtk::Overlay {
    let body = aw.content.parent().and_downcast::<gtk::Box>();
    if let Some(o) = body
        .as_ref()
        .and_then(|b| b.parent().and_downcast::<gtk::Overlay>())
    {
        return o;
    }
    let o = gtk::Overlay::new();
    o.set_vexpand(true);
    if let Some(body) = body {
        aw.root.remove(&body);
        o.set_child(Some(&body));
    }
    aw.root.append(&o);
    if aw.status.parent().is_some_and(|p| p == aw.root) {
        aw.root.reorder_child_after(&aw.status, Some(&o));
    }
    o
}

/// Slider for list rows: the knob stays inside its 16px trough (layout check clean).
pub fn row_slider(value: f64, changed: impl Fn(f64) + 'static) -> gtk::Scale {
    let s = slider(value, changed);
    s.add_css_class("row-slider");
    s
}

/// Secondary footnote under a section (wraps; never widens the page).
pub fn footnote(text: &str) -> gtk::Label {
    let l = label(text, "secondary");
    l.set_wrap(true);
    l.add_css_class("footnote");
    l
}

/// Horizontal row that wraps onto more lines when narrow (preview tiles, swatches). Not selectable.
pub fn wrap_row(spacing: i32, max_per_line: u32) -> gtk::FlowBox {
    let f = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .column_spacing(spacing as u32)
        .row_spacing(spacing as u32)
        .max_children_per_line(max_per_line)
        .min_children_per_line(1)
        .homogeneous(true)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    f.add_css_class("wrap-row");
    f
}

/// Minimum width of a trailing value in info rows (About), in characters.
pub const VALUE_CHARS: i32 = 18;

// ---- App Store components (appended) ----

/// Store tile width (Discover rows, category grids) and hero icon size.
pub const STORE_TILE: i32 = 264;
/// Summary width in characters inside a store tile.
pub const TILE_SUMMARY_CHARS: i32 = 12;
pub const RING: i32 = 24;

/// Hero banner icon size.
pub const HERO_ICON: i32 = 120;
/// App Store details: one screenshot (16:9).
pub const SCREENSHOT_W: i32 = 400;
pub const SCREENSHOT_H: i32 = 225;
/// Dock Downloads stack: file name width under each icon.
pub const STACK_NAME_CHARS: i32 = 12;
/// Icon for packages outside the catalog (`pkg search` hits).
pub const PACKAGE_ICON: &str = "wolos-software";

/// Official icon of a store catalog app (`kit::store_icon_file`), else the generic package icon.
pub fn store_icon(id: &str, size: i32) -> gtk::Widget {
    match super::store_icon_file(id) {
        Some(p) => svg_icon(p, size).upcast(),
        None => app_icon(PACKAGE_ICON, size).upcast(),
    }
}

/// Indeterminate ring in the accent colour (install in place, HIG §3).
pub fn progress_ring() -> gtk::DrawingArea {
    ring_with(Rc::new(Cell::new(None)))
}

/// Install progress: `None` spins, `Some(0..=100)` fills clockwise from the top.
pub fn ring_with(value: Rc<Cell<Option<u8>>>) -> gtk::DrawingArea {
    let a = gtk::DrawingArea::builder()
        .content_width(RING)
        .content_height(RING)
        .valign(gtk::Align::Center)
        .build();
    a.add_css_class("progress-ring");
    a.set_draw_func(move |a, cr, w, h| {
        let (w, h) = (w as f64, h as f64);
        let c = a.color();
        let r = w.min(h) / 2.0 - 2.0;
        cr.set_line_width(3.0);
        cr.set_source_rgba(c.red() as f64, c.green() as f64, c.blue() as f64, 0.2);
        cr.arc(w / 2.0, h / 2.0, r, 0.0, std::f64::consts::TAU);
        cr.stroke().ok();
        cr.set_source_rgba(c.red() as f64, c.green() as f64, c.blue() as f64, 1.0);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        let (s, len) = match value.get() {
            Some(p) => (
                -std::f64::consts::FRAC_PI_2,
                std::f64::consts::TAU * f64::from(p.min(100)) / 100.0,
            ),
            None => {
                let t = a
                    .frame_clock()
                    .map_or(0.0, |f| f.frame_time() as f64 / 1_000_000.0);
                ((t * 4.0) % std::f64::consts::TAU, 1.6)
            }
        };
        if len > 0.0 {
            cr.arc(w / 2.0, h / 2.0, r, s, s + len);
            cr.stroke().ok();
        }
    });
    a.add_tick_callback(|a, _| {
        a.queue_draw();
        glib::ControlFlow::Continue
    });
    a
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileState {
    Get,
    Installing,
    Removing,
    Progress(u8), // installing, this far (percent)
    Open,
}

impl TileState {
    pub fn busy(self) -> bool {
        matches!(self, Self::Installing | Self::Removing | Self::Progress(_))
    }
}

/// Store tile: icon, name, one-line summary, Get/Open pill (or progress ring while installing).
#[derive(Clone)]
pub struct AppTile {
    pub root: gtk::Box,
    pub action: gtk::Button,
    ring: gtk::DrawingArea,
    value: Rc<Cell<Option<u8>>>,
    /// [`AppTile::set_source`]'s badge: one label, hidden until set (audit4 S3-11: each call
    /// appended another, "Flathub Flathub").
    source: gtk::Label,
}

impl AppTile {
    pub fn new(icon: &gtk::Widget, name: &str, summary: &str) -> Self {
        let root = hbox(S3);
        root.add_css_class("app-tile");
        root.set_width_request(STORE_TILE);
        root.append(icon);
        let t = vbox(0);
        t.set_hexpand(true);
        t.set_valign(gtk::Align::Center);
        let n = label(name, "title");
        n.set_ellipsize(gtk::pango::EllipsizeMode::End);
        n.set_max_width_chars(TILE_SUMMARY_CHARS);
        n.add_css_class("may-ellipsize");
        t.append(&n);
        let s = label(summary, "secondary");
        s.set_wrap(true);
        s.set_lines(2);
        s.set_max_width_chars(TILE_SUMMARY_CHARS);
        s.set_width_chars(TILE_SUMMARY_CHARS);
        s.set_ellipsize(gtk::pango::EllipsizeMode::End);
        s.add_css_class("may-ellipsize");
        t.append(&s);
        let source = label("", "badge");
        source.set_halign(gtk::Align::Start);
        source.set_margin_top(S1);
        source.set_visible(false);
        t.append(&source);
        root.append(&t);
        let action = gtk::Button::with_label("Get");
        action.add_css_class("get-button");
        action.set_valign(gtk::Align::Center);
        root.append(&action);
        let value = Rc::new(Cell::new(None));
        let ring = ring_with(value.clone());
        ring.set_visible(false);
        root.append(&ring);
        AppTile {
            root,
            action,
            ring,
            value,
            source,
        }
    }

    /// Where the app comes from when it is not WolOS's own packages ("Flathub"): a badge under
    /// the summary.
    /// Calling it again replaces the badge; "" hides it.
    pub fn set_source(&self, source: &str) {
        self.source.set_label(source);
        self.source.set_visible(!source.is_empty());
    }

    /// The badge [`AppTile::set_source`] shows ("" when none).
    pub fn source(&self) -> String {
        if self.source.is_visible() {
            self.source.label().into()
        } else {
            String::new()
        }
    }

    pub fn set_state(&self, s: TileState) {
        self.action.set_visible(!s.busy());
        self.ring.set_visible(s.busy());
        self.value.set(match s {
            TileState::Progress(p) => Some(p),
            _ => None,
        });
        self.action
            .set_label(if s == TileState::Open { "Open" } else { "Get" });
    }
}

/// Page hero: wide banner in the app's own two colours (white text), headline, subhead and
/// action on the left, the large app icon on the right.
pub fn hero_banner(
    colors: [&str; 2],
    eyebrow: &str,
    headline: &str,
    subhead: &str,
    icon: &gtk::Widget,
    action: &gtk::Button,
) -> gtk::Box {
    let b = hbox(S6);
    b.add_css_class("hero-banner");
    b.add_css_class(&gradient_class(colors));
    let t = vbox(S2);
    t.set_hexpand(true);
    t.set_valign(gtk::Align::Center);
    t.append(&label(&eyebrow.to_uppercase(), "hero-eyebrow"));
    let tl = label(headline, "large-title");
    tl.set_wrap(true);
    t.append(&tl);
    let s = label(subhead, "hero-subhead");
    s.set_max_width_chars(46);
    s.set_wrap(true);
    t.append(&s);
    action.set_halign(gtk::Align::Start);
    action.set_margin_top(S2);
    t.append(action);
    b.append(&t);
    icon.set_valign(gtk::Align::Center);
    b.append(icon);
    b
}

thread_local! {
    static GRADIENTS: std::cell::RefCell<std::collections::HashSet<String>> = Default::default();
}

/// CSS class painting a 135deg two-stop gradient; one display provider per colour pair.
fn gradient_class(colors: [&str; 2]) -> String {
    let [a, b] = colors.map(|c| c.trim_start_matches('#').to_ascii_lowercase());
    let class = format!("gradient-{a}-{b}");
    let new = GRADIENTS.with_borrow_mut(|g| g.insert(class.clone()));
    if new && let Some(d) = gtk::gdk::Display::default() {
        let p = gtk::CssProvider::new();
        p.load_from_string(&format!(
            ".{class} {{ background-image: linear-gradient(135deg, #{a}, #{b}); }}"
        ));
        gtk::style_context_add_provider_for_display(
            &d,
            &p,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
    }
    class
}

impl AppWindow {
    /// Search field at the top of the sidebar (App Store, Settings-style apps).
    pub fn sidebar_search(&self, placeholder: &str) -> gtk::SearchEntry {
        let s = search_field(placeholder);
        let r = gtk::ListBoxRow::builder()
            .child(&s)
            .selectable(false)
            .activatable(false)
            .build();
        r.add_css_class("sidebar-search");
        self.sidebar.prepend(&r);
        // The arrow keys go from page row to page row: a header row is passed, and Up from the
        // first page row lands in the field, not on the ringed row around it where typing did
        // nothing (QA-HUNT-12). The rows stay focusable (RU-048); the cursor skips them here.
        let (field, search) = (s.downgrade(), r.downgrade());
        let is_header =
            |r: &gtk::ListBoxRow| r.child().is_some_and(|c| c.has_css_class("source-section"));
        self.sidebar
            .connect_move_cursor(move |list, _, count, _, _| {
                let (Some(field), Some(search)) = (field.upgrade(), search.upgrade()) else {
                    return;
                };
                let Some(at) = list.focus_child().and_downcast::<gtk::ListBoxRow>() else {
                    return;
                };
                if count == 0 {
                    return;
                }
                let shown = |i: i32| {
                    list.row_at_index(i)
                        .filter(|r| r.is_visible() && r.is_child_visible())
                };
                let next = if count < 0 {
                    (0..at.index()).rev().find_map(shown)
                } else {
                    (at.index() + 1..i32::MAX)
                        .map_while(|i| list.row_at_index(i).map(|_| i))
                        .find_map(shown)
                };
                let Some(next) = next else { return };
                // Past the headers to the next page row, or into the field.
                let target = if is_header(&next) {
                    let i = next.index();
                    if count < 0 {
                        (0..i).rev().find_map(shown)
                    } else {
                        (i + 1..i32::MAX)
                            .map_while(|j| list.row_at_index(j).map(|_| j))
                            .find_map(shown)
                    }
                } else {
                    Some(next.clone())
                };
                match target {
                    Some(t) if t == search => {
                        field.grab_focus();
                    }
                    Some(t) if is_header(&next) => {
                        t.grab_focus();
                    }
                    _ => return,
                }
                list.stop_signal_emission_by_name("move-cursor");
            });
        // Narrow: Return shows what the search opened (typing keeps the field on screen).
        let show = self.show_sidebar.clone();
        s.connect_activate(move |_| show(false));
        s
    }

    /// Content page with no sidebar row (search results); show it with `content.set_visible_child_name`.
    pub fn hidden_page(&self, name: &str, page: &impl IsA<gtk::Widget>) {
        let scroll = self.scrolled(page);
        self.content.add_named(&scroll, Some(name));
    }
}

impl AppWindow {
    /// Class of the last sidebar row of a group ([`source_gap`](Self::source_gap)).
    const GROUP_END: &'static str = "group-end";

    /// The sidebar search field ([`sidebar_search`](Self::sidebar_search)) has text.
    fn sidebar_searching(list: &gtk::ListBox) -> bool {
        list.row_at_index(0)
            .filter(|r| r.has_css_class("sidebar-search"))
            .and_then(|r| r.child())
            .and_downcast::<gtk::SearchEntry>()
            .is_some_and(|s| !s.text().trim().is_empty())
    }

    /// Space between sidebar groups (System Settings-style, no header): the next source row
    /// starts a group. The space is a list header, not a row, so the arrow keys go straight from
    /// the last row of one group to the first of the next. No space while the sidebar search has
    /// text (the hits read as one list).
    pub fn source_gap(&self) {
        let mut last = None;
        let mut i = 0;
        while let Some(r) = self.sidebar.row_at_index(i) {
            last = Some(r);
            i += 1;
        }
        let Some(last) = last else { return };
        last.add_css_class(Self::GROUP_END);
        self.sidebar.set_header_func(|row, before| {
            let gap = before.is_some_and(|b| b.has_css_class(Self::GROUP_END))
                && !row
                    .parent()
                    .and_downcast::<gtk::ListBox>()
                    .is_some_and(|l| Self::sidebar_searching(&l));
            if !gap {
                if row.header().is_some() {
                    row.set_header(None::<&gtk::Widget>);
                }
            } else if row.header().is_none() {
                let space = vbox(0);
                space.add_css_class("source-gap");
                row.set_header(Some(&space));
            }
        });
    }

    /// Shows the page `name`: its sidebar row when it has one, else a [`hidden_page`](Self::hidden_page)
    /// (subpages, search results) with the sidebar selection cleared.
    pub fn open(&self, name: &str) {
        let mut i = 0;
        while let Some(r) = self.sidebar.row_at_index(i) {
            if r.widget_name().eq_ignore_ascii_case(name) {
                self.sidebar.select_row(Some(&r));
                self.content.set_visible_child_name(&r.widget_name());
                return;
            }
            i += 1;
        }
        if self.content.child_by_name(name).is_some() {
            self.sidebar.unselect_all();
            self.content.set_visible_child_name(name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_keeps_its_avatar_colour() {
        assert_eq!(name_tone("Alice Liddell"), name_tone("  alice liddell "));
        assert_eq!(name_tone(""), None);
        assert_eq!(name_tone("  "), None);
    }

    #[test]
    fn avatar_colours_come_from_setups_tones_and_are_not_all_one() {
        let names = [
            "alice", "bob", "carol", "dave", "erin", "frank", "grace", "heidi",
        ];
        let tones: std::collections::BTreeSet<_> =
            names.iter().filter_map(|n| name_tone(n)).collect();
        assert!(tones.len() >= 3, "{tones:?}");
        assert!(
            !tones.contains("graphite"),
            "graphite is the grey of no name"
        );
        for t in &tones {
            assert!(shared::login::AVATAR_TONES.contains(t));
        }
    }

    #[test]
    fn a_sidebar_glyph_keeps_its_section_colour_and_every_colour_is_styled() {
        assert_eq!(
            tint_of("network-wireless-symbolic"),
            tint_of("network-wireless-symbolic")
        );
        let css = include_str!("kit.css");
        for t in TINTS {
            assert!(
                css.contains(&format!("list.sidebar row image.{t} {{")),
                "kit.css styles {t}"
            );
            assert!(css.contains(&format!("--{t}: #")), "kit.css defines --{t}");
        }
    }
}
