// SPDX-License-Identifier: GPL-2.0-or-later
//! Install WolOS (ADR 0013, ADR 0023): the disks it may use, the progress lines of
//! `/usr/lib/wolos/install-run` and what the progress page says. Pure; UI in `ui::installer`,
//! the steps in [`flow`].

#[path = "install_flow.rs"]
pub mod flow;

/// Erases the disk and installs (root; checks the disk against `install-disks` again).
pub const RUN: &str = "/usr/lib/wolos/install-run";
/// Disks that are not the boot media, one per line.
pub const DISKS: &str = "/usr/lib/wolos/install-disks";
/// Facts about one disk before anything is written (`size`, `os`, `boot`, `refuse` lines).
pub const CHECK: &str = "/usr/lib/wolos/install-check";

// The pages' sentences live in `copy::installer` (R2B-20); these names stay for the callers.
pub use crate::copy::installer::{
    SECURE_BOOT_TEXT, SECURE_BOOT_TITLE, TIPS, doing, erase_warning, refusal_text, remaining_text,
    stopped_text,
};

/// What `install-check <disk>` found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Check {
    /// Other systems on the disk, as named for people: "Windows", "Linux", "macOS", "FreeBSD".
    pub others: Vec<String>,
    /// A firmware setting must change first: as it stands the installed WolOS would not start.
    pub secure_boot: bool,
    /// The protection tier `wol boot detect` named (`own-keys`, `mok`, `off`, `firmware`).
    pub tier: Option<String>,
    /// A BitLocker volume is on the disk (`bitlocker yes`, said with `os Windows`; R2B-9).
    pub bitlocker: bool,
    pub refused: Option<String>,
}

pub fn parse_check(out: &str) -> Check {
    let mut c = Check::default();
    for line in out.lines() {
        match line.split_once(' ') {
            Some(("os", name)) => c.others.push(name.to_string()),
            Some(("boot", tier)) => {
                c.secure_boot |= tier == "firmware";
                c.tier = Some(tier.trim().to_string());
            }
            Some(("bitlocker", yes)) => c.bitlocker = yes.trim() == "yes",
            Some(("refuse", why)) => c.refused = Some(why.to_string()),
            _ => {}
        }
    }
    c
}

/// `install-check <disk>` as the app runs it: through sudo where the live image allows it (the
/// Secure Boot variable is root's to read), else directly (disks are readable by operator).
pub fn check_command(disk: &str, sudo: bool) -> Vec<String> {
    if sudo {
        ["sudo", "-n", CHECK, disk].map(String::from).to_vec()
    } else {
        [CHECK, disk].map(String::from).to_vec()
    }
}

/// Why the screen stays on and the machine awake (idle inhibitors show it).
pub const AWAKE_REASON: &str = "Installing WolOS";

/// Keeps the machine awake exactly while the install runs: `set` says when to take or give back
/// the inhibitors (Some(on)), None when nothing changes.
#[derive(Debug, Default)]
pub struct Awake {
    on: bool,
}

impl Awake {
    pub fn set(&mut self, installing: bool) -> Option<bool> {
        (installing != self.on).then(|| {
            self.on = installing;
            installing
        })
    }
}

/// Prefix of every progress line `install-run` prints.
pub const MARK: &str = "wolos-install: ";

/// Decimal size as disks are sold: "500 GB", "1 TB", "1.5 TB".
pub fn size_text(bytes: u64) -> String {
    let gb = bytes as f64 / 1e9;
    if gb >= 1000.0 {
        let tb = gb / 1000.0;
        let r = (tb * 10.0).round() / 10.0;
        if r.fract() == 0.0 {
            format!("{r:.0} TB")
        } else {
            format!("{r:.1} TB")
        }
    } else {
        format!("{} GB", gb.round() as u64)
    }
}

/// `install-check <disk> --probe`: everything about a disk, for the list and the layout pages.
pub fn probe_command(disk: &str) -> Vec<String> {
    ["sudo", "-n", CHECK, disk, "--probe"]
        .map(String::from)
        .to_vec()
}

