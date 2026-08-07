---
id: viewer-underwater-fog-background-flicker
title: Background flickers behind the underwater fog while walking underwater
topic: viewer
status: bugs
origin: observed while live-testing the teleport-handover fixes on local OpenSim (2026-08-07)
---

Context: [context/viewer.md](../context/viewer.md).

While walking underwater, the background **behind the underwater fog**
flickers. The fog itself renders; the artefact is the scene/background
*behind* it visibly flickering frame to frame as the camera moves along the
bottom.

Likely area:

- `sl-client-bevy-viewer/src/underwater_fog.rs` + `underwater_fog.wgsl` — the
  full-screen fog pass. Check how it composites over the background and what
  depth / background source it samples (a per-frame-varying or unstabilised
  input reads as flicker).
- Interaction with the sky/water background beneath the fog
  (`sky.rs` / `water.rs`) and the clear colour / far plane when submerged.

Diagnose with the debug knobs already in place:
`SL_VIEWER_DISABLE_UNDERWATER_FOG=1`
A/Bs the fog off — if the flicker persists with the fog disabled it is the
background itself (sky/water/clear), not the fog pass; if it disappears, the
fog pass is compositing an unstable background. Reproduce headlessly with the
absolute camera-pose CLI submerged (`--camera-position` below the waterline,
`--camera-look-at` roughly horizontal) plus the auto-spin/screenshot harness,
and compare consecutive frames' pixels rather than eyeballing the live window.

Not yet root-caused — filed as a follow-up so it does not block the
teleport-handover work.
