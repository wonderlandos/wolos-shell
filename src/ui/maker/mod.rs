// SPDX-License-Identifier: GPL-2.0-or-later
//! Plugin Maker / Theme Studio (`wol-plugin-maker`, ADR 0031 "Plugin Maker / Theme Studio",
//! docs/modules/plugin-maker.md): where a person or an AI agent makes a shell plugin or a theme
//! and sees it live.
//!
//! - Opens on a plugin folder (argument, Open Plugin…) or makes one with Create Plugin, which is
//!   `wol plugin new`'s scaffold with the kinds switched on ([`editor::create`]).
//! - Pages: Editor ([`editor`]: text, atomic save, problems from the CLI's check rerun after each
//!   pause in typing), Preview ([`preview`]: the shell's plugin host in a sandbox on the menu bar,
//!   Dock, desktop and Control Center), Theme Studio ([`theme_studio`]).
//! - Toolbar: Save, Check, Create Package (`wol plugin pack`), Share… (the index request as a
//!   text sheet, [`publish`]; nothing is sent from the app).
//! - Reached from Settings > Appearance ("Make a theme", `--theme`) and Store > Plugins.
//!
//! Invariant: the GTK thread never reads or writes a plugin file; every read, check, save and
//! pack runs in [`off_thread`] and lands back on it.
pub mod editor;
pub mod preview;
pub mod publish;
pub mod theme_studio;

use crate::copy::maker as copy;
use crate::kit::{self, form, widgets as w};
use crate::theme::Theme;
use crate::ui::plugins::Paths;
use gtk::{gio, glib, prelude::*};
use shared::plugins::Kind;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use wol_lua::Services;

/// The app's id (single instance: a second launch opens its argument in the first window).
pub const APP_ID: &str = "org.wolos.plugin-maker";
/// The window title and the preview's app name.
pub const APP_NAME: &str = copy::APP_NAME;
/// `wol-plugin-maker --theme [theme.json]` opens the Theme Studio (Settings > Appearance).
pub const THEME_FLAG: &str = "--theme";
/// The command that opens the app on the Theme Studio.
pub const THEME_ARGV: [&str; 2] = ["wol-plugin-maker", THEME_FLAG];
/// The command that opens the app on its start page (Store > Plugins).
pub const ARGV: [&str; 1] = ["wol-plugin-maker"];

/// Page titles in the sidebar.
pub const EDITOR: &str = copy::EDITOR_PAGE;
pub const PREVIEW: &str = copy::PREVIEW_PAGE;
pub const STUDIO: &str = copy::STUDIO_PAGE;

/// Runs `work` on a worker thread, then `done(result)` on the GTK thread.
pub fn off_thread<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(T) + 'static,
) {
    let (tx, rx) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = tx.send_blocking(work());
    });
    glib::spawn_future_local(async move {
        if let Ok(v) = rx.recv().await {
            done(v);
        }
    });
}

/// Starts the app with `argv` (Settings > Appearance, Store > Plugins), detached.
pub fn launch(argv: &[&str]) {
    kit::spawn_app(&argv.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>());
}

/// A button that starts the app with `argv`, at the start of its line.
pub fn launch_button(label: &str, argv: &'static [&'static str]) -> gtk::Button {
    let b = w::button(label, w::Kind::Default, move || launch(argv));
    b.set_halign(gtk::Align::Start);
    b
}

/// What a launch asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Launch {
    /// The start page: open or create a plugin.
    Start,
    Plugin(PathBuf),
    /// The Theme Studio, on a `theme.json` or the theme in use.
    Theme(Option<PathBuf>),
}

/// The launch from the arguments after the program name.
pub fn parse_args(args: &[String]) -> Launch {
    match args {
        [] => Launch::Start,
        [f] if f == THEME_FLAG => Launch::Theme(None),
        [f, p, ..] if f == THEME_FLAG => Launch::Theme(Some(PathBuf::from(p))),
        [p, ..] => Launch::Plugin(PathBuf::from(p)),
    }
}

/// What Create Plugin offers, with the row titles; on at first: what `wol plugin new` makes
/// when no kind is given. Planned kinds (`Kind::PLANNED`: the scaffold refuses them) are not
/// offered.
pub const KINDS: [(Kind, &str, bool); 11] = [
    (Kind::Menubar, copy::KIND_MENUBAR, true),
    (Kind::Widget, copy::KIND_WIDGET, true),
    (Kind::Dock, copy::KIND_DOCK, false),
    (Kind::Tile, copy::KIND_TILE, false),
    (Kind::Panel, copy::KIND_PANEL, false),
    (Kind::Search, copy::KIND_SEARCH, false),
    (Kind::Toggle, copy::KIND_TOGGLE, false),
    (Kind::Hotkey, copy::KIND_HOTKEY, false),
    (Kind::Webapp, copy::KIND_WEBAPP, false),
    (Kind::Theme, copy::KIND_THEME, false),
    (Kind::Ipc, copy::KIND_IPC, false),
];

