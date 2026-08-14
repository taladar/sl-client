---
id: viewer-perf-avatar-mesh-lod-screen-size
title: Screen-size mesh LOD for rigged avatar meshes (on-screen crowd draw)
topic: viewer
status: ready
origin: GPU-avatar Phase 5 crowd measurement (2026-08-14)
refs: [viewer-perf-render-app-bound-frame, viewer-perf-gpu-avatar-phase5-lod-polish, viewer-avatar-impostors-billboard]
---

Context: [context/viewer.md](../context/viewer.md).

The `SL_VIEWER_CROWD=100` measurement showed that a large **on-screen** crowd of
full-mesh avatars is **GPU-draw bound** — 100 avatars × ~45 rigged submeshes,
all skinned and drawn every frame. Frustum culling
([[viewer-perf-gpu-avatar-phase5-lod-polish]]'s GPU-bounds knob) removes the
avatars **behind** you, but the ones **in front** — most of a dance-club crowd,
many of them small on screen — still draw at full mesh resolution.

We already do screen-size mesh LOD for **prims** (`render_priority.rs`, the
`LLVOVolume::calcLOD` tier + `PrimLodTargets`, P21.2), but **rigged avatar
meshes render at full LOD regardless of screen size**. SL mesh assets ship 4 LOD
levels (high / medium / low / lowest); the reference viewer picks per object by
screen-space size. Distant-but-visible crowd avatars are exactly the case where
a lower LOD is invisible to the eye but cuts vertex/draw cost sharply.

## Direction

Extend the existing prim LOD selection to worn rigged avatar submeshes: pick the
mesh LOD per avatar (or per submesh) by screen-space size (reuse `calcLOD`), and
swap the `Mesh3d` handle to the chosen LOD. Interacts with:

- **Instancing:** avatars sharing a body **and** a LOD level still batch; LOD
  buckets the instancing by (submesh, lod). Keep same-body same-LOD wearers in
  one instanced draw.
- **The dummy-joint skin / `GpuSkinBinding`:** a LOD swap changes the mesh but
  the canonical joint indices are the mesh's own `joint_names → body index`, so
  each LOD needs its `GpuSkinBinding.canonical` rebuilt from that LOD's skin
  (they can differ per LOD).
- **Frustum culling** (sibling knob): culling removes off-screen; this removes
  detail from far-on-screen. Complementary.
- **Impostors** ([[viewer-avatar-impostors-billboard]], deferred) remain the
  extreme-count fallback below the lowest mesh LOD; this task keeps real
  geometry viable much further into a crowd first.

## Verify

`SL_VIEWER_CROWD=100`, camera facing the crowd: distant copies drop to lower
mesh LOD (no perceptible pop at sensible thresholds), `render_system` /
`camera_driver` draw cost falls materially vs full-LOD, near copies stay crisp.
A `CROWD=0` vs `CROWD=100` A/B isolates the marginal draw saving.
