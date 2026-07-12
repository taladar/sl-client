---
id: viewer-p19-2
title: Pipeline status API (library)
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 19 — Diagnostics HUD (FPS + pipeline status)
---

Context: [context/viewer.md](../context/viewer.md).

**P19.2. Pipeline status API (library).** The stores have no public
introspection today (only per-request `TextureProgress` / `MeshProgress`).
Add a public stats snapshot to `TextureStore` / `MeshStore` / `AssetStore`
and `sl-asset-sched`'s `PriorityGate`: counts by state (queued /
reading-disk / downloading / decoding / ready / failed), in-memory entries,
cache hits, bytes, and GC'd entries — aggregated from the existing progress
enums. Cross-cutting change across `sl-texture` / `sl-mesh` / `sl-asset` +
`sl-asset-sched`; wire it through both runtime crates. Reference:
`LLTextureFetch` / `LLMeshRepository` queue stats. **Done:** new
`sl-asset-sched` `stats` module with a shared domain-free `StoreStats`
(by-stage buckets + `in_memory` / `bytes` / `cache_hits` / `collected`) and a
`GateStats` (capacity / in-flight / waiting) with `PriorityGate::stats()`.
Each store gained a `stats()` (iterates its weak map, upgrades live entries,
buckets them by their own progress enum, sums an approximate in-memory byte
footprint) and a `gate_stats()`; new `cache_hits` / `collected` atomic
counters are bumped on a disk hit and in `sweep`. `StoreStats` / `GateStats`
re-exported once (via `sl_texture`) through both runtime crates. **Bug found
& fixed while wiring stats through the progress enums:** the texture/mesh
`get()` and `set_lod()` direct-fetch paths never published a terminal
`Ready` / `Failed`, leaving an entry's observable progress stuck at the
`Downloading` / `Decoding` it passed through (only the `request`/`drive`
path published terminal progress). Extracted a shared `publish()` helper so
every completion path leaves progress truthful. The `AssetStore` was
unaffected — its single `get()` already published `Ready` / `Failed`.
