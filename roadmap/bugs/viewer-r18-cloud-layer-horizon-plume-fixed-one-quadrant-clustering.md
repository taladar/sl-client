---
id: viewer-r18
title: Cloud layer — horizon plume fixed, one-quadrant clustering still broken
topic: viewer
status: bugs
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R18. Cloud layer — horizon plume fixed, one-quadrant clustering still
broken** (`sl-client-bevy` / `sl-client-bevy-viewer`, P22.4). Noticed while
verifying P23.1 water. Two distinct defects, one fixed, one **still open**:

- **(fixed) Vertical horizon plume.** The old port evaluated the cloud UV
  *per fragment* from the view direction over a **full sphere**; near the
  horizon that projection is degenerate (`base_uv ∝ (1−cos elev)`, quadratic),
  smearing the texture into a vertical plume. **Fix:** render clouds on a
  CPU replica of the reference `LLVOWLSky` dome — the `calcPhi` zenith cap
  (φ∈[0,π/8]) with the reference **baked** planar texcoords
  (`buildStripsBuffer`, `((-z0+1)/2,(-x0+1)/2)`), and the camera-height offset
  (`DOME_OFFSET × DOME_RADIUS` = `0.96×15000`) baked into the vertices so the
  shallow cap wraps down to fill the sky (the reference puts the camera high
  inside the dome). `clouds.wgsl` now samples the interpolated vertex UV.
- **(still open) Clouds cluster into ~one quadrant** with the other three
  near empty — on BOTH grids, not faithful (Firestorm spreads them evenly,
  reaching the horizon). Verified every checkable element of the port matches
  `class1/deferred/cloudsV` (the φ∈[0,π/8] dome, `calcPhi`, baked UV,
  `cloud_scale=0.4199`, the `0.96×15000` offset, the repeat sampler,
  `drawDome`→`mStripsVerts`) — there is only one cloud shader (no `class2`/
  `class3` variant; `LLVOClouds` gone), so the code path is right. Yet the
  dome projection maps the whole visible sky onto a tiny ~0.14-radius disc of
  the cloud texture (≈0.66 tile), so only 1–2 features show →
  one-sided. This mismatch with Firestorm's even clouds is **unexplained by
  source archaeology** and needs a same-grid Firestorm pixel comparison /
  runtime debugging. NOTE: a **separate confound was ruled out** — the EEP
  environment was not ingested on aditi at all (see R19), so aditi ran on
  WindLight defaults; with R19 fixed aditi now loads its real EEP and still
  shows the one-quadrant clustering, confirming a projection defect, not a
  settings problem. Candidate next step: the altitude-plane projection (sample
  the cloud texture where the view ray meets the cloud-altitude plane), which
  tiles evenly to the horizon — a deviation from the literal baked-UV formula
  but matches Firestorm's result. The `SL_VIEWER_LOG_CLOUDS` env var logs the
  live cloud EEP params + resolved texture id for comparison.
