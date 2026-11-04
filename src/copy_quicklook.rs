// SPDX-License-Identifier: GPL-2.0-or-later
//! Quick Look's sentences (docs/modules/quicklook.md): the preview window and the card shown when a
//! file has no preview hold none of their own. Invariant: every sentence in [`ALL`] passes
//! `copy::lint` (tested below).

/// The window's title before a file is shown.
pub const TITLE: &str = "Quick Look";

// ---- What a thing is (labels under the name) ----

pub const KIND_PICTURE: &str = "Picture";
pub const KIND_TEXT: &str = "Text document";
pub const KIND_AUDIO: &str = "Audio";
pub const KIND_VIDEO: &str = "Video";
pub const KIND_PDF: &str = "PDF document";
pub const KIND_FOLDER: &str = "Folder";
pub const KIND_OTHER: &str = "File";

// ---- Sentences ----

/// A file with no preview: the card shows what the system knows about it.
pub const NO_PREVIEW: &str = "There is no preview for this kind of file.";
/// The file could not be read.
pub const NOT_READ: &str =
    "This file couldn’t be opened. It may have moved, or you may not have access.";
/// A picture the loader could not decode.
pub const PICTURE_NOT_SHOWN: &str = "This picture couldn’t be shown.";
/// A picture too large to decode here.
pub const PICTURE_TOO_BIG: &str = "This picture is too large to show here.";
/// Audio or video the media player could not open.
pub const NOT_PLAYED: &str = "This file couldn’t be played here.";
/// A PDF whose first page could not be drawn (no page renderer, a locked or damaged file).
pub const PDF_NOT_SHOWN: &str = "The first page couldn’t be shown.";
/// Under a text file longer than what is read.
pub const TEXT_CUT: &str = "Showing the first 256 KB of this file.";
/// A folder with nothing in it.
pub const EMPTY_FOLDER: &str = "This folder is empty.";
/// A folder with more entries than are counted.
pub const FOLDER_PARTIAL: &str = "This folder is large, so the numbers are a little low.";

pub const ALL: [&str; 9] = [
    NO_PREVIEW,
    NOT_READ,
    PICTURE_NOT_SHOWN,
    PICTURE_TOO_BIG,
    NOT_PLAYED,
    PDF_NOT_SHOWN,
    TEXT_CUT,
    EMPTY_FOLDER,
    FOLDER_PARTIAL,
];

/// "1 item", "12 items".
pub fn items(n: usize) -> String {
    if n == 1 {
        "1 item".to_owned()
    } else {
        format!("{n} items")
    }
}

/// "3 folders, 9 files" (a count of nothing is left out).
pub fn folder_parts(folders: usize, files: usize) -> String {
    let plural = |n: usize, one: &str, many: &str| {
        if n == 1 {
            format!("1 {one}")
        } else {
            format!("{n} {many}")
        }
    };
    match (folders, files) {
        (0, f) => plural(f, "file", "files"),
        (d, 0) => plural(d, "folder", "folders"),
        (d, f) => format!(
            "{}, {}",
            plural(d, "folder", "folders"),
            plural(f, "file", "files")
        ),
    }
}

/// Under a folder's counts: how much the files directly inside hold.
pub fn folder_size(human: &str) -> String {
    format!("{human} in the files directly inside")
}

/// Tooltips of the audio player's button.
pub const PLAY: &str = "Play";
pub const PAUSE: &str = "Pause";

/// Rows of the information card.
pub const SIZE: &str = "Size";
pub const MODIFIED: &str = "Modified";

#[cfg(test)]
mod tests {
    #[test]
    fn every_quick_look_sentence_passes_the_lint() {
        for s in super::ALL {
            let p = crate::copy::lint(s);
            assert!(p.is_empty(), "“{s}”: {p:?}");
        }
    }

    #[test]
    fn counts_are_plain_words() {
        assert_eq!(super::items(1), "1 item");
        assert_eq!(super::items(12), "12 items");
        assert_eq!(super::folder_parts(0, 9), "9 files");
        assert_eq!(super::folder_parts(1, 0), "1 folder");
        assert_eq!(super::folder_parts(3, 1), "3 folders, 1 file");
    }
}
