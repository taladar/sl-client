---
id: viewer-water-surface-fog-fallback-flat
title: The water surface's deep-water colour is a flat tint, not the fogged one
topic: viewer
status: done
origin: review of viewer-audit-underwater-fog-nan (2026-08-29)
points: 3
refs: [viewer-audit-underwater-fog-nan]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-client-bevy/src/water.wgsl:214` sets the water surface's refracted term to
the authored fog colour and nothing else:

```text
let fb = water.water_fog_color;
```

The comment above it calls that "exactly the reference's non-transparent-water
fallback `applyWaterFogViewLinear(_, white)`". It is not exactly that. The
reference (`class3/environment/waterF.glsl:285`) is

```text
vec4 fb = applyWaterFogViewLinear(viewVec*2048.0, vec4(1.0));
```

which runs white through `getWaterFogViewNoClip`
(`class1/environment/waterFogF.glsl:39`) at 2048 m along the view vector, so the
result is `srgb_to_linear(fogColor) * L + D`, where both `L` (in-scatter) and
`D` (transmittance) come from the water fog **density**, the fog **KS** term,
and the eye's distance to the water plane. The authored colour is only one of
four inputs, and the other three vary with the region's water settings and with
the viewing angle.

Consequences: the water reads the same whatever density a region authors, and it
does not darken toward grazing angles the way the reference's does. It also
leaves `WaterParams::water_fog_density` (`sl-client-bevy/src/water.rs:78`,
filled at `sl-viewer-world-scene/src/water.rs:550`) declared in the uniform
block and sampled by nothing — a dead field, which
[no-dead-code-forward-api](../context/viewer.md) says should either become
load-bearing or go.

The fog math is already ported once, in `underwater_fog.wgsl:96` — same `kd` /
`ks` / `F = 0.98` shape — so this is mostly a matter of sharing it and giving
`water.wgsl` the water plane and the KS term it currently lacks.

Two things not to lose in the port:

- The density the surface shader gets must be the **eye-state-modified** one
  (`modified_water_fog_density`, ported in
  [viewer-audit-underwater-fog-nan](../done/viewer-audit-underwater-fog-nan.md)):
  the reference feeds the water shader `getModifiedWaterFogDensity(underwater)`
  at `lldrawpoolwater.cpp:242`, and `water.rs:550` currently passes the raw
  frame value. Skipping the modified one would also reintroduce the `NaN`, since
  this path does have a `powf` once the density is live.
- The reference reaches this line only with **transparent water off**
  (`#else` of `TRANSPARENT_WATER`); with it on, `fb` is a sample of the
  refraction texture. Our surface has no refraction pass, so the fallback is the
  only path we have — worth saying so in the comment rather than leaving the
  next reader to wonder which branch is being reproduced.

Needs a live look on both grids (the local OpenSim serves legacy water, aditi a
real region EEP setting), since it changes how the water reads from above at
every angle.

## Fixed (2026-08-29)

`water.wgsl` grew `water_fog_fallback`, the reference's
`applyWaterFogViewLinear(viewVec * 2048.0, vec4(1.0))` re-derived for a
horizontal plane, and `fb` is now its result. The change of frame is exact
rather than approximate: the reference's modelview is rigid, so
`waterPlane.w` is the eye's signed height above the surface, `es` is `-view.y`,
and the plane-side test is a height comparison. Nothing new is bound —
`water_fog_density` was already in the uniform block (read by nothing), the KS
term comes from the `light_dir` already there, and the water height is the
shaded fragment's own, since the surface *is* the plane.

The density bound to it is now the eye-state-modified one
(`getModifiedWaterFogDensity`, `lldrawpoolwater.cpp:242`), which is what the
sibling task ported. It follows the camera, but as a step function of which side
of the surface it is on, so the material's compare-then-`get_mut` still does not
re-prepare per frame.

Two guards the reference does without, both against a NaN pixel: `t2` held away
from zero (a grazing ray drives the reference's unguarded divisor there), and
the in-scatter clamped from *below* before `pow` — a negative density can still
reach `pow` with a negative base here even after the density itself has been
rescued.

Same-function consistency: `underwater_fog.wgsl` is the other port of
`getWaterFogViewNoClip`, so it got the lower clamp too, and both now decode the
water fog colour through `srgb_to_linear` as the reference does inside that
function. The colour is authored in sRGB and was being mixed into a linear frame
raw, which is why this is a visible darkening on both grids and not only a
tidy-up.

### How much it is worth, measured

Worth writing down, because a live look could not tell: the reporter looked at
the sea on the local grid before and after and saw no difference.

In the readback rig (no sky, no probes, so `fb` is most of the pixel) the sea
moves from about `(0.13, 0.32, 0.40)` to `(0.08, 0.14, 0.20)` — roughly halved.
On the live grid, the same camera pose over open water either side of the
change:

```text
                    old (r,g,b)          new (r,g,b)
sky (control)       203, 234, 255        200, 234, 255
sea near horizon    132, 156, 178        132, 150, 167
sea near camera     122, 152, 183        120, 140, 166
```

So it is real and it is angle-dependent (the gap widens as the view steepens),
but it is a ~7-9% darkening rather than the halving the rig shows. The
difference between the two is our water's **alpha**: the surface is
alpha-blended over the scene as a stand-in for the refraction pass we do not
have, and the probe reflection sits on top, so `fb` reaches the frame diluted.
The reference's water is opaque and samples a copy of the screen instead.

Matching Firestorm's sea is the stated goal, and that dilution is what stands in
the way of it — while most of the pixel is not the water shader's output, no
correctness in the shading can close the gap. Filed as
[viewer-water-surface-alpha-not-refraction](../bugs/viewer-water-surface-alpha-not-refraction.md),
which is the larger of the two and the one to do next; this one is a
prerequisite of it either way, since the refraction path shades on top of the
same fog.

### Tests

A readback test, `the_water_fog_density_changes_what_is_drawn`: render the
`water-surface` scene, retune every water material's density, render again, and
require the frames to differ. The clock is frozen with a zero timestep so the
GPU-driven wave scroll cannot supply the difference. Against the old flat `fb`
it fails with **0 of 262144 bytes** differing, which is the bug stated exactly.
Plus a unit test that `water_params` binds the modified density to a submerged
eye and the frame's own to one above water.
