---
id: viewer-perf-async-asset-fetchers
title: Async (non-blocking) asset fetchers — lift the IoTaskPool 4-thread download ceiling
topic: viewer
status: ideas
origin: noticed while investigating stuck-LOD / F3 slot saturation (2026-08-11)
refs: [viewer-profiling]
---

Context: [context/viewer.md](../context/viewer.md).

Every viewer asset fetcher performs a **blocking** HTTP request on a Bevy
`IoTaskPool` thread, so each in-flight download monopolises a whole IO thread
for its entire round-trip. Bevy's default `IoTaskPool` policy is
`min_threads: 1, max_threads: 4, percent: 0.25` (`bevy_app`
`TaskPoolThreadAssignmentPolicy`), so
**at most ~4 downloads of any kind run concurrently**, no matter how much work
is queued. This is why the F3 pipeline overlay rarely shows the texture store's
16-slot admission gate saturated even with hundreds of textures queued: the
gate's 16 and the CPU decode permits (`num_cpus`) both sit *downstream* of a
4-wide fetch funnel. Requests that have been `spawn`ed but not yet scheduled
onto one of the 4 IO threads never even reach the gate — the store reports them
as `queued`.

The blocking pattern is shared by all fetchers, so the ceiling is global:

- `BevyTextureFetcher` (`sl-client-bevy/src/textures.rs`) — blocking `reqwest`.
- `BevyMeshFetcher` (`sl-client-bevy`, `GetMesh2`/`GetMesh`).
- avatar bakes (`bake_inputs.rs`), animations (`animations.rs`), GLTF materials,
  sounds (`sound_cache.rs`), environment assets (`environment_assets.rs`) — each
  fetches on the `IoTaskPool` the same way.

## Two directions

1. **Cheap stopgap:** raise the viewer's `IoTaskPool` `max_threads` via
   `TaskPoolOptions` in the plugin setup (e.g. to 16, matching the texture
   gate). Low effort, but it steals threads from the compute / async-compute
   pools and scales poorly — a thread per concurrent download is wasteful. Wants
   a measured decision, not a blind bump.
2. **Proper fix:** make the fetch layer **truly async** (non-blocking `reqwest`,
   or a small shared async runtime the fetchers drive), so a handful of threads
   can service all of the store's admitted concurrent requests. Then the
   store-side admission gates (texture 16, mesh gate) actually govern
   concurrency, the F3 overlay reflects real in-flight work, and download
   throughput stops being pinned at ~4. This is the preferred shape.

## Measurement

[[viewer-profiling]] — with the fetchers still blocking, F3 shows `dl ≈ 4`,
`gate in_flight` low, `waiting ≈ 0`, and a large `queued` backlog on a
texture-heavy region; after the change the gate should fill and `queued` drain
faster. Verify no regression in frame time (the render thread must never block
on a fetch) and that the compute pools are not starved.
