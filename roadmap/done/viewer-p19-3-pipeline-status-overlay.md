---
id: viewer-p19-3
title: Pipeline status overlay
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 19 — Diagnostics HUD (FPS + pipeline status)
---

Context: [context/viewer.md](../context/viewer.md).

**P19.3. Pipeline status overlay.** A key-toggled HUD panel rendering
P19.2's texture + mesh pipeline counts (queued / decoding / ready / cached),
so the LOD and priority work below can be watched live. **Done:** extended
`diagnostics.rs` with a second `bevy_ui` `Text` node pinned top-left (clear of
the top-right frame overlay and bottom-left chat), hidden by default and
toggled with `F3` (new `PipelineOverlayVisible` resource +
`PipelineStatusText` marker). While shown it is rewritten each frame from the
P19.2 snapshots:
`TextureManager` / `MeshManager` gained `stats()` / `gate_stats()` accessors
delegating to their stores, and the panel prints two lines per pipeline —
per-stage entry counts (queued / dl / dec / ready / fail) then the in-memory
count + approximate byte footprint, cumulative `cached` (disk-cache hits) / GC
counts, and the admission gate's in-flight/capacity/waiting. Byte footprint is
rendered as MiB via integer math (the workspace denies `as` casts). An
`SL_VIEWER_PIPELINE_OVERLAY` env var starts the panel visible so the offline
screenshot harness (which cannot press `F3`) can capture it. Verified live on
OpenSim: the panel reads e.g. `tex … cached 14 … gate 0/16 wait 0`. Reference:
Firestorm `LLTextureFetch` / `LLMeshRepository` queue stats.
