// SPDX-License-Identifier: GPL-2.0-or-later
//! App Store > Web Apps (ADR 0031 W5, `wol_core::webapps`): make a web address an app with its own
//! window and icon, see the ones you have, remove one. Until now this was `wol webapp add` in the
//! Terminal (manual webapps.md) and nothing else. The page runs that same verb, so the launcher
//! file, the icon download and the own-profile folder are made by one code path whether a person
//! types the line or fills in the form. The checks run first ([`check`]) with the same rules
//! (`wol_core::webapps::WebApp::new`), so a wrong name or address is told before anything runs.
//! Slow work never runs on the UI thread; [`check`], [`add_argv`], [`parse_list`] are the pure part.
use super::{Shared, say};
use crate::copy::visual::{
    STORE_WEBAPPS_ADDRESS, STORE_WEBAPPS_ADDRESS_HINT, STORE_WEBAPPS_BAD_ADDRESS,
    STORE_WEBAPPS_BAD_NAME, STORE_WEBAPPS_EXISTS, STORE_WEBAPPS_ICON, STORE_WEBAPPS_ICON_CHOOSE,
    STORE_WEBAPPS_ICON_RESET, STORE_WEBAPPS_ICON_SITE, STORE_WEBAPPS_INTRO, STORE_WEBAPPS_LOADING,
    STORE_WEBAPPS_MADE, STORE_WEBAPPS_MAKE, STORE_WEBAPPS_MAKING, STORE_WEBAPPS_MINE,
    STORE_WEBAPPS_NAME, STORE_WEBAPPS_NAME_HINT, STORE_WEBAPPS_NEED_ADDRESS,
    STORE_WEBAPPS_NEED_NAME, STORE_WEBAPPS_NEW, STORE_WEBAPPS_NO_LIST, STORE_WEBAPPS_NONE,
    STORE_WEBAPPS_NOT_MADE, STORE_WEBAPPS_NOT_REMOVED, STORE_WEBAPPS_OWN, STORE_WEBAPPS_OWN_HINT,
    STORE_WEBAPPS_REMOVE, STORE_WEBAPPS_ROW_OWN, STORE_WEBAPPS_TITLE,
};
use crate::copy::visual::{
    STORE_WEBAPPS_DISK_FULL, STORE_WEBAPPS_GONE, STORE_WEBAPPS_NOT_WRITABLE,
};
use crate::kit::{form, widgets as w};
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;
use wol_core::webapps::{self, Profile};

/// How long the list of web apps waits for `wol webapp list`.
const LIST_WAIT: Duration = Duration::from_secs(10);
/// How long an add or a remove waits for its `wol` line (the page's icon download is the slow part).
const CHANGE_WAIT: Duration = Duration::from_secs(45);

/// What the person filled in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Form {
    pub name: String,
    pub address: String,
    /// A picture file the person chose; empty: the site's own icon.
    pub icon: String,
    pub own_profile: bool,
}

/// One web app of this account (`wol webapp list --json`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub name: String,
    pub url: String,
    pub icon: String,
    pub own_profile: bool,
}

/// The page's content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Listing {
    Loading,
    /// The list could not be read.
    Unavailable,
    Ready(Vec<Row>),
}

/// Why the form cannot be sent: one sentence the person can act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    NeedName,
    BadName,
    NeedAddress,
    BadAddress,
    Exists(String),
}

impl Problem {
    pub fn sentence(&self) -> String {
        match self {
            Problem::NeedName => STORE_WEBAPPS_NEED_NAME.into(),
            Problem::BadName => STORE_WEBAPPS_BAD_NAME.into(),
            Problem::NeedAddress => STORE_WEBAPPS_NEED_ADDRESS.into(),
            Problem::BadAddress => STORE_WEBAPPS_BAD_ADDRESS.into(),
            Problem::Exists(n) => STORE_WEBAPPS_EXISTS.replace("{n}", n),
        }
    }
}

