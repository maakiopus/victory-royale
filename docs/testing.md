# Testing

The default test run needs no game data and no console SDK:

```powershell
cargo test --workspace --locked
```

Linux CI runs the portable workspace tests. Windows CI also exercises the
Windows loader and graphics paths, and runs the tests serially to avoid
contention for the graphics driver. Both jobs run Clippy. See the
[workflow](../.github/workflows/ci.yml) for the exact commands.

The workspace suite includes the [asset-policy check](asset-policy.md).
Run `cargo xtask install-hooks` to enable it on commit.

## CI and build downloads

Every pull request and push to `main` runs the documentation and asset checks.
Changes limited to project guides and test READMEs finish there. Code, workflow,
dependency, and other changes also run the Windows and Linux workspace suites,
workflow lint, formatting, the minimum-Rust check, and the distribution test.
Manual runs and merge-group checks use the full suite.

A push merging a code PR can reuse its successful minimum-Rust job when the
merged Git tree matches the one the job tested. Direct code pushes and merges
without matching test evidence rerun it. The regression gate verifies the
expected jobs for each path. A docs-only merge does not publish a binary.

The distribution job builds the x86-64 Windows release on Linux and checks that
all three compiled DXIL helpers are present in the executable. It unpacks the
archive, exercises the offline launcher checks and pinned runtime selection with
a fake UMU, and runs the packaged executable
under Wine against synthetic PE fixtures. The suite checks import binding, TLS,
threads, memory, DLL loading, and expected title return codes. Failed runs retain
the synthetic case logs and summary for three days. Game rendering and
the full UMU/Proton runtime still require a separate workstation test.