/// This account's plugin folder (where `wol plugin new` writes).
pub fn user_plugin_dir() -> PathBuf {
    Paths::user()
        .dirs
        .into_iter()
        .find(|(_, system)| !system)
        .map(|(d, _)| d)
        .unwrap_or_default()
}

/// The app's window content and its pages.
pub struct Maker {
    pub aw: w::AppWindow,
    editor_page: gtk::Box,
    editor: RefCell<Option<Rc<editor::Editor>>>,
    pub preview: Rc<preview::Preview>,
    pub studio: Rc<theme_studio::Studio>,
    /// Save, Check, Create Package, Share: on only while a plugin is open.
    plugin_actions: Vec<gtk::Button>,
    packed: RefCell<Option<publish::Packed>>,
    sheet: gtk::Revealer,
    sheet_text: gtk::Label,
}

impl Maker {
    /// The window content: start page, preview whose scripts reach `services` with stores in
    /// `sandbox`, and the studio on `theme`. Nothing blocks here.
    pub fn new(theme: Theme, services: Rc<dyn Services>, sandbox: Paths) -> Rc<Maker> {
        let aw = w::AppWindow::new(true, true);
        let editor_page = w::vbox(0);
        let preview = preview::Preview::new(sandbox, services);
        let studio = theme_studio::Studio::new(theme);
        aw.source_section(copy::PLUGIN_SECTION);
        aw.source_item("document-edit-symbolic", EDITOR, &editor_page);
        aw.source_item("view-reveal-symbolic", PREVIEW, &preview.root);
        aw.source_section(copy::THEME_SECTION);
        aw.source_item(
            "preferences-desktop-appearance-symbolic",
            STUDIO,
            &studio.root,
        );
        let text = w::label("", "");
        text.set_selectable(true);
        text.set_wrap(true);
        text.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        let mono = gtk::pango::AttrList::new();
        mono.insert(gtk::pango::AttrString::new_family("monospace"));
        text.set_attributes(Some(&mono));
        let body = w::vbox(w::S3);
        body.append(&w::label(copy::SHARE_TITLE, "title"));
        let about = w::label(copy::SHARE_ABOUT, "secondary");
        about.set_wrap(true);
        body.append(&about);
        body.append(&share_scroll(&text));
        let buttons = w::hbox(w::S2);
        buttons.set_halign(gtk::Align::End);
        body.append(&buttons);
        let sheet = w::toolbar_sheet(&aw, &body);
        let m = Rc::new_cyclic(|me: &std::rc::Weak<Maker>| {
            let act = |label: &str, kind: w::Kind, f: fn(&Rc<Maker>)| {
                let me = me.clone();
                let b = w::button(label, kind, move || {
                    if let Some(m) = me.upgrade() {
                        f(&m);
                    }
                });
                b.set_sensitive(false);
                b
            };
            let plugin_actions = vec![
                act(copy::SAVE, w::Kind::Default, |m| {
                    m.with_editor(|e| e.save())
                }),
                act(copy::CHECK, w::Kind::Default, |m| {
                    m.with_editor(|e| e.check())
                }),
                act(copy::CREATE_PACKAGE, w::Kind::Default, Maker::pack),
                act(copy::SHARE, w::Kind::Primary, Maker::share),
            ];
            for b in &plugin_actions {
                aw.actions.append(b);
            }
            let me2 = me.clone();
            buttons.append(&w::button(copy::COPY_STEPS, w::Kind::Default, move || {
                if let Some(m) = me2.upgrade() {
                    m.aw.root.clipboard().set_text(&m.sheet_text.text());
                    m.aw.status.set_label(copy::SHARE_COPIED);
                }
            }));
            let me3 = me.clone();
            buttons.append(&w::button(copy::DONE, w::Kind::Primary, move || {
                if let Some(m) = me3.upgrade() {
                    m.sheet.set_reveal_child(false);
                }
            }));
            Maker {
                aw,
                editor_page,
                editor: RefCell::new(None),
                preview,
                studio,
                plugin_actions,
                packed: RefCell::new(None),
                sheet,
                sheet_text: text,
            }
        });
        m.start_page();
        m
    }

