// SPDX-License-Identifier: GPL-2.0-or-later
//! GTK's renderer for every WolOS app, however it was started (QA-IMG13-3-03). `wol-session`
//! exports `GSK_RENDERER` for what the desktop starts; an app started any other way (a terminal
//! over ssh, `runuser`, a D-Bus or systemd activation) had GTK's own default, Vulkan, and the
//! System Monitor showed no window in the VM where every app the desktop started did. Each app's
//! `main` calls [`settle`] first, so the session's policy (`desktop/overlay/usr/bin/wol-session`,
//! a test keeps the two equal) holds everywhere. A value already set (a person's, a test's) wins.
use std::path::Path;

/// The variable GTK reads.
pub const VAR: &str = "GSK_RENDERER";
/// Where the render nodes are listed.
pub const DRM: &str = "/sys/class/drm";

/// The renderer for render nodes driven by `drivers` (one entry per `renderD*`; "" when its
/// driver is not known): `ngl` (GL, Mesa) wherever there is one, `cairo` without one, and
/// `cairo` on vmwgfx (VirtualBox, VMware), whose compositor path refuses GTK's GL buffers.
pub fn pick(drivers: &[String]) -> &'static str {
    if drivers.iter().any(|d| d == "vmwgfx") || drivers.is_empty() {
        "cairo"
    } else {
        "ngl"
    }
}

/// The driver of each `renderD*` under `drm` (the name its `device/driver` link points to).
pub fn render_drivers(drm: &Path) -> Vec<String> {
    let Ok(dir) = std::fs::read_dir(drm) else {
        return Vec::new();
    };
    let mut v: Vec<String> = dir
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("renderD"))
        .map(|e| {
            std::fs::read_link(e.path().join("device/driver"))
                .ok()
                .and_then(|l| l.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_default()
        })
        .collect();
    v.sort();
    v
}

/// The renderer this process should ask for: None when `GSK_RENDERER` is already set.
pub fn wanted(current: Option<&str>, drm: &Path) -> Option<&'static str> {
    match current {
        Some(v) if !v.is_empty() => None,
        _ => Some(pick(&render_drivers(drm))),
    }
}

/// Starts this program again with `GSK_RENDERER` set by [`pick`] when nothing set it (an `exec`,
/// same process id, same arguments; the environment is never written in place, RR-068). Call
/// first in `main`, before GTK starts. A failed `exec` keeps going with GTK's default.
pub fn settle() {
    use std::os::unix::process::CommandExt;
    let current = gtk::glib::getenv(VAR).map(|v| v.to_string_lossy().into_owned());
    let Some(r) = wanted(current.as_deref(), Path::new(DRM)) else {
        return;
    };
    let Ok(me) = std::env::current_exe() else {
        return;
    };
    let mut args = std::env::args_os();
    let mut cmd = std::process::Command::new(me);
    if let Some(arg0) = args.next() {
        cmd.arg0(arg0);
    }
    let e = cmd.args(args).env(VAR, r).exec();
    crate::diag::say!("{VAR}={r} not applied: {e}");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn the_session_s_policy() {
        assert_eq!(pick(&[]), "cairo", "no render node: software");
        assert_eq!(pick(&s(&["amdgpu"])), "ngl");
        assert_eq!(pick(&s(&["virtio_gpu"])), "ngl");
        assert_eq!(pick(&s(&[""])), "ngl", "a node whose driver is not known");
        assert_eq!(pick(&s(&["i915", "vmwgfx"])), "cairo");
    }

    /// A computer where Vulkan does not start (radv cannot open the device, no ICD): the apps
    /// never ask GTK for Vulkan; GL where a render node is, the CPU renderer (cairo) otherwise.
    #[test]
    fn vulkan_is_never_the_pick() {
        for drivers in [
            vec![],
            s(&["amdgpu"]),
            s(&[""]),
            s(&["vmwgfx"]),
            s(&["i915", "nouveau"]),
        ] {
            let r = pick(&drivers);
            assert!(r == "ngl" || r == "cairo", "{drivers:?}: {r}");
        }
    }

    #[test]
    fn render_nodes_are_read_from_sysfs() {
        let d = std::env::temp_dir().join(format!("wol-renderer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for (node, driver) in [("renderD128", "amdgpu"), ("card0", "amdgpu")] {
            std::fs::create_dir_all(d.join(node).join("device")).unwrap();
            std::os::unix::fs::symlink(
                format!("../../../bus/pci/drivers/{driver}"),
                d.join(node).join("device/driver"),
            )
            .unwrap();
        }
        assert_eq!(render_drivers(&d), s(&["amdgpu"]));
        assert_eq!(wanted(None, &d), Some("ngl"));
        assert_eq!(wanted(Some(""), &d), Some("ngl"));
        assert_eq!(wanted(Some("vulkan"), &d), None, "a set value wins");
        assert_eq!(wanted(None, &d.join("none")), Some("cairo"));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Every app binary calls [`settle`] before anything else (the System Monitor did not).
    #[test]
    fn every_app_settles_its_renderer_first() {
        let bins = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bin");
        let mut missing = Vec::new();
        for e in std::fs::read_dir(&bins).unwrap().flatten() {
            let text = std::fs::read_to_string(e.path()).unwrap();
            let body = text.split("fn main").nth(1).unwrap_or_default();
            let first = body
                .lines()
                .skip(1)
                .map(str::trim)
                .find(|l| !l.is_empty() && !l.starts_with("//"))
                .unwrap_or_default();
            if first != "shell::kit::renderer::settle();" {
                missing.push(e.file_name().to_string_lossy().into_owned());
            }
        }
        missing.sort();
        assert!(
            missing.is_empty(),
            "main does not start with settle(): {missing:?}"
        );
    }

    /// The same policy as wol-session's shell lines, when the desktop tree is next to this crate.
    #[test]
    fn wol_session_says_the_same() {
        let f =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../desktop/overlay/usr/bin/wol-session");
        let Ok(text) = std::fs::read_to_string(&f) else {
            eprintln!("{}: not here, parity not checked", f.display());
            return;
        };
        for l in [
            "[ \"$(basename \"$(readlink \"$d\" 2>/dev/null)\")\" = vmwgfx ] && vmwgfx=1",
            "export GSK_RENDERER=${GSK_RENDERER:-cairo}",
            "export GSK_RENDERER=${GSK_RENDERER:-ngl}",
        ] {
            assert!(text.lines().any(|x| x.trim() == l), "wol-session lacks {l}");
        }
    }
}
