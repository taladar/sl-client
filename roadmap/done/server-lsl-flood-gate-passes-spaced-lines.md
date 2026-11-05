---
id: server-lsl-flood-gate-passes-spaced-lines
title: Second Life's error flood gate drops lines aditi passed
topic: server
status: done
origin: server-lsl-lib-prim-state (2026-11-04)
refs: [server-lsl-runtime-errors, server-lsl-lib-prim-state,
  server-lsl-flood-owner-lines-longer-limit,
  server-lsl-opensim-pos-local-moved-six]
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

## Outcome (2026-11-05)

Three new probes, run on aditi and both OpenSim engines: `errors/flood-kinds`,
`errors/flood-paths` and `errors/owner-lines` (book chapter
`gridspec/lsl.md`, § Floods). What Second Life counts is neither the kind
nor the function — every error line of the object counts, faults included,
and a flood drops every line of the object — but it counts at a **rate**:
ten lines four seconds apart all pass. And what made the `prim/*` probes
look exempt is that a rule list's complaints (`llSetPrimitiveParams`,
`llGetPrimitiveParams`, `llParticleSystem`) are told **the owner alone**,
let one line further and dropped silently, with no notice.

The gate (`vm::report::FloodGate`) now counts lines over a fifteen-second
window, eight said in the open and nine told the owner, and a flood lasts 34
seconds from its notice, after which the object starts afresh; both figures
are decisions inside the measured ranges. A rule list's complaints go
through `ScriptCtx::refuse_to_owner` and `Host::say_to`, which the fake grid
now delivers to the avatar's viewer as a script error on `DEBUG_CHANNEL`. A
missing sound and a missing texture cost a script a second on Second Life,
as `cost` measured. `prim/params`' `try` records carry the script's clock now,
which is what showed the one thing the window does not explain:
[[server-lsl-flood-owner-lines-longer-limit]]. The replays' `gated` helper
is gone; `vm/tests/floods.rs` replays the three new probes.
