// SPDX-License-Identifier: GPL-2.0-or-later
//! Window snapping, the shell's half (the policy is wol-hypr's, `crates/realtime/src/snap.rs`): the
//! drop preview shown while a window is dragged to a screen edge, and the snap layouts chooser
//! that opens when the pointer rests on a window's zoom button. wol-hypr tells the shell with
//! one line each over the shell socket; the chooser answers with `wolsnap <zone> address:0x..`
//! on the compositor socket. Pure here; the surfaces are `ui::snap`.

/// The places the chooser offers, by the word wol-hypr uses, in reading order: halves, then the
/// quarters (`realtime::snap::LAYOUTS`).
pub const ZONES: [&str; 6] = [
    "left",
    "right",
    "top-left",
    "top-right",
    "bottom-left",
    "bottom-right",
];

/// A line from wol-hypr.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    /// `snap-preview off`: the drag ended or left the edge.
    PreviewOff,
    /// `snap-preview <output> <zone> <x> <y> <w> <h>`: the frame the window will take, in the
    /// output's own coordinates.
    Preview {
        output: String,
        zone: String,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    },
    /// `snap-layouts <output> <address> <x> <y>`: the pointer rests on the zoom button of window
    /// `address`, whose centre is at (x, y) on the output.
    Layouts {
        output: String,
        address: String,
        x: i32,
        y: i32,
    },
}

/// A connector name (`eDP-1`, `HDMI-A-1`): what the compositor can call an output.
pub fn is_output(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 32
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.:".contains(c))
}