    fn with_editor(&self, f: impl FnOnce(&Rc<editor::Editor>)) {
        let e = self.editor.borrow().clone();
        if let Some(e) = e {
            f(&e);
        }
    }

    pub fn select(&self, page: &str) {
        self.aw.select(page);
    }

    /// The Editor page before a plugin is open: open a folder, or create a plugin.
    fn start_page(self: &Rc<Self>) {
        let p = w::page(EDITOR);
        p.append(&w::footnote(copy::START));
        let me = Rc::downgrade(self);
        p.append(&w::section(
            "",
            &[form::button_row(
                copy::PLUGIN_FOLDER,
                "",
                copy::OPEN_PLUGIN,
                move || {
                    if let Some(m) = me.upgrade() {
                        m.choose_folder();
                    }
                },
            )],
        ));
        let id = w::text_field("alice.weather");
        id.set_width_chars(w::VALUE_CHARS);
        let mut rows = vec![form::row(copy::PLUGIN_ID, "", &id)];
        let picked: Rc<RefCell<Vec<Kind>>> = Rc::new(RefCell::new(
            KINDS.iter().filter(|k| k.2).map(|k| k.0).collect(),
        ));
        for (kind, title, on) in KINDS {
            let picked = picked.clone();
            rows.push(form::toggle_row(title, "", on, move |on| {
                let mut v = picked.borrow_mut();
                v.retain(|k| *k != kind);
                if on {
                    v.push(kind);
                }
            }));
        }
        p.append(&w::section(copy::NEW_PLUGIN, &rows));
        p.append(&w::footnote(copy::NEW_ABOUT));
        let me = Rc::downgrade(self);
        let create = w::button(copy::CREATE_PLUGIN, w::Kind::Primary, move || {
            if let Some(m) = me.upgrade() {
                let kinds = picked.borrow().clone();
                m.create(id.text().trim().to_string(), kinds);
            }
        });
        create.set_halign(gtk::Align::End);
        p.append(&create);
        self.set_editor_page(&p);
    }

    fn set_editor_page(&self, page: &impl IsA<gtk::Widget>) {
        while let Some(c) = self.editor_page.first_child() {
            self.editor_page.remove(&c);
        }
        self.editor_page.append(page);
    }

    /// Create Plugin: the CLI's scaffold in this account's plugin folder, then open it.
    fn create(self: &Rc<Self>, id: String, kinds: Vec<Kind>) {
        // In table order, as `wol plugin new --kinds` lists them.
        let kinds: Vec<Kind> = Kind::ALL
            .into_iter()
            .filter(|k| kinds.contains(k) && !k.is_planned())
            .collect();
        let dir = user_plugin_dir();
        let me = Rc::downgrade(self);
        off_thread(
            move || editor::create(&dir, &id, &kinds),
            move |r| {
                let Some(m) = me.upgrade() else { return };
                match r {
                    Ok(dir) => {
                        m.aw.status.set_label(copy::CREATED);
                        m.open_plugin(dir);
                    }
                    Err(e) => m.aw.status.set_label(&e.sentence()),
                }
            },
        );
    }

    /// Open Plugin…: a folder chooser.
    fn choose_folder(self: &Rc<Self>) {
        let dlg = gtk::FileDialog::builder()
            .title(copy::OPEN_FOLDER_TITLE)
            .modal(true)
            .build();
        let win = self.aw.root.root().and_downcast::<gtk::Window>();
        let me = Rc::downgrade(self);
        dlg.select_folder(win.as_ref(), gio::Cancellable::NONE, move |r| {
            if let (Some(m), Some(p)) = (me.upgrade(), r.ok().and_then(|f| f.path())) {
                m.open_plugin(p);
            }
        });
    }

    /// Reads `dir` for the editor and the preview off the GTK thread, then shows both.
    pub fn open_plugin(self: &Rc<Self>, dir: PathBuf) {
        let me = Rc::downgrade(self);
        off_thread(
            move || (editor::load(&dir), preview::read(&dir)),
            move |(l, r)| {
                let Some(m) = me.upgrade() else { return };
                match l {
                    Ok(l) => m.show_plugin(l, r),
                    Err(s) => m.aw.status.set_label(s),
                }
            },
        );
    }

