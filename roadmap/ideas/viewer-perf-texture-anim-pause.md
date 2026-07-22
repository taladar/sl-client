---
id: viewer-perf-texture-anim-pause
title: Pause off-view texture animations, resume phase-exact
topic: viewer
status: ideas
origin: performance survey of the implemented viewer (2026-07-22), user request
refs: [viewer-profiling, viewer-perf-write-on-change-uploads]
---

Context: [context/viewer.md](../context/viewer.md).

Two stacked problems in `drive_texture_animations`
(`texture_anim.rs:231-286`), plus a user-requested behaviour improvement
over the reference viewer.

## 1. Redundant uploads for stepped flipbooks

The system writes `material.uv_transform` through
`materials.get_mut(&material.0)` **every frame unconditionally**
(`texture_anim.rs:282`). `get_mut` marks the `StandardMaterial` changed,
so its GPU uniform re-uploads that frame even when the placement is
bit-identical — and the common `llSetTextureAnim` modes are *stepped*
flipbooks whose placement changes only a few times per second. Fix:
compute the new `Affine2` first and only touch `get_mut` when it differs
from the current `material.uv_transform`. Collapses stepped animations to
a handful of uploads per second; no-op for `SMOOTH` ones.

## 2. Off-view faces still driven — pause them, but resume better

The reference viewer stops animating out-of-view faces: in
`LLVOVolume::animateTextures` (`llvovolume.cpp:750`) the per-face texture
matrix update is skipped while `facep->getVirtualSize() <=
MIN_TEX_ANIM_SIZE`. But `getVirtualSize()` is updated **lazily** by the
render pipeline, so when a face comes back into view the animation
visibly resumes late — a frozen frame for a moment (user-observed
artifact). Its global clock keeps running, so phase is right once it
does resume; the flaw is the resume latency.

We can do the same culling with a strictly less noticeable resume,
because our design already separates clock from output: the per-object
clock is one accumulated `f32` (`TextureAnimationClock`,
`texture_anim.rs:250-266`) and the placement is a **pure function**
`animate(&anim, elapsed)` (`texture_anim.rs:~180-220`). So:

- Keep advancing the clock for every animated object (one float add —
  negligible; do NOT stop it, that is what keeps phase truthful).
- Skip the per-face placement computation and material write when the
  face is not in view — Bevy's `ViewVisibility` (recomputed every frame
  in `PostUpdate`, worst case one frame stale) — or when its projected
  pixel size is below a threshold, mirroring `MIN_TEX_ANIM_SIZE` so a
  sub-pixel distant face doesn't force uploads either.
- The frame visibility returns, `animate(anim, elapsed)` immediately
  yields the exactly-correct current cell/offset — resume within one
  frame, at the phase other observers see, no frozen-frame pop.

## Estimated impact

Medium; scales with the number of animated faces (clubs, signage, water
prims can carry dozens). Item 1 alone removes ~(animated faces × 60) −
(faces × step-rate) material uploads/sec; item 2 additionally removes
all placement math and uploads for the (typically large) off-screen
majority of a region's animated faces. Behaviour bonus: measurably
better resume latency than the reference (1 frame vs. lazy virtual-size
refresh).

Confidence: high — clock/placement split verified in our code, reference
mechanism and its lazy-resume cause verified in Firestorm.
