// SPDX-License-Identifier: GPL-2.0-or-later
//! Install WolOS step by step (ADR 0023): one decision per page, pure. `ui::installer` draws a
//! page for each [`Step`]; everything it says, and the plan it hands to `install-run`, comes from
//! here. Layouts are `wol_core::install`'s: the summary and the run read the same data.
use super::size_text;
use crate::copy;
use crate::setup::{Draft, Field, Hints, KEYBOARDS, LANGUAGES};
use wol_core::install::disk::{Disk, GIB, MIB, Table};
use wol_core::install::layout::{
    Assign, Compiled, Fs, Layout, MIN_ROOT, New, ROOMY_ROOT, Refusal, Resize, compile, room, verify,
};
use wol_core::install::plan::{
    Account, Plan, Protect, alongside, alongside_blocked, alongside_range, alongside_target,
    changes,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Install or Try, on the first screen of the live image only.
    Begin,
    Language,
    Zone,
    Disk,
    Method,
    /// How much of the other system stays.
    Alongside,
    Manual,
    /// One partition of the manual list.
    Edit,
    Protect,
    Account,
    Summary,
    /// Secure Boot is on: the installed WolOS may not start; said before anything is written.
    SecureBoot,
    Installing,
    Done,
    Failed,
}

/// Every page, in the order they are added to the pager.
pub const STEPS: [Step; 15] = [
    Step::Begin,
    Step::Language,
    Step::Zone,
    Step::Disk,
    Step::Method,
    Step::Alongside,
    Step::Manual,
    Step::Edit,
    Step::Protect,
    Step::Account,
    Step::Summary,
    Step::SecureBoot,
    Step::Installing,
    Step::Done,
    Step::Failed,
];

impl Step {
    pub fn id(self) -> &'static str {
        match self {
            Step::Begin => "begin",
            Step::Language => "language",
            Step::Zone => "zone",
            Step::Disk => "disk",
            Step::Method => "method",
            Step::Alongside => "alongside",
            Step::Manual => "manual",
            Step::Edit => "edit",
            Step::Protect => "protect",
            Step::Account => "account",
            Step::Summary => "summary",
            Step::SecureBoot => "secure-boot",
            Step::Installing => "installing",
            Step::Done => "done",
            Step::Failed => "failed",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Step::Begin => "Welcome to WolOS",
            Step::Language => "Language and Keyboard",
            Step::Zone => "Time Zone",
            Step::Disk => "Choose a Disk",
            Step::Method => "How to Install",
            Step::Alongside => "Share the Disk",
            Step::Manual => "Partitions",
            Step::Edit => "Partition",
            Step::Protect => "Protect Your Files",
            Step::Account => "Create Your Account",
            Step::Summary => "Ready to Install",
            Step::SecureBoot => super::SECURE_BOOT_TITLE,
            Step::Installing => "Installing WolOS",
            Step::Done => "WolOS Is Installed",
            Step::Failed => "WolOS Couldn't Be Installed",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Step::Begin => "Install it on this computer, or try it first. Trying changes nothing.",
            Step::Language => "Choose your language and the keyboard you type on.",
            Step::Zone => "Your clock follows this place.",
            Step::Disk => "Choose where WolOS goes.",
            Step::Method => "Nothing changes until the last step.",
            Step::Alongside => "Drag to choose how much room each system gets.",
            Step::Manual => "Choose a partition to change it, or free space to add one.",
            Step::Edit => "Choose what this partition is for.",
            Step::Protect => "Encryption keeps your files private if this computer is lost.",
            Step::Account => "You use this to sign in.",
            Step::Summary => "Nothing has changed yet. This is exactly what will happen.",
            Step::SecureBoot => super::SECURE_BOOT_TEXT,
            Step::Installing => "This takes a few minutes. Keep your computer plugged in.",
            Step::Done => {
                "Restart to start using WolOS. Remove the installer when the screen goes black."
            }
            Step::Failed => "",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Erase,
    Alongside,
    Manual,
}

/// One disk as the installer found it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub disk: Disk,
    pub check: super::Check,
}

/// What Continue did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Next {
    Go(Step),
    /// Stay on the page: the problem, and the field it concerns.
    Stay(Option<Field>, String),
    /// Everything is decided: run `install-run`.
    Install,
}

fn stay(text: impl Into<String>) -> Next {
    Next::Stay(None, text.into())
}

/// What an existing or new partition is for, on the edit page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Use {
    Nothing,
    System,
    Startup,
    Home,
    Swap,
    Folder,
}

/// The edit page's choices, in order.
pub const USES: [(Use, &str); 6] = [
    (Use::Nothing, "Don't use"),
    (Use::System, "WolOS system"),
    (Use::Startup, "Startup files"),
    (Use::Home, "Your files"),
    (Use::Swap, "Swap"),
    (Use::Folder, "A folder"),
];