/// The form against the rules `wol webapp add` applies (`webapps::WebApp::new`) and against the
/// web apps there already are. Ok: the words to run.
pub fn check(f: &Form, have: &[Row]) -> Result<Vec<String>, Problem> {
    if f.name.trim().is_empty() {
        return Err(Problem::NeedName);
    }
    if f.address.trim().is_empty() {
        return Err(Problem::NeedAddress);
    }
    let app = webapps::WebApp::new(&f.name, &f.address, Profile::Shared).map_err(|e| match e {
        webapps::Error::BadUrl => Problem::BadAddress,
        _ => Problem::BadName,
    })?;
    if have.iter().any(|r| r.id == app.id) {
        return Err(Problem::Exists(app.name));
    }
    Ok(add_argv(f, &app))
}

/// `wol webapp add [--own-profile] --json -- <name> <address> [<icon>]`: the verb of the Terminal,
/// with `--` so a name that starts with dashes is a name. The checked name and address go, not the
/// typed ones.
pub fn add_argv(f: &Form, app: &webapps::WebApp) -> Vec<String> {
    let mut v: Vec<String> = ["wol", "webapp", "add", "--json"]
        .into_iter()
        .map(String::from)
        .collect();
    if f.own_profile {
        v.push("--own-profile".into());
    }
    v.extend(["--".into(), app.name.clone(), app.url.clone()]);
    if !f.icon.trim().is_empty() {
        v.push(f.icon.trim().to_string());
    }
    v
}

/// `wol webapp remove --json -- <id>`.
pub fn remove_argv(id: &str) -> Vec<String> {
    ["wol", "webapp", "remove", "--json", "--", id]
        .into_iter()
        .map(String::from)
        .collect()
}

/// The rows of `wol webapp list --json` (`{"webapps":[{id,name,url,icon,own_profile}]}`); None
/// when the text is not that.
pub fn parse_list(text: &str) -> Option<Vec<Row>> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let list = v.get("webapps").unwrap_or(&v).as_array()?;
    let s = |o: &serde_json::Value, k: &str| o.get(k).and_then(|x| x.as_str()).map(String::from);
    let mut rows: Vec<Row> = list
        .iter()
        .filter_map(|o| {
            Some(Row {
                id: s(o, "id")?,
                name: s(o, "name")?,
                url: s(o, "url").unwrap_or_default(),
                icon: s(o, "icon").unwrap_or_default(),
                own_profile: o
                    .get("own_profile")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false),
            })
        })
        .collect();
    rows.sort_by_key(|r| r.name.to_lowercase());
    Some(rows)
}

/// The second line of a row: the site, and that it keeps its own sign-ins.
pub fn subtitle(r: &Row) -> String {
    let host = webapps::parse_url(&r.url).map_or_else(|_| r.url.clone(), |u| u.host);
    if r.own_profile {
        STORE_WEBAPPS_ROW_OWN.replace("{host}", &host)
    } else {
        host
    }
}

/// Reads the list. Blocks.
fn load() -> Listing {
    let out = wol_core::system::run_with_timeout(&["wol", "webapp", "list", "--json"], LIST_WAIT);
    match out.as_deref().and_then(parse_list) {
        Some(rows) => Listing::Ready(rows),
        None => Listing::Unavailable,
    }
}

