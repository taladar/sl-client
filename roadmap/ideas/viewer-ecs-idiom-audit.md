---
id: viewer-ecs-idiom-audit
title: Audit the viewer for state modelled beside the ECS rather than in it
topic: viewer
status: ideas
origin: crate-split work (2026-08) — patterns the world-layer moves kept surfacing
refs: [build-structural-encapsulation-audit, build-split-viewer-crate]
---

Context: [context/viewer.md](../context/viewer.md).

Much of the viewer's world state is held in big keyed resources — a
`HashMap<AgentKey, …>` or `HashMap<ScopedObjectId, …>` inside `AvatarState`,
`ObjectState`, `TerrainState` — and the systems iterate the map rather than
querying entities. It works, and the maps mirror the wire model closely, which
is part of why they were written that way. But it means facts about a thing are
stored beside the entity that *is* that thing, and the split kept running into
the consequences.

This is the audit for where that is worth reversing. It is not a call to
dissolve the stores: a store keyed by a wire id is the right shape for
reconciling a wire update, and `by_scoped`-style reverse maps exist because the
protocol gives you one id and you need the other. It is a call to find the
places where the ECS would carry it better.

## Known instances

- **Deferred build state as a store field.** `TrackedObject` carried
  `pending` / `mesh_rebuild` / `prim_rebuild` / `tree_rebuild` — "this object
  is waiting for its mesh asset / sculpt map / tree species". Every tracked
  object already has an `entity`, so that is a component on the entity waiting
  for it, and the builder is a query rather than a scan of the whole map. It
  was lifted to a side table to get the store below the world; a component is
  the better destination.
- **Render handles held where state lives.** The placeholder avatar's shared
  mesh and material sat inside `AvatarState`; `TerrainState` and its region
  records held image and material handles. Spawn with a marker and let a render
  system attach `Mesh3d` / `MeshMaterial3d` — see
  [[build-structural-encapsulation-audit]], which carries this one with its
  cost attached.
- **Asset managers as call targets.** `TextureManager`, `MeshManager`,
  `MaterialManager`, `AnimationManager`, `WearableAssetManager` own caches and
  are *called* by consumers that want a decoded result. Publishing results —
  as components on the requesting entity, or a keyed resource read by a
  query — would invert the dependency and is what currently keeps the object
  and scene layers tangled.
- **Scheduling by system name.** The composition root names fifty-two world
  systems in its own ordering constraints. `WorldPhase` (added during the
  split) shows the shape of the fix: order against a `SystemSet`, not a
  function. Only six edges inside the world needed it; the root's fifty-two
  are the remaining bulk.

## Approach

Look for the shapes rather than auditing file by file: a `HashMap` keyed by an
id whose value holds an `Entity`; a field that is only ever read for one
entity at a time; a system that iterates a store to find the few members that
need work, where a query with a marker component would say the same thing.

Each candidate wants the same three questions. Does the ECS index it better
than the map does? Does the wire reconciliation still have the id-keyed lookup
it needs? And does the change remove a dependency, or only move it? The last
one is what separates a real improvement from a rewrite for its own sake.
