---
id: viewer-perf-prim-texture-apply-burst
title: Throttle prim-texture application + GPU asset uploads during rez bursts
topic: viewer
status: done
origin: Tracy profiling of Aditi rezzing (2026-07-31, 2-min capture)
refs: [viewer-perf-texture-decode-cache, viewer-perf-write-on-change-uploads]
---

Context: [context/viewer.md](../context/viewer.md).

A full 2-minute Tracy capture of aditi rezzing (4511 frames) shows a burst of
texture / mesh GPU work when a batch of prim assets lands at once. Our own
`textures::apply_prim_textures` spikes to **72 ms** in a single frame — and,
crucially, unwrapping it shows the spikes are **clustered in one tight burst
(≈39–43 s)** rather than spread, i.e. one batch of prim textures arriving is
applied all in the same few frames. Alongside it, the Bevy-internal GPU upload
systems it drives spike together:

| System | peak | mean |
| --- | --- | --- |
| `textures::apply_prim_textures` (ours) | 72 ms | 0.17 ms |
| `render_asset::extract_render_asset<GpuImage>` | 85 ms | 0.24 ms |
| `mesh::allocator::allocate_and_free_meshes` | 50 ms | 0.26 ms |
| `render_asset::prepare_assets<GpuImage>` | 47 ms | 0.13 ms |
| `render_asset::extract_render_asset<RenderMesh>` | 22 ms | 0.06 ms |

Investigate:

- Whether `apply_prim_textures` can **cap how many prim faces it (re)textures
  per frame**, draining a queue over several frames instead of applying a whole
  arriving batch at once (the same spirit as the flexi-settle / LOD throttles).
- Whether the GPU image uploads can be **rate-limited per frame** — Bevy uploads
  every `GpuImage` that became ready this frame; a decode burst therefore
  uploads them all in one frame. A small per-frame upload budget spreads the
  cost. Relates to [[viewer-perf-texture-decode-cache]] (smoothing the decode
  side) and [[viewer-perf-write-on-change-uploads]] (not re-uploading unchanged
  data).
- Whether `allocate_and_free_meshes` churn can be reduced by batching mesh
  (de)allocation rather than reacting to each rezzed prim individually.

Measure the per-event max (not just mean) of these systems before/after with a
multi-minute capture during an active rez burst (the mean hides them; see
`book/src/tools/profiling.md` for the capture/export recipe).

## Done (2026-08-10, `performance` branch)

All three investigate items are covered by shipped mechanisms:

- **Prim faces per frame are capped**: `TextureApplyBudget` (textures.rs) —
  a per-frame image-build budget (default 6) + face re-prep budget
  (default 48) with deferred-overflow queues, spent by
  `apply_prim_textures` / `patch_parked_decoded_textures` and drained by
  `drain_deferred_face_textures` / `drain_lod_reuploads`.
- **GPU image uploads are rate-limited** by the same image budget: Bevy
  only uploads `Image`s we insert, and at most `image_per_frame` decoded
  textures become `Image`s per frame.
- **Mesh (de)allocation churn is bounded** by the spawn/apply budgets:
  `SpawnBudget` (object geometry builds), and this branch's
  `GeometryApplyBudget` ([[viewer-perf-decoded-geometry-budget]]) capping
  the decode-result mesh / sculpt / rigged builds — so
  `allocate_and_free_meshes` sees a budgeted trickle instead of a burst.

Still open (not blocking): the multi-minute Tracy before/after re-measure
of the per-event max distribution during an aditi rez burst.
