---
id: viewer-water-surface-alpha-not-refraction
title: The water surface fakes refraction with alpha, so the sea cannot match the reference
topic: viewer
status: done
origin: measured while fixing viewer-water-surface-fog-fallback-flat (2026-08-29)
points: 8
refs: [viewer-water-surface-fog-fallback-flat]
---

Context: [context/viewer.md](../context/viewer.md).

The reference draws water **opaque** — `LLDrawPoolWater::renderPostDeferred`
opens with `LLGLDisable blend(GL_BLEND)` (`lldrawpoolwater.cpp:145`) — and gets
everything you see *through* the sea by sampling a copy of the screen itself.
Before the pass it copies the deferred colour and depth buffers
(`lldrawpoolwater.cpp:116`, gated on `LLPipeline::sRenderTransparentWater`, via
`gCopyDepthProgram`), and `class3/environment/waterF.glsl` then does, under
`#ifdef TRANSPARENT_WATER`:

- `fb = texture(screenTex, distort2)` — the scene behind the water, sampled at a
  screen uv displaced by the wave normal, which is what makes the bottom ripple.
- `refPos = getPositionWithNDC(...)` off `depthMap`, for a shoreline `fade`
  (`(pos.z - refPos.z) / 10`) that softens where the water meets the land, and
  the `if (pos.z < refPos.z - 0.05) distort2 = distort` correction that stops
  the distorted sample reaching for something in *front* of the surface.

Ours does none of that. `sl-client-bevy/src/water.rs` sets `AlphaMode::Blend`
and `water.wgsl` ends on `alpha = 0.6 + reflect_amount * 0.4`, letting the
already-drawn sea floor show through by blending. That is a stand-in, and it was
an honest one while there was no screen texture to sample — but it is not what
the reference does, and it changes the picture in two ways that matter:

- **No distortion.** The bottom seen through our water is geometrically exact,
  where the reference's ripples with the wave normal.
- **Everything the surface computes arrives diluted.** Whatever colour the
  water shades itself is mixed 60-100% with whatever was behind it, rather than
  replacing it. Measured while porting the fog fallback: a change that halves
  the sea's colour in the readback rig (no sky, no probes, so `fb` is most of
  the pixel) moves the live sea by only 7-9%, and the alpha is where the rest
  went.

The second is why this is filed as blocking rather than cosmetic: **while the
surface is alpha-blended, no amount of correctness in the shading can make the
sea match Firestorm**, because most of the pixel is not the water shader's
output at all.

Also gone with it: the shoreline fade, which is the reference's answer to the
hard waterline our blend leaves.

## Where the pieces already are

- Bevy 0.19 prepares exactly this texture for its own transmissive materials:
  `view_transmission_texture` (`bevy_pbr` `mesh_view_bindings.wgsl:102`), a copy
  of the main pass taken before the `Transmissive3d` phase renders
  (`bevy_pbr/src/material.rs:1226`). Whether a custom `Material` can be put in
  that phase — and get the texture bound — is the first thing to establish.
- Failing that, the plumbing for a hand-written copy is in this workspace
  already: `underwater_fog.rs` samples the main colour target *and* the main
  depth texture from a pass of its own (the depth made sampleable through
  `Camera3d::depth_texture_usages`), and `water_exclusion.rs` runs a second
  camera into a screen-space mask the water material samples. A colour+depth
  copy after the opaque pass is the same shape as both.
- The water material would then become `AlphaMode::Opaque`, which is also what
  lets its depth write stop being a special case
  ([`transparency.rs`](../../sl-viewer-world-scene/src/transparency.rs)
  documents why the current depth-writing translucent surface needs care).

Verify by comparison, not by eye: Firestorm logs into the local OpenSim (see the
sl-client skill), so the same viewpoint at the same time of day can be captured
in both and diffed — which is the only way to answer "does the sea match" rather
than "does the sea look plausible".

## Findings (2026-08-29)

The Bevy mechanism is there and fits, up to one problem that is not Bevy's.

