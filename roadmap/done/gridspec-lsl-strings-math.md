---
id: gridspec-lsl-strings-math
title: LSL library behaviour on each grid — strings, lists, maths and rotations
topic: gridspec
status: done
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

## Done (2026-10-27)

- **Discover.** Ten probes. `math/rotation-bits` prints the exact mantissa
  and exponent of rotation, vector and float arithmetic for seven pairs of
  rotations (PyOptimizer's `8.32` case among them), matched offline against
  candidate evaluations; `math/rotations` (axis order, composition order,
  rotations and axes that are not unit length, the helpers at their
  corners); `math/negate-rotation` (its own probe: XEngine does not compile
  it); `math/functions` (rounding, integer overflow and shifts, `llPow`,
  `llTan`, `llAtan2`, `llModPow`, `llFrand`); `strings/conversions` (how
  every type is written as a string and read back from one);
  `strings/functions` (code points, case, trimming, URL and Base64 escapes,
  hashes, parsing, sorting, statistics, finding); `strings/unicode`
  (`llChar`, `llOrd`, `llHash`); `strings/newer` (the functions OpenSim
  does not compile); and `vm/call-cost-lists`, `vm/call-cost-strings`,
  `vm/call-cost-newer`, timing 26 functions at four sizes each. Complete on
  aditi and both OpenSim engines (the Second-Life-only ones are a compile
  error there).
- **Found on the way.** OpenSim counts UTF-16 units, so it can cut a
  surrogate pair, and a chat line holding half a pair is cut off there;
  `strings/functions` prints such text as Base64. XEngine does not compile
  `-rotation`. YEngine parses 11 of 56 decimal literals one unit in the last
  place off. Second Life's `llBase64ToInteger("!!!!")` differs between
  runs. In `book/src/tools/lsl-probes.md`.
- **Document.** `book/src/gridspec/lsl.md` § Strings, lists, maths and
  rotations, linked from `content/scripts.md`.
- **Fake grid.** Large, as planned: the measured behaviour per flavour is in
  [[server-lsl-lib-strings-lists]], [[server-lsl-lib-math-rotations]], the
  precision in [[server-lsl-memory-sizes]] and the costs in
  [[server-lsl-call-cost-sizes]].
- **Viewer.** None.
