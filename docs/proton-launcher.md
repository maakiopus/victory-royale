# Build and run VIctory on Linux

The native Linux CLI can inspect files and translate shaders. Running the
title uses the Windows executable under Proton. The launch path requires
x86-64 Linux, an AVX2 CPU, UMU, and a Vulkan driver that exposes Shader
Model 6.6 through vkd3d-proton. World rendering also needs mesh shaders.

## Build on Linux

Install Rust with rustup, a native C toolchain, and the x86-64 Linux UCRT
distribution of LLVM-MinGW. Put its `bin` directory first on `PATH`.
To embed the three GPU helper shaders, install the Linux DirectX Shader
Compiler (`dxc`). The build command uses `dxc` on `PATH` or the compiler
selected by `VICTORY_BUILD_DXC`. Run the following from the checkout.
[LLVM-MinGW releases](https://github.com/mstorsjo/llvm-mingw/releases) ·
[DirectX Shader Compiler releases](https://github.com/microsoft/DirectXShaderCompiler/releases)

```sh
rustup target add x86_64-pc-windows-gnullvm
./scripts/victory-proton.sh --build
```

The launcher finds the release executable in the Cargo target directory.
If you build without Linux DXC, leave `VICTORY_BUILD_DXC` unset. Supply
Windows `dxcompiler.dll` and `dxil.dll` beside `victory.exe` if Wine's HLSL
compiler rejects the helper shaders on startup.

An existing x64 MSVC build also works with the launcher. With `cargo-xwin`,
LLVM, and the MSVC Rust target installed, build one using
`cargo xwin build --locked --release -p victory --target x86_64-pc-windows-msvc`.

## Launch

Install UMU so that `umu-run` is on `PATH`. Keep your game files outside
the checkout. Pass the Linux path to the title executable to the launcher.
[UMU installation](https://github.com/Open-Wine-Components/umu-launcher#installation)

```sh
./scripts/victory-proton.sh --run "/path/to/game/title.exe"
```

Pass the title path alone to check mapping and import binding first.
Invocations without a mode, including the original `TITLE.exe --execute`,
use Proton's libraries by default. The launcher
forwards other `victory run` options and arguments after `--`. To select a
specific build, set `VICTORY_EXE` to its Linux path. By default the launcher
looks beside itself for `victory.exe`, then under `${CARGO_TARGET_DIR:-target}`
for the gnullvm and MSVC release builds, in that order.

On first launch UMU fetches Proton and the Steam Linux Runtime. The default
prefix is `${XDG_DATA_HOME:-$HOME/.local/share}/victory/proton`, and the
default `GAMEID` is `umu-victory`. Set `WINEPREFIX`, `GAMEID` or `PROTONPATH`
to override them. Use Windows paths in `victory.toml`.

When Steam's `Proton - Experimental` is installed, the launcher selects it
automatically. That runtime does not accept `Z:` paths, so the launcher gives
UMU Unix paths and sets `STEAM_COMPAT_INSTALL_PATH` to a symlink under the
Steam library, with `STEAM_COMPAT_LIBRARY_PATHS` pointing at that library.
This is the configuration used to reach the first world frame on Linux.
Set `PROTONPATH` to another build to keep the `Z:` path behaviour.

The launcher creates shader-cache directories at
`${XDG_CACHE_HOME:-$HOME/.cache}/victory/vkd3d` and
`${XDG_CACHE_HOME:-$HOME/.cache}/victory/dxvk`, keeping generated caches out
of the checkout. `VKD3D_SHADER_CACHE_PATH` and `DXVK_STATE_CACHE_PATH`
override these defaults. `VKD3D_SHADER_CACHE_PATH=0` disables the VKD3D cache
without creating a directory. Empty or relative XDG base directories fall back
to the standard directories under `$HOME`.

## Capture a workstation run

For a requested PR comparison, follow the [interactive Series X test](testing.md#run-a-series-x-hardware-test). It runs both builds and bundles their logs. To capture a single launch, use the command below.

```sh
./scripts/victory-proton.sh --report "/path/to/game/title.exe"
```

The launcher records UMU output, enables Proton exception logging and Victory
pipeline tracing, and writes the launcher exit status and executable SHA-256.
The package revision or launcher checkout revision is included when available.
Each invocation prints its own archive path under
`${XDG_DATA_HOME:-$HOME/.local/share}/victory/reports`. A failed launch also
produces an archive once UMU has started. Logs can include personal paths and
title output. Review them privately and use the contributor guide when
summarizing a failure for a public issue.

Both `--run` and `--report` default to the project’s pinned vkd3d-proton build.
To compare with the libraries supplied by Proton, add `--runtime proton`.
Use this command from a checkout, or use `./victory-proton.sh` inside a CI package.

```sh
./scripts/victory-proton.sh --report --runtime proton "/path/to/game/title.exe"
```

On first use of the pinned runtime, the launcher downloads the x64 vkd3d-proton archive
using `curl` over HTTPS. It checks the SHA-256 pinned in this launcher and keeps
the verified archive under `${XDG_CACHE_HOME:-$HOME/.cache}/victory`. Subsequent
runs can use the cached archive offline. Each run stages the two DLLs beside a
temporary copy of the selected Victory executable. The report includes the
runtime selection and DLL hashes. This pin is a 3.2 development build from
upstream commit `ec20719`. The release includes provenance, notices, and source.
[Runtime release](https://github.com/ligmaSec/victory-royale/releases/tag/runtime-vkd3d-ec20719)

To test a locally built runtime, use `--vkd3d-dir DIR` and
supply the directory containing its x64 `d3d12.dll` and `d3d12core.dll`.

Set `VICTORY_TRACE_PIPELINES=1` to log the shader stages and hashes with the
Windows thread ID when each host pipeline starts compiling. Match that ID to
the faulting thread in a Proton log when several pipelines compile at once.

In a diagnostics build, the trace also logs the 32-digit host pipeline name and worker thread before
the driver call. To bypass a particular configuration that crashes the driver,
set `VICTORY_SKIP_HOST_PSOS` to its name or a comma-separated list of names.
Draws and dispatches using those configurations are dropped. Other pipelines
can still use the same shaders. Unset the variable to restore the passes.

## Testing without game files

`./scripts/victory-proton.sh --test` (or `./victory-proton.sh --test` in the package) checks argument quoting, cache paths, XDG
fallbacks and exit status with a fake UMU. It requires neither Wine nor a GPU,
and runs in Linux CI.

`./tests/proton-smoke.sh` compiles its synthetic title with the LLVM-MinGW
toolchain and gnullvm Rust target above, even when `VICTORY_EXE` points to
an MSVC build. The test checks the launcher's exit status and the return
value from the title. It uses a separate `victory/proton-smoke` prefix under
the XDG data directory. Set `VICTORY_SMOKE_PREFIX` to choose another test prefix.

To reproduce the loader integration tests after the smoke test, build the
DLL fixture on Linux and set `VICTORY_HLE_PROBE` to the Windows path of the
built DLL. This lets the test load it without starting Cargo inside Proton.
Run these commands from the checkout in Bash.

```sh
target=x86_64-pc-windows-gnullvm
fixture_dir=$(realpath -m "${CARGO_TARGET_DIR:-target}/proton-fixture")
cargo build --locked --manifest-path tests/fixtures/hle-probe/Cargo.toml \
    --target "$target" --target-dir "$fixture_dir"
export VICTORY_HLE_PROBE="Z:$fixture_dir/$target/debug/hle_probe.dll"
export WINEPREFIX=${VICTORY_SMOKE_PREFIX:-${XDG_DATA_HOME:-$HOME/.local/share}/victory/proton-smoke}
PROTON_VERB=runinprefix GAMEID=umu-victory CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_RUNNER=umu-run \
    cargo test --locked --target "$target" -p victory-loader --test load_probe -- --test-threads=1
```
