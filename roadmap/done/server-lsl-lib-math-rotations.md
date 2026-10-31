---
id: server-lsl-lib-math-rotations
title: Library tranche — maths, vectors and rotations
topic: server
status: done
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-lsl-library-surface-table,
  gridspec-lsl-strings-math]
refs: [server-lsl-value-model, server-world-determinism-contract,
  server-lsl-lib-physics-vehicles]
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
  `llSqrt` exists, add `math/float-rounding`'s `library-return` row to the
  VM replay in `vm/tests/precision.rs` (the `list-element` and
  `list-append` rows went in with `llList2Float`,
  [[server-lsl-lib-strings-lists]]).
- `llLinear2sRGB` and `llsRGB2Linear` are done
  ([[server-lsl-lib-strings-lists]], `library/math.rs`).
- A pure function whose answer differs by grid is generic over
  `library::PureCtx`; an OpenSim throw (`llModPow(…, 0)`'s
  `DivideByZeroException`) is a `CallError::Exception` — add the variant to
  `library::Exception`; the reports per engine are already there.
- The library helpers here
  compute the double result of the float argument and round it once
  (`math/rotation-bits`' `library` rows).

## Done (2026-10-31)

All 29 new functions of the tranche are implemented in `sl-lsl-runtime`
(`library/math.rs` for the numbers, `library/rotations.rs` for vectors and
rotations), each answering as the region's flavour does; `llFabs` now clears a
negative zero's sign on OpenSim. Coverage is 112 implemented of 524.
`llRotLookAt` returns nothing and only turns the prim, so it has no maths
half to write here; it is left to [[server-lsl-lib-physics-vehicles]], which
already lists it beside `llLookAt` and `llStopLookAt`.

**Second Life.** The numbers are PyOptimizer's — the double result of the
float argument rounded once, `llRound` as `floor(x + 0.5)` in float, the
x87's reduction of an integral angle for the trigonometry, `llTan(1e30)`
handed back, `llLog(0)` 0, `llModPow` unsigned and exact — and hold to
`math/rotation-bits`' scalar rows bit for bit. The rotation functions are
**not** PyOptimizer's: fitting them to `math/rotation-bits`' library rows (the
exact bits of every function for seven input pairs, already on aditi) showed
Second Life runs Linden's `llmath` in float arithmetic, every operation
rounded. Ported from the 2007 C++ (`normVec`, `getAngleAxis`, `getMatrix3`,
`orthogonalize`, `getEulerAngles`, `shortestArc`) with the later guard in
`normalize`, `llVecMag`, `llVecNorm`, `llRot2Axis`, `llRot2Angle`, `llRot2Fwd`
(and so `Left`/`Up`), `llAxisAngle2Rot`, `llRotBetween` and `llAngleBetween`
match all seven pairs, `llRot2Euler` five (the other two one unit in the last
place off). Where Second Life's library differs from the published code: an
axis-angle rotation is not renormalised (in `llAxisAngle2Rot` and in
`shortestArc`'s turn), an axis too short to normalise turns by nothing, a zero
axis from `getAngleAxis` is the zero vector, and `llRot2Euler` gives no
negative zero. `llEuler2Rot` matches no arrangement of Linden's code tried and
keeps PyOptimizer's double form (within three units in the last place on every
pair); `llAxes2Rot`, not bit-measured, keeps PyOptimizer's too. `llFrand` draws
24 bits from the region's seeded `Host::random_bits` and truncates toward zero,
so it never reaches its magnitude.

**OpenSim.** `LSL_Api.cs`'s formulas in double, rounded to the value model's
float as they are stored: `llRound` half away from zero, `llLog(0)` `-Infinity`,
`llModPow` through `Math.Pow` and x86's `(long)` (a double past the range is
`long.MinValue`) with `DivideByZeroException` for a modulus of 0 (and
`OverflowException` for −1 when the power overflows), `llRot2Euler`'s 0.4999
pole test, `llRotBetween` through `acos`, `llAxes2Rot` never normalised, and a
zero rotation made the identity as `LSL_Rotation` makes it.

**Proof.** `vm/tests/math.rs` replays the probes' own scripts in the VM:
`math/functions` on all three grids (the `llFrand` lines compared without their
draws), `math/rotations` on all three, and the library cases of `errors/math`
(8, 10, 11, 12, 13, 15) on all three, with YEngine telling the owner and XEngine
saying on `DEBUG_CHANNEL` what `llModPow(2, 3, 0)` throws. Named deviations: on
aditi two Euler round trips (`<30°, 45°, 60°>` and `<10°, −90°, 20°>`), a digit
apart because they rest on `llEuler2Rot`'s last bits; on OpenSim only the value
model — results past the float range, digits in the sixth decimal a float
rounds away, negative zero printed without its sign, and the operators
`vector * rotation` and `vector / rotation`, which are Second Life's on every
flavour. `library::rotations` holds the rotation functions to aditi's bits;
`library::math` the scalar rows and the corners; a rotation round trip over a
grid of angles holds to 1e-5 rad; `llFrand` reproduces across two runs of one
seed; and `math/float-rounding`'s `library-return` row is in the precision
replay.