/// The file systems a new or erased partition can have. Btrfs first: it keeps the two copies.
pub const FILE_SYSTEMS: [(Fs, &str); 3] = [
    (Fs::Btrfs, "Btrfs (recommended)"),
    (Fs::Ext4, "Ext4"),
    (Fs::Xfs, "XFS"),
];

fn mount_of(u: Use, folder: &str) -> Option<String> {
    Some(match u {
        Use::Nothing => return None,
        Use::System => "/".into(),
        Use::Startup => "/efi".into(),
        Use::Home => "/home".into(),
        Use::Swap => "swap".into(),
        Use::Folder => format!("/{}", folder.trim().trim_start_matches('/')),
    })
}

fn use_of(mount: &str) -> Use {
    match mount {
        "/" => Use::System,
        "/efi" => Use::Startup,
        "/home" => Use::Home,
        "swap" => Use::Swap,
        _ => Use::Folder,
    }
}

/// A row of the manual list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// An existing partition, by GPT UUID.
    Part(String),
    /// A new partition, by index into `Layout::create`.
    New(usize),
    /// Free space (bytes).
    Free(u64),
}

/// What the row says: (row, title, detail).
pub type Line = (Row, String, String);

/// The edit page's values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub row: Row,
    pub title: String,
    pub usage: Use,
    /// Erases what is on it. Always on for a new partition.
    pub erase: bool,
    pub fs: Fs,
    pub folder: String,
    pub bytes: u64,
    /// The size slider's range; `min == max`: the size can't change.
    pub min: u64,
    pub max: u64,
}

fn role_text(l: &Layout, uuid: &str) -> Option<String> {
    let a = l.assign.iter().find(|a| a.part == uuid)?;
    let word = copy::install_use(&a.mount);
    Some(if a.format.is_some() {
        format!("{word}, erased")
    } else {
        word
    })
}

/// The manual list: partitions in disk order with what will happen to each, new ones, free space.
pub fn manual_lines(l: &Layout, d: &Disk) -> Vec<Line> {
    let mut v = Vec::new();
    for p in &d.parts {
        let size = |b: u64| size_text(b);
        let deleted = l.delete.contains(&p.uuid);
        let bytes = l
            .resize
            .iter()
            .find(|r| r.part == p.uuid)
            .map_or(d.bytes_of(p), |r| r.bytes);
        let what = if deleted {
            "Will be deleted".to_string()
        } else {
            role_text(l, &p.uuid).unwrap_or_else(|| "Not used".into())
        };
        v.push((
            Row::Part(p.uuid.clone()),
            p.title(),
            format!("{what} · {}", size(bytes)),
        ));
    }
    for (i, n) in l.create.iter().enumerate() {
        v.push((
            Row::New(i),
            format!("New: {}", copy::install_use(&n.mount)),
            n.bytes.map_or("Rest of the disk".into(), size_text),
        ));
    }
    for (_, sectors) in room(l, d) {
        let bytes = sectors * d.sector;
        if bytes >= GIB {
            v.push((Row::Free(bytes), "Free space".into(), size_text(bytes)));
        }
    }
    v
}

pub struct Flow {
    pub step: Step,
    /// Started from the installer's own first screen, not from the live desktop.
    pub boot: bool,
    pub draft: Draft,
    pub zone: String,
    zone_chosen: bool,
    pub disks: Vec<Found>,
    pub chosen: Option<usize>,
    pub method: Method,
    /// Bytes the other system keeps, on the Alongside page.
    pub keep: u64,
    pub manual: Layout,
    pub edit: Option<Edit>,
    /// This computer has a security chip (TPM2).
    pub tpm: bool,
    pub protect: Protect,
    pub disk_password: String,
    pub disk_verify: String,
    /// Where Back goes from each page: the pages that were really shown.
    trail: Vec<Step>,
}

/// Where a time zone guess starts: the language's biggest city zone.
pub fn zone_for_language(i: usize) -> &'static str {
    match LANGUAGES[i.min(LANGUAGES.len() - 1)].locale {
        "en_GB.UTF-8" => "Europe/London",
        "pt_BR.UTF-8" => "America/Sao_Paulo",
        "pt_PT.UTF-8" => "Europe/Lisbon",
        "es_ES.UTF-8" => "Europe/Madrid",
        "fr_FR.UTF-8" => "Europe/Paris",
        "de_DE.UTF-8" => "Europe/Berlin",
        _ => "America/New_York",
    }
}

/// The machine's zone when it knows one (a live image says UTC: that's no guess), else the
/// language's.
pub fn guess_zone(h: &Hints, language: usize) -> String {
    h.zone
        .as_deref()
        .filter(|z| !matches!(*z, "UTC" | "Etc/UTC" | "GMT"))
        .filter(|z| wol_core::settings::timezones::is_valid(z))
        .map_or_else(|| zone_for_language(language).to_string(), String::from)
}

