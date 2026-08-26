---
id: viewer-audit-world-reset-purge-completeness
title: The world-reset purge is a hand-maintained list of five, and several stores are missing
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 5
refs: [viewer-audit-water-state-purge]
---

Context: [context/viewer.md](../context/viewer.md).

`reset_scene_on_world_reset` (`sl-viewer-world-view/src/scene_reset.rs:42`)
names five stores explicitly. Stores that are **not** purged on a distant
teleport and grow for the session:

- `WaterState` — see [[viewer-audit-water-state-purge]];
- `ObjectPhysicsShapes` (`sl-viewer-world-view/src/physics.rs:1385`) — its
  `data` and `requested` maps have
  **no `remove`, `retain` or `clear` anywhere in the file**, so every physical
  object ever seen in every region accumulates;
- `MediaData` (`sl-viewer-world-view/src/media_prim.rs:75`) — `objects` and
  `requested` are insert-only (`:304`, `:353`), and `requested` holds a `String`
  per object. This map is also the input to `drive_media_surfaces` (`:460`),
  which iterates every entry x every face every frame with an O(n*m)
  `!wanted.contains(target)` — so the per-frame cost grows all session;
- `ObjectCostModel` (`sl-viewer-world-objects/src/object_cost.rs:62`) — cleared
  only by `invalidate_all` on a relink;
- `RegionTimeDilation` — `dilations.per_region` is only ever inserted into
  (`physics.rs:221`), so `neighbours_known` (`:695`) reports a long-departed
  neighbour as live and `clip_axis` (`:759`) stays on the `crossing: true`
  branch instead of clipping to the void edge.

The decoded-asset stores (`textures.rs`, meshes, materials) expose no purge API
at all.

Separately, a genuine handle leak: `media_prim.rs:693` pushes a **strong**
`Handle` per re-apply into `slot.touch_materials`, and the only pruner
(`sl-viewer-media/src/media_engine.rs:479`) is
`retain(|h| materials.get_mut(h.id()).is_some())` — which the Vec's own strong
handles keep true forever, so it removes nothing, ever.

Scope: replace the hand-maintained call list with something that cannot silently
omit a store — a `WorldScoped` trait each store implements, or a registry the
plugins add themselves to.
