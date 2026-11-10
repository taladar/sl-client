---
id: server-lsl-restarted-sleeper-order
title: A restarted script whose sleep ran out waits for its restarter's slice
topic: server
status: done
origin: test-lsl-differential-corpus-triage (2026-11-09)
refs: [test-lsl-differential-corpus-triage]
---

Context: [context/lsl.md](../context/lsl.md).

## Observation

`time/sleep-while-stopped`, run `late`: the Controller calls
`llSetScriptState("Sleeper", TRUE)` after the Sleeper's sleep ran out while it
was stopped, then prints its own line. On aditi and on the local OpenSim the
Sleeper's `woke` line comes first, at the same time; on both fake flavours the
Controller's comes first. The runtime applies the run flag once the caller's
slice is over (`Raised::Control`, `vm/engine.rs`), and the Sleeper resumes
after.

## Fix

End the caller's slice at a control that wakes a script whose sleep has run
out, or otherwise let the woken script go first, held by a replay of the
probe (`vm/tests/clocks.rs`). Then drop its entries in `probes/accepted.toml`.

## Done (2026-11-10)

`llSetScriptState(name, TRUE)` on a script stopped in the middle of a body
whose sleep has run out (`Instance::resumes_when_started`) now ends the
caller's slice with `Outcome::Handed(target)`; the engine applies the run flag,
runs the started script's turn at once (`Engine::run_handed`, with the caller
back among the region's instances meanwhile so the started script still sees
and reaches it), then resumes the caller after the call. A script served that
way is not served again in the same first pass of the tick. The replay in
`vm/tests/clocks.rs` now holds `time/sleep-while-stopped`'s lines in the
order printed, not grouped by record. Both fake flavours re-run: each agrees
with its grid, and the two accepted entries are gone.
