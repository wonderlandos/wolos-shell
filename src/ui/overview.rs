// SPDX-License-Identifier: GPL-2.0-or-later
//! Overview surface: blurred wallpaper, desktops strip on top, the current desktop's
//! windows scaled into a grid (live screencopy via grim; app icon until it arrives).
use crate::hypr::{Client, Dispatch};
use crate::kit::{
    surface::{Keyboard, Spec},
    widgets as w,
};
use crate::overview::{self, Rect};
use gtk::{gdk, glib, prelude::*};

/// One window card: its client and an optional thumbnail.
pub struct Window<'a> {
    pub client: &'a Client,
    pub thumb: Option<gdk::Texture>,
}

/// Height reserved for the desktops strip; windows fill the rest.
const STRIP: f64 = (w::OVERVIEW_DESKTOP.1 + 2 * w::S6 + w::S4) as f64;

/// The whole overview at `size`. `on_pick` gets the window's focus or the desktop's switch.
pub fn content(
    size: (i32, i32),
    desktops: &[i64],
    current: i64,
    windows: Vec<Window>,
    on_pick: impl Fn(Dispatch) + Clone + 'static,
) -> (gtk::Widget, Vec<(String, gtk::Picture)>) {
    let root = gtk::Overlay::new();
    root.add_css_class("overview");
    root.set_child(Some(&super::lock::backdrop()));
    let col = w::vbox(0);
    root.add_overlay(&col);

    let strip = w::hbox(w::S4);
    strip.add_css_class("overview-desktops");
    strip.set_halign(gtk::Align::Center);
    for &id in desktops {
        let d = w::vbox(w::S1);
        let tile = gtk::Overlay::new();
        tile.add_css_class("overview-desktop");
        if id == current {
            tile.add_css_class("current");
        }
        // The wallpaper is an overlay so its natural size never widens the tile.
        let sizer = w::vbox(0);
        sizer.set_size_request(w::OVERVIEW_DESKTOP.0, w::OVERVIEW_DESKTOP.1);
        tile.set_child(Some(&sizer));
        tile.add_overlay(&super::lock::backdrop());
        tile.set_overflow(gtk::Overflow::Hidden);
        let b = gtk::Button::builder().child(&tile).build();
        b.add_css_class("overview-desktop-button");
        let pick = on_pick.clone();
        b.connect_clicked(move |_| pick(Dispatch::Workspace(id.to_string())));
        d.append(&b);
        let cap = w::centered(&format!("Desktop {id}"), "overview-caption");
        cap.set_halign(gtk::Align::Center);
        d.append(&cap);
        strip.append(&d);
    }
    col.append(&strip);

    let fixed = gtk::Fixed::new();
    fixed.set_vexpand(true);
    col.append(&fixed);
    let area = window_area(size);
    let sizes: Vec<(i32, i32)> = windows
        .iter()
        .map(|c| (c.client.size[0], c.client.size[1]))
        .collect();
    let mut pics = Vec::new();
    for (win, r) in windows
        .into_iter()
        .zip(overview::grid(&sizes, area, w::S8 as f64))
    {
        let card = w::vbox(w::S2);
        let frame = gtk::Overlay::new();
        frame.add_css_class("overview-window");
        frame.set_overflow(gtk::Overflow::Hidden);
        frame.set_size_request(r.w as i32, r.h as i32);
        let pic = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Fill)
            .can_shrink(true)
            .build();
        if let Some(t) = &win.thumb {
            pic.set_paintable(Some(t));
        }
        frame.set_child(Some(&pic));
        let icon_name = crate::apps::icon_for(&win.client.class, crate::apps::here(), &[]);
        let icon = w::app_icon(&icon_name, w::ICON_XL);
        icon.set_halign(gtk::Align::Center);
        icon.set_valign(gtk::Align::Center);
        frame.add_overlay(&icon);
        let b = gtk::Button::builder()
            .child(&frame)
            .tooltip_text(if win.client.title.is_empty() {
                &win.client.class
            } else {
                &win.client.title
            })
            .build();
        b.add_css_class("overview-window-button");
        let (pick, addr) = (on_pick.clone(), win.client.address.clone());
        b.connect_clicked(move |_| pick(Dispatch::FocusWindow(format!("address:{addr}"))));
        card.append(&b);
        let t = w::centered(
            &crate::clipboard::preview(&win.client.title),
            "overview-title",
        );
        t.add_css_class("may-ellipsize");
        t.set_ellipsize(gtk::pango::EllipsizeMode::End);
        t.set_max_width_chars(w::TILE_CHARS * 2);
        t.set_halign(gtk::Align::Center);
        card.append(&t);
        card.set_size_request(r.w as i32, -1);
        fixed.put(&card, r.x, r.y);
        pics.push((win.client.address.clone(), pic));
    }
    (root.upcast(), pics)
}

