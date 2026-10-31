// SPDX-License-Identifier: GPL-2.0-or-later
//! The drop shelf: files dropped on the desktop wait here until they are dragged somewhere else.
//! Paths only: nothing is copied or moved, so the shelf never touches a file.
use std::path::{Path, PathBuf};

/// More than this and the oldest drop goes first.
pub const MAX: usize = 50;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Shelf {
    items: Vec<PathBuf>,
}

impl Shelf {
    pub fn items(&self) -> &[PathBuf] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Adds `paths` in order; a path already there moves to the end. Returns how many were new.
    pub fn add(&mut self, paths: impl IntoIterator<Item = PathBuf>) -> usize {
        let mut new = 0;
        for p in paths {
            match self.items.iter().position(|q| *q == p) {
                Some(i) => {
                    let p = self.items.remove(i);
                    self.items.push(p);
                }
                None => {
                    self.items.push(p);
                    new += 1;
                }
            }
        }
        let extra = self.items.len().saturating_sub(MAX);
        self.items.drain(..extra);
        new
    }

    pub fn remove(&mut self, p: &Path) -> bool {
        let n = self.items.len();
        self.items.retain(|q| q != p);
        self.items.len() != n
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}

/// "1 item", "3 items".
pub fn count(n: usize) -> String {
    if n == 1 {
        "1 item".into()
    } else {
        format!("{n} items")
    }
}

/// What a row shows: the file name (a path with no name shows whole).
pub fn name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn adding_keeps_order_and_moves_repeats_to_the_end() {
        let mut s = Shelf::default();
        assert_eq!(s.add([p("/a"), p("/b"), p("/c")]), 3);
        assert_eq!(s.add([p("/a")]), 0, "already on the shelf");
        assert_eq!(s.items(), [p("/b"), p("/c"), p("/a")]);
    }

    #[test]
    fn the_oldest_drops_go_when_full() {
        let mut s = Shelf::default();
        s.add((0..MAX + 3).map(|i| p(&format!("/f{i}"))));
        assert_eq!(s.items().len(), MAX);
        assert_eq!(s.items()[0], p("/f3"));
    }

    #[test]
    fn removing_and_clearing() {
        let mut s = Shelf::default();
        s.add([p("/a"), p("/b")]);
        assert!(s.remove(Path::new("/a")));
        assert!(!s.remove(Path::new("/a")));
        assert_eq!(s.items(), [p("/b")]);
        s.clear();
        assert!(s.is_empty());
    }

    #[test]
    fn words() {
        assert_eq!(count(1), "1 item");
        assert_eq!(count(0), "0 items");
        assert_eq!(name(Path::new("/home/a/Report.pdf")), "Report.pdf");
        assert_eq!(name(Path::new("/")), "/");
    }
}
