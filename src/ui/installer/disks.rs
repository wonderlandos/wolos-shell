// SPDX-License-Identifier: GPL-2.0-or-later
//! The installer's disk pages: choosing the disk and the method, replacing, installing alongside, and
//! laying the disk out by hand.
use super::{EditUi, Installer, bytes_at, keep_at, manual_segments, percent_of, replace_child};
use crate::copy;
use crate::i18n::tr;
use crate::install;
use crate::install::flow::{self, DiskTitle, FILE_SYSTEMS, Method, Row, SWAPS, Step, USES, Use};
use crate::kit::assistant::{self as a, Art, Card, Choice, ChoiceList, Seg, Tone};
use crate::kit::widgets as w;
use gtk::{glib, prelude::*};
use std::rc::{Rc, Weak};

impl Installer {
    /// The disk page: the disks at once when known, else a look at them off the main thread.
    /// No disk yet: every visit looks again (the target may have been plugged in since).
    pub(super) fn enter_disks(self: &Rc<Self>) {
        if self.probed.get() && !self.flow.borrow().disks.is_empty() {
            return self.fill_disks();
        }
        replace_child(
            &self.disk_list,
            &a::waiting(crate::i18n::tr(copy::installer::LOOKING_AT_DISKS)),
        );
        let me = Rc::downgrade(self);
        let probe = self.probe.get();
        glib::spawn_future_local(async move {
            let found = gtk::gio::spawn_blocking(probe).await.unwrap_or_default();
            if let Some(s) = me.upgrade() {
                s.probed.set(true);
                s.flow.borrow_mut().set_disks(found);
                s.fill_disks();
            }
        });
    }