/// The disks `install-disks` printed.
pub fn parse_disks(allowed: &str) -> Vec<String> {
    allowed
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

/// Runs the plan: everything else install-run needs is in the files next to it.
pub fn install_command(plan: &std::path::Path) -> Vec<String> {
    vec![
        "sudo".into(),
        "-n".into(),
        RUN.into(),
        "--plan".into(),
        plan.to_string_lossy().into_owned(),
    ]
}

/// The plan goes here, 0700: passwords are side files, removed by install-run.
pub fn plan_dir(runtime: &std::path::Path) -> std::path::PathBuf {
    runtime.join("wolos-install")
}

/// What the startup setup has to tell the person, in plain words (written by the install).
pub const BOOT_NOTE: &str = "/run/wolos/boot-note";

/// Name of the side files in [`plan_dir`].
pub const PLAN_FILE: &str = "plan.json";
pub const DISK_PASSWORD: &str = "disk-password";
pub const ACCOUNT_PASSWORD: &str = "account-password";

/// Writes the plan and its passwords (0600). `extras` are keys next to the plan's own in the same
/// file (`hostname`, `autologin`, `swap`): `wol install info` reads them by name, so a plan written
/// before they existed still runs. Newlines end a password: refused by the fields' length only in
/// the sense that none can be typed.
pub fn write_plan(
    dir: &std::path::Path,
    plan: &wol_core::install::plan::Plan,
    extras: &[(&str, serde_json::Value)],
    disk_password: &str,
    account_password: &str,
) -> std::io::Result<std::path::PathBuf> {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let _ = std::fs::remove_dir_all(dir);
    std::fs::DirBuilder::new().mode(0o700).create(dir)?;
    let put = |name: &str, text: &str| -> std::io::Result<()> {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(dir.join(name))?
            .write_all(text.as_bytes())
    };
    let mut v = serde_json::to_value(plan).map_err(std::io::Error::other)?;
    if let Some(o) = v.as_object_mut() {
        for (k, x) in extras {
            o.insert((*k).to_string(), x.clone());
        }
    }
    let json = serde_json::to_string(&v).map_err(std::io::Error::other)?;
    put(PLAN_FILE, &json)?;
    put(ACCOUNT_PASSWORD, account_password)?;
    if !disk_password.is_empty() {
        put(DISK_PASSWORD, disk_password)?;
    }
    Ok(dir.join(PLAN_FILE))
}

/// The live user may restart with sudo (`image/build.sh`).
pub fn restart_command() -> Vec<String> {
    ["sudo", "-n", "systemctl", "reboot"]
        .map(String::from)
        .to_vec()
}

/// Where an install is: the stage words of the progress lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Checking the disk.
    Prepare,
    /// The partitions (5).
    Disk,
    /// The system files are copied (15; no progress of their own).
    Files,
    /// This computer's Secure Boot keys (40).
    Keys,
    /// The disk is encrypted and the key enrolled (45).
    Encrypt,
    /// The first start's files (70).
    Initramfs,
    /// Language and keyboard of the new system (68; `wol install settings`, before the first
    /// start's files, which carry the console keymap).
    Settings,
    /// The disk key is sealed to this computer (92).
    Seal,
    /// Boot loader on the EFI partition (95).
    Boot,
    /// Apps from the local repository (older scripts).
    Packages,
    /// WolOS settings, unmount (older scripts).
    Finish,
    Done,
    Failed,
}

impl Stage {
    fn parse(word: &str) -> Option<Stage> {
        Some(match word {
            "prepare" => Stage::Prepare,
            "disk" => Stage::Disk,
            "files" => Stage::Files,
            "keys" => Stage::Keys,
            "encrypt" => Stage::Encrypt,
            "initramfs" => Stage::Initramfs,
            "settings" => Stage::Settings,
            "seal" => Stage::Seal,
            "boot" => Stage::Boot,
            "packages" => Stage::Packages,
            "finish" => Stage::Finish,
            "done" => Stage::Done,
            "failed" | "fail" => Stage::Failed,
            _ => return None,
        })
    }
}

/// One progress line: `wolos-install: <stage> <percent> <message>`, or the last line of a run that
/// stopped: `wolos-install: fail <stage> <message>` (the stage it stopped in, the tool's own words).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub stage: Stage,
    /// 0-100, never backwards within a run (except `failed`, which says 0).
    pub percent: u8,
    /// For the log and the Failed page's status line; the progress page says `doing(stage)`.
    pub message: String,
    /// `fail <stage>`: where it stopped. None for every other line.
    pub at: Option<Stage>,
}

impl Report {
    pub fn new(stage: Stage, percent: u8) -> Report {
        Report {
            stage,
            percent,
            message: String::new(),
            at: None,
        }
    }
}

