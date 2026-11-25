#!/usr/bin/env bash
# Start the Windows build with UMU and its Steam Linux Runtime.
set -euo pipefail

usage() {
    cat <<'HELP'
Usage: victory-proton.sh --build | --test
       victory-proton.sh [--run | --report] [--runtime proton|pinned | --vkd3d-dir DIR] TITLE.exe [options]

  --test           Run the offline launcher checks (no game, build, or GPU needed).
  --build          Build the Windows release from a source checkout with LLVM-MinGW.
  --run            Execute the title.
  --report         Execute once and save the launcher and Proton logs in one archive.
  --runtime pinned Use the pinned vkd3d-proton build (default for --run and --report).
  --runtime proton Use the vkd3d-proton libraries supplied by Proton.
  --vkd3d-dir DIR  Use the x64 d3d12.dll and d3d12core.dll from DIR for this run.

With no mode, the launcher forwards the title and options to victory run.
Use TITLE.exe alone to check imports, or TITLE.exe --execute to launch.
HELP
}

pinned_asset=vkd3d-proton-ec20719-x64.tar.gz

verify_runtime_archive() {
    local digest=64b13a6d73bb48394e0135a2048bcf423a69c08226ab410a8a8bde075f5ed4b5
    (cd -- "$1" && printf '%s  %s\n' "$digest" "$pinned_asset" | sha256sum --check --status)
}

stage_pinned_runtime() {
    local stage_dir=$1 cache_dir=$2
    local cache="$cache_dir/$pinned_asset"
    local url="https://github.com/ligmaSec/victory-royale/releases/download/runtime-vkd3d-ec20719/$pinned_asset"
    if [[ ! -f $cache ]] || ! verify_runtime_archive "$cache_dir"; then
        echo 'Downloading pinned vkd3d-proton (ec20719)...' >&2
        curl --disable --fail --location --silent --show-error --proto '=https' --proto-redir '=https' \
            --connect-timeout 15 --max-time 120 --retry 2 --output "$stage_dir/$pinned_asset" "$url"
        if ! verify_runtime_archive "$stage_dir"; then
            echo 'The downloaded runtime failed SHA-256 verification.' >&2
            exit 1
        fi
        # Concurrent launches can publish the same verified bytes atomically.
        mv -fT -- "$stage_dir/$pinned_asset" "$cache"
    fi
    tar -xzf "$cache" -C "$stage_dir" -- d3d12.dll d3d12core.dll
    echo 'Using pinned vkd3d-proton ec20719.' >&2
}

script_dir=$(cd -- "$(dirname -- "$0")" && pwd)
if [[ ${1:-} == --test ]]; then
    if (( $# != 1 )); then usage >&2; exit 2; fi
    suite="$script_dir/tests/proton-launcher.sh"
    if [[ -f $script_dir/../.victory-root ]]; then suite="$script_dir/../tests/proton-launcher.sh"; fi
    exec bash "$suite" "$script_dir/victory-proton.sh"
fi
if [[ ${1:-} == --build ]]; then
    if (( $# != 1 )); then usage >&2; exit 2; fi
    if [[ ! -f $script_dir/../.victory-root ]]; then
        echo 'Build from a source checkout. This distribution is ready to run.' >&2
        exit 2
    fi
    export CC_x86_64_pc_windows_gnullvm=${CC_x86_64_pc_windows_gnullvm:-x86_64-w64-mingw32-clang}
    export AR_x86_64_pc_windows_gnullvm=${AR_x86_64_pc_windows_gnullvm:-llvm-ar}
    if [[ -z ${VICTORY_BUILD_DXC:-} ]] && dxc_path=$(type -P dxc); then
        export VICTORY_BUILD_DXC="$dxc_path"
    fi
    if [[ -n ${CARGO_TARGET_DIR:-} ]]; then
        CARGO_TARGET_DIR=$(realpath -m -- "$CARGO_TARGET_DIR")
        export CARGO_TARGET_DIR
    fi
    cd -- "$script_dir/.."
    exec cargo build --locked --release -p victory --target x86_64-pc-windows-gnullvm
fi

mode=
runtime=
vkd3d_dir=
while (( $# )); do
    case $1 in
        --help|-h) usage; exit 0 ;;
        --run|--report)
            if [[ -n $mode ]]; then usage >&2; exit 2; fi
            mode=$1
            shift
            ;;
        --runtime|--vkd3d-dir)
            if [[ -n $runtime || -z ${2:-} ]]; then usage >&2; exit 2; fi
            if [[ $1 == --vkd3d-dir ]]; then
                runtime=custom
                vkd3d_dir=$2
            else
                case $2 in
                    pinned|proton) runtime=$2 ;;
                    *) echo 'Choose --runtime proton or --runtime pinned.' >&2; exit 2 ;;
                esac
            fi
            shift 2
            ;;
        --) shift; break ;;
        *) break ;;
    esac
