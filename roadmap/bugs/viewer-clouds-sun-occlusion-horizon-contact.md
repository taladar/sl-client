---
id: viewer-clouds-sun-occlusion-horizon-contact
title: Clouds wrong in front of the sun, and touch the water at the horizon
topic: viewer
status: bugs
origin: user report during the R18 aditi verification (2026-07-23)
refs: [viewer-r18]
---

Context: [context/viewer.md](../context/viewer.md).

With the R18 sRGB fix in (clouds spread over the whole sky on both grids),
two residual cloud divergences remain, both needing a Firestorm
side-by-side on the same sky:

- **Something is wrong where clouds cross the sun.** Suspects: the draw /
  blend order between the cloud dome and the sun disc (the reference draws
  the sun *behind* the clouds and lets the cloud alpha attenuate it,
  `lldrawpoolwlsky` order); the sun glow term inside the cloud shader
  (`cloudsF.glsl` folds the haze glow into the cloud colour — if our port
  applies it differently the crossing looks wrong); or additive blending
  of the disc showing through where it should be occluded.
- **Clouds touch the water in the distance.** The reference dome's shallow
  zenith cap wraps down (the `DOME_OFFSET` camera height) so clouds do
  reach *toward* the horizon, but check against Firestorm whether they
  should visually meet the waterline or fade out above it — candidates:
  the `altitude_blend_factor` fade at the cap edge (droop clamp), the
  cap's lower rim sitting below the horizon at our camera heights, or fog
  over water hiding the reference's rim where ours shows it.

Pin each with the A/B env toggles (`SL_VIEWER_LOG_CLOUDS`, the fixed
`--camera-position` horizon viewpoint from the R21 water work) before
changing the port — R18 earned its "verified faithful" list the hard way.
