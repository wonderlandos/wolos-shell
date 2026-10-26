//! IaraKit component set (HIG, archicture/modules/hig.md). The ONLY place widget metrics live:
//! surfaces pass tokens from this module, never numbers.
use gtk::{glib, prelude::*};
use std::rc::Rc;

/// Spacing scale (HIG §2).
pub const S1: i32 = 4;
pub const S2: i32 = 8;
pub const S3: i32 = 12;
pub const S4: i32 = 16;
pub const S6: i32 = 24;
pub const S8: i32 = 32;
/// Icon sizes (HIG §2): rows/toolbar, file and app tiles.
pub const ICON: i32 = 16;
pub const ICON_M: i32 = 32;
pub const ICON_L: i32 = 48;
pub const ICON_XL: i32 = 64;
/// Shell metrics.
pub const DOCK_TILE: i32 = 56;
pub const CONTROL_CENTER_WIDTH: i32 = 320;
pub const SPOTLIGHT_WIDTH: i32 = 680;
pub const PANEL_TOP: i32 = 160;
pub const SIDEBAR_WIDTH: i32 = 220;
/// Default app window and the narrow size every app must fit (HIG §5).
pub const APP_SIZE: (i32, i32) = (900, 600);
pub const APP_NARROW: (i32, i32) = (480, 360);
/// Below this window width the sidebar collapses.
pub const NARROW: i32 = 640;
pub const LOCK_FIELD_WIDTH: i32 = 200;
pub const LOCK_FIELD_HEIGHT: i32 = 28;
/// Caption width under icon tiles (switcher).
pub const TILE_CHARS: i32 = 14;

pub fn vbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}
pub fn hbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Horizontal, spacing)
}