done
if (( $# == 0 )); then
    usage >&2
    exit 2
fi

target_dir=${CARGO_TARGET_DIR:-$script_dir/../target}
binary=${VICTORY_EXE:-}
if [[ -z $binary ]]; then
    for candidate in "$script_dir/victory.exe" \
        "$target_dir/x86_64-pc-windows-gnullvm/release/victory.exe" \
        "$target_dir/x86_64-pc-windows-msvc/release/victory.exe"; do
        if [[ -f $candidate ]]; then
            binary=$candidate
            break
        fi
    done
fi
if [[ ! -f $binary ]]; then
    echo "Missing Windows build. Run $0 --build from a checkout or set VICTORY_EXE to an existing build." >&2
    exit 1
fi
if [[ ! -f $1 ]]; then
    echo "Missing title executable: $1" >&2
    exit 1
fi
if ! command -v umu-run >/dev/null; then
    echo 'Install umu-launcher to provide umu-run.' >&2
    exit 1
fi
binary=$(realpath -- "$binary")
title=$(realpath -- "$1")
shift
if [[ -n $mode ]]; then
    runtime=${runtime:-pinned}
    # Accept an existing --execute, including invocations migrated from the old syntax.
    execute=(--execute)
    for arg in "$@"; do
        [[ $arg != -- ]] || break
        if [[ $arg == --execute ]]; then execute=(); break; fi
    done
    set -- "${execute[@]}" "$@"
fi

# Hand Proton Windows paths on its standard Z: drive. A Windows executable
# path also lets UMU-Proton attach this CLI to the calling terminal.
export GAMEID=${GAMEID:-umu-victory}
data_home=${XDG_DATA_HOME:-$HOME/.local/share}
cache_home=${XDG_CACHE_HOME:-$HOME/.cache}
[[ $data_home == /* ]] || data_home=$HOME/.local/share
[[ $cache_home == /* ]] || cache_home=$HOME/.cache
export WINEPREFIX=${WINEPREFIX:-$data_home/victory/proton}
# Both translation layers otherwise write derived data to the working directory.
export VKD3D_SHADER_CACHE_PATH=${VKD3D_SHADER_CACHE_PATH:-$cache_home/victory/vkd3d}
export DXVK_STATE_CACHE_PATH=${DXVK_STATE_CACHE_PATH:-$cache_home/victory/dxvk}
# VKD3D reserves 0 for disabling its cache.
if [[ $VKD3D_SHADER_CACHE_PATH != 0 ]]; then
    mkdir -p -- "$VKD3D_SHADER_CACHE_PATH"
fi
mkdir -p -- "$DXVK_STATE_CACHE_PATH"
# Stage an alternate runtime for this process without changing the build or Wine prefix.
stage=
if [[ ${runtime:-proton} != proton ]]; then
    if [[ $runtime == custom ]]; then
        for dll in d3d12.dll d3d12core.dll; do
            if [[ ! -f $vkd3d_dir/$dll ]]; then
                echo "Missing $vkd3d_dir/$dll. Pass the directory containing both x64 DLLs." >&2
                exit 1
            fi
        done
    fi
    mkdir -p -- "$cache_home/victory"
    stage=$(mktemp -d -- "$cache_home/victory/runtime.XXXXXXXX")
    trap 'rm -rf -- "$stage"' EXIT
    cp --reflink=auto -- "$binary" "$stage/victory.exe"
    # Preserve companion libraries such as the optional DXC runtime.
    shopt -s nocaseglob
    for dll in "${binary%/*}"/*.dll; do
        [[ -f $dll ]] || continue
        case ${dll,,} in */d3d12.dll|*/d3d12core.dll) continue ;; esac
        ln -s -- "$dll" "$stage/"
    done
    if [[ $runtime == pinned ]]; then
        stage_pinned_runtime "$stage" "$cache_home/victory"
    else
        cp -- "$vkd3d_dir/d3d12.dll" "$vkd3d_dir/d3d12core.dll" "$stage/"
    fi
    export WINEDLLOVERRIDES="${WINEDLLOVERRIDES:+$WINEDLLOVERRIDES;}d3d12,d3d12core=n"
fi
launch_binary=$binary
if [[ -n $stage ]]; then launch_binary=$stage/victory.exe; fi
windows_title=${title//\//\\}
windows_binary=${launch_binary//\//\\}
caller_install=${STEAM_COMPAT_INSTALL_PATH:-}
export STEAM_COMPAT_INSTALL_PATH=${caller_install:-$(dirname -- "$binary")}

# Proton Experimental does not accept the Z: paths that UMU-Proton uses.
# Give it Unix paths and a compat install path under its Steam library.
proton_path=${PROTONPATH:-}
library=
if [[ -z $proton_path ]]; then
    for root in "$data_home/Steam" "$HOME/.steam/steam" "$HOME/.local/share/Steam"; do
        if [[ -d $root/steamapps/common/Proton\ -\ Experimental ]]; then
            proton_path=$root/steamapps/common/Proton\ -\ Experimental
            library=$root/steamapps
            export PROTONPATH=$proton_path
            break
        fi
    done
fi
if [[ -z $library && -n $proton_path && -f $proton_path/version ]] &&
    grep -q 'experimental-' "$proton_path/version"; then
    library=${proton_path%/common/Proton - Experimental}
fi
if [[ -n $library ]]; then
    install=$library/common/victory-run
    target=$(dirname -- "$binary")
    mkdir -p -- "$library/common"
    if [[ -d $install && ! -L $install ]]; then
        echo "Steam compat install path is a directory: $install" >&2
        exit 1
    fi
    if [[ -L $install && $(readlink -- "$install") != "$target" ]] ||
        [[ ! -e $install && ! -L $install ]]; then
        ln -sfn -- "$target" "$install"
    fi
    export STEAM_COMPAT_INSTALL_PATH=${caller_install:-$install}
    export STEAM_COMPAT_LIBRARY_PATHS=${STEAM_COMPAT_LIBRARY_PATHS:-$library}
    command=(umu-run "$launch_binary" run "$title" "$@")
else
    command=(umu-run "Z:$windows_binary" run "Z:$windows_title" "$@")
fi
if [[ $mode != --report ]]; then
    if [[ -z $stage ]]; then exec "${command[@]}"; fi
    "${command[@]}"
    exit $?
fi

umask 077
reports=$(realpath -m -- "$data_home/victory/reports")
ancestor=$reports
while [[ $ancestor != / ]]; do
    if [[ -f $ancestor/.victory-root ]]; then
        echo 'Set XDG_DATA_HOME outside the source tree to record a run.' >&2
        exit 1
    fi
    ancestor=$(dirname -- "$ancestor")
done
mkdir -p -- "$reports"
report=$(mktemp -d -- "$reports/run-$(date -u +%Y%m%dT%H%M%SZ)-XXXXXXXX")
export PROTON_LOG=1 PROTON_LOG_DIR="$report" VICTORY_TRACE_PIPELINES=1
export WINEDEBUG=${WINEDEBUG:--all,+timestamp,+pid,+tid,+seh}
(
    cd -- "${binary%/*}"
    sha256sum -- "${binary##*/}"
    printf 'runtime=%s\n' "${runtime:-proton}"
    if [[ $runtime == pinned ]]; then printf 'runtime_asset=%s\n' "$pinned_asset"; fi
    if [[ -f REVISION ]]; then
        printf 'package_revision=%s\n' "$(<REVISION)"
    elif [[ -d $script_dir/../.git || -f $script_dir/../.git ]]; then
        revision=$(git -C "$script_dir/.." rev-parse HEAD 2>/dev/null) &&
            printf 'launcher_checkout=%s\n' "$revision"
    fi
    if [[ -n $stage ]]; then
        cd -- "$stage"
        sha256sum -- d3d12.dll d3d12core.dll
    fi
) > "$report/build.txt"
echo "Recording this run in $report"
status=0
trap ':' INT
"${command[@]}" > "$report/launcher.log" 2>&1 || status=$?
trap - INT
printf '%s\n' "$status" > "$report/launcher-exit.txt"
# Keep the raw logs if archiving fails, and publish only complete archives.
if tar -czf "$report.tar.gz.part" -C "$report" .; then
    mv -- "$report.tar.gz.part" "$report.tar.gz"
    rm -rf -- "$report"
    echo "Report: $report.tar.gz (launcher exit $status)"
else
    rm -f -- "$report.tar.gz.part"
    echo "Could not pack the report. Logs are in $report (launcher exit $status)." >&2
    if (( status == 0 )); then status=1; fi
fi
exit "$status"
