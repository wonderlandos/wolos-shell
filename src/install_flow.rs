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
    /// Secure Boot, opt-in (lead 2026-11-02): only where it changes something
    /// ([`Flow::secure_offer`]); the one-time key screen is pictured before it is chosen.
    SecureBootChoice,
    Account,
    /// Optional modules (ADR 0032 B): none ticked, the download size said.
    Modules,
    /// Media formats (the Store's codecs kit), one switch, off.
    Extras,
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
pub const STEPS: [Step; 20] = [
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
    Step::SecureBootChoice,
    Step::Account,
    Step::Modules,
    Step::Extras,
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
            Step::SecureBootChoice => "secure-boot-choice",
            Step::Account => "account",
            Step::Modules => "modules",
            Step::Extras => "extras",
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
            Step::SecureBootChoice => "Secure Boot",
            Step::Account => "Create Your Account",
            Step::Modules => "Apps for What You Do",
            Step::Extras => "Music and Video Formats",
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

/// What the Secure Boot page may offer on this computer (from `install-check`'s `boot` and
/// `boot-off` tiers).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecureOffer {
    /// Nothing to choose: Secure Boot can't be used (`off`), or only a firmware setting helps
    /// (`firmware`, the Secure Boot sheet after the summary). The page is skipped.
    Nothing,
    /// The person chooses, default off: leaving it off changes nothing at the start.
    /// `screen`: on means the one-time key screen (`mok`); else own keys, nothing to click.
    Choice { screen: bool },
    /// The computer's own Secure Boot is on, so WolOS uses it: the page explains the key screen
    /// and has no switch (off would only start after a trip to the startup settings).
    AlreadyOn,
}

/// One row of the Modules page: a registry record the page has words for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleChoice {
    pub id: String,
    pub title: &'static str,
    pub detail: &'static str,
    /// Download size from the record (MB).
    pub size_mb: u64,
}

/// The Modules page's rows from the registry records on the image, in the page's order
/// (`copy::installer::MODULE_ROWS`); a record the page has no words for is not offered.
pub fn module_choices(records: &[wol_core::modules::Module]) -> Vec<ModuleChoice> {
    copy::installer::MODULE_ROWS
        .iter()
        .filter_map(|(id, title, detail)| {
            let m = records.iter().find(|m| m.id == *id)?;
            Some(ModuleChoice {
                id: m.id.clone(),
                title,
                detail,
                size_mb: u64::from(m.size_mb),
            })
        })
        .collect()
}

/// The Store kit the Extras page adds (`wol_core::store::KITS`).
pub const CODECS_KIT: &str = "codecs-kit";

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
    /// The person turned Secure Boot on (only read when [`Flow::secure_offer`] is a choice).
    pub secure: bool,
    /// The modules the image offers, and the ids ticked (none by default).
    pub module_list: Vec<ModuleChoice>,
    pub modules: Vec<String>,
    /// The media formats are added (off by default).
    pub codecs: bool,
    /// No default route when last looked ([`online_now`], off the GTK thread): the media
    /// formats and the modules wait for the first start with a network, and Summary and Done
    /// say so ([`Flow::deferred_line`]). False until a look says otherwise.
    pub offline: bool,
    /// Where Back goes from each page: the pages that were really shown.
    trail: Vec<Step>,
}

/// Whether `/proc/net/route` (`v4`) or `/proc/net/ipv6_route` (`v6`) holds a default route
/// that is up: the same question install-run asks (`ip route show default`) before it adds the
/// media formats and the modules. A v6 default on `lo` or marked reject is not a way out.
pub fn has_default_route(v4: &str, v6: &str) -> bool {
    wol_core::controls::parse_proc_route(v4).is_some()
        || v6.lines().any(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let flags = f
                .get(8)
                .and_then(|x| u32::from_str_radix(x, 16).ok())
                .unwrap_or(0);
            f.len() >= 10
                && f[0].bytes().all(|b| b == b'0')
                && f[1] == "00"
                && f[9] != "lo"
                && flags & 0x0001 != 0
                && flags & 0x0200 == 0
        })
}