/// The zones the search shows: every word of `query` in the zone's label, nearest first.
pub fn zones_matching<'a>(all: &'a [String], query: &str) -> Vec<&'a String> {
    let fold = |s: &str| s.to_lowercase().replace(['_', '/'], " ");
    let words: Vec<String> = fold(query).split_whitespace().map(String::from).collect();
    let mut hits: Vec<&String> = all
        .iter()
        .filter(|z| {
            let z = fold(z);
            words.iter().all(|w| z.contains(w.as_str()))
        })
        .collect();
    if let Some(w) = words.first() {
        hits.sort_by_key(|z| !fold(z.rsplit('/').next().unwrap_or(z)).starts_with(w.as_str()));
    }
    hits
}

impl Flow {
    pub fn new(hints: &Hints, boot: bool, tpm: bool) -> Flow {
        let draft = Draft::detected(hints);
        Flow {
            step: if boot { Step::Begin } else { Step::Language },
            boot,
            zone: guess_zone(hints, draft.language),
            zone_chosen: false,
            draft,
            disks: Vec::new(),
            chosen: None,
            method: Method::Erase,
            keep: 0,
            manual: Layout::default(),
            edit: None,
            tpm,
            protect: if tpm { Protect::Tpm } else { Protect::Off },
            disk_password: String::new(),
            disk_verify: String::new(),
            trail: Vec::new(),
        }
    }

    pub fn set_language(&mut self, i: usize) {
        self.draft.set_language(i);
        if !self.zone_chosen {
            self.zone = zone_for_language(i).to_string();
        }
    }

    pub fn set_zone(&mut self, z: &str) {
        self.zone = z.to_string();
        self.zone_chosen = true;
    }

    /// The disks arrived: the only one is already chosen.
    pub fn set_disks(&mut self, disks: Vec<Found>) {
        self.chosen = (disks.len() == 1).then_some(0);
        self.disks = disks;
        self.reset_disk();
    }

    pub fn disk(&self) -> Option<&Disk> {
        self.disks.get(self.chosen?).map(|f| &f.disk)
    }

    /// Another disk: its plan starts over.
    pub fn pick_disk(&mut self, i: usize) {
        self.chosen = Some(i);
        self.reset_disk();
    }

    fn reset_disk(&mut self) {
        self.manual = Layout::default();
        self.edit = None;
        self.method = Method::Erase;
        if let Some(d) = self.disk() {
            let keep = alongside_target(d)
                .and_then(|p| alongside_range(d, p))
                .map(|(lo, hi)| lo + (hi - lo) / 2);
            self.keep = keep.unwrap_or(0);
            if self.alongside_problem().is_none() {
                self.method = Method::Alongside;
            }
        }
    }

    /// Install Alongside's title, by the system it shares the disk with.
    pub fn alongside_title(&self) -> String {
        match self.disk().map(Disk::others).unwrap_or_default().first() {
            Some(n) => format!("Install Alongside {n}"),
            None => "Install in the Free Space".into(),
        }
    }

    /// Why Install Alongside can't be chosen on this disk; None: it can.
    pub fn alongside_problem(&self) -> Option<String> {
        let d = self.disk()?;
        if d.table == Table::Other {
            return Some(copy::install_refusal(&Refusal::NotGpt, d));
        }
        if d.parts.is_empty() {
            return Some(copy::INSTALL_BLANK.to_string());
        }
        if self.alongside_slider().is_some() || compile(&alongside(d, None), d, false).is_ok() {
            return None;
        }
        Some(match alongside_blocked(d) {
            Some(b) => copy::install_blocked(b).to_string(),
            None => copy::install_refusal(&Refusal::NoRoom, d),
        })
    }

    /// The Alongside page's partition and range; None: nothing to shrink, the free space is used.
    pub fn alongside_slider(&self) -> Option<(String, String, u64, u64)> {
        let d = self.disk()?;
        let p = alongside_target(d)?;
        let (lo, hi) = alongside_range(d, p)?;
        Some((p.uuid.clone(), p.title(), lo, hi))
    }

    /// The layout the chosen method means right now.
    pub fn layout(&self) -> Layout {
        let Some(d) = self.disk() else {
            return Layout::default();
        };
        match self.method {
            Method::Erase => Layout::erase(),
            Method::Manual => self.manual.clone(),
            Method::Alongside => match self.alongside_slider() {
                Some((uuid, ..)) => alongside(d, Some((&uuid, self.keep))),
                None => alongside(d, None),
            },
        }
    }

    /// Compiled and verified; the same check `install-run` repeats on the disk as it is then.
    pub fn compiled(&self) -> Result<Compiled, Refusal> {
        let d = self.disk().ok_or(Refusal::DiskChanged)?;
        let l = self.layout();
        let c = compile(&l, d, self.protect != Protect::Off)?;
        verify(&c, &l, d)?;
        Ok(c)
    }

    /// What WolOS gets on the Alongside page (bytes).
    pub fn alongside_room(&self) -> Option<u64> {
        self.compiled().ok().map(|c| c.root_bytes)
    }

