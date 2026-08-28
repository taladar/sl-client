---
id: viewer-audit-water-state-purge
title: WaterState is not purged on a teleport, and leaks spawned water planes
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 2
refs: [viewer-audit-world-reset-purge-completeness]
---

Context: [context/viewer.md](../context/viewer.md).

`reset_scene_on_world_reset` (`sl-viewer-world-view/src/scene_reset.rs:42`)
purges exactly five stores — `ObjectState`, `PendingObjectEvents`,
`AvatarState`, `TerrainState`, `TerrainTextures`. `WaterState` is not among
them, and `region_heights` (`sl-viewer-world-scene/src/water.rs:166`) has an
insert at `:258` and **no removal anywhere** (only `region_planes` is
reconciled).

So after a distant teleport `root_height` becomes the destination's, and
`reconcile_region_planes` (`:388`) walks every region ever visited, spawning a
256 m alpha plane wherever `(height - root_height).abs() > HEIGHT_EPSILON`.
Fifty sea-level-20 regions followed by a teleport to a sea-level-0 region leaves
fifty planes scattered across the grid, permanently in the transparency sort.
`:402` also re-collects a `Vec` over that growing map every frame.

Fix: purge `region_heights` on `world_reset` — and prefer adding water to the
purge list over adding a sixth hand-maintained entry, see
[[viewer-audit-world-reset-purge-completeness]].

## Resolved (2026-08-29)

Done as part of [[viewer-audit-world-reset-purge-completeness]], and the way
that task's note preferred: `WaterState` implements `WorldScoped` rather than
becoming a sixth entry on a hand-maintained list. Its purge despawns every
per-region plane and clears `region_heights`; the shared material, plane mesh
and requested normal-map key are kept (the region-independent water look, which
`drive_water` refreshes from the destination's environment anyway).

Unit-verified in `water.rs`: after a purge both maps are empty and the spawned
plane entity is despawned.
