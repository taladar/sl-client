---
id: viewer-underwater-alpha-fog-remaining-materials
title: The particle billboards do not carry the water fog
topic: viewer
status: ready
origin: split from viewer-underwater-fog-swallows-translucency (2026-09-09)
refs: [viewer-underwater-fog-swallows-translucency]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-underwater-fog-swallows-translucency]] moved the water-haze pass to
where the reference runs it — over the opaque scene, **before** the alpha pools
— and gave the surfaces that needed it their own per-fragment fog:
`FaceMaterial` (so every prim / mesh / avatar / attachment / editor overlay face
fogs itself) and the water surface's underside. The sky backdrops needed no
shader change at all in the end: they are drawn **before** the haze now, as the
reference draws its WL sky pool, so the haze fogs them through the empty depth
they leave.

One translucent draw is still unfogged: **the particle billboards**
(`sl_viewer_kit::particle_render`). The reference fogs them — its particles are
in the alpha pool, and `alphaF.glsl` applies `applySkyAndWaterFog` per fragment
— and drawing them early (the backdrops' answer) would not do here: a particle
is alpha content that has to blend over the scene at its own depth, so fogging
it from the depth buffer would measure it by whatever stands behind it, which is
the bug this all started as.

## Shape of a fix

The shared arithmetic is already there: `sl_client_bevy::water_fog`
(`water_fog.wgsl`). A consumer needs the fog colour, the two densities and the
water level, and derives `KS` from the scene's directional light.

The particle renderer is not a material but a custom instanced pipeline, whose
per-cloud `@group(3)` bind group carries the diffuse texture and sampler. So:

- the fog values come from a small resource `sl-viewer-kit` owns and
  `sl-viewer-world-scene` fills from its `WaterFogSettings` (the dependency runs
  that way), extracted to the render world;
- they join the `@group(3)` bind group as a uniform. That bind group is cached
  per cloud and rebuilt when its texture changes — it has to be rebuilt when the
  fog changes too, or a cloud keeps the fog it was born with;
- `particle.wgsl` then applies it exactly as `face_material.wgsl` does: for a
  fragment below the surface, `apply_water_fog(colour, water_fog_no_clip(eye,
  world_pos, params))`, leaving the coverage alone.

Deliberately **not** on this list: the name tags, the parcel borders and the
beacons. The reference does not water-fog its HUD text or its debug overlays
either; they were only being fogged before because a fullscreen pass cannot tell
them apart from the world.
