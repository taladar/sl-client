---
id: viewer-underwater-fog-swallows-translucency
title: Underwater, the fog wipes out translucent geometry that has open water behind it
topic: viewer
status: done
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

## FIX (2026-09-09): the haze fogs the opaque scene, translucency fogs itself

The faithful one, and the reference's own arrangement — which turned out to be
one line of its pipeline: `doWaterHaze()` is called *"do water haze against
depth buffer **before rendering alpha**"* (`pipeline.cpp`, the pool loop), in
**both** eye states (its `above_water` uniform is `±1` and only changes the
exclusion-mask discard and a manual depth test). Its alpha pools then carry the
fog per fragment — `applySkyAndWaterFog` in `alphaF.glsl`, `pbralphaF.glsl`,
`fullbrightF.glsl` and the gltf PBR shader — and its underwater surface shader
carries it too (`underWaterF.glsl`'s
`fb = applyWaterFogViewLinearNoClip(vary_position, fb)`).

So:

- **One haze pass, straight after the opaque pass** — the two eye-state
  pipelines are gone (they only ever differed in *where in the frame* they ran;
  the arithmetic already worked from either side, since which side the eye is on
  changes only where the view ray enters the water). It now runs before the
  pre-water translucency, before the water surface, and before the transparent
  phase, which is where the reference runs it.
- **The fog arithmetic is a shared shader module**,
  `sl_client_bevy::water_fog` (`water_fog.wgsl`), because it now has more than
  one caller and a seam where a translucent surface meets the opaque scene
  behind it would show any difference between two copies of it. The water-plane
  **clip** deliberately stays with each caller: the haze pass reconstructs its
  position from a depth buffer and needs a distance-scaled tolerance, a material
  has the fragment's exact position and needs none.
- **`FaceMaterial` fogs itself** when it is drawn blended — prims, meshes,
  sculpts, avatars, attachments *and the editor overlays*, since the selection
  silhouette is a `FaceMaterial` draw, which is what makes this the fix for the
  reported symptom. Opaque and alpha-masked faces are left to the haze pass
  (they are in the depth buffer it reads), so nothing is fogged twice.
- **The water surface fogs its own underside**, which it used to get from the
  submerged pass running after it. What that sample shows is the world *above*
  the water, which the haze pass deliberately leaves alone, so this is the one
  place that water column is applied.

### Getting the parameters into a material

The obstacle, and why the task read as more than a scheduling change: a Bevy
material shader can read the view bind group and its own material bind group and
nothing else — there is no per-view slot an application can add a uniform to. So
the fog has to travel *in* the material, and a per-frame write to every face
material in a region is a re-prepared bind group per face per frame.

It is affordable because almost nothing in it moves:

- the colour, the two densities and the water level change when the region or
  its environment does — `water_fog::apply_water_fog_to_face_materials` sweeps
  the assets then, and otherwise only *checks* the materials some other system
  touched (a check, not a write, so its own `Modified` event does not feed
  back);
- **both** densities are carried, and the shader picks by the view position, so
  a camera bobbing through the waterline rewrites nothing;
- and `KS` — the one value that follows the sun all day — is not carried at all:
  the shader derives it from the scene's directional light, which the viewer
  aims at whichever heavenly body is up, the same quantity the CPU takes from
  the sky settings' sun / moon rotation.

That last one also gates the effect correctly for the **HUD**: a view with no
directional light is the HUD layer, whose faces sit in their own patch of world
space near the origin and would otherwise be "under" a sea at 20 m.

### What the screenshot pair caught: the clouds

The first submerged A/B (local grid, camera at 12 m under a 20 m sea, looking
west) came back all but identical to the baseline — except for a line of bright
white wisps along the horizon that the old build did not have. The **sky
backdrops**: the cloud dome, the star field and the discs are alpha draws, so
with the haze moved ahead of the transparent phase they were now painted *over*
the fog instead of being erased by it.

The fix is the reference's own arrangement again, and it needed no shader: the
reference draws its whole WL sky pool in the deferred stage, *before*
`doWaterHaze()`. So `transparency` grew a second early pass,
`sky_backdrop_pass_3d`, drawing the backdrop bucket between the opaque pass and
the haze. A backdrop writes no depth, so the pixel it paints still reads as
**empty** depth — which the haze measures out to the camera's far clip, four
kilometres of water. That is exactly how the reference makes clouds disappear
under the sea, and above water nothing changes, because there the far-clip point
of a backdrop pixel is above the surface and the haze's clip rejects it.

It also puts the backdrops *behind* the pre-water translucency rather than over
it, which is the reference's draw order (sky pool, haze, alpha pre-water, water,
alpha post-water) and strictly better than what the bucket order gave before.

### Still not fogged, and tracked separately

The particle billboards — a custom instanced pipeline whose per-cloud bind group
would have to carry the uniform; see
[[viewer-underwater-alpha-fog-remaining-materials]]. They cannot take the
backdrops' answer: a particle is alpha content that has to blend at its own
depth, so drawing it early would fog it by what stands behind it, which is the
bug this started as. The name tags, parcel borders and beacons are deliberately
**not** in that list: the reference does not water-fog its HUD text or its debug
overlays either, and they were only being fogged here because a fullscreen pass
cannot tell them apart.

## Verified on the local grid (2026-09-09)

Headless first, with the camera parked 8 m under a 20 m sea looking west into
open water: the same pose against the previous build is where the cloud wisps
turned up, and the pair after the backdrop pass shows the surface's underside
carrying the sky through it, fogging into the distance, with nothing hanging in
the fog band. An above-water pair over the same sea is unchanged (the only
pixels that differ are the cloud dome's own scroll between the two runs).

Then interactively, by the user: the selection outline on a submerged prim with
open water behind it is drawn.
