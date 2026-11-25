#!/usr/bin/env bash
# Fixed graphics/performance sweep for victory-royale on Linux.
#
# Every sweep uses identical Proton, keys, capture points and duration, so
# results from different commits and different agents are comparable.
# No system Wine is used at any point: only umu-run + Proton Experimental.
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
LABEL=${SWEEP_LABEL:-sweep}
WORK=/tmp/opencode/sweep-$LABEL
EXE=${SWEEP_EXE:-$ROOT/target/x86_64-pc-windows-msvc/diag/victory.exe}
GAME=${SWEEP_GAME:-W:\\hdd\\GTA6.Xbox.v1.0.2.1-CYBERLEEK\\game_scarlett_master.exe}
PROTON=${PROTONPATH:-$HOME/.local/share/Steam/steamapps/common/Proton - Experimental}
DURATION=${SWEEP_DURATION:-660}
KEYS=${SWEEP_KEYS:-20:Enter,70:Enter}
LOG_FILTER=${SWEEP_LOG:-victory_gpu::frame=debug,info}

mkdir -p "$WORK"
BAT="$WORK/game.bat"
LOG="$WORK/app.log"
CAP="$WORK/capture"
mkdir -p "$CAP"

# Translate the Unix exe path to the Z: drive umu-run exposes.
ZEXE=$(printf '%s' "$EXE" | sed 's#^/tmp/#Z:\\tmp\\#; s#^/home/#Z:\\home\\#; s#/#\\#g')

cat > "$BAT" <<EOF
@echo off
set VICTORY_LOG=$LOG_FILTER
set VICTORY_KEYS=$KEYS
set VICTORY_CAPTURE=Z:\\tmp\\opencode\\sweep-$LABEL\\capture
set VICTORY_CAPTURE_WORLD=0,3
"$ZEXE" run "$GAME" --execute --console series-x > Z:\\tmp\\opencode\\sweep-$LABEL\\app.log 2>&1
EOF

export PROTONPATH="$PROTON"
export STEAM_COMPAT_INSTALL_PATH=${STEAM_COMPAT_INSTALL_PATH:-$HOME/.local/share/Steam/steamapps/common/victory-run}
export STEAM_COMPAT_LIBRARY_PATHS=${STEAM_COMPAT_LIBRARY_PATHS:-$HOME/.local/share/Steam/steamapps}

# The default compat path must be a symlink to the binary's directory.
mkdir -p "$(dirname "$STEAM_COMPAT_INSTALL_PATH")"
if [ ! -L "$STEAM_COMPAT_INSTALL_PATH" ]; then
    rm -rf "$STEAM_COMPAT_INSTALL_PATH"
    ln -s "$(dirname "$EXE")" "$STEAM_COMPAT_INSTALL_PATH"
fi

echo "sweep=$LABEL exe=$EXE duration=${DURATION}s"
echo "proton=$PROTONPATH"
timeout "$DURATION" umu-run 'C:\windows\system32\cmd.exe' /c "Z:\\tmp\\opencode\\sweep-$LABEL\\game.bat" > "$WORK/runner.log" 2>&1
echo "umu_exit=$?"

python3 "$ROOT/tools/linux-sweep/analyze.py" --log "$LOG" --captures "$CAP"
