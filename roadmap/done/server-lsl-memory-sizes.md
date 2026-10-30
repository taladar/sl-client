---
id: server-lsl-memory-sizes
title: Measure Second Life's per-value memory costs and rotation arithmetic on aditi
topic: server
status: done
origin: user review of server-lsl-value-model (2026-09-27)
points: 5
refs: [server-lsl-memory-and-limits, server-lsl-value-model,
  server-lsl-float-expression-precision]
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

## First numbers (gridspec-lsl-memory-limits, 2026-10-26)

`memory/budget` and `memory/collision` (`sl-lsl-runtime/probes/`) already
show some of Mono's costs on aditi: an integer in a global list 16 bytes
(exactly 8 000 per 500), a float 16, a vector 24, a rotation 28, a key 80 to
100, a string character 2 whatever its UTF-8 length; a small script uses
about 6 500 bytes. Read the cost from `llGetUsedMemory`, never from
`llGetFreeMemory`, which is a low-water mark there; and expect the reading
to wander by a few hundred bytes between two reads of nothing (13 598, then
13 508).

## Rotation arithmetic measured (gridspec-lsl-strings-math, 2026-10-27)

`sl-lsl-runtime/probes/math/rotation-bits.lsl` is the measuring script
asked for above; its aditi transcript
(`probes/results/aditi/math/rotation-bits.txt`) holds every component's
mantissa and exponent for seven pairs of rotations, the `8.32` case first.
Matched against candidate evaluations (book: `gridspec/lsl.md` §
*Precision*):

- Mono evaluates an expression of floats in **double** and rounds to a
  float only when the value is stored: `a * b * c` in one expression and
  over two statements differ in the last bit.
- `rotation * rotation`, `vector * rotation` and `vector * vector` (dot):
  all in double, rounded once. The `8.32` case's `y` is 8.3199997, which
  `(string)` prints as `8.320000` — so "exactly 8.32" was the printed
  value, and `compose`'s products-rounded-first formula (`8.320001`) is the
  one to change.
- `rotation / rotation` (the product with the conjugate), `vector /
  rotation` (Linden's two-step `llquaternion.cpp` formula) and `vector %
  vector`: **each operation rounded to a float** — so `cross`'s current
  all-double formula is wrong too.
- Library functions: the double result of the float argument, rounded.

The memory half of this task is still open.

## Done (2026-10-30)

**Memory.** A new probe, `memory/sizes` (`sl-lsl-runtime/probes/memory/sizes/`:
the measuring script and eight `Globals <type>` scripts), read
`llGetUsedMemory` into locals around each type in each context, three times
a row, on aditi and both OpenSim engines; the transcripts are committed and
the book's `gridspec/lsl.md` § What a value costs, exactly, tabulates them.
`sl-lsl-runtime`'s `value::memory` is the cost function, per
`EngineFlavour`: `value_cost(value, flavour, Holder)` (global, local,
argument), `element_cost(element, flavour)` (a list element),
`global_cost(type, flavour)` (a declared global), `MEMORY_LIMIT`,
`MONO_REFERENCE`. Its test holds it to every row of all three transcripts:
exactly wherever two of a row's three aditi reads agree (55 of the 78 list
rows and every other row), and within 200 bytes of the median where a row of
a hundred values or more never repeats (the largest miss is 162, inside the
few hundred bytes a read wanders by). The empty-ish baseline (a script that
only reads and reports) is 4 456 bytes on aditi, the limit 65 536 everywhere.

What the model cannot see, for [[server-lsl-memory-and-limits]]: Mono
counts a list of one shared literal once plus 4 a reference, and the value
model has no object identity; and Mono counts the script's code (2 bytes a
character of a string literal, more per declaration), which a different
compiler cannot reproduce byte for byte.

**Rotations.** `ops.rs` now reproduces all 42 measured components of
`math/rotation-bits` bit for bit (test
`rotation_and_vector_arithmetic_matches_aditi_to_the_bit`): `compose` and
`dot` in double rounded once, `rotate` unchanged (it already was), and a new
`divide`, `unrotate` (Linden's two steps) and `cross` rounding every
operation to a float. The `8.32` case is the float nearest 8.32. Mono's
rounding of a longer float expression only when it is stored is a compiler
and VM change, split out as [[server-lsl-float-expression-precision]].
