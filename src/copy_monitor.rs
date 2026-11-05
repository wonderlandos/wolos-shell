// SPDX-License-Identifier: GPL-2.0-or-later
//! System Monitor's words (docs/modules/monitor.md): its pages hold none of their own.
//! Invariant: every sentence in [`ALL`] and every sentence the functions below make passes
//! `copy::lint` (tested below).
use wol_core::monitor::apps::{Energy, Rest};
use wol_core::monitor::net::Kind;

/// The app's name (window title, About).
pub const TITLE: &str = "System Monitor";

// ---- Pages (sidebar rows and page titles) ----

pub const OVERVIEW: &str = "Overview";
pub const APPS: &str = "Apps";
pub const PROCESSES: &str = "Processes";
pub const DISKS: &str = "Disks";
pub const NETWORK: &str = "Network";
pub const STARTUP: &str = "Startup";

// ---- Overview tiles ----

pub const CPU: &str = "Processor";
pub const MEMORY: &str = "Memory";
pub const SWAP: &str = "Compressed memory";
pub const DISK: &str = "Disk activity";
pub const NET: &str = "Network activity";
pub const GPU: &str = "Graphics";
pub const TEMPERATURE: &str = "Temperature";
/// "8 cores · load 0.50" next to the processor's value.
pub fn cores(n: usize, load: Option<f64>) -> String {
    let c = if n == 1 { "core" } else { "cores" };
    match load {
        Some(l) => format!("{n} {c} · load {l:.2}"),
        None => format!("{n} {c}"),
    }
}

/// "of 16.0 GB" next to a memory value.
pub fn of_total(total: &str) -> String {
    format!("of {total}")
}

/// Overview's Graphics tile when the card reports no load (i915, nouveau).
pub const GPU_NO_LOAD: &str = "This graphics card doesn’t report how busy it is.";
/// Overview's Graphics tile without a graphics card at all (a virtual machine without one).
pub const NO_GPU: &str = "No graphics card reports here.";
/// Overview's Temperature tile when no sensor reports.
pub const NO_TEMPS: &str = "This computer doesn’t report its temperatures.";

// ---- Apps ----

pub const APPS_EMPTY: (&str, &str) = (
    "No apps are open",
    "Apps you open show here with the memory and energy they use.",
);
pub const ENERGY_ABOUT: &str = "Energy is how busy an app kept the processor over the last minute.";
pub const SAVER_ON: &str = "Memory Saver gives back the memory of apps you aren’t using. A resting app takes it back as you use it again.";
pub const SAVER_OFF: &str =
    "Memory Saver is off, so apps keep all their memory in the background. Turn it on in Settings.";
pub const QUIT: &str = "Quit";
pub const FORCE_QUIT: &str = "Force Quit";
pub const QUIT_BODY: &str =
    "The app closes as if you had quit it. Anything it hasn’t saved may be lost.";
pub const FORCE_QUIT_BODY: &str =
    "The app is paused, so it stops at once. Anything it hasn’t saved is lost.";
pub const NOT_QUIT: &str = "That app couldn’t be stopped. Try Force Quit from the app menu.";

pub fn quit_title(app: &str) -> String {
    format!("Quit “{app}”?")
}

pub fn energy(e: Energy) -> &'static str {
    match e {
        Energy::Low => "Low",
        Energy::Moderate => "Moderate",
        Energy::High => "High",
    }
}

pub fn rest(r: Rest) -> &'static str {
    match r {
        Rest::Running => "Running",
        Rest::Resting => "Resting",
        Rest::Frozen => "Paused",
    }
}

/// An app row's facts: "420 MB · 3% CPU · Low energy".
pub fn app_facts(memory: &str, cpu: &str, e: Energy) -> String {
    format!("{memory} · {cpu} CPU · {} energy", energy(e).to_lowercase())
}

// ---- Processes ----

pub const FILTER: &str = "Filter Processes";
pub const FOLD: &str = "Show or Hide Child Processes";
pub const NAME: &str = "Name";
pub const PID: &str = "Number";
pub const CPU_SHORT: &str = "CPU";
pub const CANCEL: &str = "Cancel";
pub const CLOSE: &str = "Close";
/// Title of the sheet that says how a Quit or End Process went.
pub const AFTER_TITLE: &str = "Not everything was stopped";
pub const PARTLY_ENDED: &str =
    "One process had already ended, so it was left alone. The others were stopped.";
pub const ALREADY_ENDED: &str = "That process had already ended, so nothing was stopped.";
pub const END: &str = "End Process";
pub const END_BODY: &str = "Anything the process hasn’t saved is lost.";
pub const NOT_ENDED: &str =
    "That process couldn’t be ended. It may belong to another account or to the system.";
pub const NO_MATCH: (&str, &str) = (
    "No process matches",
    "Check the spelling, or search by process number.",
);

pub fn end_title(name: &str, pid: i32) -> String {
    format!("End “{name}” ({pid})?")
}

