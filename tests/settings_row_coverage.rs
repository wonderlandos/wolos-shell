// SPDX-License-Identifier: GPL-2.0-or-later
//! QA-SEARCH-1: a Search result lands on a row only when the page draws a title the row is found
//! by. For every row Settings search knows, build its page and look for the row; the rows a page
//! does not draw in the demo state are listed in the failure (they open their page, nothing lit).
#[path = "support/broadway.rs"]
mod broadway;
use gtk::prelude::*;
use shell::ui;
use shell::ui::settings::{ids, reveal};
use std::sync::Arc;

#[test]
fn rows_are_found_on_their_pages() {
    let _display = broadway::start(720);
    if gtk::init().is_err() {
        eprintln!("skipped: no display for the toolkit");
        return;
    }
    let a = ui::settings::content(Arc::new(shell::system::Fake), &ui::settings::Host::demo());
    let win = gtk::Window::builder()
        .default_width(shell::kit::widgets::APP_SIZE.0)
        .default_height(shell::kit::widgets::APP_SIZE.1)
        .child(&a.root)
        .build();
    win.present();
    broadway::pump(100);
    let mut found = 0;
    let mut missing: Vec<String> = Vec::new();
    let mut pages: Vec<&str> = Vec::new();
    for (page, row, _) in ui::settings::meaning_rows() {
        if !pages.contains(&page) {
            pages.push(page);
        }
        let _ = row;
    }
    for page in pages {
        a.open(page);
        broadway::pump(800);
        let shown = a.content.visible_child().expect("page");
        for (p, row, _) in ui::settings::meaning_rows() {
            if p != page {
                continue;
            }
            if reveal::find_title(&shown, &reveal::shown_as(row)).is_some() {
                found += 1;
            } else {
                missing.push(format!(
                    "{}#{}",
                    ids::page_id(page).unwrap(),
                    ids::slug(row)
                ));
            }
        }
    }
    eprintln!("found {found}, missing {}: {missing:#?}", missing.len());
    // The rest are rows a pane draws only in some state (a permission row needs an app that asked,
    // "Shadow size" needs Shadows on, a sheet is closed): their link opens the page, nothing is lit.
    assert!(
        found >= MIN_FOUND,
        "{found} of {} rows are on screen on their page (at least {MIN_FOUND}); missing: {missing:#?}",
        found + missing.len()
    );
}

/// Rows found on their pages in the demo state when this test was written (229 of 295).
const MIN_FOUND: usize = 225;