/// What a make (or, `removing`, a remove) that did not exit 0 tells the person: the verb's own
/// reason when it is one of the sentences it gives (`wol: <message>` on stderr), else the
/// generic one. `out` = exit status and stderr; None = it never answered.
pub fn failure(out: Option<(i32, &str)>, removing: bool) -> String {
    let generic = if removing {
        STORE_WEBAPPS_NOT_REMOVED
    } else {
        STORE_WEBAPPS_NOT_MADE
    };
    let Some((_, err)) = out else {
        return generic.into();
    };
    let Some(said) = err
        .lines()
        .rev()
        .find_map(|l| l.trim().strip_prefix("wol: "))
        .map(str::trim)
    else {
        return generic.into();
    };
    // The verb's own reasons (`webapps::Error`), in the page's words: a name taken (by a file
    // that is not one of the listed web apps, so [`check`] could not see it), one already gone.
    if let Some(n) = said
        .strip_prefix("A web app named \"")
        .and_then(|s| s.strip_suffix("\" already exists."))
    {
        return STORE_WEBAPPS_EXISTS.replace("{n}", n);
    }
    if removing && said.starts_with("There is no web app named ") {
        return STORE_WEBAPPS_GONE.into();
    }
    // An OS error after the path (the verb prints `io::Error` as std writes it): say what it
    // means for the person, not the errno. ENOSPC, EACCES, EROFS (the same numbers on Linux and
    // the BSDs).
    let os = |code: i32| said.contains(&std::io::Error::from_raw_os_error(code).to_string());
    if !removing && os(28) {
        return STORE_WEBAPPS_DISK_FULL.into();
    }
    if !removing && (os(13) || os(30)) {
        return STORE_WEBAPPS_NOT_WRITABLE.into();
    }
    generic.into()
}

/// Runs a `wol` line; Err = the sentence for the status line ([`failure`]). Blocks (the icon
/// download is the slow part).
fn run(argv: &[String], removing: bool) -> Result<(), String> {
    let a: Vec<&str> = argv.iter().map(String::as_str).collect();
    match wol_core::system::run_full(&a, None, CHANGE_WAIT) {
        Ok((0, _, _)) => Ok(()),
        Ok((code, _, err)) => Err(failure(Some((code, &err)), removing)),
        Err(_) => Err(failure(None, removing)),
    }
}

