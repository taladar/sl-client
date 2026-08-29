---
id: viewer-water-surface-alpha-not-refraction
title: The water surface fakes refraction with alpha, so the sea cannot match the reference
topic: viewer
status: bugs
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
