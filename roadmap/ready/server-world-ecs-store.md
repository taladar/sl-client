---
id: server-world-ecs-store
title: An ECS scene store for the region, in place of a Vec of objects
topic: server
status: ready
origin: LSL-on-the-fake-grid audit (2026-09-20); user request for a
  simulator-side ECS
points: 13
refs: [server-simulator-core, server-world-link-sets,
  server-world-update-scheduling, server-lsl-lib-prim-state]
---

Context: [context/lsl.md](../context/lsl.md).

`SceneFixtures` is a fixture store that has been asked to be a scene.
Today it is `Vec<ParcelInfo>` + `Vec<Object>` + `Vec<NpcFixture>` behind
one `parking_lot::Mutex`, with `object_by_local_id` doing a linear scan
over `all_objects()` (which **allocates a clone of every object in the
region** on every call), `local_id_of` doing the same, and side tables
(`task_inventories`, `object_costs`, `undo`, `parcel_access`) keyed by
`RegionLocalObjectId` in separate `BTreeMap`s. It works because a
scenario has a few dozen prims and nothing ever ticks.

A script engine changes every one of those assumptions. `llGetPos` on a
linkset root, `llGetLinkKey(3)`, `llGetObjectDetails` on an arbitrary
key, a `llSensor` sweep, a collision pass, and a per-tick "what changed
since the last update burst" query are all hot, and several of them are
per-script-per-tick.

Wanted: an **entity store** for the region. The shape that fits what is
already here:

- A stable `Entity` handle per in-world thing — prim, avatar, NPC body,
  attachment — minted once and never reused, with the existing
  `RegionLocalObjectId` and `ObjectKey` becoming **indices into** it
  rather than the identity. `mint_local_id`'s never-reuse rule is then a
  property of the index, not a scan of `all_objects()`.
- Components rather than one fat `Object`: placement, shape, texture
  entry, flags, inventory, cost, script state, physics. The wire
  `Object` record stays what it is — a *message*, built from components
  when an update goes out, rather than the storage.
- The side tables become components. A `HashMap<Entity, _>` living
  outside the store is the thing the `sl-client-entity-keyed-store-rule`
  note warns about, and the `undo` / `task_inventories` /
  `object_costs` maps are exactly that shape today.
- Indices maintained by the store, not rebuilt per query: by full id, by
  local id, by link-set root, and a coarse spatial bucket for the
  sensor / collision / interest queries that come later.
- **Change detection**, because [[server-world-update-scheduling]] needs
  "what moved this tick" and today every mutation site hand-publishes a
  `RegionChange`.

The choice of ECS is a decision, not a foregone conclusion. `bevy_ecs` is
already in the workspace (the viewer) and brings change detection and
queries for free, but it is a large dependency for a crate whose whole
point is to be startable in-process inside a `cargo test`, and its
`World` is not `Sync` in the way a `parking_lot::Mutex<SceneFixtures>`
is. `hecs` is small and does most of it. A hand-rolled generational
arena plus explicit indices is a third option and is what the side tables
already almost are. Measure the fake grid's test-suite start-up cost
before and after — the crate is started dozens of times per `cargo test`
run.

**The fixture API must survive.** `region_wide_parcel`, `box_prim`,
`prim_from_shape`, `avatar_prim`, `SceneFixtures::add_parcel` /
`add_task_inventory` and the whole of `fixtures/` are how every scenario
and every offline conformance case is written; the store change must be
behind them, not in front of them.

Acceptance: the fake grid's existing offline conformance cases and unit
tests pass unchanged; `object_by_local_id` and `local_id_of` are
index lookups with no clone of the region; the four side tables are
components; and a benchmark (or a plain timing test) shows a 1000-prim
region answering a by-key lookup in constant time.
