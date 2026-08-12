---
id: viewer-perf-hover-pick-raycast
title: Hover-tooltip world pick casts MeshRayCast over all meshes each dwelt frame
topic: viewer
status: ideas
origin: Tracy full-session aditi capture (2026-08-12)
refs: [viewer-perf-steady-state-46fps-ceiling, viewer-hover-tooltips, viewer-profiling]
---

Context: [context/viewer.md](../context/viewer.md).

## Finding

On the 2026-08-12 full-session aditi capture (release, `profile-tracy`,
2180 frames / 2:51, clean disconnect), `hover_tooltip::update_hover_tooltip`
is the **single largest system in the `Update` schedule** while the pointer
is in use:

- mean **5.88 ms/frame**, p50 **2.64 ms**, p90 **17.8 ms**, p99 33.2 ms,
  max 39.9 ms (runs on the main-app worker pool).

That is up from **1.93 ms** on the 2026-08-10 capture — not a regression, a
**measurement-condition** difference: that run's pointer sat idle (p50 0),
this run the pointer was actively resting on world content, so the dwell
gate opened most frames. It is the new top `Update` lever once the ground
probe is gone (see [[viewer-perf-steady-state-46fps-ceiling]]).

## Why it costs what it does

`update_hover_tooltip` (`hover_tooltip.rs:453`) is already **dwell-gated** —
it early-returns on button-held / cursor-motion / no-cursor and only acts
after `DWELL_SECS`. But *once dwelt*, every frame it runs two immediate-mode
`MeshRayCast` casts via the `HoverPick` param (`hover_tooltip.rs:209-263`):

1. `HoverPick::occluded()` — a cast against HUD entities
   (`pointer_over_hud(... &mut self.ray_cast)`), and
2. `HoverPick::resolve()` — the world cast (`object_picking::pick`) plus a
   terrain cast (`land_menu::pick_land`).

Bevy's `MeshRayCast` has no persistent acceleration structure: each cast is
a `par_for_each` over **every** `Mesh3d`/`Mesh2d`/`SimplifiedMesh` entity
with an AABB broadphase. In this trace that query fired **130,968** times
(≈60 parallel chunks/cast) for **8.08 s** of summed worker time. So the cost
is O(pickable meshes) and paid **twice per dwelt frame**, and it scales with
scene density (aditi's full region + neighbours) exactly when the frame is
already main-thread bound.

## Levers (pick when promoted)

- **Throttle the cast while the cursor is still.** The pointer is by
  definition not moving during dwell, and world content under a fixed screen
  point changes slowly — re-cast at ~10 Hz (or only on a content-change
  tick / camera move) instead of every frame, reusing the last
  `HoverTarget`. Biggest win for least risk; keeps the tip responsive.
- **Fold the two casts into one broadphase pass.** `occluded()` and
  `resolve()` walk the mesh set twice; a single cast that returns the
  nearest hit and classifies HUD-vs-world-vs-terrain from the hit entity
  halves the per-dwelt-frame work.
- **Give `MeshRayCast` a spatial index.** A BVH/octree over pickable prims
  (shared with any future object-click picking) turns each cast from
  O(meshes) into O(log n). Larger change; benefits every raycast consumer.

## A/B

Measure `Update` p50 and the `update_hover_tooltip` mean with the pointer
**held on dense world content** (the condition that opens the gate), window
visible/focused, before and after. Idle-pointer runs will show ~0 either way
and must not be used to judge this.
