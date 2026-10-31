#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
# Wallpaper capture on a starved CPU: renders lua-clock-small-light pinned to one core that 12 busy
# loops share. Before the harness waited for the wallpaper (wait_for_wallpaper in golden.rs) the
# render came out without it (a 9.5 KB PNG instead of 27.9 KB). Passes when the case reports ok.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
hogs=()
trap 'kill "${hogs[@]}" 2>/dev/null || true' EXIT
for _ in $(seq 12); do taskset -c 0 bash -c 'while :; do :; done' & hogs+=($!); done
WOL_GOLDEN_ONLY=lua-clock-small-light WOL_GOLDEN_RUN_ARGS="--cpuset-cpus 0" "$here/run.sh"
