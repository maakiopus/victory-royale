#!/usr/bin/env python3
"""Structured analysis of one fixed-loop victory-royale run.

Reads the app log and capture directory produced by run.sh, then prints a
stable report. Every sweep uses the same checks so results are comparable.
"""
from __future__ import annotations

import argparse
import re
import sys
from collections import Counter
from pathlib import Path

SHADER_DROP = re.compile(
    r"shader does not translate; its pipelines' draws are dropped "
    r'stage="(?P<stage>[^"]+)" hash=(?P<hash>[0-9a-f]+) error=(?P<error>.+)'
)
BLACK_FRAME = re.compile(
    r"a black frame after a frame that was not frame=(?P<frame>\d+) "
    r"resource=(?P<resource>0x[0-9a-f]+) state=(?P<state>0x[0-9a-f]+) "
    r"draws=(?P<draws>\d+) copied=(?P<copied>\w+) black_frames=(?P<count>\d+)"
)
WORLD_MARKER = re.compile(
    r"the first frame of \d+ draws or more: frames (?P<first>\d+) and "
    r"(?P<second>\d+) are captured frame=(?P<qualifying>\d+)"
)
PRESENT = re.compile(
    r"present intervals: (?P<n>\d+) frames, mean (?P<mean>[\d.]+) ms, "
    r"median (?P<median>[\d.]+) ms, p95 (?P<p95>[\d.]+) ms, "
    r"longest (?P<longest>[\d.]+) ms, (?P<over100>\d+) over 100 ms, "
    r"(?P<over1s>\d+) over 1 s"
)
FAULT = re.compile(r"\bshader fault\b|\bdevice lost\b|\bDeviceLost\b", re.IGNORECASE)
INDIRECT_FAIL = re.compile(r"no argument clamp: indirect execution dropped")
DEPTH_FAIL = re.compile(r"no pipeline to draw a depth plane from its twin")


def analyse(log: Path, captures: Path) -> int:
    text = log.read_text(errors="replace") if log.exists() else ""
    lines = text.splitlines()

    shader_drops: list[tuple[str, str, str]] = []
    black_frames: list[tuple[int, str, int]] = []
    world: tuple[int, int, int] | None = None
    presents: list[dict[str, float]] = []
    faults = 0
    indirect = 0
    depth = 0

    for line in lines:
        clean = re.sub(r"\x1b\[[0-9;]*m", "", line)
        if m := SHADER_DROP.search(clean):
            shader_drops.append((m["stage"], m["hash"], m["error"]))
        if m := BLACK_FRAME.search(clean):
            black_frames.append((int(m["frame"]), m["resource"], int(m["count"])))
        if m := WORLD_MARKER.search(clean):
            world = (int(m["qualifying"]), int(m["first"]), int(m["second"]))
        if m := PRESENT.search(clean):
            presents.append(
                {
                    "n": float(m["n"]),
                    "mean": float(m["mean"]),
                    "median": float(m["median"]),
                    "p95": float(m["p95"]),
                    "longest": float(m["longest"]),
                    "over100": float(m["over100"]),
                    "over1s": float(m["over1s"]),
                }
            )
        if FAULT.search(clean):
            faults += 1
        if INDIRECT_FAIL.search(clean):
            indirect += 1
        if DEPTH_FAIL.search(clean):
            depth += 1

    pngs = sorted(captures.glob("*.png")) if captures.exists() else []
    big = [p for p in pngs if p.stat().st_size > 500_000]

    print(f"log_lines={len(lines)} captures={len(pngs)} nonblank={len(big)}")
    print(f"world_marker={world or 'NONE'}")
    print(f"faults={faults} indirect_drops={indirect} depth_plane_drops={depth}")
    print(f"black_frame_warnings={len(black_frames)} last={black_frames[-1] if black_frames else None}")
    print(f"shader_translation_drops={len(shader_drops)}")
    for (stage, h, err), count in Counter(shader_drops).most_common(12):
        print(f"  drop stage={stage} hash={h} count={count} err={err}")
    if shader_drops:
        print(f"  example_error={shader_drops[0][2][:200]}")
    if presents:
        worst = max(presents, key=lambda p: p["longest"])
        last = presents[-1]
        print(f"present_windows={len(presents)} worst_longest_ms={worst['longest']}")
        print(f"last_window n={last['n']:.0f} mean={last['mean']} median={last['median']} "
              f"p95={last['p95']} longest={last['longest']} over100={last['over100']:.0f} "
              f"over1s={last['over1s']:.0f}")

    # A sweep fails if anything that should render is missing or errors appear.
    ok = bool(world) and not faults and not shader_drops and not black_frames
    print(f"VERDICT={'PASS' if ok else 'FAIL'}")
    return 0 if ok else 1


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--log", type=Path, required=True)
    ap.add_argument("--captures", type=Path, required=True)
    args = ap.parse_args()
    return analyse(args.log, args.captures)


if __name__ == "__main__":
    sys.exit(main())
