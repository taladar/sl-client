---
id: viewer-underwater-fog-background-flicker
title: Background flickers behind the underwater fog while walking underwater
topic: viewer
status: done
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

## Fixed (2026-08-29)

Root cause: `update_underwater_fog` built `world_from_clip` from a **frame-old**
camera pose while the depth buffer it unprojects is rendered from the current
one, so every fogged fragment was reconstructed off by exactly the frame's
camera motion — nothing while parked, growing with speed. That is the shape of
this report ("while walking", "as the camera moves along the bottom"): not an
unstable input, a reprojection error proportional to how fast the camera is
going.

It was not the fog pass compositing an unstable background, which is what the
`SL_VIEWER_DISABLE_UNDERWATER_FOG=1` A/B above was written to distinguish — the
fog pass was sampling a *correct* depth buffer through a stale matrix.

Fixed under
[`viewer-audit-stale-globaltransform-readers`](viewer-audit-stale-globaltransform-readers.md),
which found it by static audit along with three sibling readers: the camera's
`GlobalTransform` is only recomputed by propagation in `PostUpdate`, so
`.after(WorldPhase::CameraPositioned)` buys ordering and not freshness. The
system reads the camera's `Transform` now.

### Verified

Walking underwater with the camera moving, on **both** grids — a local OpenSim
session and an aditi session — shows no flicker. Both were checked because the
origin line above names the local grid but the sighting was remembered as
aditi, whose water is a real region EEP setting over denser content.
