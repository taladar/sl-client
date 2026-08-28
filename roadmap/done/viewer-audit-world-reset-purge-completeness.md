---
id: viewer-audit-world-reset-purge-completeness
title: The world-reset purge is a hand-maintained list of five, and several stores are missing
topic: viewer
status: done
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

## Resolved (2026-08-29)

The call list is gone. A store now declares its own scope: it implements
`WorldScoped` (`sl-viewer-world-api/src/world_scoped.rs`) next to itself and is
registered with `App::init_world_scoped::<T>()` in place of the `init_resource`
that would otherwise have created it — so creating a store and scoping it are
one line and cannot drift apart. `register_world_scoped::<T>()` covers a store
a startup system inserts later (`WaterState`). Registration installs the plugin
itself, so no plugin has to remember it; `WorldResetSystems::Detect` folds the
event stream into `WorldResetFrame` and `WorldResetSystems::Purge` runs every
registered purge, ordered (by the viewer) before the re-centring systems as the
old system was. Startup logs the registered set by name.

Registered: `ObjectState`, `PendingObjectEvents`, `AvatarState`, `TerrainState`,
`TerrainTextures` (the five the old list held) plus the five it missed —
`WaterState` (see [[viewer-audit-water-state-purge]]), `ObjectPhysicsShapes`,
`MediaData`, `ObjectCostModel` and `RegionTimeDilation`, plus
`RiggedBindSkipLog` (scoped-id-keyed, and an attachment that never binds never
reaches the one `remove`). `invalidate_stale_costs` also clears its `Local`
fingerprint map, which no registry can reach.

Checked and left alone, having turned out not to leak: `WorldSounds::attached`
drops a sound whose object `ObjectState` no longer knows; `ControlAvatarState`
retains against the tracked set and already caps its signalled-part map;
`ParcelBorderState` reconciles against the live region set every frame.

Also fixed here: the `MediaSlot::touch_materials` handle leak. The Vec held
**strong** `Handle<FaceMaterial>`s, so its only pruner —
`retain(|h| materials.get_mut(h.id()).is_some())` — was kept true by its own
handles and removed nothing, ever. It holds `AssetId`s now, so a material whose
last strong handle (the face's `MeshMaterial3d`) is gone is dropped on the next
pass.

What makes a store world-scoped — and what keeps one out — is written up in the
module docs, so the next store has a rule to answer against instead of a
precedent to guess from: the decoded-asset caches (textures, meshes, materials)
are keyed by grid-wide UUIDs the destination may well want again, and user state
(mutes, contact sets, per-agent render overrides) outlives the world.

One claim in the report above did not survive checking: `drive_media_surfaces`'s
`!wanted.contains(target)` is not the growth term — `wanted` is capped at
`MAX_MEDIA_SURFACES` (8) and `state.active` with it. The per-frame cost that
grew all session was the candidate scan over `MediaData::objects`, which the
purge bounds to the connected world.
