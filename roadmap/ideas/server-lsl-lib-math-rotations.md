---
id: server-lsl-lib-math-rotations
title: Library tranche — maths, vectors and rotations
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-lsl-library-surface-table]
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
