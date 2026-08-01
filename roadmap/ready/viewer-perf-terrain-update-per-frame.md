---
id: viewer-perf-terrain-update-per-frame
title: Dirty-gate terrain::update_terrain (runs every frame)
topic: viewer
status: ready
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
