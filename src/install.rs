//! Install WolOS (ADR 0013, ADR 0023): the disks it may use, the progress lines of
//! `/usr/lib/wolos/install-run` and what the progress page says. Pure; UI in `ui::installer`,
//! the steps in [`flow`].

#[path = "install_flow.rs"]
pub mod flow;

/// Erases the disk and installs (root; checks the disk against `install-disks` again).
pub const RUN: &str = "/usr/lib/wolos/install-run";
/// Disks that are not the boot media, one per line.
pub const DISKS: &str = "/usr/lib/wolos/install-disks";
/// Facts about one disk before anything is written (`size`, `os`, `secureboot`, `refuse` lines).
pub const CHECK: &str = "/usr/lib/wolos/install-check";

/// What `install-check <disk>` found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Check {
    /// Other systems on the disk, as named for people: "Windows", "Linux", "macOS", "FreeBSD".
    pub others: Vec<String>,
    /// Secure Boot is on: the installed WolOS would not start.
    pub secure_boot: bool,
    pub refused: Option<String>,
}

pub fn parse_check(out: &str) -> Check {
    let mut c = Check::default();
    for line in out.lines() {
        match line.split_once(' ') {
            Some(("os", name)) => c.others.push(name.to_string()),
            Some(("secureboot", "on")) => c.secure_boot = true,
            Some(("refuse", why)) => c.refused = Some(why.to_string()),
            _ => {}
        }
    }
    c
}

/// The erase warning names what will be lost: "This disk has Windows on it. Erasing removes it."
pub fn erase_warning(others: &[String]) -> String {
    match others {
        [] => "Everything on this disk will be erased.".to_string(),
        [one] => format!("This disk has {one} on it. Erasing removes it."),
        [rest @ .., last] => format!(
            "This disk has {} and {last} on it. Erasing removes them.",
            rest.join(", ")
        ),
    }
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

/// Why the app won't erase `title` (`refuse` reason from `install-check`); None: it may.
/// `in-use` is not a refusal here: install-run lets go of the disk's mounts first.
pub fn refusal_text(why: &str, title: &str) -> Option<String> {
    match why {
        "in-use" => None,
        "too-small" => Some(format!(
            "“{title}” is too small. WolOS needs at least 16 GB."
        )),
        _ => Some(format!(
            "“{title}” can't be erased: it's the disk WolOS started from, or it's gone. Choose another disk."
        )),
    }
}

/// Sheet shown when Secure Boot is on: honest, no jargon beyond the setting's own name.
pub const SECURE_BOOT_TITLE: &str = "Turn Off Secure Boot to Start WolOS";
pub const SECURE_BOOT_TEXT: &str = "This beta of WolOS can’t start while Secure Boot is on. \
Restart, open your computer's startup settings (often F2, F10, F12 or Delete), turn off \
Secure Boot, then install again.";

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
pub const RECOVERY_KEY: &str = "recovery-key";

/// Writes the plan and its passwords (0600). Newlines end a password: refused by the fields' length
/// only in the sense that none can be typed.
pub fn write_plan(
    dir: &std::path::Path,
    plan: &wol_core::install::plan::Plan,
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
    let json = serde_json::to_string(plan).map_err(std::io::Error::other)?;
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
    /// The partitions.
    Disk,
    /// The system files are copied (no progress of their own).
    Files,
    /// Boot loader on the EFI partition.
    Boot,
    /// Language, time zone and the account.
    Settings,
    /// Apps from the local repository.
    Packages,
    /// WolOS settings, unmount.
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
            "boot" => Stage::Boot,
            "settings" => Stage::Settings,
            "packages" => Stage::Packages,
            "finish" => Stage::Finish,
            "done" => Stage::Done,
            "failed" => Stage::Failed,
            _ => return None,
        })
    }
}

/// One progress line: `wolos-install: <stage> <percent> <message>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub stage: Stage,
    /// 0-100, never backwards within a run (except `failed`, which says 0).
    pub percent: u8,
    /// For the log; the page says `doing(stage)`.
    pub message: String,
}

impl Report {
    pub fn new(stage: Stage, percent: u8) -> Report {
        Report {
            stage,
            percent,
            message: String::new(),
        }
    }
}

/// One progress line.
pub fn parse_line(line: &str) -> Option<Report> {
    let rest = &line[line.find(MARK)? + MARK.len()..];
    let rest = rest.trim_end_matches(['\r', '\n']);
    let (stage, rest) = rest.split_once(' ')?;
    let (percent, message) = rest.split_once(' ').unwrap_or((rest, ""));
    let percent: u8 = percent.parse().ok().filter(|p| *p <= 100)?;
    Some(Report {
        stage: Stage::parse(stage)?,
        percent,
        message: message.trim().to_string(),
    })
}

/// Where `files` starts and ends (the boot loader step's percent): the bar creeps between them
/// while the system files are copied, and never past the end.
const FILES_START: f64 = 0.05;
const FILES_END: f64 = 0.21;
/// Copying the system files: about this long.
const FILES_SECS: f64 = 120.0;

