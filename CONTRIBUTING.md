# Contributing to VIctory

## Help by testing

If this is your first contribution, try one test and tell us what happened.
You can help without writing a patch. A result from a second machine can
show whether a failure follows a particular build, GPU driver, or setting.

- **Testing a pull request?** Use the [interactive Series X test](docs/testing.md#run-a-series-x-hardware-test) on Windows or Linux/Proton. Pick the PR, follow the same scenario twice, and return the ZIP privately to the requester.
- **Reporting a single Linux run?** Install UMU (`umu-run`) and extract a tested
  [Proton build](docs/testing.md#ci-and-build-downloads). From its `victory`
  directory, run `./victory-proton.sh --report "/path/to/title.exe"`.
  If you built from a checkout, use the same arguments with
  `./scripts/victory-proton.sh`.
- **Reporting a single Windows run?** Follow the [Windows build and launch instructions](README.md#getting-started).
  Keep the terminal output from the run. For a crash, check the newest entry
  in the title's `crashes.log` and confirm its timestamp matches your run.
- **Have a checkout but no game files?** On Linux, `./scripts/victory-proton.sh --test`
  exercises the launcher without Wine or a GPU. With the Rust toolchain installed,
  `cargo test --workspace --locked` runs the workspace tests. If a test fails,
  record its name and the first error.

[Search existing issues](https://github.com/ligmaSec/victory-royale/issues) for the
symptom or error. If someone has reported it, reply there with your build,
hardware, and result. A successful run on a machine where a fix needed checking
is worth posting too. For a new failure, open **one issue per distinct symptom**
using the [test report form](https://github.com/ligmaSec/victory-royale/issues/new?template=test-report.yml).

## Help by reviewing pull requests

Security reviews of [open pull requests](https://github.com/ligmaSec/victory-royale/pulls)
are especially welcome. Even a review of one changed function helps. In a review
comment, point to the relevant lines and explain what could go wrong and under
what conditions. If you have a suggested fix or regression test, share it too.

## Game data

Nothing from the game, or from any other proprietary title, enters this
repository, an issue, a comment or a pull request. That covers game files,
executable bytes, disassembly, shader dumps, captures, screenshots and logs
that contain game material. Read the [asset policy](docs/asset-policy.md)
before you share anything. Describe what you saw in words, with counts, our
own log strings and `module+rva` addresses.

## Set up

Follow the [setup instructions](README.md#getting-started). Linux host tests
require rustup and a native C toolchain. To build the Windows executable on
Linux, use the [Proton guide](docs/proton-launcher.md#build-on-linux).

```powershell
cargo xtask install-hooks                # asset-policy check on every commit
cargo test --workspace --locked          # needs no game data
```

To run the title you also need `victory.toml` (copy `victory.example.toml`)
with every path outside the repository.

## Check a change

Follow the [code standards](docs/code-standards.md). The workspace tests use synthetic data.

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo xtask policy
```

## Report a failure

Give the issue a title someone else could search for, such as “Proton exits
as the loading screen opens”. Include enough detail to repeat your run:

1. Identify the build (commit or CI run, release or diagnostics) and your
   environment. For a game run, include the title version, OS, CPU, GPU and
   driver. On Linux, add the UMU and Proton versions when applicable.
2. Give the command or shortest sequence of steps, including changed settings.
   Describe what you expected, the last stage that worked, what you saw next,
   and whether the process exited, hung, or you closed it. If you already
   compared builds, give both revisions and the result with the same settings.
3. Quote the relevant error with a few surrounding lines from that run. On
   Linux, `--report` prints a local archive path. Inside it, `build.txt`
   identifies the executable, `launcher-exit.txt` records the exit status,
   and `launcher.log` and any `steam-*.log` hold the output. If the process
   stopped silently, say so and include the last stage or exit code you saw.

Keep the full archive local. Share only reviewed excerpts allowed by the
[asset policy](docs/asset-policy.md). Replace usernames and personal paths
with placeholders and remove credentials and any output from the title itself.
If you use an AI assistant to draft the report, verify its commands, versions,
and error codes against your run. Mark missing details as unknown. A single
observed failure is enough to open an issue.

For graphics failures, see [Reporting a GPU failure](README.md#reporting-a-gpu-failure).