Successful main builds deliver `victory-proton-x86_64-<commit>` for 14 days.
Code PRs deliver `victory-proton-x86_64-pr-<number>-<merge-commit>` for three
days. The latter contains the revision GitHub tested with the base branch.
For a requested comparison, use the [interactive Series X test](#run-a-series-x-hardware-test)
to get both matched builds. For a single run, Windows testers can launch the extracted
`victory.exe` with the [usual Windows launch command](../README.md#getting-started).

Download the artifact from the CI run, unzip the Actions download, and run
the following in the extracted directory.

```sh
sha256sum --check SHA256SUMS
tar -xf victory-proton-x86_64.tar.gz
./victory/victory-proton.sh --test
./victory/victory-proton.sh --run "/path/to/game/title.exe"
```

## Run a Series X hardware test

You'll need Python 3.9 or newer and your own title executable outside the VIctory checkout.
On Linux, install Bash and UMU (`umu-run`). The Proton launcher may download its pinned
runtime on first use.

| Start here | Linux/Proton | Windows (PowerShell) |
| --- | --- | --- |
| From the repository root, choose a published PR | `python3 scripts/hardware-test/run-test.py` | `py scripts/hardware-test/run-test.py` |
| With a kit ZIP, extract it and open a terminal in the folder containing `run-test.py` | `python3 run-test.py` | `py run-test.py` |

The checkout menu uses a signed-in GitHub CLI (`gh auth login`). Type the PR number
shown in the menu, or press Enter to cancel. If a request includes a number, you
can append it directly to the checkout command. Add `--plan` to preview the scenario
and build revisions. An extracted kit includes both builds and needs no GitHub login.

1. Select your game's `.exe` when prompted. The runner checks both packages and starts
   **main** first. Follow **WHAT TO DO** above the launch and close the game at the point
   specified in the scenario.
2. The **candidate** opens next. Repeat the same steps with the same settings and window
   size. The runner gives each build its own fresh translation cache.
3. At **REPORT READY**, send the intact `report.zip` at the **Upload:** path to the
   private destination in the request, or directly to the requester. The runner leaves
   it in a new system temporary folder after exit. Send it before the system cleans
   that folder. If the runner prints **Partial report:**, send that file and say
   which run stopped.

Tell the requester what you saw on each build. For example, say whether the first
scene showed roads and vehicles or only lights and subtitles. Include your OS,
GPU and driver. Keep full logs and archives private. See the [asset policy](asset-policy.md)
for what you can share on GitHub.

## Requesting a hardware comparison

Run the [hardware kit workflow](../.github/workflows/hardware-request.yml) on
`main` with the PR number and a short Series X scenario. Specify where to start,
what to observe and when to close each run. Once it succeeds, send the tester the
checkout command for their OS with the PR number appended, plus the private report
destination. The menu also lists the request. If the PR head changes or the kit
expires, publish a fresh kit for the current revision.

For testers without a checkout, share the kit ZIP. To prepare one locally with a
signed-in GitHub CLI, run this from a trusted checkout, replacing `PR_NUMBER` with
the number of the PR.

```sh
python3 scripts/hardware-test/prepare.py PR_NUMBER --output test-kit.zip \
  --scenario "Wait ten seconds at the menu. Enter the opening scene. Close each run at the first subtitle."
```

The packager matches the successful PR build to its tested main parent and checks
both artifact digests. If either build has expired, refresh the pair first.
Post the tested revisions, configuration and observed differences on the PR. For
performance comparisons, include frame times and stalls alongside average or median FPS.

## What the host needs

The Windows graphics tests create a Direct3D 12 device.

| Need | Used by |
|------|---------|
| A D3D12 adapter with shader model 6.6 | execution tests in `victory-shader` (`emit/host.rs`) |
| Windows SDK `dxil.dll` | DXIL validation in `victory-shader` tests; they print `no dxil.dll; skipped` without it |
| `VICTORY_WARP=1` | run the host on the WARP software adapter instead of the default GPU |

`VICTORY_WARP=1` is useful on a machine whose GPU lacks a feature, or when a
test fails on one driver and you want to know whether it is the driver. WARP
is not a real GPU, so behaviour that depends on the hardware (wave size,
driver quirks) is only settled by a run on that hardware.

Run one crate or one test on its own:

```powershell
cargo test -p victory-shader --locked
cargo test -p victory-shader --locked -- <test name> --nocapture
```

## Comparing frame pacing

Keep the build settings, driver, window size and launch sequence the same. Compare
the menu and first scene after loading separately. Release logs include a
`present intervals` line for every five seconds of completed frame intervals
(or 4,096 intervals at high rates). Each line gives the sample count, mean,
median, nearest-rank p95, longest interval and counts over 100 ms and one second.
These are wall-clock times between successful host presents. Startup before
the first present and gaps while DXGI reports an occluded window are excluded.
A stall appears in the log when presentation resumes.

## Tests that skip or are ignored

* **Differential shader tests** (`victory-shader/src/emit/diff_tests.rs`)
  compile the same HLSL with the console compiler and the PC compiler and
  compare what each draws. They need `VICTORY_CONSOLE_DXC` (the console SDK's
  `dxc.exe`) and a PC `dxc.exe` (`VICTORY_PC_DXC`, else the newest Windows
  SDK's), and **skip without them**: a green run without those variables did
  not run them. `VICTORY_DIFF_ONLY` narrows them to matching cases.
* **`#[ignore]` tests** need reference data made outside the repository and
  run with `-- --ignored`:
  * `victory-formats` XMA: `VICTORY_XMA_STREAMS=<dir>` (`*.xma` with `*.ctx`),
    `VICTORY_XMA_REFERENCE=<dir>` (`mono`/`stereo` `.xmadata` and `.ref`).
  * `victory-formats` BCPack: `VICTORY_BCPACK_TABLES`, `VICTORY_BCPACK_CASES`.

  Never commit that data; see the asset policy.

## The loader's integration tests

`victory-loader/tests/load_probe.rs` loads a stand-in title,
`tests/fixtures/hle-probe`. It is not a workspace member: the test builds it
on demand on Windows. When running the test under Proton, cross-build the
fixture on Linux and set `VICTORY_HLE_PROBE` to its Windows path. See the
[loader test commands](proton-launcher.md#testing-without-game-files).

## Writing tests

* Use synthetic inputs built in the test. Do not add game files, shader
  dumps or extracted tables, even small ones.
* A test that needs a GPU or tool feature should print why and skip, as the
  existing ones do (`no SM 6.6 ... device; skipped`), rather than fail.
* Check hardware behaviour against the ISA and, when you can, a measurement on
  real hardware. Say which GPU and driver in the PR.
