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

## Re-capture 2026-08-15 (aditi) — outlier anatomy confirmed + physics co-spiker

Full-session trace `tracy-captures/aditi-2026-08-15.tracy` (release,
`profile-tracy`, 1890 frames / 2:18, clean disconnect). 28 frames were
compositor-throttled (~954 ms `present`, all in the first ~30 s while the window
was unfocused during login) and are excluded. Of the visible frames, the
non-occluded outliers fall in the rez / camera-move window and split into:

- **Rez / scene-mutation storm** (t≈51–54 s, frames to 380 ms):
  `ExtractSchedule` 157 ms, `Update` 202 ms, `allocate_and_free_meshes` 15–36 ms
  — a batch of prims landing, GPU mesh buffers rebuilding, and render entities
  extracted in one frame. Same class as the 2026-08-13 finding above.
- **Physics collider churn co-spikes with it** — `build_static_colliders` 30 ms
  in the same storm, and `RunFixedMainLoop` 40–95 ms (t≈53–54 s) as avian
  re-optimizes its collider-tree over the churning static set. This is **not**
  the dynamics solver (idle); it is avian's spatial-index maintenance on the hot
  path, root-caused in [[viewer-perf-custom-static-raycast-index]], which
  retires it. Tracked there, not here.
- **Render-only specialization spikes** (t≈28–33 s, `render_system` 130–283 ms)
  right after the window became visible — first-draw pipeline specialization
  ([[viewer-perf-pipeline-specialization-stalls]] / pre-warm).

So the asset-streaming budgeting this task proposes still stands for the texture
/ mesh landing spikes; the physics collider co-spiker and the fixed-timestep
catch-up multiplication are handled by the custom-index task.

## Verify

Tracy capture flying into un-streamed areas: the 100–200 ms
`apply_prim_textures` / `prepare_assets` / allocator spikes flatten into
bounded per-frame slices; no single-digit-FPS frames from streaming; total
rez time not materially worse. Log what got deferred so a silent under-budget
is visible.