/// A window address as the compositor prints it: `0x` and hex digits.
pub fn is_address(s: &str) -> bool {
    s.strip_prefix("0x")
        .is_some_and(|h| !h.is_empty() && h.len() <= 16 && h.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Largest frame edge or position a line may carry (px): a bad line cannot size a surface to
/// gigabytes.
const LIMIT: i32 = 32768;

impl Line {
    pub fn of(line: &str) -> Option<Line> {
        let mut w = line.split_whitespace();
        let verb = w.next()?;
        let f: Vec<&str> = w.collect();
        let num = |s: &str| {
            s.parse::<i32>()
                .ok()
                .filter(|n| (-LIMIT..=LIMIT).contains(n))
        };
        match (verb, f.as_slice()) {
            ("snap-preview", ["off"]) => Some(Line::PreviewOff),
            ("snap-preview", [out, zone, x, y, w, h])
                if is_output(out) && (ZONES.contains(zone) || *zone == "zoom") =>
            {
                let (w, h) = (num(w)?, num(h)?);
                (w > 0 && h > 0).then_some(Line::Preview {
                    output: (*out).into(),
                    zone: (*zone).into(),
                    x: num(x)?,
                    y: num(y)?,
                    w,
                    h,
                })
            }
            ("snap-layouts", [out, addr, x, y]) if is_output(out) && is_address(addr) => {
                Some(Line::Layouts {
                    output: (*out).into(),
                    address: (*addr).into(),
                    x: num(x)?,
                    y: num(y)?,
                })
            }
            _ => None,
        }
    }
}

/// The compositor request that snaps window `address` to `zone` (`wolsnap zone`, wol-hypr; without
/// `zone` the words left and right are Super+arrow keys); None for a zone the chooser does not
/// offer or a bad address.
pub fn request(zone: &str, address: &str) -> Option<String> {
    (ZONES.contains(&zone) && is_address(address))
        .then(|| format!("wolsnap zone {zone} address:{address}"))
}

/// Where a zone sits in a thumbnail, in halves of it: (x, y, w, h), each 0 to 2.
pub fn cell(zone: &str) -> Option<(i32, i32, i32, i32)> {
    Some(match zone {
        "left" => (0, 0, 1, 2),
        "right" => (1, 0, 1, 2),
        "top-left" => (0, 0, 1, 1),
        "top-right" => (1, 0, 1, 1),
        "bottom-left" => (0, 1, 1, 1),
        "bottom-right" => (1, 1, 1, 1),
        _ => return None,
    })
}

/// Pointer rest on the zoom button before the chooser opens (ms): wol-hypr's value
/// (`wol_snap_layouts_dwell_ms`), repeated for the manual and tests.
pub const DWELL_MS: u64 = 600;

/// The chooser's top-left (x, y) for a panel of `size` under the zoom button centred at `button`,
/// on an output of `screen`: centred on the button, kept `margin` from the output's sides, and
/// above the button when there is no room below.
pub fn chooser_at(
    screen: (i32, i32),
    button: (i32, i32),
    size: (i32, i32),
    margin: i32,
) -> (i32, i32) {
    let below = button.1 + 14;
    let y = if below + size.1 + margin <= screen.1 {
        below
    } else {
        (button.1 - 14 - size.1).max(margin)
    };
    let max_x = (screen.0 - size.0 - margin).max(margin);
    ((button.0 - size.0 / 2).clamp(margin, max_x), y)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REALTIME: &str = include_str!("../../realtime/src/snap.rs");
    const PLUGIN: &str = include_str!("../../../plugins/wol-hypr/main.cpp");

    #[test]
    fn preview_and_layout_lines_parse() {
        assert_eq!(Line::of("snap-preview off\n"), Some(Line::PreviewOff));
        assert_eq!(
            Line::of("snap-preview eDP-1 top-left 12 40 622 342"),
            Some(Line::Preview {
                output: "eDP-1".into(),
                zone: "top-left".into(),
                x: 12,
                y: 40,
                w: 622,
                h: 342
            })
        );
        assert!(matches!(
            Line::of("snap-preview HDMI-A-1 zoom 0 28 1280 708"),
            Some(Line::Preview { .. })
        ));
        assert_eq!(
            Line::of("snap-layouts eDP-1 0x55d0c8a1 1180 14"),
            Some(Line::Layouts {
                output: "eDP-1".into(),
                address: "0x55d0c8a1".into(),
                x: 1180,
                y: 14
            })
        );
        for bad in [
            "snap-preview",
            "snap-preview on",
            "snap-preview eDP-1 middle 0 0 10 10",
            "snap-preview eDP-1 left 0 0 0 10",
            "snap-preview eDP-1 left 0 0 99999999 10",
            "snap-preview e;DP left 0 0 10 10",
            "snap-preview eDP-1 left 0 0 10",
            "snap-layouts eDP-1 window 1 2",
            "snap-layouts eDP-1 0x 1 2",
            "snap-layouts eDP-1 0xzz 1 2",
            "snap-layouts eDP-1 0x1f 1",
            "snap-layout eDP-1 0x1f 1 2",
            "snap-preview eDP-1 left -2147483648 0 10 10",
            "snap-preview eDP-1 left 0 0 2147483647 10",
            "snap-preview eDP-1 left 0 0 -5 10",
            "snap-preview eDP-1 left 0 0 32769 10",
            "snap-layouts eDP-1 0x1f -2147483648 2",
            "snap-layouts eDP-1 0x1f 1 99999999999",
        ] {
            assert_eq!(Line::of(bad), None, "{bad}");
        }
    }

    /// Whatever the compositor side (or another process on the socket) sends, parsing never
    /// panics and a line that parses is inside the limits: a small xorshift over a token pool.
    #[test]
    fn random_tokens_never_panic_the_parser() {
        let pool = [
            "snap-preview",
            "snap-layouts",
            "off",
            "eDP-1",
            "left",
            "zoom",
            "top-left",
            "0x1f",
            "0x",
            "-2147483648",
            "2147483647",
            "32768",
            "-32768",
            "0",
            "7",
            "x",
            "",
            "99999999999",
            "-0",
        ];
        let mut s: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        for _ in 0..20_000 {
            let n = 1 + (next() % 8) as usize;
            let line: Vec<&str> = (0..n)
                .map(|_| pool[(next() % pool.len() as u64) as usize])
                .collect();
            if let Some(Line::Preview { x, y, w, h, .. }) = Line::of(&line.join(" ")) {
                assert!(w > 0 && h > 0 && [x, y, w, h].iter().all(|v| v.abs() <= 32768));
            }
        }
    }

    #[test]
    fn the_chooser_asks_for_a_zone_it_offers_and_a_real_window() {
        assert_eq!(
            request("top-right", "0x1f2e"),
            Some("wolsnap zone top-right address:0x1f2e".into())
        );
        for bad in [
            ("zoom", "0x1f"),
            ("none", "0x1f"),
            ("left", "1f"),
            ("left", "0x1f;x"),
        ] {
            assert_eq!(request(bad.0, bad.1), None, "{bad:?}");
        }
        for z in ZONES {
            let (x, y, w, h) = cell(z).unwrap();
            assert!(x + w <= 2 && y + h <= 2 && w > 0 && h > 0, "{z}");
        }
        assert_eq!(cell("zoom"), None);
    }

    /// The words and the dwell are the plugin's: the realtime crate names the zones, wol-hypr
    /// writes the lines this module reads.
    #[test]
    fn the_shell_reads_what_wol_hypr_writes() {
        for z in ZONES {
            assert!(REALTIME.contains(&format!("c\"{z}\"")), "{z}");
        }
        assert!(REALTIME.contains(&format!("LAYOUTS_DWELL_MS: u64 = {DWELL_MS}")));
        assert!(PLUGIN.contains("\"snap-preview {} {} {} {} {} {}\\n\""));
        assert!(PLUGIN.contains("\"snap-layouts {} 0x{:x} {} {}\\n\""));
        assert!(PLUGIN.contains("tellShell(\"snap-preview off\\n\")"));
        // `zone` first: left and right alone are keys.
        assert!(PLUGIN.contains("what == \"zone\""));
    }

    #[test]
    fn the_chooser_stays_on_the_output() {
        let s = (1280, 800);
        let size = (260, 120);
        // Under a zoom button in the middle: centred on it, just below.
        assert_eq!(chooser_at(s, (640, 14), size, 8), (510, 28));
        // Near the right edge: kept off the edge.
        assert_eq!(chooser_at(s, (1270, 14), size, 8), (1280 - 260 - 8, 28));
        // Near the left edge.
        assert_eq!(chooser_at(s, (4, 14), size, 8), (8, 28));
        // A window low on the screen: the chooser goes above the button.
        assert_eq!(chooser_at(s, (640, 780), size, 8), (510, 780 - 14 - 120));
    }
}
