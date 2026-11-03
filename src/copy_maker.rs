// SPDX-License-Identifier: GPL-2.0-or-later
//! Plugin Maker's sentences (ADR 0031 "Plugin Maker / Theme Studio", docs/modules/plugin-maker.md):
//! the editor, the sandboxed preview, the Theme Studio and the share sheet hold none of their own.
//! Invariant: every sentence in [`ALL`] passes `copy::lint` (tested below); [`publish_steps`] is
//! instructions with a command in it, shown in a sheet, so it is not an error sentence.

// ---- Opening and creating ----

/// The start page, before a plugin folder is open.
pub const START: &str = "Open a plugin folder to edit it, or create a new plugin from a template.";
/// The folder chosen has no manifest.
pub const NOT_A_PLUGIN: &str =
    "That folder has no plugin in it. Choose a folder with a plugin, or create one.";
/// A file of the plugin could not be read as text.
pub const NOT_READ: &str = "That plugin couldn’t be opened. Choose another folder.";
/// The id typed for a new plugin is taken in this account's plugin folder.
pub const EXISTS: &str = "A plugin with that id exists already. Pick another id.";
/// The new plugin's files could not be written.
pub const NOT_CREATED: &str =
    "The plugin couldn’t be created. Check that your home folder has room, then try again.";
/// A new plugin was made from the template.
pub const CREATED: &str = "Your plugin is ready. Edit it here and watch the preview.";
/// Under the id field of a new plugin.
pub const NEW_ABOUT: &str =
    "Use your name and the plugin’s, like alice.weather. Pick what the plugin adds below.";

// ---- Editor ----

/// Saved, and the check and the preview follow.
pub const SAVED: &str = "Saved. The preview shows the new version.";
/// The file could not be written.
pub const NOT_SAVED: &str =
    "The file couldn’t be saved. Check that the folder is yours, then try again.";
/// The check found nothing.
pub const NO_PROBLEMS: &str = "No problems found. The plugin loads as it is.";
/// Unsaved edits are not checked yet.
pub const UNSAVED: &str = "Save to check your changes and see them in the preview.";

// ---- Preview ----

/// Over the preview: what it is.
pub const PREVIEW_ABOUT: &str = "This preview is a sandbox. Nothing here changes your desktop.";
/// The plugin does not load, so there is nothing to draw.
pub const PREVIEW_FIX_FIRST: &str = "Fix the first problem in the list to see the preview.";
/// The plugin adds nothing the preview draws.
pub const PREVIEW_EMPTY: &str =
    "This plugin adds nothing to the menu bar, the Dock, the desktop or Control Center.";
/// `wol.run` in the preview.
pub const PREVIEW_NO_RUN: &str =
    "Commands don’t run in the preview. The desktop runs them once the plugin is on.";
/// `wol.http_get` in the preview.
pub const PREVIEW_NO_WEB: &str = "Nothing is read from the web in the preview.";

// ---- Package and share ----

/// The package was written next to the folder.
pub const PACKED: &str = "The package is ready. Share it to send it to the Store.";
/// The package could not be made (a problem, or the packer is missing).
pub const NOT_PACKED: &str = "The package couldn’t be made. Press Check and fix what it lists.";
/// Create Package while the editor has unsaved edits.
pub const SAVE_FIRST: &str = "Save your changes first. The package is made from the saved files.";
/// Share before a package exists.
pub const PACK_FIRST: &str = "Create the package first. Sharing needs its checksum.";
/// Top of the share sheet.
pub const SHARE_ABOUT: &str =
    "The Store lists a plugin once its request is reviewed. Nothing is sent from here.";
/// The share steps are on the clipboard.
pub const SHARE_COPIED: &str = "The steps are copied. Paste them into a terminal or a note.";

// ---- Theme Studio ----

/// Under the sliders.
pub const THEME_ABOUT: &str =
    "Move lightness and color for each group. The checks show what stays readable.";
/// Every contrast rule passes.
pub const THEME_READABLE: &str = "Every text color reads on its background.";
/// Some contrast rule fails.
pub const THEME_UNREADABLE: &str =
    "Some text is hard to read. Move the sliders until every check passes.";
/// Saved under the account's themes.
pub const THEME_SAVED: &str = "Saved to your themes. Choose it in Appearance settings.";
/// The theme could not be written.
pub const THEME_NOT_SAVED: &str =
    "The theme couldn’t be saved. Check that your home folder has room, then try again.";
/// A file that is not a theme.
pub const THEME_NOT_READ: &str = "That file isn’t a theme WolOS can read. Choose another one.";
/// The desktop picture could not be decoded.
pub const THEME_NO_PICTURE: &str =
    "The desktop picture couldn’t be read. Choose another wallpaper first.";
/// A theme that still fails the contrast rules is not saved.
pub const THEME_FIX_FIRST: &str =
    "Make every check pass before saving. Unreadable themes aren’t saved.";

