---
id: viewer-water-refraction-smears-avatar-silhouette
title: Water behind an avatar smears a skin-coloured fringe around its silhouette
topic: viewer
status: done
origin: seen on aditi while live-checking the water-clip skin fix (2026-09-07)
points: 5
refs: [viewer-water-surface-alpha-not-refraction, viewer-water-exclusion,
  viewer-water-transparency-scene-matrix]
---

Context: [context/viewer.md](../context/viewer.md).

Wherever open water sat **behind** an avatar, a ragged skin-coloured fringe
hugged the avatar's silhouette — along the arms and shoulders most visibly. It
followed the outline exactly, was the colour of whatever the avatar's edge
pixels were, and appeared against water only: the same silhouette against
terrain or sky was clean. The distance to the water did not matter, which is the
tell that it was neither geometry nor the water surface being in the wrong
place.

## The mechanism

`sl-client-bevy/src/water.wgsl` refracts by sampling Bevy's
`view_transmission_texture` — a copy of the screen holding the opaque scene — at
a UV the wave normal displaces. The avatar is opaque, so it is *in* that copy,
and a water fragment just outside the silhouette samples a texel from *inside*
it. The displacement is a screen-space offset, so it does not care how far away
the water is; only that the avatar is adjacent on screen.

The reference does not have it because it rejects exactly that sample
(`class3/environment/waterF.glsl`):

```text
depth  = texture(depthMap, distort2).r;
refPos = getPositionWithNDC(vec3(distort2 * 2.0 - vec2(1.0), depth * 2.0 - 1.0));
if (pos.z < refPos.z - 0.05) { distort2 = distort; }
```

Its view space has −Z forward, so `pos.z < refPos.z` reads "the surface is
further away than what I sampled" — the texel is in front of the water — and the
undistorted sample is used instead. (The `SHORELINE_FADE` half of that block is
dead in the reference: `llviewershadermgr.cpp` defines `TRANSPARENT_WATER` and
never `SHORELINE_FADE`. Not ported.)

## The fix

`water.wgsl` now makes that test, and a new `water_scene_depth` module in
`sl-viewer-world-scene` supplies the depth buffer it needs.

Bevy hands a material no scene depth: its own transmissive shading reads
`depth_prepass_texture`, which exists only under a `DepthPrepass` — and this
viewer deliberately has none, because a prepass builds depth pipelines for the
custom sky / terrain / water materials whose `specialize` pins bespoke vertex
layouts (the reason recorded in `underwater_fog.rs`), and would re-submit every
opaque draw for a second geometry pass on top.

So the depth is **copied, not re-rendered**: one `copy_texture_to_texture` from
the view's own `ViewDepthTexture` into an `Image` the shared `WaterMaterial`
binds (`WaterMaterial::scene_depth`, a multisampled `Depth32Float` texture read
with `textureLoad`), issued in the `Core3d` schedule after the pre-water
translucency pass and before Bevy's transmissive pass — the same seam
`view_transmission_texture` is filled at, so colour and depth are the same
instant of the frame. That is the trick Bevy's own prepass node uses to make its
depth sampleable, minus the geometry pass. It also means the depth holds
*everything* drawn by then — terrain, prims, avatars — not only the materials a
prepass would have accepted.

Two consequences worth knowing:

- The main camera's `depth_texture_usages` gained `COPY_SRC`. Bevy adds that
  itself only for a camera carrying a `DepthPrepass`, so without it the copy is
  a validation error.
- **The size is the gate, at both ends.** The copy is skipped unless source and
  destination agree on size and sample count (they briefly do not while a window
  resize works through), and the shader uses the bound depth only when it
  measures the same as the view being shaded. The second check is what makes a
  *shared* material bind group safe for the views this pass does not serve — a
  reflection-probe capture, and an offline fixture scene that never leaves the
  `1×1` placeholder — since both then decline to read a depth buffer that is not
  theirs. Failing either check costs one un-rejected refraction sample, never a
  wrong pixel.

## Verified

- Unit (`sl-viewer-world-scene`): the copy destination matches a view depth
  texture (format, sample count, mip count, usages, no pixel data); a zero-sized
  target still yields a legal texture; the first frame with a camera sizes the
  target to the view, binds it into the material and publishes it for the render
  world; and a target that already matches does not touch the material (a
  `get_mut` every frame would rebuild the bind group every frame).
- GPU (`render_readback`, 11 tests): the water scenes still render — the shader
  compiles with the new binding on a real adapter, and the sea still shows the
  red slab behind it, which is the assertion that the screen copy is sampled at
  all. Those scenes wear the `1×1` placeholder, so they are also the check that
  a view this pass does not serve is unaffected.
- Live (aditi, 2026-09-08): logged in and rendered with no wgpu validation error
  — the copy is legal, so `COPY_SRC` is present and the sizes and sample counts
  agree — and the fringe was gone by eye against open water.