    pub(super) fn fill_disks(self: &Rc<Self>) {
        let (rows, selected): (Vec<Choice>, _) = {
            let f = self.flow.borrow();
            let rows = f
                .disks
                .iter()
                .map(|d| Choice {
                    icon: Some("drive-harddisk-symbolic".into()),
                    title: d.disk.title_text(),
                    detail: flow::disk_detail(&d.disk),
                    trail: None,
                })
                .collect();
            (rows, f.chosen)
        };
        if rows.is_empty() {
            let me = Rc::downgrade(self);
            let again = w::button(tr("Look Again"), w::Kind::Default, move || {
                if let Some(s) = me.upgrade() {
                    s.probed.set(false);
                    s.enter_disks();
                }
            });
            replace_child(
                &self.disk_list,
                &w::empty_state(
                    "drive-harddisk-symbolic",
                    copy::installer::no_disk().0,
                    copy::installer::no_disk().1,
                    Some(again),
                ),
            );
            return;
        }
        let me = Rc::downgrade(self);
        let list = ChoiceList::new(rows, selected, move |i| {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().pick_disk(i);
            }
        });
        replace_child(&self.disk_list, &list.root);
    }

    pub(super) fn method(self: &Rc<Self>) -> Card {
        let body = w::vbox(w::S4);
        body.append(&self.method_list);
        let names: Vec<&str> = SWAPS.iter().map(|s| tr(s.1)).collect();
        let swap = w::dropdown(&names, 0);
        let me = Rc::downgrade(self);
        swap.connect_selected_notify(move |dd| {
            if let Some(s) = me.upgrade() {
                let i = (dd.selected() as usize).min(SWAPS.len() - 1);
                s.flow.borrow_mut().swap = SWAPS[i].0;
            }
        });
        self.swap_row.append(&a::form_row(tr("Swap"), &swap));
        *self.swap.borrow_mut() = Some(swap);
        body.append(&self.swap_row);
        body.append(&self.method_warn);
        Card::new(
            Art::Icon("drive-multidisk-symbolic"),
            Step::Method.title(),
            Step::Method.subtitle(),
            &body,
        )
    }

    /// The methods in the order the page lists them.
    const METHODS: [Method; 4] = [
        Method::Alongside,
        Method::Replace,
        Method::Erase,
        Method::Manual,
    ];

    pub(super) fn fill_method(self: &Rc<Self>) {
        let (rows, selected, swap): (Vec<Choice>, _, _) = {
            let f = self.flow.borrow();
            let row = |title: String, detail: &str| Choice {
                icon: None,
                title,
                detail: detail.into(),
                trail: None,
            };
            (
                vec![
                    row(f.alongside_title(), tr("Keeps what’s there")),
                    row(
                        Step::Replace.title().into(),
                        tr("Erases one partition, keeps the rest"),
                    ),
                    row(tr("Erase Disk").into(), tr("Starts clean")),
                    row(tr("Manual").into(), tr("For experts")),
                ],
                Self::METHODS.iter().position(|m| *m == f.method),
                SWAPS
                    .iter()
                    .position(|s| std::mem::discriminant(&s.0) == std::mem::discriminant(&f.swap))
                    .unwrap_or(0),
            )
        };
        let me = Rc::downgrade(self);
        let list = ChoiceList::new(rows, selected, move |i| {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().method = Self::METHODS[i];
                s.say_method();
            }
        });
        replace_child(&self.method_list, &list.root);
        if let Some(dd) = &*self.swap.borrow() {
            dd.set_selected(swap as u32);
        }
        self.say_method();
    }

    pub(super) fn say_method(&self) {
        let (warn, manual) = {
            let f = self.flow.borrow();
            (f.method_warning(), f.method == Method::Manual)
        };
        self.method_warn.set_visible(warn.is_some());
        self.method_warning
            .set_label(warn.as_deref().unwrap_or_default());
        // Manual has its own Swap use on the edit page.
        self.swap_row.set_visible(!manual);
    }

    pub(super) fn replace(self: &Rc<Self>) -> Card {
        let body = w::vbox(w::S2);
        body.append(&self.replace_list);
        Card::new(
            Art::Icon("drive-harddisk-symbolic"),
            Step::Replace.title(),
            Step::Replace.subtitle(),
            &body,
        )
    }

    pub(super) fn fill_replace(self: &Rc<Self>) {
        let (cands, chosen) = {
            let f = self.flow.borrow();
            (f.replace_candidates(), f.replace.clone())
        };
        let selected = chosen.and_then(|u| cands.iter().position(|c| c.0 == u));
        let rows = cands
            .iter()
            .map(|(_, title, detail)| Choice {
                icon: None,
                title: title.clone(),
                detail: detail.clone(),
                trail: None,
            })
            .collect();
        let uuids: Vec<String> = cands.iter().map(|c| c.0.clone()).collect();
        let me = Rc::downgrade(self);
        let list = ChoiceList::new(rows, selected, move |i| {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().replace = Some(uuids[i].clone());
            }
        });
        replace_child(&self.replace_list, &list.root);
    }

    pub(super) fn alongside(self: &Rc<Self>) -> Card {
        let body = &self.along_box;
        body.append(&self.along_bar.root);
        let me = Rc::downgrade(self);
        let slider = w::row_slider(50.0, move |v| {
            if let Some(s) = me.upgrade() {
                s.slide(v);
            }
        });
        body.append(&slider);
        *self.along_slider.borrow_mut() = Some(slider);
        body.append(&self.along_warn);
        Card::new(
            Art::Icon("drive-multidisk-symbolic"),
            Step::Alongside.title(),
            Step::Alongside.subtitle(),
            body,
        )
    }

    /// The slider runs from the most the other system can keep, on the left, to the least.
    pub(super) fn slide(self: &Rc<Self>, v: f64) {
        if self.filling.get() {
            return;
        }
        let range = self.flow.borrow().alongside_slider();
        if let Some((_, _, lo, hi)) = range {
            self.flow.borrow_mut().keep = keep_at(lo, hi, v);
            self.fill_along_bar();
        }
    }

    pub(super) fn fill_alongside(self: &Rc<Self>) {
        let f = self.flow.borrow();
        if let (Some((_, _, lo, hi)), Some(sl)) =
            (f.alongside_slider(), &*self.along_slider.borrow())
        {
            self.filling.set(true);
            sl.set_value((hi - f.keep.clamp(lo, hi)) as f64 * 100.0 / (hi - lo).max(1) as f64);
            self.filling.set(false);
        }
        drop(f);
        self.fill_along_bar();
    }

    pub(super) fn fill_along_bar(&self) {
        let f = self.flow.borrow();
        let Some((_, name, ..)) = f.alongside_slider() else {
            return;
        };
        let wolos = f.alongside_room().unwrap_or(0);
        self.along_bar.set(&[
            Seg::new(Tone::Other, &name, f.keep),
            Seg::new(Tone::Wolos, "WolOS", wolos),
        ]);
        let low = wolos < wol_core::install::layout::ROOMY_ROOT;
        self.along_warn.set_visible(low);
        self.along_warning
            .set_label(&copy::install_low_space(wolos));
    }

    pub(super) fn manual(self: &Rc<Self>) -> Card {
        let body = w::vbox(w::S2);
        body.append(&self.manual_bar.root);
        body.append(&self.manual_list);
        Card::new(
            Art::Icon("drive-harddisk-symbolic"),
            Step::Manual.title(),
            Step::Manual.subtitle(),
            &body,
        )
    }

    pub(super) fn fill_manual(self: &Rc<Self>) {
        let f = self.flow.borrow();
        let Some(d) = f.disk().cloned() else {
            return;
        };
        let lines = flow::manual_lines(&f.manual, &d);
        self.manual_bar.set(&manual_segments(&lines, &d, &f));
        drop(f);
        let rows = lines
            .iter()
            .map(|(_, title, detail)| Choice {
                icon: None,
                title: title.clone(),
                detail: detail.clone(),
                trail: None,
            })
            .collect();
        let me = Rc::downgrade(self);
        let list = ChoiceList::new(rows, None, move |i| {
            if let Some(s) = me.upgrade() {
                let n = s.flow.borrow_mut().open_edit(&lines[i].0);
                s.apply(Step::Manual, n);
            }
        });
        replace_child(&self.manual_list, &list.root);
    }

    pub(super) fn edit_page(self: &Rc<Self>) -> Card {
        let body = w::vbox(w::S2);
        let on_flow = |me: Weak<Installer>, f: fn(&mut flow::Edit, &Installer)| {
            move || {
                if let Some(s) = me.upgrade() {
                    if s.filling.get() {
                        return;
                    }
                    if let Some(e) = s.flow.borrow_mut().edit.as_mut() {
                        f(e, &s);
                    }
                    s.show_edit_rows();
                }
            }
        };
        let names: Vec<&str> = USES.iter().map(|u| tr(u.1)).collect();
        let usage = w::dropdown(&names, 0);
        let go = on_flow(Rc::downgrade(self), |e, s| {
            if let Some(ui) = &*s.edit.borrow() {
                e.usage = USES[(ui.usage.selected() as usize).min(USES.len() - 1)].0;
            }
        });
        usage.connect_selected_notify(move |_| go());
        body.append(&a::form_row(tr("Use for"), &usage));
        let folder = w::text_field(tr("Folder name"));
        let go = on_flow(Rc::downgrade(self), |e, s| {
            if let Some(ui) = &*s.edit.borrow() {
                e.folder = ui.folder.text().to_string();
            }
        });
        folder.connect_changed(move |_| go());
        let folder_row = a::form_row(tr("Folder"), &folder);
        body.append(&folder_row);
        let go = on_flow(Rc::downgrade(self), |e, s| {
            if let Some(ui) = &*s.edit.borrow() {
                e.erase = ui.erase.is_active();
            }
        });
        let erase = w::toggle(false, move |_| go());
        erase.set_halign(gtk::Align::Start);
        let erase_row = a::form_row(tr("Erase"), &erase);
        body.append(&erase_row);
        let fs_names: Vec<&str> = FILE_SYSTEMS.iter().map(|f| tr(f.1)).collect();
        let fs = w::dropdown(&fs_names, 0);
        let go = on_flow(Rc::downgrade(self), |e, s| {
            if let Some(ui) = &*s.edit.borrow() {
                e.fs = FILE_SYSTEMS[(ui.fs.selected() as usize).min(FILE_SYSTEMS.len() - 1)].0;
            }
        });
        fs.connect_selected_notify(move |_| go());
        let fs_row = a::form_row(tr("File system"), &fs);
        body.append(&fs_row);
        let me = Rc::downgrade(self);
        let size = w::row_slider(100.0, move |v| {
            if let Some(s) = me.upgrade() {
                s.edit_size(v);
            }
        });
        let size_text = w::label("", "assistant-subtitle");
        let row = w::hbox(w::S2);
        row.append(&size);
        row.append(&size_text);
        let size_row = a::form_row(tr("Size"), &row);
        body.append(&size_row);
        let c = Card::new(
            Art::Icon("drive-harddisk-symbolic"),
            Step::Edit.title(),
            Step::Edit.subtitle(),
            &body,
        );
        let me = Rc::downgrade(self);
        let delete = w::button(tr("Delete"), w::Kind::Default, move || {
            if let Some(s) = me.upgrade() {
                s.flow.borrow_mut().delete_edited();
                let st = s.flow.borrow().step;
                s.show(st);
            }
        });
        c.add_action(&delete);
        c.next.set_label(tr("Done"));
        *self.edit.borrow_mut() = Some(EditUi {
            usage,
            folder_row,
            folder,
            erase_row,
            erase,
            fs_row,
            fs,
            size_row,
            size,
            size_text,
            delete,
        });
        c
    }

    pub(super) fn edit_size(&self, v: f64) {
        if self.filling.get() {
            return;
        }
        let mut f = self.flow.borrow_mut();
        if let Some(e) = f.edit.as_mut() {
            e.bytes = bytes_at(e.min, e.max, v);
            if let Some(ui) = &*self.edit.borrow() {
                ui.size_text.set_label(&install::size_text(e.bytes));
            }
        }
    }

    /// Which rows the page shows for what it's set to.
    pub(super) fn show_edit_rows(&self) {
        let f = self.flow.borrow();
        let (Some(e), Some(ui)) = (f.edit.as_ref(), &*self.edit.borrow()) else {
            return;
        };
        let existing = matches!(e.row, Row::Part(_));
        let plain = matches!(e.usage, Use::System | Use::Home | Use::Folder);
        ui.folder_row.set_visible(e.usage == Use::Folder);
        ui.erase_row
            .set_visible(existing && e.usage != Use::Nothing);
        ui.fs_row.set_visible(plain && (e.erase || !existing));
        ui.size_row.set_visible(e.min < e.max);
        ui.delete.set_visible(!matches!(e.row, Row::Free(_)));
    }

    pub(super) fn fill_edit(&self) {
        {
            let f = self.flow.borrow();
            let (Some(e), Some(ui)) = (f.edit.as_ref(), &*self.edit.borrow()) else {
                return;
            };
            self.filling.set(true);
            let at = USES.iter().position(|u| u.0 == e.usage).unwrap_or(0);
            ui.usage.set_selected(at as u32);
            ui.folder.set_text(&e.folder);
            ui.erase.set_active(e.erase);
            let fs = FILE_SYSTEMS.iter().position(|x| x.0 == e.fs).unwrap_or(0);
            ui.fs.set_selected(fs as u32);
            ui.size.set_value(percent_of(e.min, e.max, e.bytes));
            ui.size_text.set_label(&install::size_text(e.bytes));
            self.filling.set(false);
            self.with_card(Step::Edit, |c| {
                c.subtitle
                    .set_label(&format!("{}. {}", e.title, Step::Edit.subtitle()))
            });
        }
        self.show_edit_rows();
    }
}
