// SPDX-License-Identifier: GPL-2.0-or-later
//! Control Center Display page: display and keyboard brightness, Night Shift, and Screen
//! Mirroring (one external display shows the built-in one through a compositor `mirror` monitor
//! rule). Opened by the Display slider's chevron and the Keyboard Brightness and Screen
//! Mirroring tiles. Invariant: a mirror rule is built only from monitor names the compositor gave us
//! that pass [`mirror_rule`]'s check, so nothing typed or read reaches `hyprctl` unchecked.
use super::{DISPLAY_SETTINGS, Ui, launch};
use crate::kit::{
    controls::{DetailPage, DeviceRow},
    widgets as w,
};
use crate::system::{Action, Monitor};
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

/// Compositor `monitor=` value that mirrors `source` on `display`, or (None) gives `display` its
/// own desktop again. None when a name is not a plain connector name (`HDMI-A-1`).
pub(super) fn mirror_rule(display: &str, source: Option<&str>) -> Option<String> {
    let ok = |n: &str| {
        !n.is_empty()
            && n.len() <= 32
            && n.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    };
    if !ok(display) || source.is_some_and(|s| !ok(s) || s == display) {
        return None;
    }
    Some(match source {
        Some(s) => format!("{display},preferred,auto,1,mirror,{s}"),
        None => format!("{display},preferred,auto,1"),
    })
}

/// The display others mirror: the built-in panel (`eDP`, `LVDS`, `DSI`), else the first one.
pub(super) fn main_display(monitors: &[Monitor]) -> Option<&Monitor> {
    crate::displays::main_of(monitors)
}

/// Displays that can mirror the main one.
pub(super) fn externals(monitors: &[Monitor]) -> Vec<Monitor> {
    let main = main_display(monitors).map(|m| m.name.clone());
    monitors
        .iter()
        .filter(|m| Some(&m.name) != main.as_ref())
        .cloned()
        .collect()
}

/// A display's name as people read it: the compositor's description, else the connector.
pub(super) fn display_name(m: &Monitor) -> &str {
    if m.description.trim().is_empty() {
        &m.name
    } else {
        &m.description
    }
}

/// One flat slider row (fat track, icon inside) on a detail page.
fn slider_row(icon_name: &str, changed: impl Fn(f64) + 'static) -> (gtk::Overlay, gtk::Scale) {
    let o = gtk::Overlay::new();
    o.set_hexpand(true);
    let scale = w::slider(0.0, changed);
    scale.add_css_class("fat");
    o.set_child(Some(&scale));
    let icon = w::icon(icon_name, w::ICON);
    icon.set_halign(gtk::Align::Start);
    icon.add_css_class("fat-icon");
    icon.set_can_target(false);
    o.add_overlay(&icon);
    (o, scale)
}

fn heading(text: &str) -> gtk::Label {
    let h = w::label(text, "secondary");
    h.add_css_class("detail-heading");
    h
}

pub(super) struct DisplayPage {
    brightness: (gtk::Overlay, gtk::Scale),
    brightness_heading: gtk::Label,
    keyboard: (gtk::Overlay, gtk::Scale),
    keyboard_heading: gtk::Label,
    night: DeviceRow,
    mirrors: gtk::Box,
    shown: RefCell<Option<(Vec<Monitor>, Option<String>)>>,
    /// The external display mirroring the main one (set by this page; the compositor does not list it).
    pub mirrored: Rc<RefCell<Option<String>>>,
}