/// [`has_default_route`] on this computer now. Blocks (reads /proc): off the GTK thread only.
pub fn online_now() -> bool {
    let read = |p: &str| std::fs::read_to_string(p).unwrap_or_default();
    has_default_route(&read("/proc/net/route"), &read("/proc/net/ipv6_route"))
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
            // Opt-in (lead, 2026-11-02): the key screen comes only to those who ask for it.
            secure: false,
            module_list: Vec::new(),
            modules: Vec::new(),
            codecs: false,
            offline: false,
            trail: Vec::new(),
        }
    }

    /// The modules this image has records for (`install::modules_on_image`); ticks of ids it
    /// does not offer are dropped.
    pub fn set_modules(&mut self, records: &[wol_core::modules::Module]) {
        self.module_list = module_choices(records);
        let offered: Vec<String> = self.module_list.iter().map(|m| m.id.clone()).collect();
        self.modules.retain(|id| offered.contains(id));
    }

    /// Ticks or unticks module `id`; an id the page does not offer is ignored.
    pub fn tick_module(&mut self, id: &str, on: bool) {
        if !self.module_list.iter().any(|m| m.id == id) {
            return;
        }
        self.modules.retain(|m| m != id);
        if on {
            self.modules.push(id.to_string());
        }
    }

    /// The ticked modules in the page's order: (title, MB).
    fn ticked(&self) -> Vec<&ModuleChoice> {
        self.module_list
            .iter()
            .filter(|m| self.modules.contains(&m.id))
            .collect()
    }

    /// What the ticked modules download (MB).
    pub fn modules_mb(&self) -> u64 {
        self.ticked().iter().map(|m| m.size_mb).sum()
    }

    fn check(&self) -> Option<&super::Check> {
        self.disks.get(self.chosen?).map(|f| &f.check)
    }

    /// What the Secure Boot page offers on this computer. A choice only where leaving it off
    /// starts WolOS as well: `boot-off off`, or Setup Mode (own keys) when install-check says
    /// nothing more. The key screen tier with the computer's Secure Boot on (or not said) is
    /// [`SecureOffer::AlreadyOn`], today's behaviour.
    pub fn secure_offer(&self) -> SecureOffer {
        let Some(c) = self.check() else {
            return SecureOffer::Nothing;
        };
        let off_is_free = match c.tier_off.as_deref() {
            Some(t) => t == "off",
            None => c.tier.as_deref() == Some("own-keys"),
        };
        match c.tier.as_deref() {
            Some(t @ ("mok" | "own-keys")) if off_is_free => {
                SecureOffer::Choice { screen: t == "mok" }
            }
            Some("mok" | "own-keys") => SecureOffer::AlreadyOn,
            _ => SecureOffer::Nothing,
        }
    }

    /// Secure Boot protection goes into the plan (`WOLOS_SECURE`): the person's answer where
    /// they had one, else yes (what boot-setup did before the page existed).
    pub fn secure_wanted(&self) -> bool {
        match self.secure_offer() {
            SecureOffer::Choice { .. } => self.secure,
            SecureOffer::Nothing | SecureOffer::AlreadyOn => true,
        }
    }

    /// The key screen may come after the restart by what was chosen: the Done page shows its
    /// steps only then, and only when boot-setup queued it (`install::key_screen_needed`).
    /// Off by choice: never (lead, 2026-11-02: repeated only when chosen).
    pub fn key_screen_chosen(&self) -> bool {
        match self.secure_offer() {
            SecureOffer::Choice { .. } => self.secure,
            SecureOffer::AlreadyOn | SecureOffer::Nothing => true,
        }
    }

    /// Paragraph under a page's title: its subtitle, what it means for a newcomer, and the next
    /// page when that is already known.
    pub fn intro(&self, step: Step) -> String {
        let mut v: Vec<&str> = vec![step.subtitle()];
        if step == Step::SecureBootChoice {
            v.extend(match self.secure_offer() {
                SecureOffer::Choice { screen: true } => &copy::installer::SECURE_CHOICE_SCREEN[..],
                SecureOffer::Choice { screen: false } => {
                    &copy::installer::SECURE_CHOICE_OWN_KEYS[..]
                }
                SecureOffer::AlreadyOn => &copy::installer::SECURE_ALREADY_ON[..],
                SecureOffer::Nothing => &[],
            });
        } else {
            v.extend(copy::installer::explain(step));
        }
        v.extend(self.next_after(step).and_then(copy::installer::next_words));
        v.retain(|s| !s.is_empty());
        v.join(" ")
    }

    /// The page Continue leads to from `step` when no choice on it decides that.
    pub fn next_after(&self, step: Step) -> Option<Step> {
        Some(match step {
            Step::Begin => Step::Language,
            Step::Language => Step::Zone,
            Step::Zone => Step::Disk,
            Step::Disk => Step::Method,
            Step::Replace | Step::Alongside | Step::Manual => Step::Protect,
            Step::Protect if self.secure_offer() != SecureOffer::Nothing => Step::SecureBootChoice,
            Step::Protect | Step::SecureBootChoice => Step::Account,
            Step::Account if !self.module_list.is_empty() => Step::Modules,
            Step::Account | Step::Modules => Step::Extras,
            Step::Extras => Step::Summary,
            _ => return None,
        })
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
                match self.next_after(Step::Protect) {
                    Some(s) => self.go(s),
                    None => self.go(Step::Account),
                }
            }
            Step::SecureBootChoice => self.go(Step::Account),
            Step::Account => match self.draft.account_problem() {
                Some((f, why)) => Next::Stay(Some(f), why.into()),
                // The chip's fallback is the account password, typed on the console keymap (B9).
                None if unlock_problem(self.protect, Some(&self.draft.password)).is_some() => {
                    Next::Stay(Some(Field::Password), copy::INSTALL_TPM_FALLBACK.into())
                }
                None if !super::is_valid_hostname(&self.hostname()) => {
                    Next::Stay(Some(Field::Hostname), copy::installer::HOSTNAME_RULE.into())
                }
                None => match self.next_after(Step::Account) {
                    Some(s) => self.go(s),
                    None => self.go(Step::Extras),
                },
            },
            Step::Modules => self.go(Step::Extras),
            Step::Extras => self.go(Step::Summary),
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
                    title: "New Partition".into(),
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
                title: "New Partition".into(),
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
        if let Some(line) = self.secure_line() {
            v.push(line.to_string());
        }
        let names: Vec<&str> = self.ticked().iter().map(|m| m.title).collect();
        v.extend(copy::installer::summary_modules(&names, self.modules_mb()));
        if self.codecs {
            v.push(copy::installer::SUMMARY_CODECS.to_string());
        }
        v.extend(self.deferred_line().map(String::from));
        v
    }

    /// Offline: the line that says the media formats and modules chosen come at first start
    /// (install-run queues them then). None online or when none were chosen.
    pub fn deferred_line(&self) -> Option<&'static str> {
        if !self.offline {
            return None;
        }
        copy::installer::deferred(self.codecs, !self.ticked().is_empty())
    }

    /// The summary's Secure Boot line, by what the page offered and what was chosen; None when
    /// the page was not shown (the Secure Boot sheet speaks for the firmware case).
    pub fn secure_line(&self) -> Option<&'static str> {
        use copy::installer as c;
        Some(match self.secure_offer() {
            SecureOffer::Nothing => return None,
            SecureOffer::Choice { .. } if !self.secure => c::SUMMARY_SECURE_OFF,
            SecureOffer::Choice { screen: false } => c::SUMMARY_SECURE_OWN_KEYS,
            SecureOffer::Choice { screen: true } | SecureOffer::AlreadyOn => {
                if self.tier() == Some("own-keys") {
                    c::SUMMARY_SECURE_OWN_KEYS
                } else {
                    c::SUMMARY_SECURE_SCREEN
                }
            }
        })
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

    /// The Secure Boot sheet's lines under its subtitle: the firmware steps, then what BitLocker
    /// needs when it applies ([`Self::bitlocker_notes`]).
    pub fn secure_boot_notes(&self) -> Vec<&'static str> {
        let mut notes = copy::installer::SECURE_BOOT_STEPS.to_vec();
        notes.extend(self.bitlocker_notes(false));
        notes
    }

    /// What Summary (`done` false), the Secure Boot sheet and Done (`done` true) say about
    /// BitLocker: only when a Windows with BitLocker stays and this install changes the Secure
    /// Boot keys Windows measures (own keys, or the firmware setting). Empty: nothing to say.
    pub fn bitlocker_notes(&self, done: bool) -> Vec<&'static str> {
        // Which tiers change the keys is core's rule, the one boot-setup's note follows too. Left
        // off by choice, the tier is the one without protection, which leaves the keys alone.
        let tier = if self.secure_wanted() {
            self.tier()
        } else {
            self.check()
                .and_then(|c| c.tier_off.as_deref())
                .or(Some("off"))
        };
        match tier.and_then(Tier::parse) {
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
                hint: a.hint,
            }),
            secure_boot: Some(self.secure_wanted()),
            modules: self.ticked().iter().map(|m| m.id.clone()).collect(),
            extras: if self.codecs {
                vec![CODECS_KIT.to_string()]
            } else {
                Vec::new()
            },
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
#[path = "install_flow/tests.rs"]
mod tests;