/// A process row's facts: "pid 4242 · ana · 120 MB".
pub fn proc_facts(pid: i32, user: &str, memory: &str) -> String {
    format!("{pid} · {user} · {memory}")
}

// ---- Disks ----

pub const DISK_SPACE: &str = "Space";
pub const READ: &str = "Read";
pub const WRITTEN: &str = "Written";
pub const NO_DISKS: (&str, &str) = (
    "No disks report here",
    "Disks show here once the computer has one mounted.",
);

/// "200 GB of 500 GB used".
pub fn used_of(used: &str, total: &str) -> String {
    format!("{used} of {total} used")
}

/// A mount point as a person names it.
pub fn mount_name(mount: &str) -> String {
    match mount {
        "/" => "System".into(),
        "/home" => "Home".into(),
        "/boot" | "/boot/efi" | "/efi" => "Startup disk".into(),
        m => m
            .rsplit('/')
            .find(|s| !s.is_empty())
            .unwrap_or(m)
            .to_owned(),
    }
}

// ---- Network ----

pub const RECEIVED: &str = "Received";
pub const SENT: &str = "Sent";
pub const CONNECTED: &str = "Connected";
pub const NOT_CONNECTED: &str = "Not connected";
pub const NO_NETWORK: (&str, &str) = (
    "No network connections",
    "Connect to Wi-Fi or plug in a cable in Settings.",
);

pub fn kind(k: Kind) -> &'static str {
    match k {
        Kind::Wired => "Ethernet",
        Kind::Wireless => "Wi-Fi",
        Kind::Virtual => "Virtual",
    }
}

/// "Received 1.5 MB · Sent 300 KB".
pub fn totals(rx: &str, tx: &str) -> String {
    format!("{RECEIVED} {rx} · {SENT} {tx}")
}

// ---- Startup ----

pub const STARTUP_ABOUT: &str =
    "Services that start when you log in. Apps that open at login are in Settings, under General.";
pub const LOGIN_ITEMS: &str = "Open Login Items";
pub const STARTUP_EMPTY: (&str, &str) = (
    "Nothing else starts with you",
    "Services you add for your account show here.",
);
pub const STARTUP_NOT_READ: &str =
    "The services that start with you couldn’t be read. Log out and back in, then try again.";
pub const STARTUP_NOT_CHANGED: &str = "That service couldn’t be switched. Try again in a moment.";
pub const SHIPPED: &str = "Part of WolOS";
pub const ON: &str = "On";
pub const OFF: &str = "Off";

// ---- Status bar ----

/// "6 apps, 284 processes".
pub fn status(apps: usize, procs: Option<usize>) -> String {
    let a = if apps == 1 { "app" } else { "apps" };
    match procs {
        Some(p) => format!("{apps} {a}, {p} processes"),
        None => format!("{apps} {a}"),
    }
}

/// Every sentence above (for the lint test).
pub const ALL: [&str; 21] = [
    PARTLY_ENDED,
    ALREADY_ENDED,
    GPU_NO_LOAD,
    NO_GPU,
    NO_TEMPS,
    APPS_EMPTY.1,
    ENERGY_ABOUT,
    SAVER_ON,
    SAVER_OFF,
    QUIT_BODY,
    FORCE_QUIT_BODY,
    NOT_QUIT,
    END_BODY,
    NOT_ENDED,
    NO_MATCH.1,
    NO_DISKS.1,
    NO_NETWORK.1,
    STARTUP_ABOUT,
    STARTUP_NOT_READ,
    STARTUP_NOT_CHANGED,
    STARTUP_EMPTY.1,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_monitor_sentence_passes_the_lint() {
        for s in ALL {
            let p = crate::copy::lint(s);
            assert!(p.is_empty(), "“{s}”: {p:?}");
        }
        // Titles and facts are not sentences: only the words are held to the lint.
        let made = [
            quit_title("Discord"),
            end_title("helium", 4242),
            app_facts("420 MB", "3%", Energy::Moderate),
            used_of("200 GB", "500 GB"),
            totals("1.5 MB", "300 KB"),
            status(1, Some(284)),
            cores(8, Some(0.5)),
        ];
        for s in &made {
            let p: Vec<String> = crate::copy::lint(s)
                .into_iter()
                .filter(|p| !p.contains("full stop") && !p.contains("capital"))
                .collect();
            assert!(p.is_empty(), "“{s}”: {p:?}");
        }
    }

    #[test]
    fn mounts_and_counts_read_as_words() {
        assert_eq!(mount_name("/"), "System");
        assert_eq!(mount_name("/run/media/ana/USB STICK"), "USB STICK");
        assert_eq!(status(1, None), "1 app");
        assert_eq!(status(6, Some(284)), "6 apps, 284 processes");
        assert_eq!(
            app_facts("1.4 GB", "12%", Energy::High),
            "1.4 GB · 12% CPU · high energy"
        );
    }
}