impl DisplayPage {
    pub fn build(ui: &Rc<Ui>) -> Self {
        let page = DetailPage::new("Display", None, "", Some("Displays Settings…"));
        {
            let u = ui.clone();
            page.back.connect_clicked(move |_| u.show("main"));
        }
        if let Some(f) = &page.footer {
            f.connect_clicked(move |b| launch(b, DISPLAY_SETTINGS));
        }
        let brightness = {
            let u = ui.clone();
            slider_row("display-brightness-symbolic", move |v| {
                u.act_latest(
                    "brightness",
                    Action::Brightness(v.round().clamp(0.0, 100.0) as u8),
                )
            })
        };
        let keyboard = {
            let u = ui.clone();
            slider_row("keyboard-brightness-symbolic", move |v| {
                u.act_latest(
                    "keyboard",
                    Action::KeyboardBrightness(v.round().clamp(0.0, 100.0) as u8),
                )
            })
        };
        let night = DeviceRow::new("night-light-symbolic", "Night Shift", "", false, None);
        {
            let u = ui.clone();
            night.button.connect_clicked(move |_| u.toggle_night());
        }
        let (brightness_heading, keyboard_heading) =
            (heading("Brightness"), heading("Keyboard Brightness"));
        let mirrors = w::vbox(0);
        page.set_rows(
            &[
                brightness_heading.clone().upcast(),
                brightness.0.clone().upcast(),
                keyboard_heading.clone().upcast(),
                keyboard.0.clone().upcast(),
                night.button.clone().upcast(),
                heading("Screen Mirroring").upcast(),
                mirrors.clone().upcast(),
            ],
            "",
        );
        ui.pages.add_named(&page.root, Some("display"));
        DisplayPage {
            brightness,
            brightness_heading,
            keyboard,
            keyboard_heading,
            night,
            mirrors,
            shown: Default::default(),
            mirrored: Default::default(),
        }
    }

    /// Called from `ControlCenter::sync` (handlers muted).
    pub fn sync(&self, ui: &Rc<Ui>) {
        let s = &*ui.sys;
        let b = s.brightness();
        self.brightness_heading.set_visible(b.is_some());
        self.brightness.0.set_visible(b.is_some());
        if let Some(b) = b
            && !ui.sliding("brightness")
        {
            set_value(&self.brightness.1, b);
        }
        let k = s.keyboard_brightness();
        self.keyboard_heading.set_visible(k.is_some());
        self.keyboard.0.set_visible(k.is_some());
        if let Some(k) = k
            && !ui.sliding("keyboard")
        {
            set_value(&self.keyboard.1, k);
        }
        let night = s.night_light().map(|n| ui.holds.shown(super::NIGHT, n));
        self.night.button.set_visible(night.is_some());
        self.night.set_active(night.unwrap_or(false));
        self.night
            .set_subtitle(if night == Some(true) { "On" } else { "Off" });

        let monitors = s.monitors();
        // The compositor says what mirrors what: after a reload, an unplug or a rule written
        // elsewhere the page follows it, not what it asked last (QA-SET-47).
        *self.mirrored.borrow_mut() = mirrored_now(&monitors);
        let state = (monitors.clone(), self.mirrored.borrow().clone());
        if self.shown.borrow().as_ref() == Some(&state) {
            return;
        }
        while let Some(c) = self.mirrors.first_child() {
            self.mirrors.remove(&c);
        }
        let source = main_display(&monitors).map(|m| m.name.clone());
        let others = externals(&monitors);
        for m in &others {
            let on = state.1.as_deref() == Some(m.name.as_str());
            let r = DeviceRow::new(
                "video-display-symbolic",
                display_name(m),
                if on { "Mirroring" } else { "" },
                on,
                on.then_some("object-select-symbolic"),
            );
            let (u, name, src, mirrored) = (
                ui.clone(),
                m.name.clone(),
                source.clone(),
                self.mirrored.clone(),
            );
            r.button.connect_clicked(move |_| {
                toggle_mirror(&u, &mirrored, &name, src.as_deref());
            });
            self.mirrors.append(&r.button);
        }
        if others.is_empty() {
            let l = w::label("No other display", "secondary");
            l.add_css_class("detail-empty");
            self.mirrors.append(&l);
        }
        *self.shown.borrow_mut() = Some(state);
    }
}

/// Mirrors the main display on `name`, or stops when `name` already mirrors it.
fn toggle_mirror(
    ui: &Rc<Ui>,
    mirrored: &Rc<RefCell<Option<String>>>,
    name: &str,
    source: Option<&str>,
) {
    let on = mirrored.borrow().as_deref() == Some(name);
    let Some(rule) = mirror_rule(name, if on { None } else { source }) else {
        return;
    };
    // The page says "Mirroring" only once the compositor took the rule (QA-SET-47).
    let (m, n) = (mirrored.clone(), name.to_string());
    ui.act_then(Action::Monitor(rule), move |ok| {
        mirror_answered(&m, &n, !on, ok);
    });
}

