---
id: viewer-clouds-sun-occlusion-horizon-contact
title: Clouds wrong in front of the sun, and touch the water at the horizon
topic: viewer
status: done
origin: user report during the R18 aditi verification (2026-07-23)
refs: [viewer-r18]
---

Context: [context/viewer.md](../context/viewer.md).

**Resolved 2026-08-03 (interactive aditi + local Firestorm side-by-side).** The
two reported divergences were **not** the draw-order / horizon-fade the ticket
guessed at — the cloud shaders, dome geometry, altitude fade, and draw order
were all confirmed faithful. The real causes were a sky **colour / HDR** gap and
its knock-on effects:

- **"Clouds wrong in front of the sun."** Clouds *do* correctly cross the sun
  (faithful). What looked wrong was that the
  **sun and whole sky were washed out and the sun read as a dull grey disc** —
  because our forward path fed the WL-sky colour straight into the linear tone
  mapper, skipping the reference's `srgb_to_linear` (`softenLight` SKIP_ATMOS).
  **Fix:** linearize the sky dome + cloud output (`sky.wgsl` / `clouds.wgsl`,
  `SL_VIEWER_SKY_LINEARIZE` A/B knob) — the sky is now saturated and matches
  Firestorm at noon / sunrise / sunset.
- **Sun glow / halo.** Added the reference `RenderGlow` **bloom** pass (Bevy
  screen-space bloom on the main camera, `bloom.rs`), so the sun (and glow /
  fullbright prims) get their soft halo. Fixed a whole-screen flicker from
  non-deterministic post-process ordering by pinning fog → bloom → tonemap.
- **Blue terrain (a knock-on the colour fix exposed).** The linearized sky made
  the reflection-probe env-map — which the terrain sampled for its ambient — too
  blue, cooling sun-shaded ground at dawn / dusk. **Fix:** re-lit the legacy
  terrain from the reference's atmospheric `amblit` / `sunlit` instead of the
  raw env-map irradiance (`terrain.wgsl` + `TerrainLighting` +
  `drive_terrain_lighting`) — warm dawn, green noon, matching Firestorm.

Also fixed two enablers found along the way: `--camera-look-at` was ignored for
fixed cameras (always faced north), and `SL_VIEWER_SKY_DAY_POSITION` did not
move the sun on a single-frame grid.

**Split out / follow-ups:**

- "Clouds touch the water at the horizon" →
  [[viewer-clouds-horizon-waterline-contact]] (could not reproduce; needs a
  Firestorm horizon A/B).
- Sun **disc** still greys on aditi EEP skies →
  [[viewer-sun-disc-grey-aditi-hdr-scale]] (needs `sky_hdr_scale` /
  `reflection_probe_ambiance`).
- Terrain fidelity polish → [[viewer-terrain-ambient-probe-classic-fidelity]].
- [[viewer-stars-srgb-linearize]] and
  [[viewer-debug-screenshot-offthread-save]].
