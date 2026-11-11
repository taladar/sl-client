---
id: server-lsl-opensim-grow-loop-slow
title: fake-opensim grows memory/collision's Full list four times slower than YEngine
topic: server
status: done
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

## Done (2026-11-11)

The runtime charged Second Life's measured speeds to every engine. YEngine
runs an empty loop about 57 times faster than aditi and joins a list for about
7 µs plus 5 ns an element, where Second Life pays about 16 µs an element, so
the Full list's summed joins took fake-opensim about a minute to reach 8 730.

- `vm/cost.rs` now holds one speed profile per engine (`Speed`: instruction,
  back-edge, call overhead, join curves, library-call curves and the region's
  script shares), fitted from each grid's committed `vm/throughput`,
  `vm/call-cost` and call-cost transcripts. Charges and budgets are counted in
  nanoseconds of modelled script time; `EngineConfig::for_step` takes the
  flavour.
- Every instruction is still charged at least 1 ns, since the engine reads an
  empty slice as "nothing to run"; YEngine's back-edge is cut to match its
  measured empty loop.
- `joins_and_library_calls_cost_what_they_cost_on_each_grid` holds all three
  engines to their transcripts.
- `memory/collision` on fake-opensim agrees with YEngine, and its whole-probe
  entry in `probes/accepted.toml` is gone; `comms/link-messages`' two
  scheduling entries are gone too, as the Sender's handler now ends inside one
  YEngine slice as it does on OpenSim.
- The book's LSL engine and determinism chapters describe the per-engine
  model.
- At YEngine's speed `vm/call-cost-lists` and `vm/call-cost-strings` take
  the runtime longer than real time on fake-opensim, so both ask for a
  720 s `deadline`. The `lsl-probes` cases' overall timeout was a flat
  30 minutes, which the corpus outgrew long ago; it is now every probe's
  deadline plus room for its setup.
