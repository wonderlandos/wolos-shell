// SPDX-License-Identifier: GPL-2.0-or-later
//! Install WolOS step by step (ADR 0023): one decision per page, pure. `ui::installer` draws a
//! page for each [`Step`]; everything it says, and the plan it hands to `install-run`, comes from
//! here. Layouts are `wol_core::install`'s: the summary and the run read the same data.
use super::size_text;
use crate::copy;
use crate::setup::{Draft, Field, Hints, KEYBOARDS, LANGUAGES};
use wol_core::boot::{Tier, changes_windows_keys};
use wol_core::install::disk::{Disk, GIB, MIB, Table};
use wol_core::install::layout::{
    Assign, Compiled, Fs, Layout, MIN_ROOT, New, ROOMY_ROOT, Refusal, Resize, compile, room, verify,
};
use wol_core::install::plan::{
    Account, Plan, Protect, alongside, alongside_blocked, alongside_range, alongside_target,
    changes, console_typable, keeps_bitlocker, keeps_windows, other_boot, replace, unlock_problem,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Install or Try, on the first screen of the live image only.
    Begin,
    Language,
    Zone,
    Disk,
    Method,
    /// Which partition WolOS takes over.
    Replace,
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
    /// Installed and encrypted, but `install-check --result` gave no recovery key: Restart is not
    /// offered until it is read (Try Again reads it again).
    KeyUnread,
}

/// Every page, in the order they are added to the pager.
pub const STEPS: [Step; 17] = [
    Step::Begin,
    Step::Language,
    Step::Zone,
    Step::Disk,
    Step::Method,
    Step::Replace,
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
    Step::KeyUnread,
];

impl Step {
    pub fn id(self) -> &'static str {
        match self {
            Step::Begin => "begin",
            Step::Language => "language",
            Step::Zone => "zone",
            Step::Disk => "disk",
            Step::Method => "method",
            Step::Replace => "replace",
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
            Step::KeyUnread => "key-unread",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Step::Begin => "Welcome to WolOS",
            Step::Language => "Language and Keyboard",
            Step::Zone => "Time Zone",
            Step::Disk => "Choose a Disk",
            Step::Method => "How to Install",
            Step::Replace => "Replace a Partition",
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
            Step::KeyUnread => "The Recovery Key Could Not Be Read",
        }
    }

    pub fn subtitle(self) -> &'static str {
        copy::installer::subtitle(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Erase,
    Alongside,
    /// One existing partition becomes the WolOS system; the rest stays.
    Replace,
    Manual,
}

/// Swap for Erase, Alongside and Replace (Manual has its own Swap use). No hibernation: the
/// kernel runs locked down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Swap {
    /// Compressed memory (zram).
    Zram,
    /// A swap file on the system partition, this many GiB.
    Swapfile(u32),
    None,
}

impl Swap {
    /// `WOLOS_SWAP` for install-run.
    pub fn word(self) -> String {
        match self {
            Swap::Zram => "zram".into(),
            Swap::Swapfile(gib) => format!("swapfile:{gib}"),
            Swap::None => "none".into(),
        }
    }
}

/// The swap choices, in order: (choice, title, detail).
pub const SWAPS: [(Swap, &str, &str); 3] = [
    (
        Swap::Zram,
        "Compressed memory",
        "Recommended. Uses no disk space",
    ),
    (
        Swap::Swapfile(0),
        "Swap file",
        "A file on the system partition",
    ),
    (Swap::None, "None", "For computers with plenty of memory"),
];

/// Swap file size for a computer with `mem_bytes` of memory: as much as the memory, 2 to 8 GiB.
pub fn swapfile_gib(mem_bytes: u64) -> u32 {
    (mem_bytes / GIB).clamp(2, 8) as u32
}