    /// Back or Escape: the page shown before this one.
    pub fn back(&mut self) -> Option<Step> {
        if matches!(self.step, Step::Installing | Step::Done) {
            return None;
        }
        let p = self.trail.pop()?;
        if self.step == Step::Edit {
            self.edit = None;
        }
        self.step = p;
        Some(p)
    }

    fn go(&mut self, s: Step) -> Next {
        self.trail.push(self.step);
        self.step = s;
        Next::Go(s)
    }

    /// Jumps to a page with no way back to the one left (installing, failed, restarting flows).
    pub fn jump(&mut self, s: Step) {
        self.trail.clear();
        self.step = s;
    }

    fn after_layout(&mut self) -> Next {
        match self.compiled() {
            Ok(_) => self.go(Step::Protect),
            Err(r) => {
                let d = self.disk().cloned().unwrap_or_default();
                stay(copy::install_refusal(&r, &d))
            }
        }
    }

    /// Continue (button or Return).
    pub fn advance(&mut self) -> Next {
        match self.step {
            Step::Begin => self.go(Step::Language),
            Step::Language => self.go(Step::Zone),
            Step::Zone => self.go(Step::Disk),
            Step::Disk => {
                let Some(i) = self.chosen else {
                    return stay("Choose a disk to install on.");
                };
                let f = &self.disks[i];
                if let Some(t) = f
                    .check
                    .refused
                    .as_deref()
                    .and_then(|w| super::refusal_text(w, &f.disk.title_text()))
                {
                    return stay(t);
                }
                if f.disk.bytes < MIN_ROOT {
                    return stay(copy::install_refusal(&Refusal::TooSmall, &f.disk));
                }
                self.go(Step::Method)
            }
            Step::Method => match self.method {
                Method::Erase => self.after_layout(),
                Method::Manual => self.go(Step::Manual),
                Method::Alongside => {
                    if let Some(p) = self.alongside_problem() {
                        stay(p)
                    } else if self.alongside_slider().is_some() {
                        self.go(Step::Alongside)
                    } else {
                        self.after_layout()
                    }
                }
            },
            Step::Alongside | Step::Manual => self.after_layout(),
            Step::Edit => match self.apply_edit() {
                Ok(()) => self.back().map_or_else(|| stay(""), Next::Go),
                Err(t) => stay(t),
            },
            Step::Protect => {
                if self.protect == Protect::Password {
                    if self.disk_password.chars().count() < 8 {
                        return Next::Stay(
                            Some(Field::Password),
                            "Use at least 8 characters.".into(),
                        );
                    }
                    if self.disk_password != self.disk_verify {
                        return Next::Stay(Some(Field::Verify), "Passwords don't match.".into());
                    }
                }
                self.go(Step::Account)
            }
            Step::Account => match self.draft.account_problem() {
                Some((f, why)) => Next::Stay(Some(f), why.into()),
                None => self.go(Step::Summary),
            },
            Step::Summary => match self.compiled() {
                Err(r) => {
                    let d = self.disk().cloned().unwrap_or_default();
                    stay(copy::install_refusal(&r, &d))
                }
                Ok(_) if self.secure_boot() => self.go(Step::SecureBoot),
                Ok(_) => Next::Install,
            },
            Step::SecureBoot => Next::Install,
            Step::Installing | Step::Done | Step::Failed => Next::Go(self.step),
        }
    }

    pub fn secure_boot(&self) -> bool {
        self.chosen
            .and_then(|i| self.disks.get(i))
            .is_some_and(|f| f.check.secure_boot)
    }

    /// The page opens on `row`.
    pub fn open_edit(&mut self, row: &Row) -> Next {
        let Some(d) = self.disk().cloned() else {
            return stay("Choose a disk to install on.");
        };
        let e = match row {
            Row::Part(uuid) => {
                let Some(p) = d.part(uuid) else {
                    return stay(copy::install_refusal(&Refusal::UnknownPartition, &d));
                };
                let a = self.manual.assign.iter().find(|a| a.part == *uuid);
                let r = self.manual.resize.iter().find(|r| r.part == *uuid);
                let size = d.bytes_of(p);
                let (min, max) = wol_core::install::layout::shrink_range(&d, p)
                    .map_or((size, size), |(lo, s)| (lo.div_ceil(MIB) * MIB, s));
                Edit {
                    row: row.clone(),
                    title: p.title(),
                    usage: a.map_or(Use::Nothing, |a| use_of(&a.mount)),
                    erase: a.is_some_and(|a| a.format.is_some()),
                    fs: a.and_then(|a| a.format).unwrap_or(Fs::Btrfs),
                    folder: a.map_or(String::new(), |a| folder_of(&a.mount)),
                    bytes: r.map_or(size, |r| r.bytes),
                    min,
                    max,
                }
            }
            Row::New(i) => {
                let n = &self.manual.create[*i];
                let bytes = n.bytes.unwrap_or(0);
                let spare = room(&self.manual, &d)
                    .iter()
                    .map(|(_, s)| s * d.sector)
                    .max()
                    .unwrap_or(0);
                Edit {
                    row: row.clone(),
                    title: "New partition".into(),
                    usage: use_of(&n.mount),
                    erase: true,
                    fs: n.fs,
                    folder: folder_of(&n.mount),
                    bytes,
                    min: GIB.min(bytes),
                    max: bytes + spare,
                }
            }
            Row::Free(bytes) => Edit {
                row: row.clone(),
                title: "New partition".into(),
                usage: Use::System,
                erase: true,
                fs: Fs::Btrfs,
                folder: String::new(),
                bytes: *bytes,
                min: GIB.min(*bytes),
                max: *bytes,
            },
        };
        self.edit = Some(e);
        self.go(Step::Edit)
    }

