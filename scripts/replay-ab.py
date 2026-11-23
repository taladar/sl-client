#!/usr/bin/env python3
"""Compare the frame times of two viewer builds on one scene recording.

Runs ``--replay-benchmark`` with each binary in turn, in an ABBA order so that
slow drift of the machine (temperature, background load) falls on both builds
alike. Every run gets a fresh settings directory, so neither build sees the
other's settings or the developer's own.

The difference between the builds only means something next to the spread
between runs of one build, so the report prints both and marks a change as
``within noise`` when it is smaller than that spread.

Usage::

    scripts/replay-ab.py --a old/sl-client-bevy-viewer \\
        --b target/release/sl-client-bevy-viewer \\
        --recording ~/.local/share/sl-client-captures/regions/NNN.scenecapture \\
        --camera-position 19.95,107.07,34.35 \\
        --camera-look-at 18.89,107.07,34.25

Both binaries must support ``--replay-benchmark``. Recordings hold other
residents' assets; the reports written here hold only timings.
"""

from __future__ import annotations

import argparse
import json
import os
import statistics
import subprocess
import sys
import tempfile
from pathlib import Path

METRICS = ("p50_ms", "p95_ms", "p99_ms")


def parse_args() -> argparse.Namespace:
    """Read the command line."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--a", type=Path, required=True, help="baseline binary")
    parser.add_argument("--b", type=Path, required=True, help="candidate binary")
    parser.add_argument("--recording", type=Path, required=True)
    parser.add_argument("--rounds", type=int, default=3, help="ABBA rounds")
    parser.add_argument("--frames", type=int, default=1200)
    parser.add_argument("--warmup", type=int, default=300)
    parser.add_argument("--layers", choices=("all", "world", "avatars"), default="all")
    parser.add_argument("--day-position", type=float, default=0.5)
    parser.add_argument("--size", default="1920x1080")
    parser.add_argument("--camera-position")
    parser.add_argument("--camera-look-at")
    parser.add_argument("--camera-path", type=Path)
    parser.add_argument(
        "--viewer-assets", type=Path, default=Path("viewer-assets/character")
    )
    parser.add_argument("--output", type=Path, help="write the comparison as JSON here")
    return parser.parse_args()


def order(rounds: int) -> list[str]:
    """Return the run order: ABBA, repeated."""
    return ["a", "b", "b", "a"] * rounds


def run_once(args: argparse.Namespace, binary: Path, scratch: Path) -> dict:
    """Run one benchmark in a fresh settings directory and read its report."""
    report = scratch / "report.json"
    environment = {
        **os.environ,
        "XDG_CONFIG_HOME": str(scratch / "config"),
        "XDG_CACHE_HOME": str(scratch / "cache"),
        "XDG_STATE_HOME": str(scratch / "state"),
        "RUST_LOG": "warn,openjp2=error",
    }
    command = [
        str(binary),
        "--replay",
        str(args.recording),
        "--viewer-assets",
        str(args.viewer_assets),
        "--headless",
        "--capture-size",
        args.size,
        "--replay-benchmark",
        str(report),
        "--replay-benchmark-frames",
        str(args.frames),
        "--replay-benchmark-warmup",
        str(args.warmup),
        "--replay-benchmark-layers",
        args.layers,
        "--replay-benchmark-day-position",
        str(args.day_position),
    ]
    if args.camera_position:
        command.append(f"--camera-position={args.camera_position}")
    if args.camera_look_at:
        command.append(f"--camera-look-at={args.camera_look_at}")
    if args.camera_path:
        command += ["--replay-camera-path", str(args.camera_path)]
    log = scratch / "viewer.log"
    with log.open("w") as sink:
        result = subprocess.run(
            command,
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=sink,
            stderr=subprocess.STDOUT,
            check=False,
        )
    if result.returncode != 0 or not report.exists():
        tail = log.read_text(errors="replace").splitlines()[-15:]
        sys.exit(
            f"{binary} failed with status {result.returncode}:\n" + "\n".join(tail)
        )
    return json.loads(report.read_text())


def spread(values: list[float]) -> float:
    """Return the largest difference between two runs, as a share of the median."""
    middle = statistics.median(values)
    return (max(values) - min(values)) / middle if middle else 0.0


def compare(runs: dict[str, list[dict]]) -> dict:
    """Summarise each build's runs and the change between them."""
    result = {}
    for metric in METRICS:
        a_values = [run["summary"][metric] for run in runs["a"]]
        b_values = [run["summary"][metric] for run in runs["b"]]
        a_median = statistics.median(a_values)
        b_median = statistics.median(b_values)
        change = (b_median - a_median) / a_median if a_median else 0.0
        noise = max(spread(a_values), spread(b_values))
        result[metric] = {
            "a_ms": a_median,
            "b_ms": b_median,
            "change": change,
            "noise": noise,
            "significant": abs(change) > noise,
        }
    return result


def print_comparison(comparison: dict) -> None:
    """Print the comparison as a table."""
    print(f"{'':6}{'A ms':>10}{'B ms':>10}{'change':>10}{'noise':>9}")
    for metric, row in comparison.items():
        verdict = "" if row["significant"] else "  within noise"
        print(
            f"{metric.removesuffix('_ms'):6}{row['a_ms']:10.2f}{row['b_ms']:10.2f}"
            f"{row['change']:10.1%}{row['noise']:9.1%}{verdict}"
        )


def main() -> None:
    """Run the comparison."""
    args = parse_args()
    binaries = {"a": args.a, "b": args.b}
    runs: dict[str, list[dict]] = {"a": [], "b": []}
    sequence = order(args.rounds)
    for index, which in enumerate(sequence, start=1):
        with tempfile.TemporaryDirectory(prefix="replay-ab-") as directory:
            report = run_once(args, binaries[which], Path(directory))
        runs[which].append(report)
        print(
            f"run {index}/{len(sequence)} {which.upper()}: "
            f"p50 {report['summary']['p50_ms']:.2f} ms, "
            f"p99 {report['summary']['p99_ms']:.2f} ms",
            flush=True,
        )
    comparison = compare(runs)
    print_comparison(comparison)
    if args.output:
        args.output.write_text(json.dumps(comparison, indent=2) + "\n")


if __name__ == "__main__":
    main()
