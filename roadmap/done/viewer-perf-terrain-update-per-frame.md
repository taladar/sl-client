---
id: viewer-perf-terrain-update-per-frame
title: Dirty-gate terrain::update_terrain (runs every frame)
topic: viewer
status: done
origin: Tracy profiling of Aditi rezzing (2026-07-30)
---

Context: [context/viewer.md](../context/viewer.md).

Tracy self-time over the first ~10 s of rezzing on Aditi shows
`sl_client_bevy_viewer::terrain::update_terrain` at **1.06 ms/frame**, running
once per frame (n=204) — a top-5 viewer-owned system.

During rez the terrain genuinely changes (land patches arrive, the mesh is
rebuilt), so some cost is expected. The task is to confirm the system **no-ops
cheaply when the terrain has not changed** rather than re-doing work every
frame:

- Check whether `update_terrain` early-returns when no new land patches / no
  revision bump since last run, or whether it re-scans / re-uploads
  unconditionally.
- If it rebuilds meshes or re-uploads textures each frame, gate it on the
  terrain revision (the minimap already tracks a `map_revision()` for the same
  data) so a stationary, fully-rezzed region costs ~0.

Verify with a ≤10 s `tracy-capture` capture while stationary and fully rezzed:
`update_terrain` self-time should fall to near-zero when nothing changes.

## Done (2026-08-10, `performance` branch)

Findings + fix:

- **The idle path was already event-driven.** `update_terrain` only acts on
  `TerrainPatch` / `RegionInfoHandshake` / `TextureDecoded` messages; with
  the readers empty it does nothing, so the 1.06 ms/frame during rezzing
  was real (patches genuinely arriving), not a missing dirty gate.
- **The real problem was the per-frame burst shape**: each arriving patch
  also rebuilt up to three neighbour patch meshes to close seams
  (constantly re-rebuilding the same patches during a stream), and a
  `RegionInfoHandshake` rebuilt every rendered patch of the region (up to
  16×16 = 256 mesh builds) in one frame.
- Fix: a fresh patch still builds inline (its own latency matters), but
  the neighbour-seam and whole-region rebuilds now queue into a **deduped**
  `PendingPatchRebuilds` and `drain_patch_rebuilds` runs at most
  `SL_VIEWER_TERRAIN_REBUILD_BUDGET` (default 8) of them per frame,
  chained after `update_terrain`. A rebuild reads state current at drain
  time, so deferral is never stale; vanished patches drain free; the dedup
  collapses the re-queued seam neighbours to one rebuild each.

The stationary-idle Tracy re-measure remains worthwhile but the
unconditional-work suspicion is answered (event-driven confirmed).
