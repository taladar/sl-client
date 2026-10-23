---
id: gridspec-lsl-runtime-errors
title: LSL run-time errors on OpenSim and the debug channel in our viewer
topic: gridspec
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-runtime-errors,
  viewer-script-warning-window]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

[[server-lsl-runtime-errors]] (in progress) carries the aditi probe; OpenSim's
texts include the C# exception. Debug-channel chat now reaches the viewer's
Nearby transcript as kind `debug` (`sl-viewer-people/src/conversations.rs`,
`chat_kind`; pinned by `e2e_pilot.rs`'s debug-channel test), but there is no
script error / debug window yet ([[viewer-script-warning-window]]).

## Discover

The OpenSim column of every run-time error; list bounds; stack-heap wording;
Mono vs LSO format.

## Document

`book/src/gridspec/lsl.md` § Run-time errors.

## Fake grid

Within [[server-lsl-runtime-errors]] per flavour.

## Viewer

Merge the two script error window tasks; route the debug channel into that
window; live check with a division by zero.

## From gridspec-lsl-probe-corpus (2026-10-23)

`errors/runtime-errors` is committed and run on all three engines (table in
`book/src/tools/lsl-probes.md`):

- **Second Life** sends two `DEBUG_CHANNEL` messages from the prim:
  `Object [script:<name>] Script run-time error`, then `Math Error` (integer
  or float division, modulo) or `Stack-Heap Collision`. A listening script
  10 m away hears both; one 50 m away hears neither.
- **OpenSim** sends one message to the owner that no script hears.
  YEngine's reads `YEngine: Division by Zero (script: … primID: … at:
  <…>) Script must be Reset to re-enable.`, XEngine's `Integer division by
  Zero(script: …)` / `Float division by zero(…)`. A library function's own
  error (`llGetScriptState: Can't find script 'Nope'`) is heard at 50 m.
- **The faulted script**: stopped for good on Second Life
  (`llSetScriptState(…, TRUE)` does not restart it); reported running but
  deaf on YEngine; running on, in the state it faulted in, on XEngine.
- **Unbounded recursion under XEngine aborts the OpenSim process.**