/// `MemTotal:` of /proc/meminfo, bytes.
pub fn meminfo_bytes(text: &str) -> Option<u64> {
    text.lines()
        .find_map(|l| l.strip_prefix("MemTotal:"))
        .and_then(|v| v.split_whitespace().next()?.parse::<u64>().ok())
        .map(|kib| kib * 1024)
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
    /// The partition Replace takes over (GPT UUID).
    pub replace: Option<String>,
    pub swap: Swap,
    /// Swap file size when that is the choice (GiB).
    pub swapfile_gib: u32,
    /// The computer's name; None: follows the account name.
    hostname: Option<String>,
    /// Sign in without a password at start.
    pub autologin: bool,
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
            replace: None,
            swap: Swap::Zram,
            swapfile_gib: swapfile_gib(4 * GIB),
            hostname: None,
            autologin: false,
            manual: Layout::default(),
            edit: None,
            tpm,
            // Encrypted by default: the chip when there is one, else a password.
            protect: if tpm { Protect::Tpm } else { Protect::Password },
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

    /// The keyboard list shows `i`. `by_person`: the person chose it (the page's list changed
    /// outside its own filling), which is a pick even when it is the layout the language
    /// suggested; else the page made the list follow a language, which the draft already holds
    /// (B8, audit3 R2B-8). Returns whether it was recorded as the person's pick.
    pub fn keyboard_shown(&mut self, i: usize, by_person: bool) -> bool {
        if by_person {
            self.draft.set_keyboard(i);
        }
        by_person
    }

    pub fn set_zone(&mut self, z: &str) {
        self.zone = z.to_string();
        self.zone_chosen = true;
    }

    /// The computer's memory: sizes the swap file.
    pub fn set_memory(&mut self, bytes: u64) {
        self.swapfile_gib = swapfile_gib(bytes);
    }

    /// The swap the plan gets (the file sized for this computer).
    pub fn swap(&self) -> Swap {
        match self.swap {
            Swap::Swapfile(_) => Swap::Swapfile(self.swapfile_gib),
            s => s,
        }
    }

    /// The computer's name: typed, else from the account name.
    pub fn hostname(&self) -> String {
        self.hostname
            .clone()
            .unwrap_or_else(|| super::hostname_for(&self.draft.name))
    }

    /// The person typed a name; empty goes back to following the account.
    pub fn set_hostname(&mut self, h: &str) {
        let h = h.trim();
        self.hostname = (!h.is_empty()).then(|| h.to_string());
    }

    /// The plan file's keys beyond [`Plan`]'s own: `hostname`, `autologin` and `swap`, which
    /// `wol install info` turns into `WOLOS_HOSTNAME`, `WOLOS_AUTOLOGIN` and `WOLOS_SWAP`.
    pub fn plan_extras(&self) -> Vec<(&'static str, serde_json::Value)> {
        vec![
            ("hostname", serde_json::Value::String(self.hostname())),
            ("autologin", serde_json::Value::Bool(self.autologin)),
            ("swap", serde_json::Value::String(self.swap().word())),
        ]
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
        self.replace = None;
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

    /// Partitions Replace can take over: (uuid, title, detail), in disk order. Not the startup
    /// partition, not one in use, and big enough for WolOS.
    pub fn replace_candidates(&self) -> Vec<(String, String, String)> {
        let Some(d) = self.disk() else {
            return Vec::new();
        };
        d.parts
            .iter()
            .filter(|p| !p.is_esp() && !p.mounted && d.bytes_of(p) >= MIN_ROOT)
            .map(|p| {
                let what = p
                    .system()
                    .or_else(|| (!p.fstype.is_empty()).then(|| p.fstype.clone()))
                    .unwrap_or_else(|| "Not formatted".into());
                (
                    p.uuid.clone(),
                    p.title(),
                    format!("{what} · {}", size_text(d.bytes_of(p))),
                )
            })
            .collect()
    }

    /// Why Replace can't be chosen on this disk; None: it can.
    pub fn replace_problem(&self) -> Option<String> {
        let d = self.disk()?;
        if d.table == Table::Other {
            return Some(copy::install_refusal(&Refusal::NotGpt, d));
        }
        if self.replace_candidates().is_empty() {
            return Some(copy::INSTALL_NOTHING_TO_REPLACE.to_string());
        }
        None
    }

    /// Replace: the chosen partition is erased for the system; the startup partition is reused
    /// when it has room, else WolOS gets its own.
    /// Replace's layout is core's (`plan::replace`): a small Windows startup partition and a full
    /// disk give the new one the tail of the replaced partition (audit2 B6).
    fn replace_layout(&self, d: &Disk, uuid: &str) -> Layout {
        replace(d, uuid)
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
            Method::Replace => match &self.replace {
                Some(uuid) => self.replace_layout(d, uuid),
                None => Layout::default(),
            },
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
        if matches!(self.step, Step::Installing | Step::Done | Step::KeyUnread) {
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

    /// Try Again after a stop: the disk page, with Back working as if the person had just come
    /// from the first pages (the disk may be anything now, so the plan starts there).
    pub fn restart(&mut self) {
        self.trail = if self.boot {
            vec![Step::Begin, Step::Language, Step::Zone]
        } else {
            vec![Step::Language, Step::Zone]
        };
        self.step = Step::Disk;
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
                    return stay(copy::installer::CHOOSE_DISK);
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
                Method::Replace => match self.replace_problem() {
                    Some(p) => stay(p),
                    None => self.go(Step::Replace),
                },
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
            Step::Replace => {
                if self.replace.is_none() {
                    return stay(copy::installer::CHOOSE_REPLACE);
                }
                self.after_layout()
            }
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
                            copy::installer::PASSWORD_SHORT.into(),
                        );
                    }
                    if !passphrase_typable(&self.disk_password) {
                        return Next::Stay(
                            Some(Field::Password),
                            copy::INSTALL_PASSPHRASE_ASCII.into(),
                        );
                    }
                    if self.disk_password != self.disk_verify {
                        return Next::Stay(
                            Some(Field::Verify),
                            copy::installer::PASSWORDS_DIFFER.into(),
                        );
                    }
                }
                self.go(Step::Account)
            }
            Step::Account => match self.draft.account_problem() {
                Some((f, why)) => Next::Stay(Some(f), why.into()),
                // The chip's fallback is the account password, typed on the console keymap (B9).
                None if unlock_problem(self.protect, Some(&self.draft.password)).is_some() => {
                    Next::Stay(Some(Field::Password), copy::INSTALL_TPM_FALLBACK.into())
                }
                None if !super::is_valid_hostname(&self.hostname()) => {
                    Next::Stay(Some(Field::Hostname), copy::installer::HOSTNAME_RULE.into())
                }
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
            Step::Installing | Step::Done | Step::Failed | Step::KeyUnread => Next::Go(self.step),
        }
    }

    /// The plan encrypted the disk, so install-run made a recovery key, yet none came back: the
    /// person must not restart without it.
    pub fn key_unread(&self, key: Option<&str>) -> bool {
        self.protect != Protect::Off && key.is_none_or(|k| k.trim().is_empty())
    }

    pub fn secure_boot(&self) -> bool {
        self.chosen
            .and_then(|i| self.disks.get(i))
            .is_some_and(|f| f.check.secure_boot)
    }

    /// The page opens on `row`.
    pub fn open_edit(&mut self, row: &Row) -> Next {
        let Some(d) = self.disk().cloned() else {
            return stay(copy::installer::CHOOSE_DISK);
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

    /// The summary, one sentence per change, then the settings that are not about the disk.
    pub fn summary(&self) -> Vec<String> {
        let (Some(d), Ok(c)) = (self.disk(), self.compiled()) else {
            return Vec::new();
        };
        let mut v: Vec<String> = changes(&self.layout(), &c, d, self.protect)
            .iter()
            .map(copy::install_change)
            .collect();
        // BitLocker next: what may lock the person out of Windows is said before the details.
        v.extend(self.bitlocker_notes(false).into_iter().map(String::from));
        if self.method != Method::Manual
            && let Some(line) = copy::install_swap(self.swap())
        {
            v.push(line);
        }
        if let Some(w) = self.windows_start() {
            v.push(w);
        }
        if self.windows_stays() {
            v.push(copy::installer::CLOCK_LOCAL.to_string());
        }
        v.push(copy::install_hostname(&self.hostname()));
        if self.autologin {
            v.push(copy::INSTALL_AUTOLOGIN.to_string());
        }
        v
    }

    /// Another system stays, but WolOS gets a startup partition of its own: its menu cannot list
    /// it, so the person is told how that system starts (Summary and Done pages). None: it goes,
    /// or shares the startup partition WolOS reuses and stays in its menu (R2B-11).
    pub fn windows_start(&self) -> Option<String> {
        // The same answer `wol install info` gives, from core's one "what stays" rule.
        other_boot(&self.layout(), self.disk()?).map(|name| copy::installer::other_start(&name))
    }

    /// The other disks the installer saw (they are never written).
    fn other_disks(&self) -> impl Iterator<Item = &Found> {
        self.disks
            .iter()
            .enumerate()
            .filter(move |(i, _)| Some(*i) != self.chosen)
            .map(|(_, f)| f)
    }

    /// Windows is on this computer after the install: on this disk by core's rule
    /// (`plan::keeps_windows`), or on another disk (`os Windows`). install-run decides the
    /// clock by the same two facts and keeps it in local time (R2B-12).
    pub fn windows_stays(&self) -> bool {
        self.disk()
            .is_some_and(|d| keeps_windows(&self.layout(), d))
            || self
                .other_disks()
                .any(|f| f.check.others.iter().any(|o| o == "Windows"))
    }

    /// A Windows that stays has BitLocker on: on this disk by core's rule
    /// (`plan::keeps_bitlocker`), or on another (`bitlocker yes`), as install-run reads it (R2B-9).
    pub fn bitlocker_stays(&self) -> bool {
        self.disk()
            .is_some_and(|d| keeps_bitlocker(&self.layout(), d))
            || self.other_disks().any(|f| f.check.bitlocker)
    }

    /// The protection tier install-check named for this computer (`own-keys`, `firmware`, ...).
    pub fn tier(&self) -> Option<&str> {
        self.disks
            .get(self.chosen?)
            .and_then(|f| f.check.tier.as_deref())
    }

    /// What Summary (`done` false), the Secure Boot sheet and Done (`done` true) say about
    /// BitLocker: only when a Windows with BitLocker stays and this install changes the Secure
    /// Boot keys Windows measures (own keys, or the firmware setting). Empty: nothing to say.
    pub fn bitlocker_notes(&self, done: bool) -> Vec<&'static str> {
        // Which tiers change the keys is core's rule, the one boot-setup's note follows too.
        match self.tier().and_then(Tier::parse) {
            Some(t) if self.bitlocker_stays() && changes_windows_keys(t) => {
                copy::installer::bitlocker(t.as_str(), done)
            }
            _ => Vec::new(),
        }
    }

    /// The warning over the Install button. Don't Encrypt is said once more here.
    pub fn summary_warning(&self) -> Option<String> {
        let c = self.compiled().ok()?;
        let d = self.disk()?;
        let l = self.layout();
        let erases = l.wipe || !l.delete.is_empty() || !l.chosen().is_empty();
        let disk = if c.root_bytes < ROOMY_ROOT {
            Some(copy::install_low_space(c.root_bytes))
        } else if l.wipe && !d.others().is_empty() {
            Some(super::erase_warning(&d.others()))
        } else if erases {
            Some(copy::INSTALL_POINT_OF_NO_RETURN.to_string())
        } else {
            None
        };
        match (disk, self.protect == Protect::Off) {
            (Some(w), true) => Some(format!("{w} {}", copy::INSTALL_UNENCRYPTED)),
            (None, true) => Some(copy::INSTALL_UNENCRYPTED.to_string()),
            (w, false) => w,
        }
    }

    /// The warning on the How to Install page; None: nothing to warn about.
    pub fn method_warning(&self) -> Option<String> {
        match (self.method, self.disk()) {
            (Method::Erase, Some(d)) => Some(super::erase_warning(&d.others())),
            (Method::Alongside, _) => self.alongside_problem(),
            (Method::Replace, _) => self.replace_problem(),
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

/// The disk password is typed before any desktop exists, on the console keymap: printable ASCII
/// is what every keymap types the same.
pub fn passphrase_typable(p: &str) -> bool {
    console_typable(p)
}

/// How a disk reads under its name: size, device and what kind of disk (from the device name).
pub fn disk_detail(d: &Disk) -> String {
    let dev = d.path.rsplit('/').next().unwrap_or(&d.path);
    let bus = if dev.starts_with("nvme") {
        Some("NVMe")
    } else if dev.starts_with("mmcblk") {
        Some("SD card or eMMC")
    } else if dev.starts_with("vd") || dev.starts_with("xvd") {
        Some("Virtual disk")
    } else {
        None
    };
    match bus {
        Some(b) => format!("{} · {} · {b}", size_text(d.bytes), d.path),
        None => format!("{} · {}", size_text(d.bytes), d.path),
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
        // Encrypted by default: no chip in the test, so the disk password is asked for first.
        assert!(matches!(f.advance(), Next::Stay(Some(Field::Password), _)));
        f.disk_password = "white rabbit".into();
        f.disk_verify = "white rabbit".into();
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
        // The install is encrypted (a password: no chip in the test); the kept volume is not.
        assert_ne!(f.protect, Protect::Off);
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("Keep Linux") && l.ends_with("It stays unencrypted.")),
            "{lines:?}"
        );
        f.protect = Protect::Off;
        assert!(
            !f.summary().iter().any(|l| l.contains("unencrypted")),
            "nothing to say when nothing is encrypted"
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
        let extras = format!("{:?}", f.plan_extras());
        assert!(
            !extras.contains("rabbit") && !extras.contains("curiouser"),
            "{extras}"
        );
    }

    #[test]
    fn encryption_is_the_default_by_chip_when_there_is_one_else_by_password() {
        assert_eq!(
            Flow::new(&Hints::default(), false, true).protect,
            Protect::Tpm
        );
        assert_eq!(
            Flow::new(&Hints::default(), false, false).protect,
            Protect::Password
        );
        // Don't Encrypt stays a choice, and the summary says so once more.
        let mut f = flow(dual());
        f.method = Method::Erase;
        f.protect = Protect::Off;
        let w = f.summary_warning().unwrap();
        assert!(
            w.contains("Windows") && w.contains("won't be encrypted"),
            "{w}"
        );
        f.protect = Protect::Tpm;
        assert!(!f.summary_warning().unwrap().contains("encrypted"));
        let mut blank = dual();
        blank.parts.clear();
        blank.table = Table::None;
        let mut f = flow(blank);
        f.method = Method::Erase;
        f.protect = Protect::Off;
        let w = f.summary_warning().unwrap();
        assert!(w.ends_with(copy::INSTALL_UNENCRYPTED), "{w}");
    }

    #[test]
    fn the_disk_password_is_typable_before_any_desktop() {
        let mut f = flow(dual());
        f.method = Method::Erase;
        f.step = Step::Protect;
        f.protect = Protect::Password;
        f.disk_password = "café-au-lait".into();
        f.disk_verify = "café-au-lait".into();
        let n = f.advance();
        assert!(
            matches!(&n, Next::Stay(Some(Field::Password), why) if why == copy::INSTALL_PASSPHRASE_ASCII),
            "{n:?}"
        );
        f.disk_password = "white rabbit!".into();
        f.disk_verify = "white rabbit!".into();
        assert_eq!(f.advance(), Next::Go(Step::Account));
        assert!(passphrase_typable("Abc 123 ~!@#"));
        assert!(!passphrase_typable("tab\there"));
    }

    #[test]
    fn replace_takes_one_partition_and_keeps_the_rest() {
        let mut f = flow(dual());
        to_method(&mut f);
        f.method = Method::Replace;
        assert_eq!(f.replace_problem(), None);
        assert_eq!(f.advance(), Next::Go(Step::Replace));
        assert!(matches!(f.advance(), Next::Stay(None, _)), "nothing chosen");
        let c = f.replace_candidates();
        assert_eq!(c.len(), 2, "{c:?}");
        assert_eq!((c[0].1.as_str(), c[1].1.as_str()), ("Windows", "Linux"));
        assert!(c[1].2.contains("100 GB"), "{c:?}");
        f.replace = Some("u3".into());
        assert_eq!(f.advance(), Next::Go(Step::Protect));
        let l = f.layout();
        assert_eq!(l.assign.len(), 2, "{l:?}");
        assert!(l.create.is_empty(), "the startup partition is reused");
        assert_eq!(l.chosen().len(), 1);
        let lines = f.summary();
        assert!(
            lines.iter().any(|l| l.starts_with("Erase Linux")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.contains("Windows stays")),
            "{lines:?}"
        );
        assert_eq!(f.back(), Some(Step::Replace));
        assert_eq!(f.back(), Some(Step::Method));
        // No room for a new startup partition, and nothing to take over: Replace says why.
        let mut blank = dual();
        blank.parts.clear();
        blank.table = Table::None;
        let f = flow(blank);
        assert!(f.replace_problem().is_some());
        let mut mbr = dual();
        mbr.table = Table::Other;
        assert!(flow(mbr).replace_problem().unwrap().contains("Erase Disk"));
    }

    #[test]
    fn swap_name_and_automatic_login_reach_the_summary_and_the_run() {
        let mut f = flow(dual());
        f.method = Method::Erase;
        fill_account(&mut f);
        assert_eq!(f.hostname(), "aliceliddell-wolos", "follows the account");
        assert_eq!(f.swap(), Swap::Zram);
        let lines = f.summary();
        assert!(
            lines.iter().any(|l| l.contains("compressed memory")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.contains("aliceliddell-wolos")),
            "{lines:?}"
        );
        f.set_memory(16 * GIB);
        f.swap = Swap::Swapfile(0);
        f.autologin = true;
        f.set_hostname(" rabbit-hole ");
        assert_eq!(f.swap(), Swap::Swapfile(8));
        let lines = f.summary();
        assert!(
            lines.iter().any(|l| l.contains("swap file of 8 GB")),
            "{lines:?}"
        );
        assert!(lines.iter().any(|l| l.contains("rabbit-hole")), "{lines:?}");
        assert!(
            lines.iter().any(|l| l.contains("automatically")),
            "{lines:?}"
        );
        for l in &lines {
            assert!(copy::lint(l).is_empty(), "{l}: {:?}", copy::lint(l));
        }
        let x = f.plan_extras();
        assert_eq!(
            x,
            [
                ("hostname", serde_json::json!("rabbit-hole")),
                ("autologin", serde_json::json!(true)),
                ("swap", serde_json::json!("swapfile:8")),
            ]
        );
        f.swap = Swap::None;
        assert_eq!(f.swap().word(), "none");
        assert!(!f.summary().iter().any(|l| l.contains("swap")));
        f.set_hostname("");
        assert_eq!(f.hostname(), "aliceliddell-wolos", "empty follows again");
        // A bad name stops at the account page, under its field.
        f.step = Step::Account;
        f.set_hostname("-bad-");
        assert!(matches!(f.advance(), Next::Stay(Some(Field::Hostname), _)));
        f.set_hostname("good-1");
        assert_eq!(f.advance(), Next::Go(Step::Summary));
        assert_eq!(swapfile_gib(GIB), 2);
        assert_eq!(swapfile_gib(64 * GIB), 8);
        assert_eq!(
            meminfo_bytes("MemTotal:       16318028 kB\nMemFree: 1 kB\n"),
            Some(16318028 * 1024)
        );
        assert_eq!(meminfo_bytes(""), None);
    }

    #[test]
    fn try_again_goes_to_the_disk_page_with_a_way_back() {
        let mut f = flow(dual());
        to_method(&mut f);
        f.jump(Step::Failed);
        assert_eq!(f.back(), None, "nothing to go back to from Failed");
        f.restart();
        assert_eq!(f.step, Step::Disk);
        assert_eq!(f.back(), Some(Step::Zone));
        assert_eq!(f.back(), Some(Step::Language));
        assert_eq!(f.back(), Some(Step::Begin));
        let mut live = Flow::new(&Hints::default(), false, false);
        live.restart();
        assert_eq!(live.back(), Some(Step::Zone));
        assert_eq!(live.back(), Some(Step::Language));
        assert_eq!(live.back(), None);
    }

    #[test]
    fn disks_say_their_device_and_kind() {
        let d = dual();
        assert_eq!(disk_detail(&d), "500 GB · /dev/vda · Virtual disk");
        let mut n = dual();
        n.path = "/dev/nvme0n1".into();
        assert_eq!(disk_detail(&n), "500 GB · /dev/nvme0n1 · NVMe");
        let mut sd = dual();
        sd.path = "/dev/sda".into();
        assert_eq!(disk_detail(&sd), "500 GB · /dev/sda");
    }

    #[test]
    fn secure_boot_is_said_before_anything_is_written() {
        let mut f = flow(dual());
        f.disks[0].check.secure_boot = true;
        f.method = Method::Erase;
        f.step = Step::Summary;
        assert_eq!(f.advance(), Next::Go(Step::SecureBoot));
        assert_eq!(f.advance(), Next::Install);
        // R2B-3: the shim is on the image, so the setting is changed after this install, never
        // by a second one, and the keys are never cleared (that trips BitLocker, R2B-9).
        let t = Step::SecureBoot.subtitle().to_lowercase();
        assert!(!t.contains("install again"), "{t}");
        assert!(!t.contains("clear the secure boot keys"), "{t}");
        assert!(
            t.contains("third-party certificates") && t.contains("blue screen"),
            "{t}"
        );
    }

    /// R2B-9: a Windows with BitLocker that stays is warned about when this install changes the
    /// Secure Boot keys Windows measures, and only then.
    #[test]
    fn bitlocker_is_said_when_the_secure_boot_keys_change_next_to_it() {
        let mut d = dual();
        d.parts[1].fstype = "BitLocker".into();
        d.parts[1].blocked = Some(wol_core::install::disk::Blocked::BitLocker);
        let mut f = flow(d);
        f.method = Method::Replace;
        f.replace = Some("u3".into());
        f.disks[0].check.tier = Some("own-keys".into());
        assert!(f.windows_stays() && f.bitlocker_stays());
        let lines = f.summary();
        for want in [
            copy::installer::BITLOCKER_RECOVERY,
            copy::installer::BITLOCKER_KEY_WHERE,
            copy::installer::BITLOCKER_AVOID_OWN_KEYS,
        ] {
            assert!(lines.iter().any(|l| l == want), "{lines:?}");
        }
        assert!(lines.iter().any(|l| l.contains("BitLocker")), "{lines:?}");
        assert_eq!(
            f.bitlocker_notes(true).len(),
            2,
            "after the install: the key only"
        );
        f.disks[0].check.tier = Some("firmware".into());
        assert!(
            f.bitlocker_notes(true)
                .contains(&copy::installer::BITLOCKER_AVOID_SETTING)
        );
        // The key screen (mok) leaves Windows' keys alone.
        f.disks[0].check.tier = Some("mok".into());
        assert!(f.bitlocker_notes(false).is_empty());
        // Erased: no Windows stays on this disk...
        f.disks[0].check.tier = Some("own-keys".into());
        f.method = Method::Erase;
        assert!(!f.windows_stays() && f.bitlocker_notes(false).is_empty());
        // ...unless another disk has a locked Windows.
        let mut other = Found {
            disk: Disk::default(),
            check: super::super::Check::default(),
        };
        other.check.others = vec!["Windows".into()];
        other.check.bitlocker = true;
        f.disks.push(other);
        assert!(f.windows_stays() && !f.bitlocker_notes(false).is_empty());
        for s in copy::installer::bitlocker("own-keys", false)
            .into_iter()
            .chain(copy::installer::bitlocker("firmware", false))
        {
            assert!(copy::lint(s).is_empty(), "{s}: {:?}", copy::lint(s));
        }
    }

    /// R2B-12: next to Windows the clock keeps local time (install-run writes it), and the summary says so.
    #[test]
    fn the_clock_follows_windows_when_it_stays() {
        let mut f = flow(dual());
        f.method = Method::Replace;
        f.replace = Some("u3".into());
        assert!(f.windows_stays());
        assert!(
            f.summary()
                .iter()
                .any(|l| l == copy::installer::CLOCK_LOCAL)
        );
        f.method = Method::Erase;
        assert!(
            !f.summary()
                .iter()
                .any(|l| l == copy::installer::CLOCK_LOCAL)
        );
        assert!(copy::lint(copy::installer::CLOCK_LOCAL).is_empty());
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
        for (_, title, detail) in SWAPS {
            assert!(!title.is_empty() && !detail.is_empty());
        }
        assert!(Step::Done.subtitle().contains("USB stick"));
    }

    /// B2: the live system runs from the stick; it comes out after Restart, not before.
    #[test]
    fn the_medium_comes_out_only_after_the_screen_goes_dark() {
        for s in [
            Step::Done.subtitle(),
            copy::INSTALL_KEY_SCREEN_MEDIA,
            copy::INSTALL_OWN_KEYS_MEDIA,
        ] {
            let l = s.to_lowercase();
            assert!(l.contains("screen goes dark"), "{s}");
            assert!(!l.contains("before") && !l.contains(", first"), "{s}");
        }
    }

    /// B8: the page setting the keyboard list to follow a language is no choice of the person's.
    #[test]
    fn a_language_click_moves_the_keyboard_until_the_person_picks_one() {
        let at = |k: &str| KEYBOARDS.iter().position(|x| x.0 == k).unwrap();
        let mut f = Flow::new(&Hints::default(), true, false);
        f.set_language(2);
        assert_eq!(f.draft.keyboard, at("br"));
        // The list echoes the index the language set: not a pick.
        assert!(!f.keyboard_shown(f.draft.keyboard, false));
        f.set_language(6);
        assert_eq!(
            f.draft.keyboard,
            at("de"),
            "the second language moves it too"
        );
        assert!(f.keyboard_shown(at("us"), true));
        f.set_language(2);
        assert_eq!(f.draft.keyboard, at("us"), "a picked keyboard stays");
    }

    /// R2B-8: picking the very layout the language suggested is still the person's pick.
    #[test]
    fn picking_the_suggested_keyboard_is_a_pick_too() {
        let at = |k: &str| KEYBOARDS.iter().position(|x| x.0 == k).unwrap();
        let mut f = Flow::new(&Hints::default(), true, false);
        f.set_language(2);
        assert_eq!(f.draft.keyboard, at("br"));
        assert!(f.keyboard_shown(at("br"), true));
        f.set_language(6);
        assert_eq!(f.draft.keyboard, at("br"), "the picked keyboard stays");
    }

    /// B9: with the security chip the account password is the disk's fallback at the console.
    #[test]
    fn the_chip_fallback_password_is_typable_before_any_desktop() {
        let mut f = flow(dual());
        f.method = Method::Erase;
        f.protect = Protect::Tpm;
        f.step = Step::Account;
        fill_account(&mut f);
        f.draft.password = "café12345".into();
        f.draft.verify = "café12345".into();
        let n = f.advance();
        assert!(
            matches!(&n, Next::Stay(Some(Field::Password), why) if why == copy::INSTALL_TPM_FALLBACK),
            "{n:?}"
        );
        // A disk password of its own: the account password never opens the disk.
        f.protect = Protect::Password;
        assert_eq!(f.advance(), Next::Go(Step::Summary));
        f.step = Step::Account;
        f.protect = Protect::Tpm;
        fill_account(&mut f);
        assert_eq!(f.advance(), Next::Go(Step::Summary));
    }

    /// B3: next to a Windows startup partition too small to share, Windows leaves the menu.
    #[test]
    fn the_summary_says_how_windows_starts_when_it_leaves_the_menu() {
        let mut small = dual();
        small.parts[0].free_bytes = Some(60 * MIB);
        let mut f = flow(small);
        assert_eq!(f.method, Method::Alongside);
        assert_eq!(
            f.windows_start().as_deref(),
            Some(copy::INSTALL_WINDOWS_START)
        );
        let lines = f.summary();
        assert!(
            lines.iter().any(|l| l == copy::INSTALL_WINDOWS_START),
            "{lines:?}"
        );
        // Shared startup partition: Windows stays in the menu.
        assert_eq!(flow(dual()).windows_start(), None);
        // Erased or taken over: no Windows to start.
        f.method = Method::Erase;
        assert_eq!(f.windows_start(), None);
        f.method = Method::Replace;
        f.replace = Some("u2".into());
        // A plain data partition starts nothing: nothing to say.
        assert_eq!(f.windows_start(), None);
        f.replace = Some("u3".into());
        assert_eq!(
            f.windows_start().as_deref(),
            Some(copy::INSTALL_WINDOWS_START)
        );
    }

    /// R2B-11: another Linux keeps its own startup partition next to WolOS's new one: it is named.
    #[test]
    fn the_summary_says_how_another_linux_starts_when_it_leaves_the_menu() {
        let mut d = dual();
        d.parts[0].free_bytes = Some(60 * MIB);
        d.parts[2].os = Some("Fedora Linux 41".into());
        let mut f = flow(d);
        f.method = Method::Replace;
        f.replace = Some("u2".into());
        let want = copy::installer::other_start("Fedora Linux 41");
        assert_eq!(f.windows_start().as_deref(), Some(want.as_str()));
        assert!(f.summary().contains(&want), "{:?}", f.summary());
    }

    /// An encrypted install with no recovery key read back never reaches Done's Restart; an
    /// unencrypted one has no key to miss. The page cannot be left with Back or Escape.
    #[test]
    fn an_encrypted_install_without_its_recovery_key_does_not_offer_restart() {
        let mut f = flow(dual());
        for p in [Protect::Tpm, Protect::Password] {
            f.protect = p;
            assert!(f.key_unread(None), "{p:?}");
            assert!(f.key_unread(Some("  ")), "{p:?}");
            assert!(!f.key_unread(Some("1234-5678")), "{p:?}");
        }
        f.protect = Protect::Off;
        assert!(!f.key_unread(None));
        f.jump(Step::KeyUnread);
        assert_eq!(f.back(), None);
        assert_eq!(f.advance(), Next::Go(Step::KeyUnread));
        let t = Step::KeyUnread.subtitle();
        assert!(
            t.contains("Try Again") && crate::copy::lint(t).is_empty(),
            "{t}"
        );
    }

    /// R2B-10: a Windows that did not shut down fully stops every layout but Erase Disk, with a
    /// sentence that says how to shut it down.
    #[test]
    fn a_hibernated_windows_is_said_before_the_disk_is_touched() {
        let mut d = dual();
        d.parts[1].blocked = Some(wol_core::install::disk::Blocked::NotShutDown);
        let mut f = flow(d);
        f.method = Method::Replace;
        f.replace = Some("u3".into());
        f.step = Step::Replace;
        match f.advance() {
            Next::Stay(None, why) => {
                assert!(why.contains("Fast Startup"), "{why}");
                assert!(copy::lint(&why).is_empty(), "{why}");
            }
            n => panic!("went on: {n:?}"),
        }
        f.method = Method::Erase;
        f.step = Step::Method;
        assert_eq!(f.advance(), Next::Go(Step::Protect));
    }
}
