---
id: server-lsl-link-applies-after-tick
title: A script's link or delink applies only after the tick
topic: server
status: done
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

## Done (2026-11-10)

- **In the call.** `RegionHost` makes a script's `llCreateLink`,
  `llBreakLink` and `llBreakAllLinks` in the store as the call is made
  (`change_now`). What the change asks of the engine, lent to the tick — the
  `changed` events and a sitter's permission lapse — is noted on
  `RegionScripts::deferred` while the engine is out and replayed after the
  tick as `Effect::Engine`, in order with the other effects, and what the
  sessions are told goes out as `Effect::Tell`. A call into the engine made
  under the tick by any other path is now made once the engine is back,
  where it used to go to the stand-in engine and be lost.
- **A link message reaches whom it named when sent.** The runtime resolved
  `llMessageLinked`'s recipients and sender number at the end of the slice,
  so a child's message to `LINK_ROOT` followed by `llBreakAllLinks` reached
  nobody once the break was in the call. `ScriptCtx::send_linked` now reads
  them at the call (`Raised::Linked { message, sender, reached }`), as both
  live grids deliver it (`prim/link` step 7).
- **Measured again:** `prim/link-refusals` on aditi (2026-11-09, server
  2026-11-05) raised `CHANGED_LINK` twice in the Leader again for the refused
  far link and the granted one in the same event, and `prim/link`'s lone
  granted link raises it once. So Second Life's too-far refusal is heard in
  the caller's prim: `RefusedLinkHeard::TheCaller`, on the
  Second Life flavour only (`book/src/gridspec/lsl.md` § Linking). Whether it
  is the calling prim or its whole set is unmeasured (the Leader was alone).
- `sl-fake-grid/tests/scripts.rs`'s scripted linkset asks the children to
  count the set before the root's `llBreakAllLinks` in one event; they read
  `0/1` (it read `2/3` and `3/3` on the OpenSim flavour before).
- `prim/link` agrees between the local OpenSim and fake-opensim but for
  other tasks' entries, and `prim/link-refusals` agrees outright between
  aditi and fake-sl; both entries naming this task are gone, and so is the
  sit-order scheduling entry for aditi/fake-sl, which aditi's new run no
  longer shows.
