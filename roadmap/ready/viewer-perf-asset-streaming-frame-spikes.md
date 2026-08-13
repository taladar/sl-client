---
id: viewer-perf-asset-streaming-frame-spikes
title: Budget asset upload/prep to kill single-digit-FPS streaming spikes
topic: viewer
status: ready
origin: GPU-avatar Phase 4 perf capture (2026-08-13)
refs: [viewer-perf-gpu-avatar-phase4-remove-scaffolding, viewer-perf-render-app-bound-frame]
---

Context: [context/viewer.md](../context/viewer.md).

Post-Phase-4, the frame's single-digit-FPS **outliers** are asset-streaming
hitches, not avatar work: a batch of freshly-streamed textures/meshes lands
and is uploaded/prepared in **one** frame, stalling the render thread. From a
27.6 s aditi capture (Main spiking to 585 ms, Render to 490 ms), the offenders
by single-instance max:

- `apply_prim_textures` — **max 201 ms** (mean 1.3 ms) — applying arrived prim
  textures.
- `bevy_render::render_asset::prepare_assets<…texture…>` — **max 122 ms**.
- `bevy_render::render_asset::extract_render_asset<…>` — **max 131 ms**.
- `bevy_render::mesh::allocator::allocate_and_free_meshes` — **max 115 ms**.

All are mean-cheap, max-huge → a big upload landing all at once, not steady
cost.

## Direction: per-frame budgeting (amortize)

Spread the work across frames instead of draining the whole arrival queue in
one: cap how many textures/meshes are uploaded/prepared/applied per frame (a
byte or count budget), and carry the remainder to the next frame. Our own
`apply_prim_textures` is directly ours to budget; the Bevy
`prepare_assets`/`extract_render_asset`/mesh-allocator costs are driven by how
many assets we hand Bevy per frame, so throttling **our** ingestion (how many
decoded textures/meshes we insert into `Assets<…>` per frame) bounds them too.

Keep the ingestion **rate** high enough that the world still rezzes promptly —
budget the per-frame batch, do not slow the fetch (cf. the probe-cadence
lesson: throttle the per-frame work, never the acquisition cadence).

## Verify

Tracy capture flying into un-streamed areas: the 100–200 ms
`apply_prim_textures` / `prepare_assets` / allocator spikes flatten into
bounded per-frame slices; no single-digit-FPS frames from streaming; total
rez time not materially worse. Log what got deferred so a silent under-budget
is visible.
