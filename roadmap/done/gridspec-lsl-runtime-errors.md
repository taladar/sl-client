---
id: gridspec-lsl-runtime-errors
title: LSL run-time errors on OpenSim and the debug channel in our viewer
topic: gridspec
status: done
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

## Done (2026-10-27)

- **Discover.** Five probes beside `errors/runtime-errors`, each fault in a
  script of its own: `errors/math` (fifteen arithmetic cases, the prim
  renamed first, two ways back for a faulted script, listeners 19 m and
  21 m away), `errors/math-lso` (the same compiled to LSO), `errors/bounds`
  and `errors/bounds-newer` (list and string calls past either end, reversed
  and with odd strides; the newer functions OpenSim does not compile in a
  probe of their own) and `errors/flood` (twelve errors in a row, a second
  script and another object asked mid-flood, then one after 10, 30 and 60 s
  of quiet). Complete on aditi and both OpenSim engines; the math probes
  pause between faults since Second Life's flood limit otherwise swallowed
  four of LSO's.
- **Found.** Mono faults only on division by zero; LSO also on every
  non-finite result. Second Life passes an object's first eight error lines,
  then `Too many errors... dropping further messages until the flood
  stops`, per object, until 10–30 s of quiet. A fault's lines are said (20
  m). A reset brings a faulted script back on both grids, `llSetScriptState`
  alone does not. XEngine throws `OverflowException` on `INT_MIN / -1`;
  OpenSim throws inside four list and string calls Second Life answers, and
  YEngine reports a library exception as owner-say lines with the stack,
  not on `DEBUG_CHANNEL`. A refused `llGetScriptState` costs the script a
  second on every grid.
- **Document.** `book/src/gridspec/lsl.md` § Run-time errors, linked from
  `content/scripts.md` and the corpus table in `tools/lsl-probes.md`.
- **Fake grid.** As planned, within [[server-lsl-runtime-errors]] per
  flavour; the measured flavour differences are noted there, and the
  library halves on [[server-lsl-lib-strings-lists]] and
  [[server-lsl-lib-math-rotations]].
- **Viewer.** The two window tasks were already merged
  ([[viewer-script-error-window]] is `wont-do`); the window itself is built
  ([[viewer-script-warning-window]], done), with the debug channel routed
  into it. The live check with a division by zero is the `e2e_objects`
  stage `a_scripts_fault_opens_the_script_errors_window_on_a_live_grid`.
- **Fixed on the way.** The live stage found that no script could be opened
  in the editor on Second Life: it lists every script's asset id as nil, even
  to its owner, and the Content tab refused a nil id while the editor fetched
  the source by asset id over HTTP. The editor now reads a script by its
  item through the simulator (`SimInvItem`: `FetchTaskItemAsset` for a
  prim's, `FetchInventoryItemAsset` for the agent's own), as the reference's
  `getInvItemAsset` does and as the notecard editor already did, and the
  Content tab opens a script whatever its id; a refused transfer says so in
  the window.
- **Filed.** [[test-conformance-lsl-probes-teardown-unacked]]: every OpenSim
  probe run ends with its last derez and both logouts unacknowledged.