// ---- Labels and titles (not sentences: not in ALL) ----

/// The window title and the preview's app name.
pub const APP_NAME: &str = "Plugin Maker";
/// Page titles in the sidebar, and its sections.
pub const EDITOR_PAGE: &str = "Editor";
pub const PREVIEW_PAGE: &str = "Preview";
pub const STUDIO_PAGE: &str = "Theme Studio";
pub const PLUGIN_SECTION: &str = "Plugin";
pub const THEME_SECTION: &str = "Theme";
/// The toolbar, on while a plugin is open.
pub const SAVE: &str = "Save";
pub const CHECK: &str = "Check";
pub const CREATE_PACKAGE: &str = "Create Package";
pub const SHARE: &str = "Share…";
/// The share sheet.
pub const SHARE_TITLE: &str = "Share";
pub const COPY_STEPS: &str = "Copy Steps";
pub const DONE: &str = "Done";
/// The start page.
pub const PLUGIN_FOLDER: &str = "Plugin folder";
pub const OPEN_PLUGIN: &str = "Open Plugin…";
pub const PLUGIN_ID: &str = "Plugin id";
pub const NEW_PLUGIN: &str = "New Plugin";
pub const CREATE_PLUGIN: &str = "Create Plugin";
/// Create Plugin's kind rows, in `ui::maker::KINDS` order.
pub const KIND_MENUBAR: &str = "Menu bar item";
pub const KIND_WIDGET: &str = "Desktop card";
pub const KIND_DOCK: &str = "Dock entry";
pub const KIND_TILE: &str = "Control Center tile";
pub const KIND_PANEL: &str = "Panel";
pub const KIND_SEARCH: &str = "Search results";
pub const KIND_TOGGLE: &str = "Toggle";
pub const KIND_HOTKEY: &str = "Keyboard shortcut";
pub const KIND_WEBAPP: &str = "Web app";
pub const KIND_THEME: &str = "Theme";
/// The Open Plugin folder chooser's title.
pub const OPEN_FOLDER_TITLE: &str = "Open a Plugin Folder";
/// The Theme Studio's Open Theme chooser: its title and its file filter's name.
pub const OPEN_THEME_TITLE: &str = "Open a Theme";
pub const THEME_FILTER: &str = "Themes";

// ---- Entry points elsewhere ----

/// Settings > Appearance row subtitle.
pub const APPEARANCE_ROW: &str = "Change colors with sliders and keep all text readable.";

/// Every sentence above (for the lint test).
pub const ALL: [&str; 30] = [
    START,
    NOT_A_PLUGIN,
    NOT_READ,
    EXISTS,
    NOT_CREATED,
    CREATED,
    NEW_ABOUT,
    SAVED,
    NOT_SAVED,
    NO_PROBLEMS,
    UNSAVED,
    PREVIEW_ABOUT,
    PREVIEW_FIX_FIRST,
    PREVIEW_EMPTY,
    PREVIEW_NO_RUN,
    PREVIEW_NO_WEB,
    PACKED,
    NOT_PACKED,
    PACK_FIRST,
    SAVE_FIRST,
    SHARE_ABOUT,
    SHARE_COPIED,
    THEME_ABOUT,
    THEME_READABLE,
    THEME_UNREADABLE,
    THEME_SAVED,
    THEME_NOT_SAVED,
    THEME_NOT_READ,
    THEME_NO_PICTURE,
    THEME_FIX_FIRST,
];

/// The plugin index repository a share request goes to (ADR 0031: `wolos-plugins`).
pub const INDEX_REPO: &str = "wonderlandos/wolos-plugins";

/// The share sheet's steps: where the package goes, the index entry with its checksum, and the
/// one command that opens the request. Nothing here runs; the person copies it.
pub fn publish_steps(file: &str, id: &str, version: &str, entry: &str) -> String {
    format!(
        "1. Put {file} where anyone can download it, such as a release of your own repository, \
and set \"url\" below to that address.\n\
2. Add this entry to index.json in a fork of {INDEX_REPO}:\n\n{entry}\n\n\
3. Open the request from your fork:\n\n\
gh pr create --repo {INDEX_REPO} --title \"Add {id} {version}\" --body \"sha256 of {file} is in the entry.\"\n"
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_maker_sentence_passes_the_lint() {
        for s in super::ALL {
            let p = crate::copy::lint(s);
            assert!(p.is_empty(), "“{s}”: {p:?}");
        }
    }

    #[test]
    fn the_share_steps_name_the_file_the_entry_and_the_command() {
        let s = super::publish_steps("a.b-0.1.wolplug", "a.b", "0.1", "{\"id\": \"a.b\"}");
        assert!(s.contains("a.b-0.1.wolplug"));
        assert!(s.contains("{\"id\": \"a.b\"}"));
        assert!(s.contains("gh pr create --repo wonderlandos/wolos-plugins"));
        assert!(s.contains("Add a.b 0.1"));
    }
}
