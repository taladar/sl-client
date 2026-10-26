---
id: gridspec-lsl-memory-limits
title: LSL memory and limits on OpenSim and the remaining SL unknowns
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-memory-sizes, viewer-script-limits,
  server-lsl-memory-and-limits, viewer-region-script-count-monitor]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

[[server-lsl-memory-sizes]] measures SL per-value costs; recursion depth,
`llSetMemoryLimit` below usage, LSO accounting and OpenSim's heap model are
unmeasured.

## Discover

The OpenSim column (YEngine heap tracker, XEngine); SL recursion depth at
stack-heap collision; `OBJECT_SCRIPT_MEMORY` per grid.

## Document

`book/src/gridspec/lsl.md` § Memory.

## Fake grid

Large — [[server-lsl-memory-and-limits]].

## Viewer

Script limits / region script count against real numbers.

## Done (2026-10-26)

- **Discover.** Five probes, `sl-lsl-runtime/probes/memory/`: `budget` and
  its LSO twin `budget-lso` (the memory functions at start, a global list of
  0–64 values of each type, a global string of one-, two- and three-byte
  characters, the profiler, `llSetMemoryLimit` above, at and below the
  script's use); `recursion` (five recursers — narrow, wide, under a lowered
  limit, and the two in LSO — writing their depth into the prim's
  description so the controller reads it after the collision); `collision`
  (a doubled string in Mono and LSO, a list of integers at the default and
  a lowered limit, each bounded so XEngine, which has no limit, stops short
  of the host); `object` (`OBJECT_SCRIPT_MEMORY` and the script counts of an
  object of Mono, LSO, lowered, stopped and idle scripts, another object,
  the owner, keys that name nothing). Complete on aditi and both OpenSim
  engines.
- **Runner.** Two probe header keys: `target: lsl2` uploads a script for
  LSO (noted `# target:`; OpenSim ignores the field), and
  `stats: active-scripts` notes the region's `ActiveScripts` stat as it
  changes (`# sim-stats`).
- **Found on the way.** A recursion that calls a library function at every
  level aborts the YEngine simulator (the native stack overflows inside
  YEngine's own stack check), so the recursers are `skip-on` both OpenSim
  engines; Second Life's Mono recursion depth differs between runs; a busy
  script on Second Life gets a slice of each frame, so a controller must
  wait for its growers by state, not by time. All in
  `book/src/tools/lsl-probes.md`.
- **Document.** `book/src/gridspec/lsl.md` § Memory and limits, linked from
  `content/scripts.md`.
- **Fake grid.** Large, as planned: the measured model per flavour is in
  [[server-lsl-memory-and-limits]]; first per-value numbers in
  [[server-lsl-memory-sizes]].
- **Viewer.** No viewer surface reads these calls. The region's
  active-script count (a fractional average on Second Life, a whole count
  of present scripts on OpenSim) and the object script-memory figures are
  noted on [[viewer-region-script-count-monitor]] and
  [[viewer-script-limits]], which build the surfaces that show them.