/// Where the window cards go on a screen of `size`: below the strip, inside the padding. Never
/// negative (a 0x0 size before the first allocation made every card's size request invalid).
fn window_area(size: (i32, i32)) -> Rect {
    let (sw, sh) = (f64::from(size.0), f64::from(size.1));
    let pad = w::S8 as f64 * 2.0;
    Rect {
        x: pad,
        y: 0.0,
        w: (sw - 2.0 * pad).max(0.0),
        h: (sh - STRIP - pad - w::OVERVIEW_TITLE as f64 - w::S6 as f64).max(0.0),
    }
}

/// On demand, not exclusive: with an exclusive layer mapped the compositor refuses `focuswindow`, so a
/// card click only closed Overview (audit2 A2-3; the switcher learnt it live, `ui::switcher::SPEC`).
pub const SPEC: Spec = Spec::panel("wol-panel-overview", [true; 4], Keyboard::OnDemand);

/// What a pick does, in order: the dispatch first (focus works while the layer is mapped on
/// demand), then Overview hides; the compositor's refocus on unmap then lands on the picked window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Dispatch(Dispatch),
    Hide,
}

/// The steps for `what` (a window's focus or a desktop's switch).
pub fn pick(what: Dispatch) -> [Step; 2] {
    [Step::Dispatch(what), Step::Hide]
}

/// Clients and the active desktop (`.socket.sock`; run off the UI thread).
fn read_state() -> (Vec<Client>, i64) {
    let clients = crate::hypr::clients();
    let current = crate::hypr::active_workspace().unwrap_or(1);
    (clients, current)
}

/// The layer surface; rebuilt from compositor state on every show.
pub fn build(app: &gtk::Application) -> gtk::Window {
    let win = super::window(app, "panel", SPEC);
    crate::kit::surface::survive_output_loss(&win, SPEC);
    win.add_css_class("overview-window-surface");
    // Fades in on the window's clock each open (`ease`, 400 ms: a large surface); Esc fades it
    // out. A pick hides at once: the focus goes to the window picked on the unmap.
    super::transient::fade_on_map(win.upcast_ref(), crate::kit::motion::Duration::Slow);
    let w2 = win.clone();
    crate::kit::surface::on_escape(&win, move || super::transient::dismiss(&w2));
    // Which show a compositor answer belongs to (a quick close and reopen must not fill twice).
    let shows = std::rc::Rc::new(std::cell::Cell::new(0u32));
    win.connect_show(move |win| {
        // The backdrop at once (never the last open's cards), the windows once the compositor answered.
        win.set_child(Some(
            &content(screen(win.upcast_ref()), &[], 1, vec![], |_| {}).0,
        ));
        let show = shows.get().wrapping_add(1);
        shows.set(show);
        let (win, shows) = (win.clone(), shows.clone());
        glib::spawn_future_local(async move {
            let Ok((clients, current)) = gtk::gio::spawn_blocking(read_state).await else {
                return;
            };
            if win.is_visible() && shows.get() == show {
                fill(win.upcast_ref(), &clients, current);
            }
        });
    });
    win.upcast()
}

