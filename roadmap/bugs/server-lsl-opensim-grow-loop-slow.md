---
id: server-lsl-opensim-grow-loop-slow
title: fake-opensim grows memory/collision's Full list four times slower than YEngine
topic: server
status: bugs
origin: test-lsl-differential-corpus-triage (2026-11-09)
refs: [test-lsl-differential-corpus-triage]
---

Context: [context/lsl.md](../context/lsl.md).

## Observation

`memory/collision`'s Full list script loops `llSetObjectDesc((string)n)` and a
ten-integer append up to 20 000 elements. The local OpenSim (YEngine) finishes
within the controller's 60 timer ticks; fake-opensim reaches 8 730 by then,
about 15 iterations a second of region time. The Full list is still running
when the controller moves on, so it overwrites the shared description and the
Limited list reads `last=9620` in place of 4 000. Every one of the probe's 28
differences follows from that.

## What is known

`vm/throughput` and `vm/call-cost` agree, so plain instruction speed is right.
The runtime's own replay (`vm/tests/memory.rs`) rewrites `llSetObjectDesc` as a
print, so the path through the fake grid's prim write
(`library/prim_state.rs` `change_own` → `RegionHost::set_prim`) is untested for
speed. The other suspect is what the runtime charges a growing list's append
against YEngine's real cost.

## Fix

Time `grow(20000)` from `full-list.lsl` in the runtime's region harness on the
YEngine flavour with `llSetObjectDesc` kept, as iterations per simulated
second, then through a fake-grid box, and find where the time goes. Drop
`memory/collision`'s whole-probe fake-opensim entry in `probes/accepted.toml`
once it agrees.