/// The display that mirrors another one now, as the compositor lists it (`mirrorOf`).
pub(super) fn mirrored_now(monitors: &[Monitor]) -> Option<String> {
    monitors
        .iter()
        .find(|m| m.mirror_of.is_some())
        .map(|m| m.name.clone())
}

/// The compositor answered the mirror rule for `name` (`turn_on` = what was asked): only a
/// success changes what the page shows; a refusal leaves it as it was.
pub(super) fn mirror_answered(
    mirrored: &RefCell<Option<String>>,
    name: &str,
    turn_on: bool,
    ok: bool,
) {
    if ok {
        *mirrored.borrow_mut() = turn_on.then(|| name.to_string());
    }
}

fn set_value(s: &gtk::Scale, v: u8) {
    if (s.value() - f64::from(v)).abs() >= 0.5 {
        s.set_value(f64::from(v));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(name: &str, description: &str) -> Monitor {
        Monitor {
            name: name.into(),
            description: description.into(),
            ..Default::default()
        }
    }

    #[test]
    fn mirror_rules_take_plain_connector_names_only() {
        assert_eq!(
            mirror_rule("HDMI-A-1", Some("eDP-1")).as_deref(),
            Some("HDMI-A-1,preferred,auto,1,mirror,eDP-1")
        );
        assert_eq!(
            mirror_rule("HDMI-A-1", None).as_deref(),
            Some("HDMI-A-1,preferred,auto,1")
        );
        for (d, s) in [
            ("", Some("eDP-1")),
            ("HDMI-A-1,1x1", Some("eDP-1")),
            ("HDMI-A-1", Some("eDP-1;exec")),
            ("DP-1", Some("DP-1")),
            ("DP 1", None),
        ] {
            assert_eq!(mirror_rule(d, s), None, "{d:?} {s:?}");
        }
    }

    /// QA-SET-47 / QA-SHADOW-60: toggle_mirror set the name before hyprctl answered, so a refused
    /// rule left "Mirroring" lit. Only a success moves the state.
    #[test]
    fn mirrored_only_on_ok() {
        let st = RefCell::new(None);
        mirror_answered(&st, "HDMI-A-1", true, false);
        assert_eq!(*st.borrow(), None, "refused: nothing lit");
        mirror_answered(&st, "HDMI-A-1", true, true);
        assert_eq!(st.borrow().as_deref(), Some("HDMI-A-1"));
        mirror_answered(&st, "HDMI-A-1", false, false);
        assert_eq!(
            st.borrow().as_deref(),
            Some("HDMI-A-1"),
            "stop refused: still on"
        );
        mirror_answered(&st, "HDMI-A-1", false, true);
        assert_eq!(*st.borrow(), None);
    }

    /// QA-SET-47 stale-after-reload: what the page lights is what the compositor reports, so a
    /// mirror ended by a reload or an unplug is not shown as "Mirroring" any more.
    #[test]
    fn mirrored_follows_the_compositor() {
        let mut tv = m("HDMI-A-1", "LG");
        let list = [m("eDP-1", "Built-in Display"), tv.clone()];
        assert_eq!(mirrored_now(&list), None);
        tv.mirror_of = Some("eDP-1".into());
        let list = [m("eDP-1", "Built-in Display"), tv];
        assert_eq!(mirrored_now(&list).as_deref(), Some("HDMI-A-1"));
        assert_eq!(mirrored_now(&[]), None);
    }

    #[test]
    fn the_built_in_panel_is_the_one_mirrored() {
        let list = [
            m("HDMI-A-1", "LG"),
            m("eDP-1", "Built-in Display"),
            m("DP-2", ""),
        ];
        assert_eq!(main_display(&list).map(|m| m.name.as_str()), Some("eDP-1"));
        let names: Vec<_> = externals(&list).into_iter().map(|m| m.name).collect();
        assert_eq!(names, ["HDMI-A-1", "DP-2"]);
        assert_eq!(display_name(&list[2]), "DP-2");
        assert_eq!(display_name(&list[0]), "LG");
        let desk = [m("DP-1", "Dell"), m("DP-2", "Dell")];
        assert_eq!(main_display(&desk).map(|m| m.name.as_str()), Some("DP-1"));
        assert!(externals(&[]).is_empty());
    }
}