type Show = Rc<dyn Fn(Listing)>;
/// The page's reload, set once the page exists.
type Reload = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// The Web Apps page, and the function that shows a listing on it.
pub(super) fn page(sh: &Rc<Shared>) -> (gtk::Box, Show) {
    let p = w::page(STORE_WEBAPPS_TITLE);
    let intro = w::label(STORE_WEBAPPS_INTRO, "secondary");
    intro.set_wrap(true);
    p.append(&intro);
    let have: Rc<RefCell<Vec<Row>>> = Default::default();
    let list = w::vbox(w::S6);
    let status = w::label("", "secondary");
    status.set_wrap(true);
    status.set_visible(false);
    let reload: Reload = Default::default();

    // The form.
    let name = w::text_field(STORE_WEBAPPS_NAME_HINT);
    let address = w::text_field(STORE_WEBAPPS_ADDRESS_HINT);
    name.set_width_chars(w::VALUE_CHARS);
    address.set_width_chars(w::VALUE_CHARS);
    let icon: Rc<RefCell<String>> = Default::default();
    let icon_note = w::label(STORE_WEBAPPS_ICON_SITE, "secondary");
    let choose = w::button(STORE_WEBAPPS_ICON_CHOOSE, w::Kind::Default, || {});
    let reset = w::button(STORE_WEBAPPS_ICON_RESET, w::Kind::Plain, || {});
    reset.set_visible(false);
    {
        let (icon_a, note_a, reset_a, anchor) = (
            icon.clone(),
            icon_note.clone(),
            reset.clone(),
            choose.clone(),
        );
        choose.connect_clicked(move |_| {
            let (icon, note, reset) = (icon_a.clone(), note_a.clone(), reset_a.clone());
            crate::kit::pickers::choose_file_of(
                &anchor,
                STORE_WEBAPPS_ICON,
                "Pictures",
                &["png", "svg", "ico", "jpg", "jpeg", "webp"],
                move |path| {
                    note.set_label(
                        &path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                    );
                    *icon.borrow_mut() = path.display().to_string();
                    reset.set_visible(true);
                },
            );
        });
        let (icon, note, reset2) = (icon.clone(), icon_note.clone(), reset.clone());
        reset.connect_clicked(move |_| {
            icon.borrow_mut().clear();
            note.set_label(STORE_WEBAPPS_ICON_SITE);
            reset2.set_visible(false);
        });
    }
    let icon_ctl = w::hbox(w::S2);
    icon_ctl.append(&icon_note);
    icon_ctl.append(&reset);
    icon_ctl.append(&choose);
    let own = w::toggle(false, |_| {});
    let make = w::button(STORE_WEBAPPS_MAKE, w::Kind::Primary, || {});
    make.set_halign(gtk::Align::End);
    {
        let (name, address, icon, own, have, status, reload, make2) = (
            name.clone(),
            address.clone(),
            icon.clone(),
            own.clone(),
            have.clone(),
            status.clone(),
            reload.clone(),
            make.clone(),
        );
        make.connect_clicked(move |_| {
            let form = Form {
                name: name.text().to_string(),
                address: address.text().to_string(),
                icon: icon.borrow().clone(),
                own_profile: own.is_active(),
            };
            let argv = match check(&form, &have.borrow()) {
                Ok(a) => a,
                Err(why) => return say(&status, &why.sentence()),
            };
            say(&status, "");
            // One make at a time; the icon download can take a few seconds.
            make2.set_sensitive(false);
            make2.set_label(STORE_WEBAPPS_MAKING);
            let (make, status, reload, name, address) = (
                make2.clone(),
                status.clone(),
                reload.clone(),
                name.clone(),
                address.clone(),
            );
            let shown = form.name.trim().to_string();
            super::plugins::off_ui(
                move || run(&argv, false),
                move |done| {
                    make.set_sensitive(true);
                    make.set_label(STORE_WEBAPPS_MAKE);
                    match done {
                        Ok(()) => {
                            name.set_text("");
                            address.set_text("");
                            say(&status, &STORE_WEBAPPS_MADE.replace("{n}", &shown));
                        }
                        Err(why) => say(&status, &why),
                    }
                    if let Some(r) = reload.borrow().as_ref() {
                        r();
                    }
                },
            );
        });
    }
    let form_rows = [
        form::row(STORE_WEBAPPS_NAME, "", &name),
        form::row(STORE_WEBAPPS_ADDRESS, "", &address),
        form::row(STORE_WEBAPPS_ICON, "", &icon_ctl),
        form::row(STORE_WEBAPPS_OWN, STORE_WEBAPPS_OWN_HINT, &own),
    ];
    p.append(&w::section(STORE_WEBAPPS_NEW, &form_rows));
    p.append(&make);
    p.append(&status);
    p.append(&list);

    let show: Show = {
        let (list, have, status, reload) =
            (list.clone(), have.clone(), status.clone(), reload.clone());
        Rc::new(move |l: Listing| fill(&list, &have, &status, &reload, l))
    };
    let r: Rc<dyn Fn()> = {
        let show = show.clone();
        Rc::new(move || {
            let show = show.clone();
            super::plugins::off_ui(load, move |l| show(l));
        })
    };
    *reload.borrow_mut() = Some(r.clone());
    show(Listing::Loading);
    if sh.env.pkg_search {
        // Every time the page is shown: a web app made or removed by hand since is there.
        p.connect_map(move |_| r());
    } else {
        show(Listing::Ready(Vec::new()));
    }
    (p, show)
}

/// The Your Web Apps card, or what stands in its place.
fn fill(
    list: &gtk::Box,
    have: &Rc<RefCell<Vec<Row>>>,
    status: &gtk::Label,
    reload: &Reload,
    l: Listing,
) {
    while let Some(c) = list.first_child() {
        list.remove(&c);
    }
    let rows = match l {
        Listing::Loading => {
            list.append(&w::label(STORE_WEBAPPS_LOADING, "secondary"));
            return;
        }
        Listing::Unavailable => {
            list.append(&w::label(STORE_WEBAPPS_NO_LIST, "secondary"));
            return;
        }
        Listing::Ready(r) => r,
    };
    *have.borrow_mut() = rows.clone();
    if rows.is_empty() {
        list.append(&w::footnote(STORE_WEBAPPS_NONE));
        return;
    }
    let cards: Vec<gtk::Widget> = rows.iter().map(|r| row_widget(r, status, reload)).collect();
    list.append(&w::section(STORE_WEBAPPS_MINE, &cards));
}