    /// The edit page's values into the layout; only the partition it was opened on changes.
    fn apply_edit(&mut self) -> Result<(), String> {
        let Some(e) = self.edit.clone() else {
            return Ok(());
        };
        let mount = mount_of(e.usage, &e.folder);
        if e.usage == Use::Folder
            && !mount
                .as_deref()
                .is_some_and(wol_core::install::layout::mount_allowed)
        {
            return Err(copy::install_refusal(
                &Refusal::BadMount(String::new()),
                &Disk::default(),
            ));
        }
        let fs = match e.usage {
            Use::Startup => Fs::Vfat,
            Use::Swap => Fs::Swap,
            _ => e.fs,
        };
        match &e.row {
            Row::Part(uuid) => {
                self.manual.assign.retain(|a| a.part != *uuid);
                self.manual.resize.retain(|r| r.part != *uuid);
                self.manual.delete.retain(|p| p != uuid);
                if let Some(mount) = mount {
                    self.manual.assign.push(Assign {
                        part: uuid.clone(),
                        mount,
                        format: e.erase.then_some(fs),
                    });
                }
                if e.bytes != e.max {
                    self.manual.resize.push(Resize {
                        part: uuid.clone(),
                        bytes: e.bytes,
                    });
                }
            }
            Row::New(i) => match mount {
                Some(mount) => {
                    self.manual.create[*i] = New {
                        bytes: Some(e.bytes),
                        mount,
                        fs,
                    }
                }
                None => {
                    self.manual.create.remove(*i);
                }
            },
            Row::Free(_) => {
                if let Some(mount) = mount {
                    self.manual.create.push(New {
                        bytes: Some(e.bytes),
                        mount,
                        fs,
                    });
                }
            }
        }
        self.edit = None;
        Ok(())
    }

    /// Delete on the edit page: the partition goes on the list of things to delete, nothing else.
    pub fn delete_edited(&mut self) {
        let Some(e) = self.edit.take() else {
            return;
        };
        match e.row {
            Row::Part(uuid) => {
                self.manual.assign.retain(|a| a.part != uuid);
                self.manual.resize.retain(|r| r.part != uuid);
                if !self.manual.delete.contains(&uuid) {
                    self.manual.delete.push(uuid);
                }
            }
            Row::New(i) => {
                self.manual.create.remove(i);
            }
            Row::Free(_) => {}
        }
        self.back();
    }

    /// The summary, one sentence per change.
    pub fn summary(&self) -> Vec<String> {
        let (Some(d), Ok(c)) = (self.disk(), self.compiled()) else {
            return Vec::new();
        };
        changes(&self.layout(), &c, d, self.protect)
            .iter()
            .map(copy::install_change)
            .collect()
    }

    /// The warning over the Install button.
    pub fn summary_warning(&self) -> Option<String> {
        let c = self.compiled().ok()?;
        let d = self.disk()?;
        let l = self.layout();
        let erases = l.wipe || !l.delete.is_empty() || !l.chosen().is_empty();
        if c.root_bytes < ROOMY_ROOT {
            Some(copy::install_low_space(c.root_bytes))
        } else if l.wipe && !d.others().is_empty() {
            Some(super::erase_warning(&d.others()))
        } else if erases {
            Some(copy::INSTALL_POINT_OF_NO_RETURN.to_string())
        } else {
            None
        }
    }

    /// The warning on the How to Install page; None: nothing to warn about.
    pub fn method_warning(&self) -> Option<String> {
        match (self.method, self.disk()) {
            (Method::Erase, Some(d)) => Some(super::erase_warning(&d.others())),
            (Method::Alongside, _) => self.alongside_problem(),
            _ => None,
        }
    }

    /// What `install-run` gets. Passwords are side files, never in here.
    pub fn plan(&self) -> Option<Plan> {
        let d = self.disk()?;
        let a = self.draft.account();
        Some(Plan {
            disk: d.path.clone(),
            fingerprint: d.fingerprint(),
            layout: self.layout(),
            protect: self.protect,
            locale: LANGUAGES[self.draft.language.min(LANGUAGES.len() - 1)]
                .locale
                .into(),
            keyboard: KEYBOARDS[self.draft.keyboard.min(KEYBOARDS.len() - 1)]
                .0
                .into(),
            zone: self.zone.clone(),
            account: Some(Account {
                name: a.name,
                full_name: a.full_name,
                tone: a.avatar,
            }),
        })
    }
}