What fits: `Material::reads_view_transmission_texture()` is a trait method a
custom material may override (`bevy_pbr/src/material.rs:186`), and returning
`true` puts it in the `Transmissive3d` phase (`:1690`) — provided its
`alpha_mode` is not a blending one, since those are matched first. That phase's
pipeline is built on the **opaque** branch (`render/mesh.rs:3433`): `blend =
None`, `depth_write_enabled = true`, which is the reference's `LLGLDisable
blend(GL_BLEND)` plus its depth write, for free. `view_transmission_texture` and
its sampler are bindings 24/25 of view group 0, present unconditionally and
falling back to a zero image when the view has no transmission texture
(`render/mesh_view_bindings.rs:868`). `ScreenSpaceTransmissionPlugin` is already
in the app — `PbrPlugin` adds it (`bevy_pbr/src/lib.rs:238`). So the copy, the
pass, the phase, and the binding all exist.

What does not fit: **when** the copy is taken. `main_transmissive_pass_3d`
copies the main texture into the transmission texture at the start of its own
pass (`transmission/node.rs:75`), which runs after opaque and alpha-mask and
*before* `Transparent3d`. The reference copies inside the water pool's own
render (`lldrawpoolwater.cpp:116`), which is after `POOL_ALPHA_PRE_WATER` — so
its copy contains the **underwater translucent content**, and ours would not.

That matters because this viewer already solved the ordering that depends on it.
[`transparency.rs`](../../sl-viewer-world-scene/src/transparency.rs) re-sorts
`Transparent3d` into below-water → water → above-water buckets, a port of
`LLDrawPoolAlpha`'s pre/post-water split, so underwater particles draw before
the water and show through it. Make the water opaque and take the copy before
the `Transparent3d` phase, and those particles are drawn and then painted over,
with nothing in the copy to bring them back: they vanish under the sea rather
than being seen through it. Bevy's phase set gives no seam between "underwater
translucency" and "the water", and the split cannot be made per material (the
same particle and prim materials appear on both sides of the waterline), so
reusing `Transmissive3d` as the pre-water bucket is not open either.

So there are two shapes, and they differ by a lot more than effort:

- **Full port.** A pre-water translucent phase of our own plus our own copy pass
  between it and the water, which is `LLDrawPoolAlpha`'s split as real phases
  rather than the current re-sort. Faithful, and underwater translucency
  refracts as the reference's does. Costs a custom phase type with its own queue
  and draw function, and it replaces the re-sort machinery.
- **Bevy's texture.** Water opaque and transmissive, sampling the post-opaque
  copy. Gets the distortion, the shoreline fade, and — the point of this task —
  the surface's own shading arriving undiluted. Costs underwater translucent
  content seen from above the surface, which stops showing through the sea.

## Fixed (2026-08-29)

The water is opaque now, and what you see through it is a sample of the screen.

`WaterMaterial` returns `true` from `reads_view_transmission_texture` and
`AlphaMode::Opaque`, which puts it in Bevy's `Transmissive3d` phase: that phase
copies the main texture at the start of its own pass and then draws, which is
the shape of the reference's water pass (`lldrawpoolwater.cpp:116` copies the
deferred colour buffer, and `waterF.glsl:288` samples it as `screenTex`). Its
pipeline is built on the opaque branch — no blending, depth written — so
`LLGLDisable blend(GL_BLEND)` and the depth write come along for free.
`water.wgsl` samples that copy at the reference's `distort2`: the wave normal's
horizontal pair, scaled by `refScale` and by `1 / max(sqrt(dist), 1)` so a
ripple stays a constant size on screen. `refScale` is a new uniform field, bound
as the reference binds it (`scaleAbove` above the surface, `scaleBelow` under
it, `lldrawpoolwater.cpp:299`).

Getting the *right* thing into that copy is the rest of the work, and it is
where the shape of the frame changed.

### The frame

opaque → below-water translucency → haze → copy → water → above-water
translucency, with the haze moving to the other end when the eye is submerged.

- **Below-water translucency** now draws in a pass of its own
  (`transparency.rs`), between the opaque pass and the transmissive one — the
  reference's `POOL_ALPHA_PRE_WATER`. Without it, an opaque sea simply hides the
  particles under it: they are drawn, painted over, and absent from the copy.
  The bucket sort that used to order them within `Transparent3d` now marks where
  the below-water head of the phase ends, and the new pass renders that range.
- Those items are then **suppressed** rather than moved: their batch ranges are
  emptied, which is what `render_range` skips on, so Bevy's own transparent pass
  draws only the rest. Moving them out of the phase is not open — items are
  *retained* there across frames, so removing one drops it until its entity
  becomes visible again — and emptying a batch range needs no undoing, because
  the batching systems assign every item a fresh one every frame.
- The `WaterSurface` marker and the water sort bucket are gone with it: the
  water is not in that phase any more, so there is nothing to pin.

### Two things the reference does that we were not doing

**The haze is what colours the sea.** The surface shows a sample of the scene,
so the scene has to be fogged before the sample is taken; `underwater_fog.rs`
used to fog only when the eye was submerged (the R21 scope note), which left an
opaque sea refracting an unfogged sea floor — a sheet of glass. It now fogs from
either side of the surface. Two things fell out of doing that:

- A pixel with **no geometry at all** — the void past a region edge, where there
  is no sea floor — was never fogged, because the pass bailed on a zero depth.
  The reference's haze reads the far depth there and fogs it like anything else,
  which is what gives open ocean its colour; ours now takes a point 2048 m down
  the view ray instead, reverse-Z's far plane being a point at infinity. This
  was wrong before this task and is the larger half of why the sea looks right
  now.
- The pass **blends** rather than reading and rewriting. It has to: it runs
  inside the main pass now, the viewer renders at `Msaa::Sample4`, and the
  resolved texture a post-process reads is overwritten by the next pass's
  resolve — the fog was applied and then silently thrown away. The reference's
  own haze is a `(ONE, SOURCE_ALPHA)` blend for exactly this reason: the shader
  emits in-scatter as colour and transmittance as alpha, and the blender
  computes `dst * D + L`.

**The surface seen from below is a different shader.** The reference draws it
with `underWaterF.glsl`: no fresnel, no reflection, no specular, just the
refracted world above, fogged. Ported as a branch on the eye's side of the
surface, with the plain `refScale` displacement the reference uses there (no
distance falloff).

### Where the haze runs, and why it moved twice

Above water it must run **before** the water, or the copy is unfogged. Submerged
it must run **after everything**, because then the fog is not a backdrop but the
medium the whole picture is seen through — and the translucent content drawn
after the water is inside that picture. That was found by looking: with the haze
before the water in both states, the cloud dome (alpha-blended, so drawn after)
hung in the distance underwater as a bright unfogged band. One shader, an
`#ifdef` for the eye state, two pipelines, two placements; each does nothing in
the other's state.

