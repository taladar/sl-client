---
id: viewer-underwater-fog-swallows-translucency
title: Underwater, the fog wipes out translucent geometry that has open water behind it
topic: viewer
status: bugs
origin: user report while verifying the selection silhouette on the local grid (2026-09-08)
refs: [viewer-outline-swallows-thin-hollow-prim, viewer-object-selection-core]
---

Context: [context/viewer.md](../context/viewer.md).

Submerged, looking at an ordinary cube from an angle where the **void** (no
sea floor, no water surface) is behind it, the cube's selection outline is not
drawn at all. Against a sea floor, or against the water surface, the same
outline shows normally.

## Why

`sl_viewer_world_scene::underwater_fog` is a **fullscreen** pass that fogs the
composited image using the **main-pass depth buffer**
(`underwater_fog.wgsl`: `textureLoad(depth_texture, coord, 0)` → a world
position → transmittance and in-scatter). Submerged, it is scheduled *after*
`main_transparent_pass_3d`, deliberately, so that the fog is the medium the
whole picture is seen through — including the water surface and the cloud
dome.

An `AlphaMode::Blend` draw writes colour but **no depth**. So at a pixel where
a translucent surface is the only thing drawn, the fog does not read *its*
depth: it reads whatever opaque surface is behind it — and where that is the
void, the shader deliberately substitutes the camera's **far clip** (4096 m,
the fix from `viewer-sea-distance-band-hard-seam`). Four kilometres of water
transmits nothing, so the pixel is replaced by pure fog colour and the
translucent contribution is gone.

That is why it depends on the backdrop: a sea floor two metres behind the
cube's rim leaves the fog almost clear and the outline survives; open water
behind it erases it. The prediction that goes with this mechanism, worth
confirming when it is picked up: the sliver of outline that lies **over** the
cube's own surface should survive (that pixel has the cube's depth), and only
the part that stands out past the silhouette is lost.

Nothing about the outline is special here. **Every** alpha-blended surface
underwater is fogged by the distance of what is behind it rather than its own:
a transparent prim, a particle system, a glow-blended effect. The outline is
simply the first case anyone looked straight at, because it is drawn at the
rim of an object, which is exactly where the backdrop is far away.

## What the reference does

It does not fog translucency in the deferred pass at all. The alpha pool's own
shaders carry the water fog per fragment (`alphaF.glsl`'s `WATER_FOG` branch →
`applyWaterFogView…`), at the fragment's **own** position, and they are drawn
after the deferred haze. So a translucent surface underwater is fogged by how
far *it* is, not by how far the thing behind it is.

`underwater_fog`'s module documentation already names this gap for the
above-water half ("the pre-water translucency, which the reference fogs in its
alpha shaders and we do not yet"); submerged it is the same gap, and it is
much more visible, because submerged the fog is applied to everything rather
than only to what is under the surface.

## Shape of a fix

- The faithful one: give the alpha-blended materials their own water fog —
  `FaceMaterial` first (`face_material.wgsl` already reads a water level and a
  clip sign for the waterline split, so the plumbing for "which side am I on"
  exists), then the other alpha materials, and stop the submerged fullscreen
  pass from fogging what has already fogged itself. That needs the fog pass to
  run before the transparent phase, as the above-water one already does.
- The narrow one for **editor overlays only** (the selection outline, the
  face cursor): the reference draws its silhouettes after the deferred stage
  entirely, so they are never fogged. Drawing the overlays after the fog would
  match that — but it needs a render phase of their own, which is the same
  machinery [[viewer-selection-hidden-silhouette]] wants for its depth-inverted
  pass. Worth doing together.
