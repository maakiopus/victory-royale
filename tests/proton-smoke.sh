#!/usr/bin/env bash
# Exercise the normal launcher and title entry point without any game files.
set -euo pipefail
root=$(cd -- "$(dirname -- "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
title="$work/probe title.exe"
export WINEPREFIX=${VICTORY_SMOKE_PREFIX:-${XDG_DATA_HOME:-$HOME/.local/share}/victory/proton-smoke}
rustc --edition 2024 --target x86_64-pc-windows-gnullvm -C target-feature=+crt-static \
    -C panic=abort -C opt-level=2 \
    -C link-arg=-nostartfiles -C link-arg=-Wl,--entry=title_main \
    "$root/tests/fixtures/proton-smoke.rs" -o "$title" || {
    echo 'Building the smoke fixture requires the gnullvm Rust target and LLVM-MinGW (docs/proton-launcher.md).' >&2
    exit 1
}
# VIctory exits with the status the title's entry point returned, which this
# fixture makes 42 when both of its imports worked. So a successful run ends
# 42, not 0: check that exact status rather than letting a non-zero one end
# the script, and check the line VIctory printed as well.
status=0
"$root/scripts/victory-proton.sh" "$title" --execute > "$work/run.log" 2>&1 || status=$?
cat -- "$work/run.log"
test "$status" -eq 42
grep -Eq '^title exited with 42[[:space:]]*$' "$work/run.log"
