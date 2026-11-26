---
id: viewer-underwater-alpha-fog-remaining-materials
title: The particle billboards do not carry the water fog
topic: viewer
status: done
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

## Done (2026-11-26)

`particle.wesl` fogs a billboard under the surface per fragment, with the same
arithmetic, gates and inputs as `face_material.wesl`'s translucent branch: the
shared `water_fog_no_clip` / `apply_water_fog`, nothing above the surface, no
zero density, nothing in a view without a directional light (the HUD), and
`KS` from that light. Every blend is fogged alike, as the reference's alpha
pool fogs it (`applySkyAndWaterFog` in `fullbrightF.glsl` / `alphaF.glsl`). The
radiance goes through the fog and the coverage or glow alpha is left alone.

The values travel as planned, with one change. `sl_viewer_kit::particle_render`
owns `ParticleWaterFog`, and `sl-viewer-world-scene`'s `WaterFogPlugin` mirrors
its `WaterFogSettings` into it on change (`mirror_water_fog_to_particles`). It
is extracted to the render world and written into **one** uniform buffer that
every cloud's `@group(3)` binds at binding 2. The planned per-cloud copy is not
used. The shared buffer is created once and rewritten in place, so a fog change
reaches every live cloud without rebuilding any bind group. A bind group still
records the buffer it was built over and is rebuilt if that buffer is ever
replaced.

Tests: `a_particle_under_the_water_is_fogged` (GPU readback: a white unlit
billboard 2.5 m under the surface reads dark, an identical one over it white),
`the_particles_get_the_same_fog_and_a_settled_scene_stops_writing`,
`the_fog_packs_as_the_shader_reads_it` and `the_default_fog_has_no_density`.

Found on the way, filed separately:
[[viewer-particle-additive-ignores-source-factor]]. The additive pipeline's
`(One, One)` drops the system's source factor, so the fog's in-scatter is not
weighted by coverage there either until that is fixed.
