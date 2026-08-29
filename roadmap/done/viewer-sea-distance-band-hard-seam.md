---
id: viewer-sea-distance-band-hard-seam
title: The open sea has a hard ring at a fixed distance, light beyond it and dark inside
topic: viewer
status: done
origin: user report while verifying viewer-nametags-occluded-by-clouds (2026-08-29)
refs: [viewer-nametags-occluded-by-clouds, viewer-sea-grid-edge-visible-from-height]
---

Context: [context/viewer.md](../context/viewer.md).

Looking out over open water, the sea was split into two visibly different
surfaces by a **hard, camera-centred boundary** at a fixed distance: everything
beyond it markedly lighter (close to the sky's own colour), everything inside it
the normal darker sea, with no gradient across the seam. Reported as "a visible
dome that dissects the infinite ocean into a part inside and outside it".

## What it was

The **above-water water-haze pass** (`underwater_fog.wgsl`, the
`WATER_HAZE_ABOVE` pipeline), not the water surface and not the sky.

That pass fogs each pixel by the column of water between the eye and whatever is
behind it, reading the depth buffer for "whatever is behind it". Where the depth
buffer is empty — open sky, or the void past a region edge, which over the sea
is *every* pixel, since the pass runs before the water surface is drawn —
reverse-Z's infinite far plane gives a point at infinity rather than a distance,
so the shader substituted a point **2048 m** down the view ray (after
`waterF.glsl:285`'s `viewVec*2048.0`).

2048 m is far shorter than the sea the viewer actually draws (17 region cells,
~4.4 km). So any ray shallow enough to meet the water surface *beyond* 2048 m
had its substitute sample still up in the air, was rejected by the per-fragment
water-plane clip, and came out **unfogged** — showing the raw sky the surface
refracts. The sea inside that radius was fogged. The boundary is the circle
where the substituted point crosses the surface, `asin(eye_height / 2048)` below
the horizon, and it is a step rather than a ramp because the water column grows
~40 m per pixel row there, saturating the transmittance within two or three
rows.

Everything in the original ticket's "what is established" list stands — it is
not the transparent-phase ordering, not a depth prepass, not the sea grid
running out — and the leading "a sky dome meets the water plane" hypothesis was
wrong: with the water discarded the region below the seam was neither the sky
dome, nor the clouds / stars / sun disc / terrain, nor the clear colour, and
`SL_VIEWER_DISABLE_UNDERWATER_FOG=1` made it vanish.

## The fix

Measure an empty pixel's water column out to the distance its empty depth
actually stands for — **the camera's own far clip** — instead of a fixed 2048 m.
`UnderwaterFog` carries `far_plane` for it (Bevy's reverse-Z perspective is
*infinite*, so the far plane is not in the projection matrix and the shader
cannot recover it from `world_from_clip`), filled from the camera's
`Projection::far()` and pinned by a unit test.

The crossing then lands at the edge of what the frame draws at all — where the
sea ends and the sky begins anyway — rather than in the middle of the ocean.
Verified on the local OpenSim with the screenshot harness from `30,128,60`
looking west over the void: the sea is now one uniform ocean from the viewpoint
to a clean horizon, where before a column down the frame stepped sky → flat
light band → sea in one sample.

## Two things worth keeping

- **A long capture eats live input, and that includes the camera.**
  `--camera-position` starts the camera in flycam, and `drive_flycam`
  integrates live SpaceNavigator axes and right-drag mouse motion onto it every
  frame, so a 30 s screenshot delay came out pitched ~30° down with the horizon
  off-frame and captures taken minutes apart framed different scenes — which
  silently invalidates any A/B across runs. Not a viewer defect: `poll_device`
  zeroes the axes and drains the evdev backlog whenever the primary window is
  unfocused, and the device rests at exactly 0 on all six axes; the screenshot
  window simply has focus. Keep such runs ≤10 s, and take frames that must
  share a camera from *one* run.
- **A time-gated shader diagnostic is the tool for this class of bug.** Gating a
  marker return on `globals.time` and taking several screenshot frames in one
  run gives several diagnostic images with an *identical* camera — which is what
  made it possible to map screen rows to world distance (paint the water by
  `floor(distance/1000)`, read the km bands off a column) and then attribute
  each band to a shader term.

The sea's finite grid is now visible at its outer edge from a camera high above
the water, which the unfogged band used to hide:
[[viewer-sea-grid-edge-visible-from-height]].