/// Bar position for `r`, `secs` after its stage began.
pub fn fraction(r: &Report, secs: f64) -> f64 {
    let at = f64::from(r.percent) / 100.0;
    match r.stage {
        Stage::Files => {
            let creep =
                FILES_START + (FILES_END - FILES_START) * 0.9 * (secs / FILES_SECS).min(1.0);
            at.max(creep)
        }
        Stage::Failed => 0.0,
        _ => at,
    }
}

/// Seconds left, from how fast the bar moved since `start` (None until it has moved enough).
pub fn seconds_left(start: (f64, f64), now: (f64, f64)) -> Option<f64> {
    let (moved, took) = (now.0 - start.0, now.1 - start.1);
    (moved >= 0.05 && took > 0.0).then(|| (1.0 - now.0) * took / moved)
}

/// The line under the bar.
pub fn remaining_text(secs: Option<f64>) -> String {
    match secs {
        None => "Estimating time remaining…".into(),
        Some(s) if s < 60.0 => "Less than a minute remaining".into(),
        Some(s) => {
            let m = (s / 60.0).round().max(1.0) as u64;
            if m == 1 {
                "About a minute remaining".into()
            } else {
                format!("About {m} minutes remaining")
            }
        }
    }
}

/// What the page says the installer is doing.
pub fn doing(stage: Stage) -> &'static str {
    match stage {
        Stage::Prepare | Stage::Disk => "Preparing the disk…",
        Stage::Files => "Copying system files…",
        Stage::Boot => "Installing the startup files…",
        Stage::Settings => "Saving settings…",
        Stage::Packages => "Installing apps…",
        Stage::Finish => "Finishing up…",
        Stage::Done => "Done",
        Stage::Failed => "",
    }
}

/// Failed page subtitle, by the stage the install stopped in.
pub fn stopped_text(at: Stage) -> &'static str {
    match at {
        Stage::Prepare | Stage::Disk => {
            "The disk couldn't be prepared. Check that it's connected, then try again or choose another disk."
        }
        _ => "The installation stopped before it finished. Try again, or choose another disk.",
    }
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
        let f = write_plan(&dir, &plan, "disk pw", "account pw").unwrap();
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&f), 0o600);
        assert_eq!(mode(&dir.join(DISK_PASSWORD)), 0o600);
        assert_eq!(
            std::fs::read_to_string(dir.join(ACCOUNT_PASSWORD)).unwrap(),
            "account pw"
        );
        // A second run starts clean; no disk password leaves no file behind.
        write_plan(&dir, &plan, "", "account pw").unwrap();
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
                message: "Preparing to install on ada0".into()
            })
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
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../image/");
        let mut seen = Vec::new();
        for f in ["files/usr/lib/wolos/install-run"] {
            let text = std::fs::read_to_string(format!("{root}{f}")).unwrap();
            // echo/printf lines ("%d" is a percent) and install-run's `say <stage> <percent> ...`.
            let printed = text.match_indices(MARK).map(|(i, _)| &text[i..]);
            let said = text
                .match_indices("say ")
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
            Stage::Finish,
            Stage::Failed,
        ] {
            assert!(seen.contains(&s), "no script reports {s:?}");
        }
        // (report, seconds since its stage began), as the page samples them.
        let order = [
            (Report::new(Stage::Prepare, 0), 0.0),
            (Report::new(Stage::Disk, 2), 9.0),
            (Report::new(Stage::Files, 5), 0.0),
            (Report::new(Stage::Files, 5), 30.0),
            (Report::new(Stage::Files, 6), 30.0),
            (Report::new(Stage::Files, 6), 600.0),
            (Report::new(Stage::Boot, 22), 0.0),
            (Report::new(Stage::Settings, 24), 0.0),
            (Report::new(Stage::Packages, 25), 0.0),
            (Report::new(Stage::Packages, 95), 300.0),
            (Report::new(Stage::Finish, 95), 0.0),
            (Report::new(Stage::Finish, 97), 5.0),
            (Report::new(Stage::Done, 100), 0.0),
        ];
        let bar: Vec<f64> = order.iter().map(|(r, t)| fraction(r, *t)).collect();
        assert!(bar.windows(2).all(|w| w[0] <= w[1]), "{bar:?}");
        assert_eq!(*bar.last().unwrap(), 1.0);
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

    #[test]
    fn check_names_other_systems_in_the_warning() {
        let c = parse_check("size 500000000000\nos Windows\nos Linux\nsecureboot on\n");
        assert_eq!(c.others, ["Windows", "Linux"]);
        assert!(c.secure_boot && c.refused.is_none());
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
            "/../../image/files/usr/lib/wolos/install-check"
        );
        let text = std::fs::read_to_string(script).unwrap();
        for w in [
            "\"os \" names[i]",
            "secureboot on",
            "refuse too-small",
            "Windows Linux macOS FreeBSD",
        ] {
            assert!(text.contains(w), "install-check speaks {w}");
        }
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
    }
}
