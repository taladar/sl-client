---
id: viewer-perf-decoded-geometry-budget
title: Budget decode-result geometry application (mesh / sculpt / rigged)
topic: viewer
status: done
origin: unbounded-frame-work survey (2026-08-09, performance branch)
refs: [viewer-perf-object-update-coalesce]
---

Context: [context/viewer.md](../context/viewer.md).

The decode-result apply side was unbounded even though `update_objects`'
spawn path is budgeted: `apply_object_meshes` and `apply_object_sculpts`
applied **all** `MeshDecoded` / `TextureDecoded` events in one frame — each
event scanning all tracked objects and building submeshes / sculpt faces
(GPU uploads) for every match — and `apply_rigged_attachments` bound every
pending skinned mesh in one frame. A cache-warm login (everything decodes
at once) is the worst case.

Fix: one shared `GeometryApplyBudget` (default 8 builds/frame, env
`SL_VIEWER_GEOMETRY_APPLY_BUDGET`), refilled by
`reset_geometry_apply_budget` and spent by all three systems (chained /
ordered after the refill). Deliberately separate from `SpawnBudget` so a
decode burst cannot starve object spawning.

- Mesh / sculpt: decoded keys park in deduped queues
  (`PendingDecodedMeshes` / `PendingDecodedSculpts`) and drain per key;
  each `build_mesh_submeshes` / `build_sculpt_faces` call charges one unit.
  A key with more instances than the remaining budget still finishes (soft
  overrun) — per-key work is not resumable without duplicate LOD rebuilds.
- Rigged: builds spend from the same pool; the unbuilt rest stays pending
  and is re-collected next frame. The cheap not-ready retries (skeleton /
  finest-LOD still loading) stay free.

Design deviation from the original plan: no mesh-key→objects index. The
plan proposed one to kill the O(decoded × objects) scans, but budgeting at
key granularity already bounds the scans (≤ scan-cap of 64 keys/frame ×
one map scan), and the index's maintenance across every `pending` /
`mesh_rebuild` write site was the main bug surface (a missed site = a
permanently grey object). The sculpt drain also early-drops its whole
backlog with one scan when no sculpt build is pending at all (most decoded
textures are ordinary face textures).

Verify: Tracy per-event max of the three systems during a cache-warm login
into a mesh-heavy region; A/B via `SL_VIEWER_GEOMETRY_APPLY_BUDGET`; no
permanently-grey objects after a minute (deferred keys must all drain).
