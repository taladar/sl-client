---
id: gridspec-lsl-memory-limits
title: LSL memory and limits on OpenSim and the remaining SL unknowns
topic: gridspec
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-memory-sizes, viewer-script-limits]
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
