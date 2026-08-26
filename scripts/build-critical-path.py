#!/usr/bin/env python3
"""Critical-path analysis of a ``cargo build --timings`` report.

Cargo's own HTML report shows *wall clock* — what 24 cores happened to do on
one run. This answers the different question: **what is the longest chain of
crates that must compile one after another**, i.e. what the build would still
cost on infinitely many cores. That number is what crate-structure work moves;
wall clock also moves with load, cache warmth and core count.

The report embeds ``const UNIT_DATA = [...]``, and each unit's
``unblocked_units`` / ``unblocked_rmeta_units`` **are** the dependency edges
(the second kind releases a consumer at metadata time — pipelined compilation —
so it is relaxed against the producer's ``rmeta_time``, not its full duration).
Beware the spelling: ``unblocked``, not ``unlocked``. The wrong one parses
happily and silently yields an all-zero graph.

Two things to know before trusting a figure:

- **A warm dependency tree reports 0.0 s for third-party units.** Only the
  crates that actually rebuilt are real, so a run with warm deps measures the
  workspace half of the chain and understates everything below it.
- **Per-unit compile times swing between runs** — an untouched crate has come
  back 26.5 s and then 31.3 s. A before/after comparison must therefore not
  compare two raw critical paths. Pass ``--baseline`` and the new report's
  *graph* is re-solved with the baseline's *durations*: same clock, different
  edges, so what is left is the effect of the dependency change alone.

Usage::

    # what the current build's serial chain is
    cargo build --release -p sl-client-bevy-viewer --timings
    python3 scripts/build-critical-path.py target/cargo-timings/cargo-timing.html

    # what a dependency change bought, with compile noise held out
    python3 scripts/build-critical-path.py --baseline before.html after.html

To make a comparison meaningful, rebuild from the same point each time — e.g.
``touch`` the lowest crate in the tier under study so everything above it
recompiles — and note that the two reports need only share unit *names*, not
the same set of units.

This is a dev tool; like ``roadmap/index.py`` it is deliberately **not** a
member of the cargo workspace and has no third-party dependencies.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

#: Identifies a compilation unit across two reports: cargo distinguishes the
#: library, each binary and the build script of one package by mode and target.
UnitKey = tuple[str, str, str]


def load_units(path: Path) -> list[dict]:
    """Parse the ``UNIT_DATA`` array out of a cargo timing report."""
    html = path.read_text(encoding="utf-8")
    match = re.search(r"const UNIT_DATA = (\[.*?\]);\n", html, re.DOTALL)
    if match is None:
        raise SystemExit(
            f"{path}: no UNIT_DATA array — is this a cargo --timings report?"
        )
    return json.loads(match.group(1))


def unit_key(unit: dict) -> UnitKey:
    """The identity used to match a unit against the same unit in another run."""
    return (unit["name"], unit["mode"], unit["target"])


def unit_label(unit: dict) -> str:
    """A human-readable name for a unit, naming its target only when it adds something."""
    target = unit["target"]
    if not target or target == unit["name"]:
        return f"{unit['name']} {unit['mode']}".strip()
    return f"{unit['name']} {unit['mode']} ({target})".strip()


def build_edges(units: list[dict]) -> tuple[dict[int, list], dict[int, list]]:
    """Producer→consumer edges, as ``(preds, succs)`` keyed by unit index.

    Each predecessor is recorded as ``(index, kind)`` where ``kind`` is
    ``"rmeta"`` for an edge released at metadata time and ``"full"`` for one
    that waits for the whole unit.
    """
    preds: dict[int, list] = {unit["i"]: [] for unit in units}
    succs: dict[int, list] = {unit["i"]: [] for unit in units}
    for unit in units:
        for kind, field in (
            ("full", "unblocked_units"),
            ("rmeta", "unblocked_rmeta_units"),
        ):
            for consumer in unit.get(field, []):
                if consumer not in preds:
                    continue
                preds[consumer].append((unit["i"], kind))
                succs[unit["i"]].append(consumer)
    return preds, succs


def topological_order(units: list[dict], preds, succs) -> list[int]:
    """Unit indices in dependency order. Iterative, so a deep graph cannot blow the stack."""
    remaining = {unit["i"]: len(preds[unit["i"]]) for unit in units}
    ready = [i for i, count in remaining.items() if count == 0]
    order: list[int] = []
    while ready:
        current = ready.pop()
        order.append(current)
        for consumer in succs[current]:
            remaining[consumer] -= 1
            if remaining[consumer] == 0:
                ready.append(consumer)
    if len(order) != len(units):
        raise SystemExit("the timing graph has a cycle — cargo should never emit one")
    return order


def solve(
    units: list[dict], durations: dict[UnitKey, tuple[float, float]] | None = None
):
    """Earliest possible start and finish of every unit, given unlimited cores.

    ``durations`` optionally supplies ``(duration, rmeta_time)`` per unit key,
    overriding what this report measured — that is how a graph from one run is
    priced with another run's clock. Units missing from it keep their own times.

    Returns ``(start, finish, best_pred, dur)``, where ``best_pred`` names the
    predecessor that actually gated each unit, so the critical path can be
    walked back from the last unit to finish.
    """
    preds, succs = build_edges(units)
    dur: dict[int, float] = {}
    rmeta: dict[int, float] = {}
    for unit in units:
        override = durations.get(unit_key(unit)) if durations else None
        full, meta = (
            override if override else (unit["duration"], unit.get("rmeta_time"))
        )
        dur[unit["i"]] = full
        # A unit with no separate metadata phase releases its consumers only
        # when it is wholly done.
        rmeta[unit["i"]] = meta if meta else full

    start: dict[int, float] = {}
    finish: dict[int, float] = {}
    rmeta_finish: dict[int, float] = {}
    best_pred: dict[int, int | None] = {}
    for index in topological_order(units, preds, succs):
        earliest = 0.0
        gate: int | None = None
        for producer, kind in preds[index]:
            ready_at = rmeta_finish[producer] if kind == "rmeta" else finish[producer]
            if ready_at > earliest:
                earliest, gate = ready_at, producer
        start[index] = earliest
        best_pred[index] = gate
        rmeta_finish[index] = earliest + rmeta[index]
        finish[index] = earliest + dur[index]
    return start, finish, best_pred, dur


def critical_path(
    finish: dict[int, float], best_pred: dict[int, int | None]
) -> list[int]:
    """The chain of units ending at the last one to finish, in build order."""
    chain: list[int] = []
    current: int | None = max(finish, key=lambda i: finish[i])
    while current is not None:
        chain.append(current)
        current = best_pred[current]
    chain.reverse()
    return chain


def print_path(units: list[dict], start, finish, dur, chain: list[int]) -> None:
    """Print the critical path as a table of ``start + self -> finish``."""
    by_index = {unit["i"]: unit for unit in units}
    for index in chain:
        print(
            f"  {start[index]:7.1f} +{dur[index]:6.1f} -> {finish[index]:7.1f}"
            f"  {unit_label(by_index[index])}"
        )


def pick_anchor(
    units: list[dict], start: dict[int, float], base_units: list[dict], base_start
) -> UnitKey:
    """The unit both runs share that the *later* run starts earliest.

    Two runs rarely rebuild the same set of crates — a run that only touched the
    workspace has no `bevy_pbr` in it at all — so their absolute critical paths
    are not comparable and subtracting them is meaningless. What *is* comparable
    is the segment from a unit both runs actually compiled through to the final
    link, and the natural choice is the root of the later run's own tree.
    """
    base_keys = {unit_key(unit) for unit in base_units if unit["i"] in base_start}
    shared = [
        unit for unit in units if unit_key(unit) in base_keys and unit["duration"] > 0.0
    ]
    if not shared:
        raise SystemExit("the two reports share no rebuilt unit — nothing to compare")
    return unit_key(min(shared, key=lambda unit: start[unit["i"]]))


def segment(
    units: list[dict], start: dict[int, float], end: float, anchor: UnitKey
) -> float:
    """How long the build still takes once ``anchor`` starts."""
    for unit in units:
        if unit_key(unit) == anchor:
            return end - start[unit["i"]]
    raise SystemExit(f"unit {anchor[0]!r} is not in one of the reports")


def print_chain_above(units: list[dict], start, dur, end: float, prefix: str) -> None:
    """Rank matching units by the work still ahead of them once they start.

    That figure is what an edit to a crate costs: everything from the moment it
    is unblocked to the final link, modulo parallelism. It is the number to
    rank crates by when deciding where to spend structural work.
    """
    rows = [
        (end - start[unit["i"]], dur[unit["i"]], unit_label(unit))
        for unit in units
        # Units that cost nothing were cached; they say nothing about structure.
        if unit["name"].startswith(prefix) and dur[unit["i"]] > 0.0
    ]
    if not rows:
        return
    print(
        f"\nchain above each rebuilt `{prefix}` unit (from its start to the final link):"
    )
    for above, self_time, label in sorted(rows, reverse=True):
        print(f"  {above:7.1f}  self {self_time:6.1f}  {label}")


def main() -> int:
    """Parse arguments, solve the requested report(s) and print the result."""
    parser = argparse.ArgumentParser(
        description="Longest serial chain through a cargo --timings report.",
        epilog="With --baseline, the report's graph is priced with the baseline's "
        "durations, so the printed delta is the dependency change alone.",
    )
    parser.add_argument("report", type=Path, help="the cargo timing HTML to analyse")
    parser.add_argument(
        "--baseline",
        type=Path,
        help="an earlier report to compare against, and to take durations from",
    )
    parser.add_argument(
        "--prefix",
        default="sl-",
        help="unit-name prefix for the chain-above table (default: %(default)s)",
    )
    parser.add_argument(
        "--anchor",
        help="crate to measure the compared segment from; defaults to the root of "
        "the later run's own tree (the two runs rarely rebuild the same set)",
    )
    args = parser.parse_args()

    units = load_units(args.report)
    wall = max(unit["start"] + unit["duration"] for unit in units)

    if args.baseline is None:
        start, finish, best_pred, dur = solve(units)
        end = max(finish.values())
        print(f"critical path: {end:.1f}s  (wall clock in this run: {wall:.1f}s)")
        print_path(units, start, finish, dur, critical_path(finish, best_pred))
        print_chain_above(units, start, dur, end, args.prefix)
        return 0

    base_units = load_units(args.baseline)
    base_start, base_finish, base_pred, base_dur = solve(base_units)
    base_end = max(base_finish.values())
    print(f"baseline: {base_end:.1f}s")
    print_path(
        base_units,
        base_start,
        base_finish,
        base_dur,
        critical_path(base_finish, base_pred),
    )

    clock = {
        unit_key(unit): (unit["duration"], unit.get("rmeta_time"))
        for unit in base_units
    }
    missing = sorted({unit["name"] for unit in units if unit_key(unit) not in clock})
    start, finish, best_pred, dur = solve(units, clock)
    end = max(finish.values())
    print(f"\nafter (this run's graph, baseline durations): {end:.1f}s")
    print_path(units, start, finish, dur, critical_path(finish, best_pred))
    if missing:
        print(
            "\nnote: not in the baseline, so priced at their own measured time: "
            + ", ".join(missing)
        )

    if args.anchor:
        candidates = [unit for unit in units if unit["name"] == args.anchor]
        if not candidates:
            raise SystemExit(f"unit {args.anchor!r} is not in {args.report}")
        anchor = unit_key(min(candidates, key=lambda unit: start[unit["i"]]))
    else:
        anchor = pick_anchor(units, start, base_units, base_start)
    before = segment(base_units, base_start, base_end, anchor)
    after = segment(units, start, end, anchor)
    print(
        f"\nfrom {anchor[0]} to the final link — the segment both runs share:"
        f"\n  before {before:7.1f}s"
        f"\n  after  {after:7.1f}s"
        f"\n  delta  {after - before:+7.1f}s"
    )
    print_chain_above(units, start, dur, end, args.prefix)
    return 0


if __name__ == "__main__":
    sys.exit(main())
