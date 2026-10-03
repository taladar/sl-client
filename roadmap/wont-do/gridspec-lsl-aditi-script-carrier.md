---
id: gridspec-lsl-aditi-script-carrier
title: Put scripts into prims on aditi through a carrier object and UpdateScriptTask
topic: gridspec
status: wont-do
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey,
  test-phase-z-deferred-04,
  repl-lsl-script-control]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Won't do (2026-10-03)

Superseded by fixing the real fault: our client plants scripts on aditi
directly with `RezScript` / `UpdateScriptTask` ([[test-phase-z-deferred-04]]),
so no carrier object is needed.

## Known already

SL drops our `RezScript` / `UpdateTaskInventory` writes; rez-from-inventory
works on aditi. Whether `UpdateScriptTask` works for us on SL is untested
(`script-upload` fails at planting first). The unanswered `ObjectSelect` on
SL hints at a wrong local id / circuit.

## Discover

The user makes one "probe carrier" prim with a script in Firestorm, once,
and takes it to inventory; our client rezzes copies and overwrites the
script via `UpdateScriptTask`. If that works, aditi probing is self-service
(and `script-upload` / `script-running` go `[both]`); if not, follow
deferred-04's next steps (the select hint first).

## Document

`book/src/tools/lsl-probes.md` § Planting on aditi.

## Fake grid

Not applicable.

## Viewer

Not applicable.