The per-fragment water-plane clip also grew a tolerance that scales with
distance. The position it tests is reconstructed from a depth buffer, and the
thing most often sitting exactly on the plane is the water surface itself, whose
far pixels otherwise fall on either side of the test from one to the next and
break the fog up along the horizon.

### The seam the sea had all along

Reported while reviewing this: straight cuts across the water where the ripple
pattern does not line up, near the camera, over open water. Reproduced from a
low camera about three quarters of a region out
(`--camera-position 210,144,22 --camera-look-at 900,300,19`), and localised by
suppressing the per-region planes — the seam stayed, so it was inside a single
surface.

It was the endless ocean's own triangle diagonal. The plane was one 40 km quad,
and a fragment's world position is interpolated across a triangle whose `w`
ranges over four orders of magnitude; at the grazing angle the sea is nearly
always seen at, that interpolation loses enough precision for the two triangles
to disagree about where a fragment is, and the wave texcoords turn the
disagreement into a visible step. The quad's diagonal passes through the
camera's own position (the plane is centred on it), which is why the cut is
always in frame and always seems to start at your feet.

Fixed by subdividing the plane (`OCEAN_SUBDIVISIONS = 64`, ~625 m cells, a few
thousand triangles): each triangle then interpolates over a `w` range small
enough for the precision to hold. The same pose renders seamless afterwards. The
per-region planes are 256 m and left as single quads — the effect scales with
the triangle's `w` range, and theirs is three orders of magnitude smaller.

Worth re-checking against [[viewer-water-wave-phase-jumps-far-from-origin]]: the
same interpolation error moves as the camera moves, so some of the phase jumping
may be this rather than the texcoord precision that task blames, and may already
be gone.

### Tests

`the_sea_shows_what_is_behind_it` in the readback tier: the `water-surface`
scene gained a strongly red slab on the sea bed, and the check sweeps the whole
frame for pixels that only the slab can explain. It is a whole-frame sweep
rather than a projected point because the refraction *displaces* the sample, so
where the slab lands is exactly what must not be assumed. With the sample
removed it fails with zero red pixels — an opaque sea hides whatever is under
it. Plus a unit test that the eye state picks the reference's `scaleAbove` /
`scaleBelow`.

### Left open

- The reference's shoreline **fade** and its `pos.z < refPos.z` correction both
  need the *depth* of the refracted sample, which Bevy's transmission texture
  does not carry (the reference copies depth alongside colour). Without them a
  distorted sample can reach for something in front of the surface, and the
  waterline is harder than the reference's.
- Below-water translucency is fogged by the haze rather than by its own shader,
  which is where the reference does it. The end state is the same and it costs
  one pass instead of a fogged variant of every translucent material.
- The wave phase quantises far from the origin:
  [[viewer-water-wave-phase-jumps-far-from-origin]], found while reviewing this,
  but not caused by it.
