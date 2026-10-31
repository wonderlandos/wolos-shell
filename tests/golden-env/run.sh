#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
# One command: build the golden image and run tests/golden.rs in it. From any directory:
#   crates/shell/tests/golden-env/run.sh                      all cases, compare with the goldens
#   WOL_GOLDEN_ONLY=lua-clock crates/shell/tests/golden-env/run.sh   cases whose name contains it
#   WOL_GOLDEN_UPDATE=1 crates/shell/tests/golden-env/run.sh  rewrite the goldens (review the diff!)
#   crates/shell/tests/golden-env/starve.sh                   wallpaper capture on a starved CPU
# Renders land in $WOL_GOLDEN_TARGET/tmp/golden (default <repo>/target/golden-env).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../../.." && pwd)
engine=${CONTAINER_ENGINE:-$(command -v podman || command -v docker)}
target=${WOL_GOLDEN_TARGET:-$root/target/golden-env}
mkdir -p "$target"
user=(--user "$(id -u):$(id -g)")
[[ $engine == *podman ]] && user=(--userns=keep-id)
"$engine" build -t wol-golden "$here"
env_args=()
for v in WOL_GOLDEN_UPDATE WOL_GOLDEN_ONLY; do [[ -v $v ]] && env_args+=(-e "$v=${!v}"); done
# WOL_GOLDEN_RUN_ARGS: extra container flags, e.g. "--cpuset-cpus 0" (see starve.sh).
# shellcheck disable=SC2086
exec "$engine" run --rm "${user[@]}" "${env_args[@]}" ${WOL_GOLDEN_RUN_ARGS:-} \
  -v "$root":/work -v "$target":/target -e CARGO_TARGET_DIR=/target \
  wol-golden sh -c 'mkdir -p -m700 "$XDG_RUNTIME_DIR" "$HOME" && exec cargo test --test golden "$@"' sh "$@"


