---
id: viewer-water-surface-fog-fallback-flat
title: The water surface's deep-water colour is a flat tint, not the fogged one
topic: viewer
status: bugs
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