/// One progress line.
pub fn parse_line(line: &str) -> Option<Report> {
    let rest = &line[line.find(MARK)? + MARK.len()..];
    let rest = rest.trim_end_matches(['\r', '\n']);
    let (stage, rest) = rest.split_once(' ')?;
    if stage == "fail" {
        let (at, message) = rest.split_once(' ').unwrap_or((rest, ""));
        return Some(Report {
            stage: Stage::Failed,
            percent: 0,
            message: message.trim().to_string(),
            at: Stage::parse(at),
        });
    }
    let (percent, message) = rest.split_once(' ').unwrap_or((rest, ""));
    let percent: u8 = percent.parse().ok().filter(|p| *p <= 100)?;
    Some(Report {
        stage: Stage::parse(stage)?,
        percent,
        message: message.trim().to_string(),
        at: None,
    })
}

/// Stages that take long and report nothing while they work: (stage, where it starts, where the
/// next stage starts, about how long it takes). The bar creeps from the start toward the end,
/// never past 90 % of the way, so the next report always moves it forward.
const CREEP: [(Stage, f64, f64, f64); 3] = [
    (Stage::Files, 0.15, 0.40, 120.0),
    (Stage::Encrypt, 0.45, 0.70, 90.0),
    (Stage::Initramfs, 0.70, 0.90, 60.0),
];

/// Bar position for `r`, `secs` after its stage began.
pub fn fraction(r: &Report, secs: f64) -> f64 {
    let at = f64::from(r.percent) / 100.0;
    if r.stage == Stage::Failed {
        return 0.0;
    }
    match CREEP.iter().find(|(s, ..)| *s == r.stage) {
        Some((_, start, end, took)) => {
            let creep = start + (end - start) * 0.9 * (secs / took).min(1.0);
            at.max(creep)
        }
        None => at,
    }
}

/// Seconds left, from how fast the bar moved since `start` (None until it has moved enough).
pub fn seconds_left(start: (f64, f64), now: (f64, f64)) -> Option<f64> {
    let (moved, took) = (now.0 - start.0, now.1 - start.1);
    (moved >= 0.05 && took > 0.0).then(|| (1.0 - now.0) * took / moved)
}

/// The refusal in a Failed line from install-run (`... (refuse Word): ...` or
/// `refuse Word("part")`): the word and the partition it names. None: no refusal in it.
pub fn refused_word(message: &str) -> Option<(&str, Option<&str>)> {
    let (_, rest) = message.split_once("refuse ")?;
    let end = rest
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(rest.len());
    let (word, tail) = rest.split_at(end);
    if !word.starts_with(|c: char| c.is_ascii_uppercase()) {
        return None;
    }
    let part = tail
        .strip_prefix("(\"")
        .and_then(|t| t.split_once("\")"))
        .map(|(p, _)| p);
    Some((word, part))
}

/// What the last install-run left for the app, from a folder only root can write (audit3 R2B-1):
/// `install-check --result` prints the recovery key once (and deletes it), then the log.
pub fn result_command() -> Vec<String> {
    ["sudo", "-n", CHECK, "--result"].map(String::from).to_vec()
}

/// [`result_command`]'s answer: the recovery key (`recovery-key <key>`; None: no key was made or
/// it was already handed out) and everything the tools printed (`log <line>` each), for Details.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunResult {
    pub key: Option<String>,
    pub log: String,
}

pub fn parse_result(out: &str) -> RunResult {
    let mut r = RunResult::default();
    for line in out.lines() {
        if let Some(k) = line.strip_prefix("recovery-key ") {
            r.key = Some(k.trim().to_string()).filter(|k| !k.is_empty());
        } else if let Some(l) = line.strip_prefix("log ") {
            r.log.push_str(l);
            r.log.push('\n');
        } else if line == "log" {
            r.log.push('\n');
        }
    }
    r
}

/// The last `lines` lines of a log, for the Details view.
pub fn log_tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let from = all.len().saturating_sub(lines);
    all[from..].join("\n")
}

/// `wol encrypt decide auto`: the one check for a usable security chip (TPM2), the same one
/// install-run makes. Prints `tpm2` when the disk can be unlocked by the chip.
pub fn tpm_command() -> Vec<String> {
    ["wol", "encrypt", "decide", "auto"]
        .map(String::from)
        .to_vec()
}

/// What `decide auto` said: a security chip the install can use.
pub fn tpm_from_decide(out: &str) -> bool {
    out.trim() == "tpm2"
}

/// Written by boot-enroll while this computer's key waits for the firmware's key screen.
pub const MOK_PENDING: &str = "/run/wolos/mok-pending";

