---
id: viewer-audit-underwater-fog-nan
title: A negative underwater fog density NaNs the screen
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 1
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-world-scene/src/underwater_fog.rs:194` —
`base.powf(water.underwater_fog_mod.clamp(0.0, 10.0))` with a
**negative `base`** and a non-integral exponent is `NaN`, which reaches the
uniform and NaNs the whole screen.

The reference already fixed this: `llsettingswater.cpp:393` guards it, with the
comment *"BUG-233797/BUG-233798 -ve underwater fog density can cause
(unrecoverable) blackout"*.

Port the guard. `underwater_fog.rs` has no tests; `water_params` and this
expression are pure and need no GPU.

## Fixed (2026-08-29)

The expression is now a named pure function, `modified_water_fog_density`, one
per the reference's `getModifiedWaterFogDensity`, and `update_underwater_fog`
calls it. The guard is the reference's second remedy, the one it says it chose:
when the density is negative and the *clamped* modifier is not integral, the
density becomes `1.0` before the power, which keeps some notion of fog rather
than inverting the water's colour (the first remedy, rounding the modifier,
which the reference rejected).

Two details of the reference the straight-line port would have lost:

- Integrality is tested **after** the clamp, so a modifier of `10.5` clamps to
  `10`, is integral, and takes the plain power — no rescue, no divergence from
  the reference on a value it lets through.
- The rescue replaces the density, not the modifier, so the modifier the power
  uses is still the authored one.

Both values are things a region can legitimately send — the density is a free
`f32` off the wire and the modifier is authored per water frame — so this is
reachable on any grid, not only a hand-edited setting.

The sibling density in `water.rs:550` (the water-surface shader's uniform) is
the *unmodified* one, where the reference's `lldrawpoolwater.cpp:242` passes the
modified one. That cannot `NaN` today, because `water.wgsl` declares
`water_fog_density` and never samples it — but only because of a second, larger
divergence underneath it, filed as [[viewer-water-surface-fog-fallback-flat]]:
the surface's deep-water colour is a flat authored tint where the reference
computes it from that density. Whoever fixes that must feed it through
`modified_water_fog_density`, which is why this lives in a named function rather
than inline in the fog system.

### Tests

Five in `underwater_fog.rs`. Four on the pure function — untouched above water,
untouched for a non-positive modifier, the ordinary power (including the `[0,
10]` modifier clamp), and the negative-density case all three ways: rescued for
a fractional modifier, let through for an integral one, and let through for a
`10.5` that clamps to an integral one. The fifth is end-to-end: poison every
water frame of the default day cycle with the reference's bug values, put the
camera under the water level, run the system, and assert the uniform holds a
real number. Both of the last two fail with `NaN` if the guard is removed.
