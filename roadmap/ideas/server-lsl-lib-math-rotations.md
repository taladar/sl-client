---
id: server-lsl-lib-math-rotations
title: Library tranche — maths, vectors and rotations
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-lsl-library-surface-table,
  gridspec-lsl-strings-math]
refs: [server-lsl-value-model, server-world-determinism-contract]
---

Context: [context/lsl.md](../context/lsl.md).

The second pure tranche. ~45 functions: `llAbs`, `llFabs`, `llCeil`,
`llFloor`, `llRound`, `llSqrt`, `llPow`, `llLog`, `llLog10`, `llSin`,
`llCos`, `llTan`, `llAsin`, `llAcos`, `llAtan2`, `llModPow`, `llFrand`,
`llVecMag`, `llVecNorm`, `llVecDist`, `llRot2Euler`, `llEuler2Rot`,
`llAxisAngle2Rot`, `llRot2Axis`, `llRot2Angle`, `llAxes2Rot`,
`llRot2Fwd`/`Left`/`Up`, `llRotBetween`, `llAngleBetween`,
`llRotLookAt`'s maths half, `llGetDeterminant`-style helpers.

What makes it more than a wrapper over `std`:

- **Everything is `f32`.** Computing in `f64` and rounding at the end
  gives different answers from computing in `f32` throughout, and
  scripted rotations accumulate. Match the reference's precision, not
  Rust's convenience.
- **`llRot2Euler` / `llEuler2Rot` use Linden's axis order** and have
  documented gimbal behaviour; `llAxes2Rot` takes three axis vectors and
  is the one most often got wrong. `sl_types::lsl::Rotation` exists, and
  the `llquaternion-composes-backwards` finding applies: Linden's `a*b`
  is glam's `b*a`, so every composition in this tranche needs the order
  checked rather than assumed.
- **`llFrand` must be seeded from the grid's minter**, not
  `rand::thread_rng` — [[server-world-determinism-contract]]. It takes a
  magnitude and returns `[0, mag)` with a **negative** magnitude giving
  `(mag, 0]`.
- **Error values, not errors.** `llSqrt(-1)` returns `NaN` and raises a
  math error; `llLog(0)` and `llPow`'s edge cases each have a stated
  answer. `llRound` rounds half **away from zero**, unlike Rust's
  `f32::round` on ties in the negative direction — check it.
- **`llModPow`** is deliberately restricted (a modulus bound) and sleeps.

Oracle: `lslbasefuncs.py` again for the edge cases, plus a differential
run against the local OpenSim for the rotation helpers, where a single
axis-order mistake produces plausible-looking but wrong numbers that no
unit test written from the same misunderstanding would catch.

Acceptance: the tranche fully implemented; a rotation round-trip test
(`llEuler2Rot` → `llRot2Euler`) holding to `f32` tolerance over a grid of
angles; `llFrand` reproducing across two runs of one seed; and the
axis-order cases checked against a script run on the local OpenSim.

## Measured (gridspec-lsl-runtime-errors, 2026-10-27)

`book/src/gridspec/lsl.md` § Run-time errors, *What faults*: which
arithmetic results are a run-time error under Mono, LSO and each OpenSim
engine, and what the others return. For this tranche: `llSqrt` of a
negative, `llAcos` / `llAsin` out of range, `llPow` giving NaN are `NaN`
under Mono and both OpenSim engines and a `Math Error` under LSO; `llLog(0)`
and `llLog10(-1)` are `0.0` on Second Life and `-Infinity` / `NaN` on
OpenSim; `llModPow(…, 0)` is `0` on Second Life and an exception on OpenSim;
`llVecNorm(ZERO_VECTOR)` is zero everywhere; `llRot2Euler(<0, 0, 0, 0>)` is
zero and `llAxes2Rot` of three zero vectors `<1, 0, 0, 0>` on Second Life,
`<0, 0, 0, 0.5>` on OpenSim; a rotation divided by `<0, 0, 0, 0>` is
`<0, 0, 0, 0>` there and `<0, 0, 0, 1>` here. Floats are doubles on OpenSim:
`1e38 * 1e38` is `1e76` there and `Infinity` (Mono) or a `Math Error` (LSO)
on Second Life.

## Measured (gridspec-lsl-strings-math, 2026-10-27)

`book/src/gridspec/lsl.md` § Strings, lists, maths and rotations
(*Precision*, *Rotations*, *Numbers*). The axis order agrees across the
grids for unit input. Where the flavours differ:

- **`llRound` is `floor(x + 0.5)` in float on Second Life**: −0.5 → 0,
  −2.5 → −2, 0.49999997 → 1. The "half away from zero" above is OpenSim's
  (−1, −3, 0). Fix the acceptance accordingly.
- `vector * rotation` by a rotation that is not unit length scales by its
  squared length on Second Life (`<1,0,0> * <0,0,0,2>` is `<4,0,0>`;
  by `<0,0,0,0>` `ZERO_VECTOR`); OpenSim normalises first and uses a
  formula that agrees only for unit rotations (`<1,0,0> * <1,2,3,4>` is
  `<4,28,−10>` there, `<−25,28,−10>` here).
- `llAxisAngle2Rot` of a zero or tiny axis is the identity there;
  `llVecNorm` of `<0,0,1e-7>` is `ZERO_VECTOR` there, a unit vector here.
- `llEuler2Rot` past a full turn may return the negated quaternion on
  OpenSim.
- `llModPow` reads its operands as unsigned 32-bit and is exact on Second
  Life; OpenSim goes through `Math.Pow` and is wrong past 2^53.
- `llTan(1e30)` hands the argument back on Second Life; `llPow` and
  products past the float range are `Infinity` there, doubles here.
- `-rotation` does not compile under XEngine.
- Precision: the operators are Second Life's to the bit since
  [[server-lsl-memory-sizes]], and a longer float expression's rounding
  since [[server-lsl-float-expression-precision]] (an argument is rounded
  before a library function sees it, its result is a float). Once
  `llSqrt` and `llList2Float` exist, add `math/float-rounding`'s
  `library-return`, `list-element` and `list-append` rows to the VM replay
  in `vm/tests/precision.rs`. The library helpers here
  compute the double result of the float argument and round it once
  (`math/rotation-bits`' `library` rows).