    /// Shows a plugin already read: the editor on `l`, the preview on `r`.
    pub fn show_plugin(self: &Rc<Self>, l: editor::Loaded, r: preview::Read) {
        let ed = editor::Editor::new(l);
        let me = Rc::downgrade(self);
        ed.on_saved(Rc::new(move || {
            if let Some(m) = me.upgrade() {
                m.packed.replace(None);
                m.with_editor(|e| m.preview.reload(e.dir()));
            }
        }));
        self.set_editor_page(&ed.root);
        self.editor.replace(Some(ed));
        self.preview.show(r);
        self.packed.replace(None);
        for b in &self.plugin_actions {
            b.set_sensitive(true);
        }
        self.select(EDITOR);
    }

    /// Create Package: `wol plugin pack` on the saved folder; refused while edits are unsaved.
    fn pack(self: &Rc<Self>) {
        let Some((dir, unsaved)) = self
            .editor
            .borrow()
            .as_ref()
            .map(|e| (e.dir().to_path_buf(), e.has_unsaved()))
        else {
            return;
        };
        if let Err(s) = pack_gate(unsaved) {
            self.aw.status.set_label(s);
            return;
        }
        let me = Rc::downgrade(self);
        off_thread(
            move || publish::pack(&dir),
            move |r| {
                let Some(m) = me.upgrade() else { return };
                match r {
                    Ok(p) => {
                        m.packed.replace(Some(p));
                        m.aw.status.set_label(copy::PACKED);
                    }
                    Err(s) => m.aw.status.set_label(s),
                }
            },
        );
    }

    /// Share…: the index request steps for the last package, in a sheet.
    fn share(self: &Rc<Self>) {
        let packed = self.packed.borrow().clone();
        match packed {
            Some(p) => self.show_share(p),
            None => self.aw.status.set_label(copy::PACK_FIRST),
        }
    }

    /// The share sheet on package `p` (kept as the last package).
    pub fn show_share(&self, p: publish::Packed) {
        self.sheet_text.set_label(&publish::steps(&p));
        self.packed.replace(Some(p));
        self.sheet.set_reveal_child(true);
    }
}

/// The share text in a scroller of a fixed height, so a long entry never grows the sheet.
fn share_scroll(text: &gtk::Label) -> gtk::ScrolledWindow {
    let s = kit::data::scroller(text);
    s.set_min_content_height(SHARE_HEIGHT);
    s.set_max_content_height(SHARE_HEIGHT);
    s
}

/// Whether Create Package may run: never on unsaved edits, so the package holds what is shown.
pub fn pack_gate(unsaved: bool) -> Result<(), &'static str> {
    if unsaved {
        Err(copy::SAVE_FIRST)
    } else {
        Ok(())
    }
}

/// Share text height in px (about 16 lines).
const SHARE_HEIGHT: i32 = 260;

/// `wol-plugin-maker [folder | --theme [theme.json]]`.
pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let shown: Rc<RefCell<Option<Rc<Maker>>>> = Rc::default();
    app.connect_command_line(move |app, cmd| {
        let cwd = cmd.cwd();
        let args: Vec<String> = cmd
            .arguments()
            .iter()
            .skip(1)
            .map(|a| match &cwd {
                Some(c)
                    if !std::path::Path::new(a).is_absolute()
                        && a.to_string_lossy() != THEME_FLAG =>
                {
                    c.join(a).to_string_lossy().into_owned()
                }
                _ => a.to_string_lossy().into_owned(),
            })
            .collect();
        present(app, &shown, parse_args(&args));
        glib::ExitCode::SUCCESS
    });
    // The preview sandbox goes when run returns, synchronously: no UI is left by then.
    let _sandbox = preview::SandboxGuard(preview::sandbox_root());
    app.run()
}

