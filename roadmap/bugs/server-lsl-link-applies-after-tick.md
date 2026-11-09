---
id: server-lsl-link-applies-after-tick
title: A script's link or delink applies only after the tick
topic: server
status: bugs
origin: test-lsl-differential-corpus-triage (2026-11-09)
refs: [test-lsl-differential-corpus-triage]
---

Context: [context/lsl.md](../context/lsl.md).

## Observation

The fake grid applies `llCreateLink`, `llBreakLink` and `llBreakAllLinks` as
effects once the engine's tick is over (`sl-fake-grid/src/scripts.rs`,
`Effect::Link`, `Effect::BreakLink`, `Effect::BreakAllLinks`). A script that
runs later in the same tick still reads the old linkset. In `prim/link` step 7
the Leader, a child by then, sends `llMessageLinked(LINK_ROOT, "report")` and
calls `llBreakAllLinks()`: aditi and the local OpenSim both report
`child label=as-child link=0 … prims=1`, the break already in, and
fake-opensim reports `link=1 prims=2`.

## Also to measure

On aditi a Leader that called a refused (too far) `llCreateLink` and then a
granted one in the same event printed `changed bits=LINK` twice
(`prim/link-refusals`), so a refused link may raise `CHANGED_LINK` in the
caller's prim too. One sample; measure it again before modelling it.

## Fix

Apply a script's link change as the call makes it, under the region lock, or
let reads later in the same tick see the pending change. Then drop the entries
in `probes/accepted.toml` that name this task.
