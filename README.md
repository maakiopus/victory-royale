# Victory Royale

Victory Royale continues development of VIctory, which runs an Xbox Series X|S
title on Windows or under Proton on Linux using files you supply.

This independent repository started from
[Project Flamingo’s November 25 snapshot](https://github.com/JMAN730/project-flamingo/commit/8cfe46d5b17a61e962b495192e866022200c5f50).

The title's x86-64 code runs unchanged inside our process; VIctory supplies
the platform underneath it and converts the title's data, ahead of time where
it can and on first use where it must (shaders). See
[docs/architecture.md](docs/architecture.md).

The Windows executable can also be launched under Proton on x86-64 Linux. See
[Running with the Proton launcher](docs/proton-launcher.md). Builds and checksums are
available from [successful main CI runs](docs/testing.md#ci-and-build-downloads).

## Status

Boot, first-time setup, menus, and intro video work. The title can enter the
world, but the scene can render mostly black, run slowly, or hang the GPU.
World gameplay remains unplayable. On Windows a GPU hang can trigger a display
driver reset. Use the diagnostics build with `VICTORY_D3D12_WARP=1` to
investigate on the software adapter.

## Run on Linux

Install UMU (`umu-run`) and extract the verified Proton build from a successful
`main` CI run. From the extracted `victory` directory, choose one command.

```sh
./victory-proton.sh --test                                # check the launcher offline
./victory-proton.sh --run "/path/to/title.exe"             # launch with pinned vkd3d-proton
./victory-proton.sh --report "/path/to/title.exe"          # same runtime, with diagnostic logs
```

`--report` launches once and prints the path to a compressed diagnostic archive
when the process exits. Both modes download pinned vkd3d-proton on first use
and verify the cache on each launch. Add `--runtime proton` to use Proton's own libraries.
Contributors running from source use
`./scripts/victory-proton.sh --build` after installing the cross-toolchain,
then the same commands with `./scripts/victory-proton.sh`.
[Build downloads and checksums](docs/testing.md#ci-and-build-downloads) ·
[Linux requirements and runtime options](docs/proton-launcher.md)

## Getting started

Requirements: Windows 10/11 x64, a CPU with AVX2, a Direct3D 12 GPU with
shader model 6.6 (and mesh shaders for the world), Rust (pinned in
`rust-toolchain.toml`). On x86-64 Linux, follow the
[Proton guide](docs/proton-launcher.md) to build and launch the Windows executable.

Clone the repository and run the remaining commands from the checkout.

```sh
git clone https://github.com/ligmaSec/victory-royale.git
cd victory-royale
```

To install Rust, use rustup (`winget install --id Rustlang.Rustup -e`, or
the installer from https://rustup.rs). The first `cargo` command in this
repository then fetches the pinned toolchain. Building needs the MSVC C++
build tools: Visual Studio or its Build Tools with the "Desktop development
with C++" workload.

The title also needs several GiB of available Windows commit at startup.
Check **Committed** under **Task Manager → Performance → Memory**. If
`crashes.log` reports Windows error 1455, close other applications or increase
the Windows paging file size, then retry.

```powershell
cargo xtask install-hooks                # asset-policy check on every commit
copy victory.example.toml victory.toml   # then set your paths (outside the repo)
cargo test --workspace -- --test-threads=1 # serializes D3D12 tests; no game data
cargo run -p victory -- config           # check your paths
```

Run the title, replacing the example path with the full path to your game's executable:

```powershell
cargo build --release -p victory
.\target\release\victory.exe run 'D:\Xbox Games\Title\title.exe' --execute
```

### Selecting the Series S profile

The title chooses its render configuration from the console model, not from the
display modes VIctory advertises through DXGI. To use the lower-resolution
profile, pass `--console series-s` when starting the title:

```powershell
target\release\victory.exe run <game dir>\<the title's exe> --execute --console series-s
```

Series S lowers the scene resolution and can reduce memory usage. The UI
uses a separate presentation plane and can still render at a higher resolution.

The window caption shows the framerate. Saves, settings and `crashes.log`
are kept under the data folder (default `%LOCALAPPDATA%\victory\data`), derived
data under the cache folder.

Input: XInput pads. A DualSense or DualSense Edge (USB or Bluetooth) is read
over HID as well and plays as the first pad when no XInput pad answers. With
no pad connected the keyboard is a pad while the title's window has focus:

| Keys | Pad |
|------|-----|
| W A S D / I J K L | left / right stick |
| arrows | D-pad |
| Enter, O | A, B |
| Backspace, X, Y | B, X, Y |
| Q / E, Z / C | shoulders, triggers |
| F / H | stick clicks |
| Esc, Tab | Menu, View |

## Commands

```
victory inspect <exe|dll> [--imports] [--exports]
victory census [paths...] [--missing] [--strict]
victory lib-exports <import.lib> [--list]
victory shader-info [paths...] [--verbose] [--disasm]
victory translate [paths...] [--validate] [--pipelines] [--list] [--jobs N]
victory run <exe> [--execute] [--unimplemented trap|zero] [--strict]
            [--console series-x|series-s] [-- <title arguments>]
victory config
```

`translate --jobs N` requires a positive worker count. Omit it to use the
default worker count.

Logging: `VICTORY_LOG=victory_loader=trace,info` (a release build logs
info and above).

For structured failure reports, see [Dependency diagnostics](docs/dependency-diagnostics.md).

## Debugging

If the title presents one or two black frames and exits with `0x80000003`,
check the archive files before investigating rendering. The loader's import
report only counts bindings, so it can report zero missing imports with a
damaged archive. Check the SHA-256 of the files your run actually uses against
a known-good copy of the **same build**. For example, in PowerShell:

```powershell
Get-FileHash -Algorithm SHA256 -LiteralPath 'C:\path\to\title\anim.rpf'
```

If a hash differs, restore that file from your verified backup and rerun with
the same build of VIctory and the same options. A breakpoint can have other
causes. Include the archive verification outcome in your test report.

A release build has no debugging aids. Build them in with

```powershell
cargo build --profile diag -p victory --features diagnostics   # target\diag\victory.exe
```

The on/off switches below (`...=1`) are off when unset or set to `0`.

| Variable | Effect |
|----------|--------|
| `VICTORY_LOG=<target>=debug,...` | debug and trace output |
| `VICTORY_STACKS=20,60` | every thread's stack at those seconds |
| `VICTORY_WATCH=<rva>,...` | logs a title u32 when it changes |
| `VICTORY_KEYS=30:Enter,35:X` | presses keys at those seconds, focus or not |
| `VICTORY_CRASH_BREAKPOINTS=1` | appends first-chance breakpoints to `crashes.log`, including ones a handler may recover from |
| `VICTORY_CRASH_SAVE=<rva>:<len>,...` | saves title memory to the cache folder on a crash |
| `VICTORY_CAPTURE=<dir>` | saves presented frames (powers of two, or `VICTORY_CAPTURE_FRAMES=<list>`) |
| `VICTORY_CAPTURE_WORLD=1` | with `VICTORY_CAPTURE`, also saves the first frame that ran 500 draws (the world's first), its 2nd and 6th followers, and logs that frame |
| `VICTORY_DUMP_TARGETS=1` | with `VICTORY_CAPTURE` and `VICTORY_CAPTURE_FRAMES`, saves every render target of those frames |
| `VICTORY_SHOW_TARGET=2560x1440-f28` | shows the last render target of that size and DXGI format each frame instead of the title's frame (copied 1:1 to the top left, only when its format matches the window's), e.g. the world's geometry while its lit image is black |
| `VICTORY_D3D12_DEBUG=1` | host debug layer |
| `VICTORY_D3D12_GBV=1` | with the above, GPU-based validation (bad descriptor use reported, not faulted; slow) |
| `VICTORY_D3D12_GBV_EAGER=1` | with the two above, compile the active GPU validation shader variant at pipeline creation rather than first bind |
| `VICTORY_D3D12_GBV_EAGER=guarded` | as above, explicitly selecting guarded GPU validation (fails host initialization if guarded mode cannot be verified; diagnostics only) |
| `VICTORY_D3D12_DRED=1` | on a GPU failure, the allocation hit and the commands in flight |
| `VICTORY_LOG=victory_gpu::draw=debug` | with frames listed for capture: one line per draw and dispatch of those frames with its targets (named as in the `VICTORY_DUMP_TARGETS` files) and, if it was dropped, why |
| `VICTORY_SKIP_PIPELINES=<hash>,...` | omit draws and dispatches, including indirect ones, whose pipeline contains a listed 16-digit shader hash; diagnostics builds also refuse the pipeline before creating it |
| `VICTORY_SKIP_HOST_PSOS=<name>,...` | omit a host pipeline whose 32-digit library name matches exactly; get names and worker thread IDs with `VICTORY_TRACE_PIPELINES=1` |
| `VICTORY_SKIP_WAVE64=mesh,compute` | omit draws and dispatches whose pipeline has a program of a listed stage written for 64-lane waves (on hosts with 32-lane waves, such as NVIDIA's, these can hang the GPU) |
| `VICTORY_SLOT67_HR=<name>` | what the device's `DeserializeComputePipelineStateX` (slot 67, used by the AMD upscaler with GFX10 compute programs we do not translate) returns: `E_FAIL` (the default, also without this build), `E_INVALIDARG`, `E_NOTIMPL` or `S_OK` (a pipeline whose dispatches are dropped); the title's reaction to each shows whether it has a fallback |
| `VICTORY_FSPX_REJECT_SLOT62=1` | fail `CreateComputePipelineStateX` (slot 62) with `E_FAIL` for FSPX compute modules instead of making a pipeline whose dispatches are dropped |
| `VICTORY_GPU_TIMING=1` | with the above, each list of a submission run and timed on its own; slow lists logged with their slowest commands |
| `VICTORY_WAIT_TIMEOUT_MS=<ms>` | override the 2000 ms fail-open timeout for mid-list GPU memory waits |
| `VICTORY_D3D12_WARP=1` | host device on the software adapter (slow; cannot reset the driver) |
| `VICTORY_ADAPTER=<name>` | select a D3D12 adapter by case-insensitive substring (default: the first GPU, in high-performance order, with shader model 6.6, resource binding tier 3 and mesh shaders; the log says what is missing when none has all three) |
| `VICTORY_SHADER_LOOP_LIMIT=<n>` | translated shader loops stop after `n` iterations per lane instead of 2^20 (own shader cache entries); if a lower `n` makes world frames much faster, loops hitting the limit are what makes them slow |
| `VICTORY_SHADER_OPT=<bits>` | which clean-up passes run over each translated shader (own shader cache entries): `0` none, 1 dead code, 2 constants and identities, 4 reuse of values, 8 branch facts, 16 loads within a block; unset runs all. Every pass is exact, so a fault that goes away with `0` is in a pass |
| `VICTORY_ALIAS_COPY=0` | views the title places in the memory of a texture the host writes are filled from title memory again instead of copied from that texture on the host |
| `VICTORY_AUDIO_CAPTURE=<file.wav>` | records what the title plays (our process only) |
| `VICTORY_XMA_DUMP=<dir>` | saves the first 32 XMA voices' context and input |
| `VICTORY_NO_WATCHDOG=1` | apply the opt-in title watchdog disarm hooks (Series 1.0.2.1) |
| `VICTORY_SKIP_CPU_CHECK=1` | skip the AVX2 pre-flight check before `--execute` (for CPU emulators such as Intel SDE) |
| `VICTORY_PAD_LOG=1` | log DualSense / HID pad discovery and reports |
| `VICTORY_COPY_MODE=plain` | copy title GPU memory without the timed/stats path used for the frame-rate line |
| `VICTORY_WARP=1` | force the shader-test host device onto WARP (distinct from `VICTORY_D3D12_WARP`) |
| `VICTORY_SAVE_STOP=<n>` | test helper: hang the process at save step `n` (used by crash-survival tests) |

Every build appends unhandled exceptions, panics, and host GPU failures to
`crashes.log` in the title's data folder.

For a startup breakpoint (`0x80000003`), run the diagnostics build with
`VICTORY_CRASH_BREAKPOINTS=1`. Keep the console output along with the new
`crashes.log` entries. A host DLL refusal includes the caller stack in the
console output. The refusal is expected for DLLs outside the host policy,
including `rpcrt4.dll`.

On Series 1.0.2.1, a missing `appdata0.rpf` reproduced the breakpoint at
`+0x2237178` before the first window. Restore the archive from the same
package beside the title executable. `run --execute` checks that it can
read the RPF9 header before starting this build.

To compare hardware and software rendering, repeat with
`VICTORY_D3D12_WARP=1` and check the console for
`host device on the software adapter (WARP)` to confirm the WARP device was
created. Compare the exception's module and offset, stack frames, and
preceding messages across the two runs.

### Reporting a GPU failure

A host device removal, a hang, or a frame that comes out wrong is much easier
to place with these next to it:

* the adapter and the driver, as the title's own window sees them (GPU model,
  the Adrenalin or GeForce version, and the Windows build number - a driver
  update between two reports changes what is worth comparing);
* whether `VICTORY_D3D12_WARP=1` reproduces it: a failure that survives the
  software adapter is ours, one that does not points at the driver;
* the last lines of `crashes.log`, which name the faulting code and the
  commands in flight when `VICTORY_D3D12_DRED=1` was set;
* the variables from the table above that were set for the run, and the build
  profile (`diag` or release);
* how long it took, and whether the display reset (a TDR) or the process
  stopped.

Nothing from the title's data belongs in the report: no shader dumps, no
frames, no memory, per [docs/asset-policy.md](docs/asset-policy.md). Names of
our own functions, error codes and timings are enough to start.

## Contributing

A first contribution can be a test result from your machine. The
[testing and reporting guide](CONTRIBUTING.md#help-by-testing) covers how to
pick a test, confirm an existing issue, or report a new failure.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