/// Shows the window (built on the first launch) on what `launch` asks for.
fn present(app: &gtk::Application, shown: &Rc<RefCell<Option<Rc<Maker>>>>, launch: Launch) {
    let existing = shown.borrow().clone();
    let m = match existing {
        Some(m) => m,
        None => {
            kit::style::install("");
            let dark =
                gtk::Settings::default().is_some_and(|s| s.is_gtk_application_prefer_dark_theme());
            let theme = crate::theme::resolve(kit::style::active_id().as_deref(), dark);
            let sandbox = preview::sandbox();
            let services = Rc::new(preview::PreviewServices::new(&sandbox, None));
            // The sandbox is removed by run()'s guard once the app is done.
            let m = Maker::new(theme, services, sandbox);
            let win = gtk::ApplicationWindow::builder()
                .application(app)
                .title(APP_NAME)
                .default_width(w::APP_SIZE.0)
                .default_height(w::APP_SIZE.1)
                .decorated(false)
                .build();
            win.set_child(Some(&m.aw.root));
            kit::snapshot_on_map(&win, "plugin-maker");
            shown.replace(Some(m.clone()));
            m
        }
    };
    match launch {
        Launch::Start => {}
        Launch::Plugin(dir) => m.open_plugin(dir),
        Launch::Theme(file) => {
            m.select(STUDIO);
            if let Some(f) = file {
                m.studio.open(f);
            }
        }
    }
    if let Some(win) = app
        .active_window()
        .or_else(|| app.windows().into_iter().next())
    {
        win.present();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_package_is_never_made_from_unsaved_edits() {
        assert_eq!(pack_gate(true), Err(copy::SAVE_FIRST));
        assert_eq!(pack_gate(false), Ok(()));
    }

    #[test]
    fn the_preview_sandbox_is_gone_when_the_guard_drops() {
        let root = std::env::temp_dir().join(format!("wol-maker-guard-{}", std::process::id()));
        let p = preview::sandbox_in(&root);
        // What the host writes there: the enable list, a plugin's state and a toggle.
        wol_core::plugins::save_enabled(&p.config_home, &Default::default()).unwrap();
        wol_core::plugins::save_state(&p.state_home, "alice.demo", "{}").unwrap();
        wol_core::plugins::toggle_set(&p.state_home, "alice.demo", "wifi", true).unwrap();
        drop(preview::SandboxGuard(root.clone()));
        assert!(
            !root.exists(),
            "removed on the dropping thread, before exit"
        );
        // Only named files go (RR-041): a file the host did not write keeps its folder.
        std::fs::create_dir_all(&p.state_home).unwrap();
        std::fs::write(p.state_home.join("notes.txt"), "not ours").unwrap();
        drop(preview::SandboxGuard(root.clone()));
        assert!(p.state_home.join("notes.txt").is_file());
        assert!(!p.config_home.exists());
        std::fs::remove_file(p.state_home.join("notes.txt")).unwrap();
        preview::remove_sandbox(&root);
        assert!(!root.exists());
        assert!(
            preview::sandbox_root().ends_with(format!("wol-plugin-maker-{}", std::process::id()))
        );
    }

    /// S8-12: every label and title of the app's frame (and Store > Plugins) lives in the
    /// copy modules: no capitalised string literal outside the tests.
    #[test]
    fn the_frame_holds_no_words_of_its_own() {
        for (file, src) in [
            ("ui/maker/mod.rs", include_str!("mod.rs")),
            ("ui/store_plugins.rs", include_str!("../store_plugins.rs")),
        ] {
            let body = src.split("#[cfg(test)]").next().unwrap_or(src);
            let words: Vec<&str> = body
                .lines()
                .map(|l| l.split("//").next().unwrap_or(""))
                .filter(|l| {
                    l.split('"')
                        .skip(1)
                        .step_by(2)
                        // A word, not an env name like CARGO_PKG_VERSION.
                        .any(|lit| {
                            lit.starts_with(|c: char| c.is_ascii_uppercase())
                                && lit.contains(|c: char| c.is_ascii_lowercase())
                        })
                })
                .collect();
            assert!(words.is_empty(), "{file}: {words:?}");
        }
    }

    #[test]
    fn launches_read_their_arguments() {
        assert_eq!(parse_args(&[]), Launch::Start);
        assert_eq!(
            parse_args(&args(&["/p/alice.demo"])),
            Launch::Plugin("/p/alice.demo".into())
        );
        assert_eq!(parse_args(&args(&["--theme"])), Launch::Theme(None));
        assert_eq!(
            parse_args(&args(&["--theme", "/t/theme.json"])),
            Launch::Theme(Some("/t/theme.json".into()))
        );
        assert_eq!(THEME_ARGV, ["wol-plugin-maker", "--theme"]);
    }

    #[test]
    fn create_offers_every_kind_once_and_starts_like_the_cli() {
        // Every kind offered is one the scaffold makes.
        for (k, _, _) in KINDS {
            assert!(
                wol_core::plugins::scaffold("alice.k", "K", &[k]).is_ok(),
                "{k:?}"
            );
        }
        let mut ids: Vec<&str> = KINDS.iter().map(|k| k.0.id()).collect();
        ids.sort_unstable();
        let mut all: Vec<&str> = Kind::ALL
            .iter()
            .filter(|k| !k.is_planned())
            .map(|k| k.id())
            .collect();
        all.sort_unstable();
        assert_eq!(ids, all);
        let on: Vec<Kind> = KINDS.iter().filter(|k| k.2).map(|k| k.0).collect();
        assert_eq!(
            on,
            [Kind::Menubar, Kind::Widget],
            "wol plugin new's default"
        );
    }
}
