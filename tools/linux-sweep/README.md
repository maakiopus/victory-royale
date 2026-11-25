# Fixed Linux graphics/performance sweep

Runs victory-royale through one deterministic scenario on Proton Experimental
and reports graphics and frame-time defects in a stable format. Every sweep
uses the same Proton, keys, capture points and duration, so results from
different commits and different agents are comparable.

**No system Wine is used.** The only launch path is `umu-run` with
`PROTONPATH` pointing at Steam Proton Experimental. UMU-Proton 10/11 emit
invalid SPIR-V that RADV faults on in `radv_shader_spirv_to_nir`.

## Usage

```
SWEEP_LABEL=baseline ./tools/linux-sweep/run.sh
```

Overrides: `SWEEP_EXE`, `SWEEP_GAME`, `SWEEP_DURATION`, `SWEEP_KEYS`,
`PROTONPATH`, `STEAM_COMPAT_INSTALL_PATH`, `STEAM_COMPAT_LIBRARY_PATHS`.

The default `SWEEP_KEYS` presses A (Enter on the keyboard-to-pad table) every
10 seconds from 10 s to 200 s. The retail game needs an A press after start
before it leaves the title screen and begins loading; two presses were not
enough to make that transition deterministic.

Output goes to `/tmp/opencode/sweep-$SWEEP_LABEL/`: `app.log` (app output),
`runner.log` (umu output), `capture/` (PNG captures), and a structured verdict
from `analyze.py`.

## Checks

The analyzer flags:

- shader translation drops (stage, hash, converter error)
- black-frame warnings (frame, resource, count)
- shader faults and device loss
- indirect/depth-plane pipeline drops
- world marker and nonblank capture count
- present-interval percentiles (mean/median/p95/longest, over 100 ms/1 s)

`VERDICT=PASS` requires a world marker, no faults, no shader drops and no
black-frame warnings. It exits non-zero on failure so a sweep loop can chain
fix attempts automatically.
