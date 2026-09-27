---
id: server-lsl-memory-sizes
title: Measure Second Life's per-value memory costs and rotation arithmetic on aditi
topic: server
status: ready
origin: user review of server-lsl-value-model (2026-09-27)
points: 5
refs: [server-lsl-memory-and-limits, server-lsl-value-model]
---

Context: [context/lsl.md](../context/lsl.md).

Content is tuned to the memory the reference charges per value: a
notecard reader stops at `llGetFreeMemory() < 2000`, a cache trims its
list when it nears the limit. If the fake grid charges less than Second
Life, a script hits its **stack-heap collision** (LSL's out-of-memory
error) **later** than it would there; if it charges more, it collides
**where Second Life would not**. Either way a
test on the fake grid passes or fails for the wrong reason. So the cost
of each value has to be the reference's exact number — not an
approximation — and it has to be measured, since no source on this
machine models it (PyOptimizer and tailslide do not; OpenSim's
`YEngine/XMRHeapTracker.cs` is OpenSim's own model, not Second Life's).

Wanted:

- **A measuring script** run on aditi (Mono) that reads
  `llGetUsedMemory()` before and after creating each kind of value and
  reports the deltas: per type (`integer`, `float`, `string` by length
  and by character class — ASCII against multi-byte — `key`, `vector`,
  `rotation`, `list`) and per **context** — a global, a local, a
  function argument, a list element, the list itself by length. Repeat
  each measurement enough to see the allocation granularity rather than
  one noisy delta.
- **The numbers recorded in the repo** with how they were measured (the
  script is committed alongside), so the tests that use them quote a
  source.
- **A per-value cost function beside `Value`** in `sl-lsl-runtime`
  (`value.rs`), table-driven and tested against the recorded numbers,
  for [[server-lsl-memory-and-limits]] to account with. Also record the
  empty-script baseline and `llGetMemoryLimit()` default.

## Rotation arithmetic precision

The same aditi session settles the one value-model divergence left open by
[[server-lsl-value-model]]. Second Life stores floats as `f32`, but some
operations compute intermediates at a higher precision: `<3,5,7,17> *
<.22,.26,.38,.86>` has a `y` of exactly `8.32` there (the FIXME in
PyOptimizer's `unit_tests/expr.suite/operators.lsl`), while
`sl-lsl-runtime`'s `compose` — PyOptimizer's formula, each partial product
rounded to `f32` and the four summed in `f64` — gives `8.320001`, and
neither pure-`f32` nor pure-`f64` summation in any order tried reproduces
it.

- **Measure** `rotation * rotation` and `rotation / rotation` on aditi for
  inputs that tell the candidate precisions apart, printing each component
  exactly — `(string)` gives only seven significant digits, so extract the
  IEEE bits arithmetically in the script (`llFrexp` does not exist; scale
  by powers of two and read the mantissa as an integer) — including the
  `8.32` case above and ones chosen where
  `f32`, `f64` and fused-multiply-add evaluation disagree.
- **Also check** `vector * rotation`, `vector / rotation` and the dot and
  cross products, which currently follow PyOptimizer's all-`f64`
  formulas, against the same kind of distinguishing inputs.
- **Change** `compose` / `rotate` / `dot` / `cross` in
  `sl-lsl-runtime/src/ops.rs` to whichever evaluation reproduces every
  measured value, and replace the recorded-divergence comment in the
  `vector_and_rotation_arithmetic` test with the aditi measurements.

Acceptance: every type × context the measuring script covers has a
recorded aditi value, and the cost function reproduces each one exactly;
every measured rotation and vector product is reproduced bit-for-bit by
`sl-lsl-runtime`, the `8.32` case included.
