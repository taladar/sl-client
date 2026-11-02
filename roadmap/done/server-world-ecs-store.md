---
id: server-world-ecs-store
title: An ECS scene store for the region, in place of a Vec of objects
topic: server
status: done
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

## As built (2026-11-02)

`sl_fake_grid::scene_store::ObjectStore` is `SceneFixtures::objects`: a
`bevy_ecs` `World` with one entity per prim, its wire record the `Body`
component, and indices the store keeps itself — by region-local id, by full
id and by link-set root (`children_of`), the order prims came in (`Seq`; a
world's own iteration order shifts as entities leave), and the highest local
id ever held, which `mint_local_id` now counts instead of scanning.
`object_by_local_id`, `local_id_of` and the new `object_by_key` are index
lookups that clone only the prim found (an NPC's body and attachments are
still made from its fixture, looked for after the store). Every burst and
refetch goes out in the order it did before. `get_mut` hands out an
`ObjectMut` that writes through `bevy_ecs`'s `Mut`, so the world's change
ticks see it, and refiles the prim under any id the write moved when dropped.

The fixture API kept its shape: `world.objects.push` / `extend` / `retain` /
`iter` / `first` / `len` compile unchanged in every fixture, the viewer's
e2e tests and the conformance cases. Three of the four side tables are
components — `Contents` (the task inventory: `contents`, `contents_mut`,
`contents_or_empty`, `set_contents`), `Cost` (`cost`, `set_cost`) and
`History` (the edit history) — and leave with the prim. `add_task_inventory`
now refuses an inventory for a prim that is not in the region (it hands it
back) rather than holding it under a bare id. A 1000-prim lookup test (best
of five against a 10-prim region) is in `scene_store`'s unit tests.

`bevy_ecs` (the workspace's patched 0.19, without reflection or an
executor) over `hecs` or a hand-rolled store, because it is the viewer's ECS
and the fake grid is to grow physics, sensors and real simulator object
counts in Bevy's terms. It adds 29 small crates to the fake grid's closure.
Start-up cost, measured as the release test suites' own run times before
and after: the unit tests 0.36 s both, `client_end_to_end` 8.38 s / 8.37 s,
`heartbeat` 2.17 s / 2.16 s. A `World` cannot be cloned, and a scenario's
world is copied once per region at grid start, so the store's `Clone`
respawns its prims into a fresh world.

Not done here, and why:

- **Components rather than one fat `Object`.** The wire `Object` is still
  the `Body` component. Splitting out placement, shape, texture entry and
  flags would rewrite every edit path for no reader yet; the first reader
  is the terse-versus-full choice in [[server-world-update-scheduling]].
- **Reading the change ticks** is [[server-world-update-scheduling]]'s, the
  first thing that reads them; the writes already go through `Mut`.
- **The spatial bucket** went to [[server-lsl-lib-detection-sensors]] (a
  query by place, a scan first) and [[server-world-collision-and-physics]].
- **`parcel_access`** stays keyed by the parcel: it is a parcel's, not a
  prim's, and parcels are not entities of this world.
- **Script state and physics components** arrive with
  [[server-fake-grid-script-engine-wiring]] and
  [[server-world-collision-and-physics]].
- **NPC bodies and the agent's own avatar** are not entities yet; they are
  still made from their fixtures when sent.
