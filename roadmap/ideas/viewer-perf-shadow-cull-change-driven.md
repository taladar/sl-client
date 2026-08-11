---
id: viewer-perf-shadow-cull-change-driven
title: Change-driven sun shadow-caster cull (idle static scene = zero re-test)
topic: viewer
status: ideas
origin: shadow-cull amortisation session (2026-08-11)
refs:
  - viewer-perf-pbr-shadow-cluster-rez
  - viewer-perf-probe-scheduling
  - viewer-perf-probe-capture-shadows
---

Context: [context/viewer.md](../context/viewer.md).

Follow-up to the round-robin cull in [[viewer-perf-pbr-shadow-cluster-rez]].
That lever spreads the static casters' frustum test over `stride` frames, but
even at `stride = 60` a still, fully-rezzed scene keeps re-testing `1/stride`
of the casters **every frame for no reason** — nothing has moved. The elegant
end state is **change-driven**, the same model as
[[viewer-perf-probe-scheduling]]: on a parked scene with a static sun, the
static caster re-test drops to **zero**.

Key insight that makes aggressive laziness safe (do not re-litigate): we only
ever stale the **frustum-culling decision** (the per-cascade include-list),
never the shadow itself — the shadow map is re-rendered from the casters'
**live** transforms every frame. So a caster with a stale include-list still
casts a correct shadow at its current position; the only artifact is a brief
missing/extra contribution when a caster **crosses a cascade boundary or the
edge of the shadowed range**. That is minor and transient, and a slightly-late
shadow is vastly better than the shadows-off users fall back to.

Design:

- Dirty the static caster set only when the cascade frusta actually shift
  enough to matter: the sun angle steps (already texel-snapped, so it changes
  in discrete jumps — hook that), or the camera moves/turns past a threshold
  (the cascades are camera-fit). Below the threshold, re-test nothing.
- Keep the cheap **change-detection fast path** for individual moved / spawned
  casters (already in place) — those are few and must stay responsive at
  cascade boundaries.
- The per-frame floor that remains is then just the `ViewVisibility` marking +
  the (unchanged) cascade lists; removing *that* from the critical path is the
  separate double-buffer/background task.

Acceptance: on a parked, rezzed scene with the day cycle paused, the
`SL_VIEWER_LOG_SHADOW_CULL` readout shows `tested ~= 0` per frame (vs
`total/stride` today); nudging the camera or stepping the sun re-tests the
affected casters within a frame; no visible shadow regression walking a moving
avatar past buildings.
