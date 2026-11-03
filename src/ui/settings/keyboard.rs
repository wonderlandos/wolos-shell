// SPDX-License-Identifier: GPL-2.0-or-later
//! Keyboard: key repeat, input sources (desktop prefs, applied live) and the way into Keyboard Shortcuts.
use super::*;

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let desk = &e.desk;
    let p = w::page("Keyboard");
    let status = status_label();
    let d = desk.get();
    // The sliders, so a failed save can put them back to the saved values (Desk::set_or).
    let (rate, delay) = (
        Rc::new(glib::WeakRef::<gtk::Scale>::new()),
        Rc::new(glib::WeakRef::<gtk::Scale>::new()),
    );
    let (d1, s1, d2, s2) = (desk.clone(), status.clone(), desk.clone(), status.clone());
    let (r1, r2) = (rate.clone(), delay.clone());
    let shortcuts = form::nav_row(
        "",
        "Keyboard Shortcuts",
        "Change or add shortcuts for WolOS",
    );
    let env = e.clone();
    shortcuts.connect_clicked(move |_| {
        env.shortcut_group.set(None);
        env.open(shortcuts::TITLE);
    });
    let rate_row = form::slider_row(
        "Key repeat rate",
        (5.0, 60.0, 5.0),
        d.repeat_rate.into(),
        ("Slow", "Fast"),
        move |v| {
            let (dk, r) = (d1.clone(), r1.clone());
            d1.set_or(
                &s1,
                |p| p.repeat_rate = v as u32,
                move || put_back(&r, dk.get().repeat_rate.into()),
            )
        },
    );
    // Negated so a shorter delay sits to the right, as on the Mac.
    let delay_row = form::slider_row(
        "Delay until repeat",
        (-1000.0, -150.0, 50.0),
        -f64::from(d.repeat_delay),
        ("Long", "Short"),
        move |v| {
            let (dk, r) = (d2.clone(), r2.clone());
            d2.set_or(
                &s2,
                |p| p.repeat_delay = (-v) as u32,
                move || put_back(&r, -f64::from(dk.get().repeat_delay)),
            )
        },
    );
    rate.set(control_in::<gtk::Scale>(&rate_row).as_ref());
    delay.set(control_in::<gtk::Scale>(&delay_row).as_ref());
    p.append(&w::section("", &[rate_row, delay_row, shortcuts.upcast()]));
    p.append(&sources(desk, &status));
    let field = w::text_field("Type here to test");
    field.set_hexpand(true);
    p.append(&w::section("", &[form::row("Try it", "", &field)]));
    p.append(&status);
    p
}

/// A slider back at the saved `value` after a failed save; the change it reports is the saved
/// value again, which Desk does not save.
fn put_back(slider: &glib::WeakRef<gtk::Scale>, value: f64) {
    if let Some(s) = slider.upgrade() {
        s.set_value(value);
    }
}

fn layout_name(code: &str) -> &str {
    prefs::LAYOUTS
        .iter()
        .find(|l| l.0 == code)
        .map_or(code, |l| l.1)
}

/// Input Sources: one row per layout (first = default), Add lists the others.
fn sources(desk: &Rc<Desk>, status: &gtk::Label) -> gtk::Widget {
    let holder = w::vbox(w::S2);
    fn fill(holder: &gtk::Box, desk: &Rc<Desk>, status: &gtk::Label) {
        while let Some(c) = holder.first_child() {
            holder.remove(&c);
        }
        let layouts = desk.get().layouts;
        let mut rows: Vec<gtk::Widget> = layouts
            .iter()
            .enumerate()
            .map(|(i, code)| {
                let (h, dk, st, c) = (holder.clone(), desk.clone(), status.clone(), code.clone());
                let remove = w::icon_button("list-remove-symbolic", "Remove", move || {
                    dk.set_or(&st, |p| p.layouts.retain(|x| *x != c), refill(&h, &dk, &st));
                    fill(&h, &dk, &st);
                });
                remove.set_sensitive(layouts.len() > 1);
                form::row(
                    layout_name(code),
                    if i == 0 { "Default" } else { "" },
                    &remove,
                )
            })
            .collect();
        let others: Vec<(&str, &str)> = prefs::LAYOUTS
            .iter()
            .filter(|l| !layouts.iter().any(|x| x == l.0))
            .copied()
            .collect();
        if !others.is_empty() {
            let (h, dk, st) = (holder.clone(), desk.clone(), status.clone());
            let add = w::button("Add Input Source…", w::Kind::Default, || {});
            add.connect_clicked(move |b| {
                let items: Vec<w::MenuItem> = others
                    .iter()
                    .map(|(code, name)| {
                        let (h, dk, st, code) = (h.clone(), dk.clone(), st.clone(), *code);
                        let act: Box<dyn Fn()> = Box::new(move || {
                            dk.set_or(
                                &st,
                                |p| p.layouts.push(code.to_string()),
                                refill(&h, &dk, &st),
                            );
                            fill(&h, &dk, &st);
                        });
                        (*name, act)
                    })
                    .collect();
                let m = w::menu(items);
                m.set_parent(b);
                m.popup();
            });
            rows.push(form::row("", "", &add));
        }
        holder.append(&w::section("Input Sources", &rows));
        if layouts.len() > 1 {
            holder.append(&w::footnote(
                "Press ⌃Space to switch to the next input source.",
            ));
        }
    }
    /// What fills the list again after a failed save (Desk read the file back), while it shows.
    fn refill(holder: &gtk::Box, desk: &Rc<Desk>, status: &gtk::Label) -> impl FnOnce() + 'static {
        let (h, dk, st) = (holder.downgrade(), desk.clone(), status.clone());
        move || {
            if let Some(h) = h.upgrade() {
                fill(&h, &dk, &st);
            }
        }
    }
    fill(&holder, desk, status);
    holder.upcast()
}
