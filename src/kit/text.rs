// SPDX-License-Identifier: GPL-2.0-or-later
//! The text area of the WolOS Design System (ADR 0058 draft): text of several lines that wraps at
//! the field's width and scrolls when long. libWolkit's text area (WK-TEXT) is the same component
//! for libwol apps. Spec: `design/components/text-area.toml`; test: `tests/kit_calendar.rs`.
use super::widgets as w;
use gtk::prelude::*;

/// A text area at least three lines (84 px) tall; it grows with its page, and the text scrolls
/// inside it beyond that. `view` is the text view (its buffer holds the text), `root` the field.
pub struct TextArea {
    pub root: gtk::ScrolledWindow,
    pub view: gtk::TextView,
    placeholder: gtk::Label,
}

impl TextArea {
    /// `placeholder` shows, dim, while the area is empty: an example of what goes in.
    pub fn new(text: &str, placeholder: &str) -> TextArea {
        let view = gtk::TextView::builder()
            .wrap_mode(gtk::WrapMode::WordChar)
            .accepts_tab(false)
            .top_margin(w::S1)
            .bottom_margin(w::S1)
            .left_margin(w::S2)
            .right_margin(w::S2)
            .build();
        view.add_css_class("text-area");
        view.buffer().set_text(text);
        let placeholder = w::label(placeholder, "secondary");
        placeholder.add_css_class("text-area-placeholder");
        placeholder.set_valign(gtk::Align::Start);
        placeholder.set_can_target(false);
        let overlay = gtk::Overlay::builder().child(&view).build();
        overlay.add_overlay(&placeholder);
        placeholder.set_visible(text.is_empty());
        {
            let p = placeholder.downgrade();
            view.buffer().connect_changed(move |b| {
                if let Some(p) = p.upgrade() {
                    p.set_visible(b.char_count() == 0);
                }
            });
        }
        let root = gtk::ScrolledWindow::builder()
            .child(&overlay)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_height(TEXT_AREA_MIN)
            .build();
        root.add_css_class("text-area-field");
        TextArea {
            root,
            view,
            placeholder,
        }
    }

    pub fn text(&self) -> String {
        let b = self.view.buffer();
        b.text(&b.start_iter(), &b.end_iter(), false).into()
    }

    /// Read-only and dimmed: the text can be read, selected and copied, not changed.
    pub fn set_editable(&self, on: bool) {
        self.view.set_editable(on);
        self.view.set_cursor_visible(on);
        self.root.set_sensitive(on);
        self.placeholder
            .set_visible(on && self.view.buffer().char_count() == 0);
    }
}

/// The smallest drawn height of a text area: three lines of 20 plus the 4 px margins and the
/// field's 8 px of padding (84, a named fen length).
pub const TEXT_AREA_MIN: i32 = 84;