fn row_widget(r: &Row, status: &gtk::Label, reload: &Reload) -> gtk::Widget {
    let (id, st, rl) = (r.id.clone(), status.clone(), reload.clone());
    let more = form::actions_button(vec![(
        STORE_WEBAPPS_REMOVE,
        Box::new(move || {
            let (st, rl) = (st.clone(), rl.clone());
            let argv = remove_argv(&id);
            super::plugins::off_ui(
                move || run(&argv, true),
                move |done| {
                    say(&st, done.err().as_deref().unwrap_or(""));
                    if let Some(r) = rl.borrow().as_ref() {
                        r();
                    }
                },
            );
        }),
    )]);
    let row = w::list_row(
        if r.icon.starts_with('/') {
            ""
        } else {
            icon_name(&r.icon)
        },
        &r.name,
        &subtitle(r),
        more.as_ref().map(|m| m.upcast_ref()),
    );
    if r.icon.starts_with('/') {
        // A picture file (the site's own icon, or one chosen): outside input, so it goes through
        // `kit::picture` (bounded, decoded at the row's size off the GTK thread; QA-A10-SD-02: a
        // 1 MiB PNG claiming 12000x12000 froze the Store). The generic icon until it arrives,
        // and for good if the file is refused.
        let img = gtk::Image::from_icon_name(icon_name(&r.icon));
        img.set_pixel_size(w::ICON);
        crate::kit::picture::show_in(&img, &r.icon, w::ICON);
        row.prepend(&img);
    }
    row.upcast()
}