/// The Done page needs the key screen card: the key is queued, or the startup note says the blue
/// screen comes (`wol boot note` says so for the mok tier).
pub fn key_screen_needed(note: Option<&str>, pending: bool) -> bool {
    pending || note.is_some_and(|n| n.contains("blue screen"))
}

/// What the Done page says under its subtitle: the startup note's first line, and for a computer
/// that now trusts only its own keys, that the install media will not start on it any more.
pub fn done_notes(note: Option<&str>) -> Vec<String> {
    let mut v = Vec::new();
    let Some(first) = note.and_then(|n| n.lines().next()).map(str::trim) else {
        return v;
    };
    if !first.is_empty() {
        v.push(first.to_string());
    }
    if first.contains("own WolOS keys") && !first.contains("install media") {
        v.push(crate::copy::INSTALL_OWN_KEYS_MEDIA.to_string());
    }
    v
}

/// What the recovery key is saved as on a USB stick.
pub const RECOVERY_FILE: &str = "WolOS recovery key.txt";

/// Where removable disks are mounted on the live image.
pub const MEDIA_ROOTS: [&str; 2] = ["/run/media", "/media"];

/// Mounted removable disks under `roots`: `/run/media/<user>/<disk>` and `/media/<disk>`.
pub fn media_dirs(roots: &[std::path::PathBuf]) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for root in roots {
        let Ok(rd) = std::fs::read_dir(root) else {
            continue;
        };
        for e in rd.flatten().filter(|e| e.path().is_dir()) {
            let p = e.path();
            if root.ends_with("run/media") {
                // One level down: per-user directories.
                if let Ok(inner) = std::fs::read_dir(&p) {
                    out.extend(inner.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
                }
            } else {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Writes the recovery key as a small text file on `dir` (a USB stick).
pub fn save_key(dir: &std::path::Path, key: &str) -> std::io::Result<std::path::PathBuf> {
    let p = dir.join(RECOVERY_FILE);
    std::fs::write(
        &p,
        format!(
            "WolOS recovery key\n\n{key}\n\n{}\n",
            crate::copy::INSTALL_RECOVERY_NOTE
        ),
    )?;
    Ok(p)
}

/// Computer name: the one rule `wol install info` and Settings > Sharing use
/// ([`wol_core::settings::hostname::is_valid`]), so the app never offers a name the backend
/// replaces with the default (R2B-18).
pub fn is_valid_hostname(h: &str) -> bool {
    wol_core::settings::hostname::is_valid(h)
}

/// The computer name to suggest: the account name, "-wolos".
pub fn hostname_for(account: &str) -> String {
    if account.is_empty() {
        "wolos".into()
    } else {
        format!("{account}-wolos")
    }
}

/// Seconds each tip stays.
pub const TIP_SECS: f64 = 12.0;

/// The tip to show `secs` after the install began.
pub fn tip_at(secs: f64) -> &'static str {
    TIPS[((secs.max(0.0) / TIP_SECS) as usize) % TIPS.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disks_are_the_ones_install_disks_allows_and_the_plan_goes_by_file() {
        assert_eq!(
            parse_disks("/dev/nvme0n1\n\n/dev/sda\n"),
            ["/dev/nvme0n1", "/dev/sda"]
        );
        assert_eq!(size_text(1_500_000_000_000), "1.5 TB");
        let argv = install_command(std::path::Path::new("/run/x/plan.json"));
        assert_eq!(argv[..3], ["sudo", "-n", RUN]);
        assert_eq!(argv[3..], ["--plan", "/run/x/plan.json"]);
        assert_eq!(restart_command().join(" "), "sudo -n systemctl reboot");
    }

    #[test]
    fn the_plan_and_its_passwords_are_private_files() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("wolos-plan-{}", std::process::id()));
        let plan = wol_core::install::plan::Plan::default();
        let extras = [
            ("hostname", serde_json::json!("alice-wolos")),
            ("autologin", serde_json::json!(true)),
            ("swap", serde_json::json!("swapfile:4")),
        ];
        let f = write_plan(&dir, &plan, &extras, "disk pw", "account pw").unwrap();
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&f), 0o600);
        assert_eq!(mode(&dir.join(DISK_PASSWORD)), 0o600);
        assert_eq!(
            std::fs::read_to_string(dir.join(ACCOUNT_PASSWORD)).unwrap(),
            "account pw"
        );
        // The extras sit next to the plan's own keys, and the plan still reads as a Plan.
        let text = std::fs::read_to_string(&f).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["hostname"], "alice-wolos");
        assert_eq!(v["autologin"], true);
        assert_eq!(v["swap"], "swapfile:4");
        assert_eq!(
            serde_json::from_str::<wol_core::install::plan::Plan>(&text).unwrap(),
            plan
        );
        // A second run starts clean; no disk password leaves no file behind.
        write_plan(&dir, &plan, &[], "", "account pw").unwrap();
        assert!(!dir.join(DISK_PASSWORD).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn progress_lines_from_the_installer() {
        assert_eq!(
            parse_line("wolos-install: prepare 0 Preparing to install on ada0"),
            Some(Report {
                stage: Stage::Prepare,
                percent: 0,
                message: "Preparing to install on ada0".into(),
                at: None,
            })
        );
        // The new stages, in the order install-run says them.
        for (word, stage, pct) in [
            ("keys", Stage::Keys, 40),
            ("encrypt", Stage::Encrypt, 45),
            ("initramfs", Stage::Initramfs, 70),
            ("settings", Stage::Settings, 90),
            ("seal", Stage::Seal, 92),
            ("boot", Stage::Boot, 95),
        ] {
            let r = parse_line(&format!("{MARK}{word} {pct} x")).unwrap();
            assert_eq!((r.stage, r.percent), (stage, pct), "{word}");
        }
        // The last line of a run that stopped: the stage and the tool's own words.
        let f = parse_line("wolos-install: fail encrypt cryptsetup: Cannot format device").unwrap();
        assert_eq!(
            (f.stage, f.percent, f.at),
            (Stage::Failed, 0, Some(Stage::Encrypt))
        );
        assert_eq!(f.message, "cryptsetup: Cannot format device");
        assert_eq!(
            parse_line("wolos-install: fail disk").map(|r| r.at),
            Some(Some(Stage::Disk))
        );
        let p = parse_line("\x1b[2J\x1b[Hwolos-install: packages 53 Installing apps (30 of 74)\r")
            .unwrap();
        assert_eq!((p.stage, p.percent), (Stage::Packages, 53));
        assert_eq!(p.message, "Installing apps (30 of 74)");
        assert_eq!(
            parse_line("wolos-install: done 100").map(|r| r.stage),
            Some(Stage::Done)
        );
        assert_eq!(
            parse_line("wolos-install: failed 0 disk: a step failed").map(|r| r.stage),
            Some(Stage::Failed)
        );
        for junk in [
            "wolos-install: packages x Installing",
            "wolos-install: packages 101 Too far",
            "wolos-install: shrug 5 What",
            "wolos-install: done",
            "Installing WolOS...",
        ] {
            assert_eq!(parse_line(junk), None, "{junk}");
        }
    }

    /// Every progress line the scripts print parses, in order, and the bar never goes back.
    #[test]
    fn the_scripts_speak_this_protocol() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../");
        let mut seen = Vec::new();
        for f in ["files/usr/lib/wolos/install-run"] {
            let text = std::fs::read_to_string(format!("{root}{f}")).unwrap();
            // Comments may use the word "say"; statements are what is checked.
            let text: String = text
                .lines()
                .filter(|l| !l.trim_start().starts_with('#'))
                .map(|l| format!("{l}\n"))
                .collect();
            // echo/printf lines ("%d" is a percent) and install-run's `say <stage> <percent> ...`.
            let printed = text.match_indices(MARK).map(|(i, _)| &text[i..]);
            let said = text
                .match_indices("say ")
                .filter(|(i, _)| {
                    !text[..*i]
                        .chars()
                        .next_back()
                        .is_some_and(|c| c.is_alphanumeric() || c == '_')
                })
                .map(|(i, _)| &text[i + "say ".len()..]);
            for (line, from_say) in printed.map(|l| (l, false)).chain(said.map(|l| (l, true))) {
                let line = line
                    .lines()
                    .next()
                    .unwrap()
                    .replace('"', "")
                    .replace("%d", "50");
                if line.contains('<') || line.contains("$*") {
                    continue; // the format in comments, say's own definition
                }
                let line = if from_say {
                    format!("{MARK}{line}")
                } else {
                    line
                };
                let r = parse_line(&line).unwrap_or_else(|| panic!("{f}: {line}"));
                seen.push(r.stage);
            }
        }
        for s in [
            Stage::Disk,
            Stage::Files,
            Stage::Boot,
            Stage::Settings,
            Stage::Done,
        ] {
            assert!(seen.contains(&s), "no script reports {s:?}");
        }
        // A stop is one `fail <stage> <message>` line with the stage it happened in.
        let text =
            std::fs::read_to_string(format!("{root}files/usr/lib/wolos/install-run")).unwrap();
        assert!(
            text.contains("say fail \"$stage\""),
            "install-run reports a stop as `fail <stage> <message>`"
        );
        // (report, seconds since its stage began), as the page samples them: install-run's
        // stages disk 5, files 15, keys 40, encrypt 45, initramfs 70, settings 90, seal 92,
        // boot 95, done 100.
        let order = [
            (Report::new(Stage::Prepare, 0), 0.0),
            (Report::new(Stage::Disk, 5), 9.0),
            (Report::new(Stage::Files, 15), 0.0),
            (Report::new(Stage::Files, 15), 30.0),
            (Report::new(Stage::Files, 16), 30.0),
            (Report::new(Stage::Files, 16), 600.0),
            (Report::new(Stage::Keys, 40), 0.0),
            (Report::new(Stage::Encrypt, 45), 0.0),
            (Report::new(Stage::Encrypt, 45), 400.0),
            (Report::new(Stage::Initramfs, 70), 0.0),
            (Report::new(Stage::Initramfs, 70), 300.0),
            (Report::new(Stage::Settings, 90), 0.0),
            (Report::new(Stage::Seal, 92), 0.0),
            (Report::new(Stage::Boot, 95), 0.0),
            (Report::new(Stage::Done, 100), 0.0),
        ];
        let bar: Vec<f64> = order.iter().map(|(r, t)| fraction(r, *t)).collect();
        assert!(bar.windows(2).all(|w| w[0] <= w[1]), "{bar:?}");
        assert_eq!(*bar.last().unwrap(), 1.0);
        // Files creeps while the copy says nothing, and never reaches the next stage.
        assert!(fraction(&Report::new(Stage::Files, 15), 60.0) > 0.15);
        assert!(fraction(&Report::new(Stage::Files, 15), 1e6) < 0.40);
        assert_eq!(fraction(&Report::new(Stage::Failed, 0), 50.0), 0.0);
        // The older scripts' words still parse.
        for w in ["packages 50 x", "finish 98 x", "failed 0 x"] {
            assert!(parse_line(&format!("{MARK}{w}")).is_some(), "{w}");
        }
    }

    #[test]
    fn the_failed_page_has_the_real_words_and_the_log() {
        let log = "a\n\nb\nc\nd\n";
        assert_eq!(log_tail(log, 2), "c\nd");
        assert_eq!(log_tail(log, 10), "a\nb\nc\nd");
        assert_eq!(log_tail("", 3), "");
        for s in [
            Stage::Prepare,
            Stage::Files,
            Stage::Keys,
            Stage::Encrypt,
            Stage::Initramfs,
            Stage::Seal,
            Stage::Boot,
            Stage::Settings,
            Stage::Packages,
        ] {
            assert!(!doing(s).is_empty(), "{s:?}");
            let l = crate::copy::lint(stopped_text(s));
            assert!(l.is_empty(), "{s:?}: {l:?}");
        }
        assert!(stopped_text(Stage::Encrypt).contains("encrypted"));
    }

    #[test]
    fn the_security_chip_check_is_the_one_install_run_makes() {
        assert_eq!(tpm_command().join(" "), "wol encrypt decide auto");
        assert!(tpm_from_decide("tpm2\n"));
        assert!(!tpm_from_decide("passphrase\n"));
        assert!(!tpm_from_decide("off"));
        assert!(!tpm_from_decide(""));
    }

    #[test]
    fn a_refusal_is_read_from_the_failed_line() {
        let m = "the disk is not what the plan was made for (refuse WeakPassword): see the log";
        assert_eq!(refused_word(m), Some(("WeakPassword", None)));
        let m = "the disk is not what the plan was made for (refuse Mounted(\"u3\"))";
        assert_eq!(refused_word(m), Some(("Mounted", Some("u3"))));
        assert_eq!(refused_word("sfdisk: cannot open /dev/nvme0n1"), None);
        assert_eq!(
            refused_word("refuse bios"),
            None,
            "install-check's own words"
        );
    }

    #[test]
    fn the_done_page_knows_about_the_key_screen_and_own_keys() {
        let mok = format!(
            "{}\n{}",
            wol_core::boot::note(wol_core::boot::Tier::Mok),
            wol_core::boot::key_screen_steps().join("\n")
        );
        assert!(key_screen_needed(Some(&mok), false));
        assert!(key_screen_needed(None, true));
        let own = wol_core::boot::note(wol_core::boot::Tier::OwnKeys);
        assert!(!key_screen_needed(Some(own), false));
        let notes = done_notes(Some(own));
        assert!(
            notes.iter().any(|n| n.contains("install media")),
            "says the media will not start: {notes:?}"
        );
        let short = "Secure Boot: this computer will trust only its own WolOS keys.";
        let notes = done_notes(Some(short));
        assert_eq!(notes.len(), 2, "an older note gets the sentence: {notes:?}");
        assert!(notes[1].contains("install media"), "{notes:?}");
        assert_eq!(done_notes(Some(&mok)).len(), 1, "the steps go on the card");
        assert!(done_notes(None).is_empty());
        assert!(done_notes(Some("  \n")).is_empty());
        assert_eq!(MOK_PENDING, "/run/wolos/mok-pending");
        for s in wol_core::boot::key_screen_steps() {
            assert!(s.contains(wol_core::boot::MOK_PASSWORD) || !s.contains("password"));
        }
    }

    #[test]
    fn the_recovery_key_can_go_on_a_usb_stick() {
        let root = std::env::temp_dir().join(format!("wolos-media-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let run_media = root.join("run/media");
        let media = root.join("media");
        std::fs::create_dir_all(run_media.join("alice/STICK")).unwrap();
        std::fs::create_dir_all(media.join("OTHER")).unwrap();
        std::fs::write(media.join("a-file"), "").unwrap();
        let dirs = media_dirs(&[run_media.clone(), media.clone()]);
        assert_eq!(
            dirs,
            [media.join("OTHER"), run_media.join("alice/STICK")],
            "{dirs:?}"
        );
        assert!(media_dirs(&[root.join("nowhere")]).is_empty());
        let f = save_key(&dirs[1], "ABCDE-FGHIJ").unwrap();
        assert_eq!(f.file_name().unwrap().to_str().unwrap(), RECOVERY_FILE);
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.contains("ABCDE-FGHIJ") && text.contains("Write this key down"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn computer_names_follow_the_backends_rule() {
        for ok in ["wolos", "alice-wolos", "pc1", "4-x", &"x".repeat(63)] {
            assert!(is_valid_hostname(ok), "{ok}");
        }
        for bad in [
            "",
            "-wolos",
            "wolos-",
            "al ice",
            "alice_pc",
            "a..b",
            "a.b-c.d",
            "Alice-PC",
            "café",
            &"x".repeat(64),
        ] {
            assert!(!is_valid_hostname(bad), "{bad}");
        }
        assert_eq!(hostname_for("alice"), "alice-wolos");
        assert_eq!(hostname_for(""), "wolos");
        assert!(is_valid_hostname(&hostname_for("aliceliddell")));
    }

    #[test]
    fn tips_rotate_and_read_well() {
        assert_eq!(tip_at(0.0), TIPS[0]);
        assert_eq!(tip_at(TIP_SECS - 0.1), TIPS[0]);
        assert_eq!(tip_at(TIP_SECS), TIPS[1]);
        assert_eq!(tip_at(TIP_SECS * TIPS.len() as f64), TIPS[0], "wraps");
        assert_eq!(tip_at(-5.0), TIPS[0]);
        for t in TIPS {
            let l = crate::copy::lint(t);
            assert!(l.is_empty(), "{t}: {l:?}");
        }
    }

    #[test]
    fn awake_only_while_installing() {
        let mut a = Awake::default();
        assert_eq!(a.set(false), None, "nothing held on the disk page");
        assert_eq!(a.set(true), Some(true), "Erase and Install takes it");
        assert_eq!(a.set(true), None, "held once");
        assert_eq!(a.set(false), Some(false), "done or stopped gives it back");
        assert_eq!(a.set(true), Some(true), "Try Again takes it again");
    }

    #[test]
    fn the_app_checks_a_disk_and_says_why_not() {
        assert_eq!(check_command("ada0", true), ["sudo", "-n", CHECK, "ada0"]);
        assert_eq!(probe_command("/dev/sda")[3..], ["/dev/sda", "--probe"]);
        assert_eq!(refusal_text("in-use", "SanDisk"), None);
        for why in ["too-small", "not-listed"] {
            let t = refusal_text(why, "SanDisk").unwrap();
            assert!(
                crate::copy::lint(&t).is_empty(),
                "{t}: {:?}",
                crate::copy::lint(&t)
            );
        }
    }

    /// R2B-1: the key and the log come from `install-check --result`, in the shape it prints.
    #[test]
    fn the_result_is_read_from_install_check() {
        let r = parse_result("recovery-key ABCDE-FGHIJ\nlog + sfdisk /dev/vda\nlog\nlog done\n");
        assert_eq!(r.key.as_deref(), Some("ABCDE-FGHIJ"));
        assert_eq!(r.log, "+ sfdisk /dev/vda\n\ndone\n");
        assert_eq!(parse_result("log x\n").key, None, "handed out once");
        assert_eq!(result_command()[..3], ["sudo", "-n", CHECK]);
        assert_eq!(result_command()[3], "--result");
        let script = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../files/usr/lib/wolos/install-check"
        );
        let text = std::fs::read_to_string(script).unwrap();
        for w in ["--result", "'recovery-key %s\\n'", "'s/^/log /'"] {
            assert!(text.contains(w), "install-check --result speaks {w}");
        }
    }

    #[test]
    fn check_names_other_systems_in_the_warning() {
        let c = parse_check("size 500000000000\nos Windows\nos Linux\nboot firmware\n");
        assert!(!parse_check("boot mok\n").secure_boot, "those just install");
        assert_eq!(c.others, ["Windows", "Linux"]);
        assert!(c.secure_boot && c.refused.is_none());
        assert_eq!(c.tier.as_deref(), Some("firmware"));
        assert!(!c.bitlocker);
        let b = parse_check("os Windows\nbitlocker yes\nboot own-keys\n");
        assert!(b.bitlocker && !b.secure_boot, "R2B-9");
        assert!(!parse_check("os Windows\nbitlocker no\n").bitlocker);
        assert_eq!(b.tier.as_deref(), Some("own-keys"));
        assert_eq!(
            erase_warning(&c.others[..1]),
            "This disk has Windows on it. Erasing removes it."
        );
        assert_eq!(
            erase_warning(&c.others),
            "This disk has Windows and Linux on it. Erasing removes them."
        );
        assert_eq!(
            erase_warning(&[]),
            "Everything on this disk will be erased."
        );
        assert_eq!(
            parse_check("refuse too-small\n").refused.as_deref(),
            Some("too-small")
        );
        let script = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../files/usr/lib/wolos/install-check"
        );
        let text = std::fs::read_to_string(script).unwrap();
        for w in [
            "wol install facts",
            "(size|os|bitlocker)",
            "echo \"boot ",
            "refuse too-small",
        ] {
            assert!(text.contains(w), "install-check speaks {w}");
        }
        // The facts lines come from core's disk model (R2B-16): read in the shape it prints them.
        let facts = wol_core::install::disk::Facts {
            bytes: 500_000_000_000,
            systems: vec!["Windows", "Linux"],
            bitlocker: true,
            ..Default::default()
        };
        let c = parse_check(&facts.lines());
        assert_eq!(c.others, ["Windows", "Linux"]);
        assert!(c.bitlocker, "{}", facts.lines());
    }

    #[test]
    fn time_left_reads_well_and_copy_is_clean() {
        assert_eq!(remaining_text(None), "Estimating time remaining…");
        assert_eq!(
            seconds_left((0.25, 100.0), (0.27, 110.0)),
            None,
            "too little to tell"
        );
        let left = seconds_left((0.25, 100.0), (0.6, 240.0)).unwrap();
        assert!((left - 160.0).abs() < 1e-6, "{left}");
        assert_eq!(remaining_text(Some(left)), "About 3 minutes remaining");
        assert_eq!(remaining_text(Some(70.0)), "About a minute remaining");
        assert_eq!(remaining_text(Some(20.0)), "Less than a minute remaining");
        for s in [
            Stage::Disk,
            Stage::Files,
            Stage::Boot,
            Stage::Settings,
            Stage::Packages,
            Stage::Finish,
        ] {
            assert!(!doing(s).is_empty());
            let l = crate::copy::lint(stopped_text(s));
            assert!(l.is_empty(), "{s:?}: {l:?}");
        }
        for s in [Stage::Disk, Stage::Boot, Stage::Done] {
            assert!(
                !CREEP.iter().any(|c| c.0 == s),
                "{s:?} reports its own percent"
            );
        }
    }
}
