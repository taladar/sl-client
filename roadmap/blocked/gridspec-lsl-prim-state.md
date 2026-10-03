---
id: gridspec-lsl-prim-state
title: LSL library behaviour on each grid — reading and writing the prim
topic: gridspec
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-lib-prim-state]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

No aditi or OpenSim measurements for this tranche beyond the library table's
documented values.

## Discover

Probes for: `llSetPos` clamp, `PRIM_*` round-trip order, which `changed` bits
each setter raises. Run on OpenSim YEngine / XEngine and aditi.

## Document

`book/src/gridspec/lsl.md` § Reading and writing the prim.

## Fake grid

Large — [[server-lsl-lib-prim-state]].

## Viewer

Prim edits made by scripts render as the grid sends them.