/// A theme icon name for a launcher whose icon is a name (`web-browser`), symbolic.
fn icon_name(name: &str) -> &'static str {
    let _ = name;
    "web-browser-symbolic"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(name: &str, address: &str) -> Form {
        Form {
            name: name.into(),
            address: address.into(),
            ..Default::default()
        }
    }

    fn mail() -> Row {
        Row {
            id: "mail".into(),
            name: "Mail".into(),
            url: "https://mail.example.com".into(),
            icon: "web-browser".into(),
            own_profile: false,
        }
    }

    /// The form runs the verb of the Terminal: `wol webapp add Mail https://mail.example.com`.
    #[test]
    fn the_form_is_the_terminal_verb() {
        let argv = check(&form("Mail", "mail.example.com"), &[]).unwrap();
        assert_eq!(
            argv,
            [
                "wol",
                "webapp",
                "add",
                "--json",
                "--",
                "Mail",
                "https://mail.example.com"
            ]
        );
        // The own-profile flag and a chosen picture ride along.
        let f = Form {
            icon: "/home/a/Pictures/m.png".into(),
            own_profile: true,
            ..form("Mail", "https://mail.example.com/inbox")
        };
        let argv = check(&f, &[]).unwrap();
        assert_eq!(
            argv,
            [
                "wol",
                "webapp",
                "add",
                "--json",
                "--own-profile",
                "--",
                "Mail",
                "https://mail.example.com/inbox",
                "/home/a/Pictures/m.png"
            ]
        );
    }

    /// Wrong input is told before anything runs, in the Store's words, with the core's rules.
    #[test]
    fn a_wrong_form_says_why() {
        assert_eq!(
            check(&form("", "a.example.com"), &[]),
            Err(Problem::NeedName)
        );
        assert_eq!(check(&form("Mail", " "), &[]), Err(Problem::NeedAddress));
        assert_eq!(
            check(&form("!!!", "a.example.com"), &[]),
            Err(Problem::BadName)
        );
        for bad in [
            "not a url",
            "https://",
            "javascript:alert(1)",
            "a.example.com/\"x",
        ] {
            assert_eq!(
                check(&form("Mail", bad), &[]),
                Err(Problem::BadAddress),
                "{bad}"
            );
        }
        assert_eq!(
            check(&form("mail", "a.example.com"), &[mail()]),
            Err(Problem::Exists("mail".into())),
            "same id as Mail"
        );
        for p in [
            Problem::NeedName,
            Problem::BadName,
            Problem::NeedAddress,
            Problem::BadAddress,
            Problem::Exists("Mail".into()),
        ] {
            let s = p.sentence();
            assert!(
                crate::copy::lint(&s).is_empty(),
                "{s}: {:?}",
                crate::copy::lint(&s)
            );
        }
    }

    /// A name that starts with dashes is a name, not a flag.
    #[test]
    fn a_dashed_name_is_not_a_flag() {
        let argv = check(&form("--own-profile Club", "club.example.com"), &[]).unwrap();
        let at = argv.iter().position(|a| a == "--").unwrap();
        assert_eq!(argv[at + 1], "--own-profile Club", "after the separator");
        assert!(!argv[..at].iter().any(|a| a == "--own-profile"), "{argv:?}");
    }

    #[test]
    fn the_list_is_what_the_verb_prints() {
        let text = r#"{"webapps":[
            {"id":"mail","name":"Mail","url":"https://mail.example.com","icon":"web-browser","own_profile":true},
            {"id":"cal","name":"calendar","url":"https://cal.example.com/x","icon":"/home/a/i.png","own_profile":false}]}"#;
        let rows = parse_list(text).unwrap();
        assert_eq!(
            rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["cal", "mail"],
            "by name, ignoring case"
        );
        assert_eq!(subtitle(&rows[0]), "cal.example.com");
        assert_eq!(subtitle(&rows[1]), "mail.example.com, own sign-ins");
        assert_eq!(parse_list(r#"{"webapps":[]}"#), Some(vec![]));
        assert_eq!(parse_list("not json"), None);
        assert_eq!(
            remove_argv("mail")[..4],
            ["wol", "webapp", "remove", "--json"]
        );
    }

    /// QA-A10-SD-06: a make or a remove that failed says why, from the verb's own reason
    /// (`wol: <message>` on stderr): a name taken by a file that is not a web app, a full disk, a
    /// folder that cannot be written, a web app already gone. A reason the page does not know,
    /// or no answer, keeps the generic sentence; every sentence passes the copy lint.
    #[test]
    fn a_failed_change_says_why() {
        let exists = webapps::Error::Exists("Mail".into()).to_string();
        let gone = webapps::Error::NotFound("mail".into()).to_string();
        let os = |n| std::io::Error::from_raw_os_error(n);
        let full = format!(
            "wol: /home/a/.local/share/applications/wol-webapp-mail.desktop: {}\n",
            os(28)
        );
        let ro = format!("wol: web app profile folder: {}\n", os(13));
        for (out, removing, want) in [
            (
                Some((65, format!("wol: {exists}\n"))),
                false,
                STORE_WEBAPPS_EXISTS.replace("{n}", "Mail"),
            ),
            (Some((1, full)), false, STORE_WEBAPPS_DISK_FULL.into()),
            (Some((1, ro)), false, STORE_WEBAPPS_NOT_WRITABLE.into()),
            (
                Some((69, format!("noise\nwol: {gone}\n"))),
                true,
                STORE_WEBAPPS_GONE.into(),
            ),
            (
                Some((1, "wol: something new\n".into())),
                false,
                STORE_WEBAPPS_NOT_MADE.into(),
            ),
            (
                Some((1, String::new())),
                true,
                STORE_WEBAPPS_NOT_REMOVED.into(),
            ),
            (None, false, STORE_WEBAPPS_NOT_MADE.into()),
        ] {
            let got = failure(out.as_ref().map(|(c, e)| (*c, e.as_str())), removing);
            assert_eq!(got, want, "{out:?}");
            assert!(
                crate::copy::lint(&got).is_empty(),
                "{got}: {:?}",
                crate::copy::lint(&got)
            );
        }
    }
}
