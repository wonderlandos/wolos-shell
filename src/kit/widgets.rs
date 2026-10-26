//! IaraKit components used by the shell: GlassPanel, Tile, Switch row, Slider, ListRow, Button, IconButton,
//! Avatar, ClockHeader, UserPanel. Plain GTK widgets + CSS classes from `css/shell.css`.
use gtk::{glib, prelude::*};

pub fn vbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}
pub fn hbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Horizontal, spacing)
}

pub fn label(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    if !class.is_empty() {
        l.add_css_class(class);
    }
    l
}

pub fn spacer() -> gtk::Box {
    let b = hbox(0);
    b.set_hexpand(true);
    b
}

pub fn glass_panel(spacing: i32) -> gtk::Box {
    let b = vbox(spacing);
    b.add_css_class("glass-panel");
    b
}

pub fn tile(title: &str, child: &impl IsA<gtk::Widget>) -> gtk::Box {
    let b = vbox(8);
    b.add_css_class("tile");
    b.append(&label(title, "tile-title"));
    b.append(child);
    b
}

pub fn switch_row(text: &str, on: bool, changed: impl Fn(bool) + 'static) -> gtk::Box {
    let row = hbox(8);
    let l = label(text, "");
    l.set_hexpand(true);
    let s = gtk::Switch::builder()
        .active(on)
        .valign(gtk::Align::Center)
        .build();
    s.connect_active_notify(move |s| changed(s.is_active()));
    row.append(&l);
    row.append(&s);
    row
}

pub fn slider(icon: Option<&str>, value: f64, changed: impl Fn(f64) + 'static) -> gtk::Box {
    let row = hbox(8);
    if let Some(i) = icon {
        row.append(&gtk::Image::from_icon_name(i));
    }
    let s = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    s.set_value(value);
    s.set_draw_value(false);
    s.set_hexpand(true);
    s.connect_value_changed(move |s| changed(s.value()));
    row.append(&s);
    row
}

pub fn button(text: &str, variant: &str, clicked: impl Fn() + 'static) -> gtk::Button {
    let b = gtk::Button::with_label(text);
    b.add_css_class("wol-button");
    if !variant.is_empty() {
        b.add_css_class(variant);
    }
    b.connect_clicked(move |_| clicked());
    b
}

pub fn icon_button(icon: &str, tooltip: &str, clicked: impl Fn() + 'static) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.add_css_class("icon-button");
    b.set_tooltip_text(Some(tooltip));
    b.connect_clicked(move |_| clicked());
    b
}

pub fn list_row(icon: &str, title: &str, subtitle: &str) -> gtk::Button {
    let row = hbox(10);
    if !icon.is_empty() {
        row.append(&gtk::Image::from_icon_name(icon));
    }
    let t = vbox(0);
    t.append(&label(title, ""));
    if !subtitle.is_empty() {
        t.append(&label(subtitle, "subtitle"));
    }
    row.append(&t);
    let b = gtk::Button::builder().child(&row).build();
    b.add_css_class("list-row");
    b
}

pub fn segmented(options: &[&str], selected: usize, picked: impl Fn(usize) + 'static) -> gtk::Box {
    let row = hbox(4);
    let picked = std::rc::Rc::new(picked);
    let buttons: Vec<gtk::Button> = options.iter().map(|o| gtk::Button::with_label(o)).collect();
    for (i, b) in buttons.iter().enumerate() {
        b.add_css_class(if i == selected { "seg-on" } else { "seg" });
        let (all, picked) = (buttons.clone(), picked.clone());
        b.connect_clicked(move |_| {
            for (j, o) in all.iter().enumerate() {
                o.set_css_classes(&[if j == i { "seg-on" } else { "seg" }]);
            }
            picked(i);
        });
        row.append(b);
    }
    row
}

pub fn avatar(name: &str, size: i32) -> gtk::Label {
    let l = gtk::Label::new(Some(&crate::login::initials(name)));
    l.add_css_class("avatar");
    l.add_css_class(&format!("avatar-{size}"));
    l.set_halign(gtk::Align::Center);
    l
}

/// Lock/login clock: big time + long date, ticking on minute boundaries.
pub fn clock_header() -> gtk::Box {
    let b = vbox(0);
    let (time, date) = (label("", "lock-time"), label("", "lock-date"));
    for l in [&time, &date] {
        l.set_xalign(0.5);
        b.append(l);
    }
    super::every_minute(move || {
        let (t, mut s) = (super::now(), String::with_capacity(32));
        crate::clock::lock_time(&mut s, t);
        time.set_label(&s);
        crate::clock::lock_date(&mut s, t);
        date.set_label(&s);
    });
    b
}

/// Avatar, name, round password field, message line. `submit` gets the password.
pub struct UserPanel {
    pub root: gtk::Box,
    pub field: gtk::PasswordEntry,
    pub message: gtk::Label,
}

impl UserPanel {
    pub fn new(name: &str) -> Self {
        let root = vbox(10);
        root.set_halign(gtk::Align::Center);
        root.append(&avatar(name, 96));
        let n = label(name, "lock-name");
        n.set_xalign(0.5);
        root.append(&n);
        let field = gtk::PasswordEntry::builder()
            .placeholder_text("Enter Password")
            .show_peek_icon(false)
            .build();
        field.add_css_class("lock-field");
        field.set_halign(gtk::Align::Center);
        field.set_width_request(280);
        root.append(&field);
        let message = label("", "lock-message");
        message.set_xalign(0.5);
        root.append(&message);
        UserPanel {
            root,
            field,
            message,
        }
    }

    /// Wrong password: shake once, clear, show why.
    pub fn reject(&self, why: &str) {
        self.message.set_label(why);
        self.field.set_text("");
        self.field.remove_css_class("shake");
        let f = self.field.clone();
        glib::idle_add_local_once(move || f.add_css_class("shake"));
    }
}