/// Screen size for the grid: the display the surface is on, else the first display (audit2 A2-2:
/// the first open after login had no output yet), else the window's own size.
fn screen(win: &gtk::Window) -> (i32, i32) {
    crate::kit::surface::screen_of(win).unwrap_or((win.width(), win.height()))
}

/// The desktops strip and the current desktop's windows, then their live thumbnails.
fn fill(win: &gtk::Window, clients: &[Client], current: i64) {
    let here: Vec<Window> = clients
        .iter()
        .filter(|c| c.workspace.id == current)
        .map(|client| Window {
            client,
            thumb: None,
        })
        .collect();
    let w2 = win.clone();
    // One pick per open (audit3 R2A-3): a second click during the first's dispatch would send
    // a second dispatch and a second hide.
    let picking = std::rc::Rc::new(std::cell::Cell::new(false));
    let (child, pics) = content(
        screen(win),
        &overview::desktops(clients, current),
        current,
        here,
        move |d| {
            if picking.replace(true) {
                return;
            }
            let w2 = w2.clone();
            glib::spawn_future_local(async move {
                for step in pick(d) {
                    match step {
                        Step::Dispatch(req) => {
                            let _ =
                                gtk::gio::spawn_blocking(move || crate::hypr::dispatch(&req)).await;
                        }
                        Step::Hide => w2.set_visible(false),
                    }
                }
            });
        },
    );
    win.set_child(Some(&child));
    // Each window's own contents (covered or not) via hyprland_toplevel_export_v1.
    for (addr, pic) in pics {
        glib::spawn_future_local(async move {
            let out = gtk::gio::spawn_blocking(move || {
                std::process::Command::new("wol-window-shot")
                    .arg(&addr)
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| o.stdout)
            })
            .await;
            let Ok(Some(bytes)) = out else { return };
            if let Some(t) = texture(&bytes) {
                pic.set_paintable(Some(&t));
                if let Some(icon) = pic.next_sibling() {
                    icon.set_visible(false);
                }
            }
        });
    }
}

/// `wol-window-shot` output -> texture.
fn texture(bytes: &[u8]) -> Option<gdk::MemoryTexture> {
    use crate::winshot::{Layout, decode, layout};
    let (f, px) = decode(bytes)?;
    let format = match layout(f.format)? {
        Layout::Bgra => gdk::MemoryFormat::B8g8r8a8,
        Layout::Bgrx => gdk::MemoryFormat::B8g8r8x8,
        Layout::Rgba => gdk::MemoryFormat::R8g8b8a8,
        Layout::Rgbx => gdk::MemoryFormat::R8g8b8x8,
    };
    Some(gdk::MemoryTexture::new(
        i32::try_from(f.width).ok()?,
        i32::try_from(f.height).ok()?,
        format,
        // In usize: the u32 product of another process's header overflowed (audit3 R2A-20);
        // `decode` checked the length in u64.
        &glib::Bytes::from(px.get(..f.stride as usize * f.height as usize)?),
        f.stride as usize,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Audit2 A2-3: an exclusive Overview made the compositor refuse the card's `focuswindow`; the
    /// dispatch also ran before the unmap was flushed. On demand, focus first, then hide.
    #[test]
    fn overview_takes_the_keyboard_on_demand_and_focuses_before_hiding() {
        assert_eq!(SPEC.keyboard, Keyboard::OnDemand);
        assert_eq!(SPEC.namespace, "wol-panel-overview");
        let focus = Dispatch::FocusWindow("address:0x1".into());
        assert_eq!(pick(focus.clone()), [Step::Dispatch(focus), Step::Hide]);
    }

    /// Audit2 A2-2: the first open after login had a 0x0 surface on no output, so the card area
    /// was negative and no card showed (the display choice: `kit::surface::pick_screen`).
    #[test]
    fn the_first_open_sizes_from_a_display_and_never_goes_negative() {
        let a = window_area((0, 0));
        assert!(a.w >= 0.0 && a.h >= 0.0, "{a:?}");
        let a = window_area((1920, 1080));
        assert!(a.w > 0.0 && a.h > 0.0, "{a:?}");
    }
}