/// Text styles: "" body 13, "title" 13 semibold, "secondary" 11 dim, "large-title" 22 semibold.
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
    let img = gtk::Image::new();
    img.set_pixel_size(size);
    let Some(path) = super::icon_file(name) else {
        img.set_icon_name(Some(name));
        return img;
    };
    let px = size * 2;
    let key = (path, px);
    if let Some(t) = APP_ICONS.with_borrow(|c| c.get(&key).cloned()) {
        img.set_paintable(Some(&t));
        return img;
    }
    let i2 = img.clone();
    glib::spawn_future_local(async move {
        let p = key.0.clone();
        let r = gtk::gio::spawn_blocking(move || {
            gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(&p, px, px, true)
                .ok()
                .map(|pb| gtk::gdk::Texture::for_pixbuf(&pb))
        })
        .await;
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
}

pub fn button(text: &str, kind: Kind, clicked: impl Fn() + 'static) -> gtk::Button {
    let b = gtk::Button::with_label(text);
    b.add_css_class("btn");
    b.add_css_class(match kind {
        Kind::Default => "btn-default",
        Kind::Primary => "btn-primary",
        Kind::Destructive => "btn-destructive",
        Kind::Plain => "btn-plain",
    });
    b.connect_clicked(move |_| clicked());
    b
}

/// Icon-only button: needs a tooltip (HIG §1).
pub fn icon_button(name: &str, tooltip: &str, clicked: impl Fn() + 'static) -> gtk::Button {
    let b = gtk::Button::builder()
        .child(&icon(name, ICON))
        .tooltip_text(tooltip)
        .build();
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
    s
}

pub fn password_field(placeholder: &str) -> gtk::PasswordEntry {
    let p = gtk::PasswordEntry::builder()
        .placeholder_text(placeholder)
        .build();
    p.add_css_class("text-field");
    p
}

pub fn spinner() -> gtk::Spinner {
    let s = gtk::Spinner::builder()
        .spinning(true)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .vexpand(true)
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
/// Menu entry: label (empty = separator) and action.
pub type MenuItem<'a> = (&'a str, Box<dyn Fn()>);

pub fn menu(items: Vec<MenuItem>) -> gtk::Popover {
    let p = gtk::Popover::new();
    p.add_css_class("menu");
    p.set_has_arrow(false);
    let b = vbox(0);
    for (text, act) in items {
        if text.is_empty() {
            b.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
            continue;
        }
        let it = gtk::Button::builder().child(&label(text, "")).build();
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

// ---- Lists ----

/// Card list: rows separated by hairlines (Settings-style).
pub fn section(title: &str, rows: &[gtk::Widget]) -> gtk::Box {
    let b = vbox(S2);
    if !title.is_empty() {
        b.append(&label(title, "title"));
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
    // Titles wrap instead of forcing the window wider (narrow layouts).
    let tl = label(title, "");
    tl.set_wrap(true);
    tl.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    t.append(&tl);
    if !subtitle.is_empty() {
        t.append(&label(subtitle, "secondary"));
    }
    row.append(&t);
    if let Some(w) = trailing {
        w.set_valign(gtk::Align::Center);
        row.append(w);
    }
    row
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
    gtk::Revealer::builder()
        .child(&card)
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .build()
}

/// Admin authorization sheet (IaraOS ADR 0026).
pub struct AuthSheet {
    pub revealer: gtk::Revealer,
    pub field: gtk::PasswordEntry,
    pub cancel: gtk::Button,
    pub allow: gtk::Button,
    pub message: gtk::Label,
}

pub fn authenticate_sheet(who: &str) -> AuthSheet {
    let b = vbox(S2);
    let head = hbox(S3);
    head.append(&icon("system-lock-screen-symbolic", ICON_M));
    let t = vbox(0);
    t.append(&label("Authenticate", "title"));
    let why = label(
        &format!("{who} is trying to make changes. Enter your password to allow this."),
        "secondary",
    );
    why.set_wrap(true);
    t.append(&why);
    head.append(&t);
    b.append(&head);
    let field = password_field("Password");
    b.append(&field);
    let message = label("", "secondary");
    b.append(&message);
    let row = hbox(S2);
    row.append(&spacer());
    let cancel = button("Cancel", Kind::Default, || {});
    let allow = button("Allow", Kind::Primary, || {});
    row.append(&cancel);
    row.append(&allow);
    b.append(&row);
    AuthSheet {
        revealer: sheet(&b),
        field,
        cancel,
        allow,
        message,
    }
}

// ---- App scaffold ----

/// HIG §1 AppWindow: 52px glass toolbar (traffic lights, title, actions, search), 220px sidebar, content, status bar.
pub struct AppWindow {
    pub root: gtk::Box,
    pub actions: gtk::Box,
    pub sidebar: gtk::ListBox,
    pub content: gtk::Stack,
    pub status: gtk::Label,
    pub title: gtk::Label,
}

impl AppWindow {
    pub fn new(title: &str, with_sidebar: bool, with_status: bool) -> Self {
        let root = vbox(0);
        root.add_css_class("app-window");
        let bar = hbox(S2);
        bar.add_css_class("toolbar");
        let lights = hbox(S2);
        lights.add_css_class("traffic-lights");
        for (c, tip) in [
            ("close", "Close"),
            ("minimize", "Minimize"),
            ("zoom", "Zoom"),
        ] {
            let b = gtk::Button::new();
            b.add_css_class("light");
            b.add_css_class(c);
            b.set_tooltip_text(Some(tip));
            b.set_valign(gtk::Align::Center);
            if c == "close" {
                b.connect_clicked(|b| {
                    if let Some(w) = b.root().and_downcast::<gtk::Window>() {
                        w.close();
                    }
                });
            }
            if c == "minimize" {
                b.connect_clicked(|b| {
                    if let Some(w) = b.root().and_downcast::<gtk::Window>() {
                        w.minimize();
                    }
                });
            }
            lights.append(&b);
        }
        bar.append(&lights);
        let title_l = label(title, "title");
        title_l.add_css_class("toolbar-title");
        bar.append(&title_l);
        bar.append(&spacer());
        let actions = hbox(S2);
        bar.append(&actions);
        let handle = gtk::WindowHandle::builder().child(&bar).build();
        root.append(&handle);
        let body = hbox(0);
        body.set_vexpand(true);
        let sidebar = gtk::ListBox::new();
        sidebar.add_css_class("sidebar");
        sidebar.set_width_request(SIDEBAR_WIDTH);
        if with_sidebar {
            body.append(&sidebar);
        }
        let content = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .hexpand(true)
            .hhomogeneous(false)
            .build();
        content.add_css_class("content");
        body.append(&content);
        root.append(&body);
        let status = label("", "secondary");
        status.add_css_class("status-bar");
        if with_status {
            root.append(&status);
        }
        // Narrow windows hide the sidebar (HIG: collapsible); content keeps the full width.
        if with_sidebar {
            let sb = sidebar.clone();
            root.connect_realize(move |r| {
                let Some(win) = r.root().and_downcast::<gtk::Window>() else {
                    return;
                };
                let fit = |w: &gtk::Window, sb: &gtk::ListBox| {
                    let on = w.default_width() >= NARROW;
                    if sb.is_visible() != on {
                        sb.set_visible(on);
                    }
                };
                fit(&win, &sb);
                let sb = sb.clone();
                win.connect_default_width_notify(move |w| fit(w, &sb));
            });
        }
        AppWindow {
            root,
            actions,
            sidebar,
            content,
            status,
            title: title_l,
        }
    }

    /// Small-caps dim header in the sidebar (not selectable).
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
        row.append(&icon(icon_name, ICON));
        row.append(&label(title, ""));
        let r = gtk::ListBoxRow::builder().child(&row).name(title).build();
        r.add_css_class("source-item");
        self.sidebar.append(&r);
        let scroll = gtk::ScrolledWindow::builder()
            .child(page)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        self.content.add_named(&scroll, Some(title));
        let content = self.content.clone();
        if self.sidebar.selected_row().is_none() {
            self.sidebar.select_row(Some(&r));
            content.set_visible_child_name(title);
        }
        let st = self.content.clone();
        self.sidebar.connect_row_selected(move |_, sel| {
            if let Some(s) = sel
                && s.widget_name() != ""
            {
                st.set_visible_child_name(&s.widget_name());
            }
        });
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
pub struct DockTile {
    pub button: gtk::Button,
    dot: gtk::Box,
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
        DockTile { button, dot }
    }
    pub fn set_running(&self, on: bool) {
        let o = if on { 1.0 } else { 0.0 };
        if self.dot.opacity() != o {
            self.dot.set_opacity(o);
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

/// macOS fat slider: title above, 24px pill track with the icon inside at the left.
pub fn control_slider(
    title: &str,
    icon_name: &str,
    value: f64,
    changed: impl Fn(f64) + 'static,
) -> gtk::Box {
    let b = vbox(S2);
    b.append(&label(title, "title"));
    let o = gtk::Overlay::new();
    let s = slider(value, changed);
    s.add_css_class("fat");
    o.set_child(Some(&s));
    let i = icon(icon_name, ICON);
    i.set_halign(gtk::Align::Start);
    i.add_css_class("fat-icon");
    i.set_can_target(false);
    o.add_overlay(&i);
    b.append(&o);
    module(&b)
}

/// Small square tile (Screenshot, Record): icon over a secondary label.
pub fn square_tile(
    icon_name: &str,
    title: &str,
    clicked: impl Fn(&gtk::Button) + 'static,
) -> gtk::Button {
    let c = vbox(S1);
    let i = icon(icon_name, ICON);
    i.set_halign(gtk::Align::Center);
    c.append(&i);
    c.append(&centered(title, "secondary"));
    let b = gtk::Button::builder().child(&c).hexpand(true).build();
    b.add_css_class("square-tile");
    b.connect_clicked(move |b| clicked(b));
    b
}

pub fn avatar(name: &str, size: i32) -> gtk::Label {
    let l = gtk::Label::new(Some(&crate::login::initials(name)));
    l.add_css_class("avatar");
    l.add_css_class(match size {
        96.. => "avatar-xl",
        48.. => "avatar-l",
        _ => "avatar-s",
    });
    l.set_size_request(size, size);
    l.set_halign(gtk::Align::Center);
    l
}
pub const AVATAR_L: i32 = 48;
pub const AVATAR_XL: i32 = 96;

/// Lock/login clock: big time + long date, ticking on minute boundaries.
pub fn clock_header() -> gtk::Box {
    let b = vbox(0);
    let (time, date) = (centered("", "lock-time"), centered("", "lock-date"));
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
    pub message: gtk::Label,
}

impl UserPanel {
    pub fn new(name: &str) -> Self {
        let root = vbox(S2);
        root.set_halign(gtk::Align::Center);
        root.append(&avatar(name, AVATAR_XL));
        let n = centered(name, "lock-name");
        root.append(&n);
        let pill = hbox(0);
        pill.add_css_class("lock-pill");
        pill.set_halign(gtk::Align::Center);
        pill.set_size_request(LOCK_FIELD_WIDTH, LOCK_FIELD_HEIGHT);
        let field = gtk::PasswordEntry::builder()
            .placeholder_text("Enter Password")
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        field.add_css_class("lock-field");
        field.set_width_chars(1);
        pill.append(&field);
        let submit = gtk::Button::builder()
            .label("→")
            .tooltip_text("Unlock")
            .valign(gtk::Align::Center)
            .sensitive(false)
            .build();
        submit.add_css_class("lock-submit");
        pill.append(&submit);
        let s2 = submit.clone();
        field.connect_changed(move |f| s2.set_sensitive(!f.text().is_empty()));
        let f2 = field.clone();
        submit.connect_clicked(move |_| {
            f2.emit_by_name::<()>("activate", &[]);
        });
        root.append(&pill);
        let message = centered("", "lock-message");
        root.append(&message);
        UserPanel {
            root,
            name: n,
            pill,
            field,
            submit,
            message,
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
        self.message.set_label(why);
        self.field.set_text("");
        self.pill.remove_css_class("shake");
        let p = self.pill.clone();
        glib::idle_add_local_once(move || p.add_css_class("shake"));
    }
}
