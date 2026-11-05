---
id: server-lsl-flood-gate-passes-spaced-lines
title: Second Life's error flood gate drops lines aditi passed
topic: server
status: bugs
origin: server-lsl-lib-prim-state (2026-11-04)
refs: [server-lsl-runtime-errors, server-lsl-lib-prim-state]
---

Context: [context/lsl.md](../context/lsl.md).

## Observation

The runtime's flood gate on a Second-Life-flavoured region (`vm::report`,
`FloodGate`) passes an object's first eight debug-channel lines, says
`Too many errors... dropping further messages until the flood stops`, and
drops the rest until twenty seconds of quiet — the shape `errors/flood`
measured, twelve `llGetScriptState` errors a second apart.

The `prim/*` replays (`vm/tests/prim_state.rs`) contradict it: aditi passed
all eleven lines of `prim/params` and nine of `prim/params-rules` (spaced by
`llSetPrimitiveParams`'s 0.2 s forced delay) and all fifteen of
`prim/effects` (half a second apart and more), where the gate drops the ninth
and says the notice. `inventory/listing` and `memory/recursion` flooded on
aditi after eight lines, as the gate does.

## What to find out

What Second Life counts: the lines' spacing (a rate, not a count in a
window), the kind of line (a run-time error and a library's complaint
counted apart), or the function. A probe that says nine complaints of one
kind at a set spacing, then of another kind, on aditi answers it.

## Done when

The gate passes what aditi passed in all five probes, and the replays'
`gated` helper (which writes the gate's answer into the expected lines on
Second Life) goes.