fn folder_of(mount: &str) -> String {
    if use_of(mount) == Use::Folder {
        mount.trim_start_matches('/').to_string()
    } else {
        String::new()
    }
}

/// How a disk reads in a list.
pub trait DiskTitle {
    fn title_text(&self) -> String;
}

impl DiskTitle for Disk {
    /// The model, else the device name.
    fn title_text(&self) -> String {
        if self.model.is_empty() {
            self.path.clone()
        } else {
            self.model.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wol_core::install::disk::{DATA_TYPE, ESP_TYPE, Part};
    use wol_core::install::layout::Slots;

    const GB: u64 = 1_000_000_000;

    fn part(n: u32, start_gb: u64, gb: u64, kind: &str, fs: &str) -> Part {
        Part {
            number: n,
            path: format!("/dev/vda{n}"),
            start: start_gb * GB / 512 / 2048 * 2048 + 2048,
            sectors: gb * GB / 512 / 2048 * 2048,
            kind: kind.into(),
            uuid: format!("u{n}"),
            fstype: fs.into(),
            fsuuid: format!("f{n}"),
            ..Part::default()
        }
    }

    /// 500 GB: a 100 MB-ish ESP, Windows 300 GB with 100 GB used, a Linux /home of 100 GB.
    fn dual() -> Disk {
        let mut esp = part(1, 0, 1, ESP_TYPE, "vfat");
        esp.free_bytes = Some(800 * MIB);
        let mut win = part(2, 2, 300, "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7", "ntfs");
        win.min_bytes = Some(100 * GB);
        let mut home = part(3, 310, 100, DATA_TYPE, "ext4");
        home.min_bytes = Some(20 * GB);
        Disk {
            path: "/dev/vda".into(),
            model: "WD Blue".into(),
            bytes: 500 * GB,
            sector: 512,
            table: Table::Gpt,
            id: "t".into(),
            first: 2048,
            last: 500 * GB / 512 - 34,
            parts: vec![esp, win, home],
        }
    }

    fn flow(d: Disk) -> Flow {
        let mut f = Flow::new(&Hints::default(), true, false);
        f.set_disks(vec![Found {
            disk: d,
            check: super::super::Check::default(),
        }]);
        f
    }

    fn to_method(f: &mut Flow) {
        for s in [Step::Language, Step::Zone, Step::Disk, Step::Method] {
            assert_eq!(f.advance(), Next::Go(s));
        }
    }

    fn fill_account(f: &mut Flow) {
        f.draft.set_full_name("Alice Liddell");
        f.draft.password = "curiouser".into();
        f.draft.verify = "curiouser".into();
    }

    #[test]
    fn the_steps_of_an_erase_are_one_decision_each_and_back_retraces_them() {
        let mut f = flow(dual());
        assert_eq!(f.step, Step::Begin);
        to_method(&mut f);
        f.method = Method::Erase;
        assert_eq!(f.advance(), Next::Go(Step::Protect));
        assert_eq!(f.advance(), Next::Go(Step::Account));
        assert!(matches!(f.advance(), Next::Stay(Some(Field::FullName), _)));
        fill_account(&mut f);
        assert_eq!(f.advance(), Next::Go(Step::Summary));
        assert_eq!(f.back(), Some(Step::Account));
        assert_eq!(f.back(), Some(Step::Protect));
        assert_eq!(f.back(), Some(Step::Method), "Alongside was never shown");
        for s in [Step::Protect, Step::Account, Step::Summary] {
            assert_eq!(f.advance(), Next::Go(s));
        }
        assert_eq!(f.advance(), Next::Install);
        assert!(f.plan().unwrap().layout.wipe);
    }

    #[test]
    fn from_the_live_desktop_there_is_no_begin_page() {
        let f = Flow::new(&Hints::default(), false, false);
        assert_eq!(f.step, Step::Language);
    }

    #[test]
    fn the_zone_is_guessed_from_the_machine_then_the_language() {
        let h = |z: &str| Hints {
            zone: Some(z.into()),
            ..Hints::default()
        };
        assert_eq!(guess_zone(&h("Europe/Paris"), 0), "Europe/Paris");
        assert_eq!(
            guess_zone(&h("UTC"), 2),
            "America/Sao_Paulo",
            "live images say UTC"
        );
        assert_eq!(guess_zone(&Hints::default(), 6), "Europe/Berlin");
        let mut f = Flow::new(&Hints::default(), false, false);
        f.set_language(5);
        assert_eq!(f.zone, "Europe/Paris", "follows the language");
        f.set_zone("Asia/Tokyo");
        f.set_language(2);
        assert_eq!(f.zone, "Asia/Tokyo", "until the person chooses");
    }

    #[test]
    fn zone_search_finds_cities_as_people_type_them() {
        let all: Vec<String> = [
            "America/New_York",
            "America/Sao_Paulo",
            "Europe/London",
            "Asia/Sao_Tome",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(zones_matching(&all, "sao paulo"), [&all[1]]);
        assert_eq!(zones_matching(&all, "NEW"), [&all[0]]);
        assert_eq!(zones_matching(&all, "").len(), 4);
        assert!(zones_matching(&all, "atlantis").is_empty());
        assert_eq!(zones_matching(&all, "lon")[0], &all[2]);
    }

    #[test]
    fn alongside_defaults_when_it_can_and_the_slider_changes_the_shrink() {
        let mut f = flow(dual());
        assert_eq!(f.method, Method::Alongside);
        assert_eq!(f.alongside_title(), "Install Alongside Windows");
        to_method(&mut f);
        assert_eq!(f.advance(), Next::Go(Step::Alongside));
        let (uuid, name, lo, hi) = f.alongside_slider().unwrap();
        assert_eq!((uuid.as_str(), name.as_str()), ("u2", "Windows"));
        assert!(lo >= 104 * GB && lo < hi && hi <= 300 * GB);
        assert!(f.keep > lo && f.keep < hi, "starts in the middle");
        f.keep = lo;
        let most = f.alongside_room().unwrap();
        f.keep = hi;
        assert!(f.alongside_room().unwrap() < most);
        assert!(
            f.alongside_room().unwrap() >= MIN_ROOT,
            "the end of the slider still fits"
        );
        assert_eq!(f.advance(), Next::Go(Step::Protect));
        let lines = f.summary();
        assert!(
            lines.iter().any(|l| l.starts_with("Shrink Windows")),
            "{lines:?}"
        );
        assert!(lines.iter().any(|l| l.contains("Linux stays")), "{lines:?}");
    }

    #[test]
    fn alongside_says_why_not() {
        let mut d = dual();
        d.parts[1].blocked = Some(wol_core::install::disk::Blocked::BitLocker);
        d.parts[2].min_bytes = None;
        // No free space to fall back on.
        d.bytes = 411 * GB;
        d.last = d.bytes / 512 - 34;
        let f = flow(d);
        assert_eq!(f.method, Method::Erase, "the default falls back");
        assert!(f.alongside_problem().unwrap().contains("BitLocker"));
        let mut mbr = dual();
        mbr.table = Table::Other;
        assert!(
            flow(mbr)
                .alongside_problem()
                .unwrap()
                .contains("Erase Disk")
        );
        let mut blank = dual();
        blank.parts.clear();
        blank.table = Table::None;
        assert!(flow(blank).alongside_problem().is_some());
    }

    #[test]
    fn manual_keeps_home_and_touches_only_what_the_person_chose() {
        let mut f = flow(dual());
        to_method(&mut f);
        f.method = Method::Manual;
        assert_eq!(f.advance(), Next::Go(Step::Manual));
        assert!(matches!(f.advance(), Next::Stay(None, _)), "no system yet");
        // Free space becomes the system.
        let lines = manual_lines(&f.manual, f.disk().unwrap());
        assert_eq!(lines.len(), 5, "{lines:?}"); // three partitions and two gaps
        let free = lines
            .iter()
            .filter_map(|l| match l.0 {
                Row::Free(b) => Some(b),
                _ => None,
            })
            .max()
            .unwrap();
        assert_eq!(f.open_edit(&Row::Free(free)), Next::Go(Step::Edit));
        assert_eq!(f.edit.as_ref().unwrap().usage, Use::System);
        assert_eq!(f.advance(), Next::Go(Step::Manual));
        // The startup partition stays, with WolOS added.
        f.open_edit(&Row::Part("u1".into()));
        f.edit.as_mut().unwrap().usage = Use::Startup;
        f.advance();
        // /home is kept.
        f.open_edit(&Row::Part("u3".into()));
        let e = f.edit.as_mut().unwrap();
        e.usage = Use::Home;
        assert!(!e.erase, "keeping is the default");
        f.advance();
        assert_eq!(f.advance(), Next::Go(Step::Protect));
        let l = f.layout();
        assert!(l.chosen().is_empty(), "nothing existing changes: {l:?}");
        assert_eq!(f.compiled().unwrap().slots, Slots::Two);
        let lines = f.summary();
        assert!(
            lines.iter().any(|l| l.contains("Windows stays")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.starts_with("Keep Linux")),
            "{lines:?}"
        );
    }

    #[test]
    fn a_layout_that_can_not_keep_two_copies_says_so_in_the_summary() {
        let mut f = flow(dual());
        f.method = Method::Manual;
        f.manual.create.push(New {
            bytes: Some(30 * GB),
            mount: "/".into(),
            fs: Fs::Ext4,
        });
        f.manual.assign.push(Assign {
            part: "u1".into(),
            mount: "/efi".into(),
            format: None,
        });
        assert_eq!(f.compiled().unwrap().slots, Slots::One);
        let lines = f.summary();
        assert!(lines.iter().any(|l| l.contains("one copy")), "{lines:?}");
    }

    #[test]
    fn deleting_goes_through_the_edit_page_and_can_be_taken_back() {
        let mut f = flow(dual());
        to_method(&mut f);
        f.method = Method::Manual;
        f.advance();
        f.open_edit(&Row::Part("u2".into()));
        f.delete_edited();
        assert_eq!(f.step, Step::Manual);
        assert_eq!(f.manual.delete, ["u2"]);
        let lines = manual_lines(&f.manual, f.disk().unwrap());
        assert!(lines[1].2.starts_with("Will be deleted"), "{lines:?}");
        assert!(
            lines
                .iter()
                .filter(|l| matches!(l.0, Row::Free(_)))
                .any(|l| { matches!(l.0, Row::Free(b) if b > 250 * GB) }),
            "the room shows at once: {lines:?}"
        );
        // Opening it again and choosing a job takes the delete back.
        f.open_edit(&Row::Part("u2".into()));
        f.edit.as_mut().unwrap().usage = Use::Folder;
        f.edit.as_mut().unwrap().folder = "../etc".into();
        assert!(matches!(f.advance(), Next::Stay(None, _)), "bad name stays");
        f.edit.as_mut().unwrap().folder = "windows".into();
        assert_eq!(f.advance(), Next::Go(Step::Manual));
        assert!(f.manual.delete.is_empty());
    }

    #[test]
    fn a_new_partition_can_be_changed_or_removed() {
        let mut f = flow(dual());
        to_method(&mut f);
        f.method = Method::Manual;
        f.advance();
        f.open_edit(&Row::Free(40 * GB));
        f.edit.as_mut().unwrap().bytes = 30 * GB;
        f.advance();
        assert_eq!(f.manual.create[0].bytes, Some(30 * GB));
        f.open_edit(&Row::New(0));
        assert!(f.edit.as_ref().unwrap().max > 30 * GB);
        f.delete_edited();
        assert!(f.manual.create.is_empty());
    }

    #[test]
    fn a_disk_password_is_asked_for_and_stays_out_of_the_plan() {
        let mut f = flow(dual());
        f.method = Method::Erase;
        f.step = Step::Protect;
        f.protect = Protect::Password;
        f.disk_password = "short".into();
        assert!(matches!(f.advance(), Next::Stay(Some(Field::Password), _)));
        f.disk_password = "white rabbit".into();
        f.disk_verify = "white rabbi".into();
        assert!(matches!(f.advance(), Next::Stay(Some(Field::Verify), _)));
        f.disk_verify = "white rabbit".into();
        assert_eq!(f.advance(), Next::Go(Step::Account));
        fill_account(&mut f);
        let json = serde_json::to_string(&f.plan().unwrap()).unwrap();
        assert!(
            !json.contains("rabbit") && !json.contains("curiouser"),
            "{json}"
        );
    }

    #[test]
    fn the_security_chip_is_the_default_only_when_there_is_one() {
        assert_eq!(
            Flow::new(&Hints::default(), false, true).protect,
            Protect::Tpm
        );
        assert_eq!(
            Flow::new(&Hints::default(), false, false).protect,
            Protect::Off
        );
    }

    #[test]
    fn secure_boot_is_said_before_anything_is_written() {
        let mut f = flow(dual());
        f.disks[0].check.secure_boot = true;
        f.method = Method::Erase;
        f.step = Step::Summary;
        assert_eq!(f.advance(), Next::Go(Step::SecureBoot));
        assert_eq!(f.advance(), Next::Install);
    }

    #[test]
    fn a_refused_disk_stops_at_the_disk_page() {
        let mut f = flow(dual());
        f.disks[0].check.refused = Some("not-listed".into());
        f.step = Step::Disk;
        assert!(matches!(f.advance(), Next::Stay(None, _)));
    }

    #[test]
    fn low_space_and_other_systems_are_warned_about() {
        let mut f = flow(dual());
        f.method = Method::Erase;
        assert!(f.summary_warning().unwrap().contains("Windows"));
        let mut small = dual();
        small.bytes = 30 * GB;
        small.parts.clear();
        small.table = Table::None;
        let mut f = flow(small);
        f.method = Method::Erase;
        assert!(f.summary_warning().unwrap().contains("30 GB"));
    }

    #[test]
    fn every_title_and_sentence_here_passes_the_lint() {
        for s in STEPS {
            assert!(!s.title().is_empty());
            if !s.subtitle().is_empty() && s != Step::SecureBoot {
                let l = crate::copy::lint(s.subtitle());
                assert!(l.is_empty(), "{s:?}: {l:?}");
            }
        }
    }
}
