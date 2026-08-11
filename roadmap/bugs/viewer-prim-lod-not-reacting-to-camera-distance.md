---
id: viewer-prim-lod-not-reacting-to-camera-distance
title: Prim tessellation LoD does not react to camera distance
topic: viewer
status: bugs
origin: observed during shadow-cull profiling on aditi (2026-08-11)
refs: [viewer-profiling]
---

Context: [context/viewer.md](../context/viewer.md).

Observed on aditi: a plain (tessellated) prim keeps the same tessellation
level regardless of how near or far the camera is — walking up to or away
from a prim never coarsens or refines its geometry. The reference viewer
drops distant / small prims to a coarser `LLVolumeLODGroup` tier.

The machinery to do this already exists, so this is a **regression / wiring
bug in an implemented path**, not a missing feature:

- `render_priority.rs:237` computes each plain prim's target level with
  `PrimLod::for_distance(scale_length, distance, DEFAULT_LOD_FACTOR)` — the
  same tier selection as `LLVolumeLODGroup` — and records it in
  `PrimLodTargets`.
- `objects.rs::apply_prim_lod` (budgeted, alongside `apply_tree_lod`) is
  meant to re-tessellate a prim when its `PrimLodTargets` entry differs from
  the tracked `prim_lod`; prims start at `INITIAL_MANAGED_PRIM_LOD =
  PrimLod::Low`.

Something in that chain is not taking effect at runtime. Candidate causes to
check:

- whether `render_priority` actually writes changing targets as the camera
  moves (distance / `scale_length` / pixel-area inputs stale or wrong), or
  only ever writes the initial `Low`;
- whether `apply_prim_lod` is starved by its apply budget, or its
  change-guard (`tracked.prim_lod == desired`) never sees a new `desired`;
- whether the re-tessellation runs but the geometry-cache key
  (`GeometryKey` includes `lod: PrimLod`) or the spawned mesh is not swapped
  in, so the coarser/finer geometry never reaches the entity.

The user suspects the **mesh** LoD path (`MeshLod::for_distance` /
`MeshManager::set_lod_for_area`, driven from the same `render_priority`
system) may share the same root cause — verify both together, since they are
fed by the same per-object distance / pixel-area computation and a bug in
that shared input would break both. Sculpt LoD (also managed there) is worth
a glance for the same reason.

Verify with a live run: park in front of a prim, dolly the camera in/out,
and confirm (via the pick read-out — `objects.rs:2081` logs `lod=` on pick,
or a debug overlay) that the applied `prim_lod` tier changes with distance.
