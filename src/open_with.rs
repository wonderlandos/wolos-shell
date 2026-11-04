// SPDX-License-Identifier: GPL-2.0-or-later
//! STAND-IN (E2E-SHARE): the three functions of E2E-OPEN's `open_with` module that the Open With
//! sheet calls, with the same signatures, so `app_session` is written against the final API.
//! E2E-OPEN's real module (wt/e2e-open, crates/shell/src/open_with.rs) replaces this file when
//! INT-2 merges it: take theirs, keep `crate::open_with::{added_for, promote, remember}`.
//! What it does: the apps Open With launched for a kind of file are kept in the person's
//! `[Added Associations]` of `~/.config/mimeapps.list` (the default app is not touched), and the
//! next chooser for that kind lists them first.
use std::path::Path;

const FILE: &str = "mimeapps.list";
const ADDED: &str = "[Added Associations]";

/// The apps named for `mime` in the Added Associations of `text`, in order.
pub fn added_in(text: &str, mime: &str) -> Vec<String> {
    let mut inside = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line == ADDED;
        } else if inside
            && let Some((k, v)) = line.split_once('=')
            && k.trim() == mime
        {
            return v
                .split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
        }
    }
    Vec::new()
}

/// `text` with `app_id` first among the Added Associations of `mime`; None when it already is.
pub fn remembered(text: &str, mime: &str, app_id: &str) -> Option<String> {
    let mut apps = added_in(text, mime);
    if apps.first().map(String::as_str) == Some(app_id) {
        return None;
    }
    apps.retain(|a| a != app_id);
    apps.insert(0, app_id.to_owned());
    let row = format!("{mime}={};", apps.join(";"));
    let mut out = Vec::new();
    let (mut inside, mut done, mut seen) = (false, false, false);
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            if inside && !done {
                out.push(row.clone());
                done = true;
            }
            inside = t == ADDED;
            seen |= inside;
        } else if inside && t.split_once('=').is_some_and(|(k, _)| k.trim() == mime) {
            if !done {
                out.push(row.clone());
                done = true;
            }
            continue;
        }
        out.push(line.to_owned());
    }
    if !done {
        if !seen {
            out.push(ADDED.to_owned());
        }
        out.push(row);
    }
    Some(out.join("\n") + "\n")
}

fn user_file() -> std::path::PathBuf {
    Path::new(&crate::kit::config_home()).join(FILE)
}

/// The apps the person's file offers first for the kind of `path`, in order. Reads off the GTK
/// thread.
pub async fn added_for(path: &Path) -> Vec<String> {
    let (mime, _) = gtk::gio::content_type_guess(Some(path), None);
    let mime = mime.to_string();
    gtk::gio::spawn_blocking(move || {
        added_in(
            &std::fs::read_to_string(user_file()).unwrap_or_default(),
            &mime,
        )
    })
    .await
    .unwrap_or_default()
}

/// `items` with those whose id is in `first` moved to the front, in the order of `first`; the
/// rest keep their order.
pub fn promote<T>(items: Vec<T>, id_of: impl Fn(&T) -> String, first: &[String]) -> Vec<T> {
    let mut items = items;
    items.sort_by_key(|t| {
        let id = id_of(t);
        first.iter().position(|f| *f == id).unwrap_or(usize::MAX)
    });
    items
}

/// Open With picked `app_id` for `path`: the app comes first in the next chooser for files of
/// that kind. Quiet and best effort, on a thread of its own.
pub fn remember(path: &Path, app_id: &str) {
    let (mime, _) = gtk::gio::content_type_guess(Some(path), None);
    let (mime, app_id) = (mime.to_string(), app_id.to_owned());
    let _ = std::thread::Builder::new()
        .name("wol-open-with".into())
        .spawn(move || {
            let file = user_file();
            let now = std::fs::read_to_string(&file).unwrap_or_default();
            if let Some(next) = remembered(&now, &mime, &app_id) {
                let tmp = file.with_extension("new");
                if std::fs::write(&tmp, next).is_ok() {
                    let _ = std::fs::rename(&tmp, &file);
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chosen_app_comes_first_for_its_kind_and_nothing_else_moves() {
        let none = "[Default Applications]\nimage/png=a.desktop\n";
        let one = remembered(none, "image/png", "b.desktop").unwrap();
        assert_eq!(
            one,
            "[Default Applications]\nimage/png=a.desktop\n[Added Associations]\nimage/png=b.desktop;\n"
        );
        let two = remembered(&one, "image/png", "c.desktop").unwrap();
        assert_eq!(added_in(&two, "image/png"), ["c.desktop", "b.desktop"]);
        let again = remembered(&two, "image/png", "b.desktop").unwrap();
        assert_eq!(added_in(&again, "image/png"), ["b.desktop", "c.desktop"]);
        assert!(remembered(&again, "image/png", "b.desktop").is_none());
        // Another kind, in a file with the section in the middle: the rest is kept.
        let mid = "[Added Associations]\ntext/plain=t.desktop;\n[Default Applications]\nimage/png=a.desktop\n";
        let r = remembered(mid, "image/png", "v.desktop").unwrap();
        assert_eq!(
            r,
            "[Added Associations]\ntext/plain=t.desktop;\nimage/png=v.desktop;\n[Default Applications]\nimage/png=a.desktop\n"
        );
        assert_eq!(added_in(&r, "text/plain"), ["t.desktop"]);
    }

    #[test]
    fn remembered_apps_go_to_the_front_and_the_rest_keep_their_order() {
        let ids = ["x", "y", "z", "w"].map(String::from).to_vec();
        let first = vec!["z".to_owned(), "w".to_owned()];
        assert_eq!(promote(ids, |s| s.clone(), &first), ["z", "w", "x", "y"]);
    }
}
