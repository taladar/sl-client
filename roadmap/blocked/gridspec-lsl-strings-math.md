---
id: gridspec-lsl-strings-math
title: LSL library behaviour on each grid — strings, lists, maths and rotations
topic: gridspec
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-lib-strings-lists,
  server-lsl-lib-math-rotations, server-lsl-call-cost-sizes]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

No aditi or OpenSim measurements for this tranche beyond the library table's
documented values.

## Discover

Probes for: Only size-proportional costs and rotation precision on aditi
(PyOptimizer covers semantics); rotation axis order against OpenSim. Run on
OpenSim YEngine / XEngine and aditi.

## Document

`book/src/gridspec/lsl.md` § Strings, lists, maths and rotations.

## Fake grid

Large — [[server-lsl-lib-strings-lists]] and [[server-lsl-lib-math-rotations]].

## Viewer

None.
