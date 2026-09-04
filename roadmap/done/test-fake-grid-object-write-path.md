---
id: test-fake-grid-object-write-path
title: The fake grid has no way to rez, take or fill an object
topic: test
status: done
origin: split out of test-fake-grid-simulator-request-surfaces (2026-09-04)
points: 5
refs: [test-fake-grid-simulator-request-surfaces]
---

Done 2026-09-04. `task-inventory` is in `fake::OFFLINE_CASES` and passes; see
"What landed" below.

Context: [context/testing.md](../context/testing.md).

Everything the fake grid serves today it was handed: a fixture states an
object and the region streams it. Nothing a client sends ever *changes* what
the region holds, which is why `task-inventory` is the one case
[[test-fake-grid-simulator-request-surfaces]] could not take offline with the
other five. Three client messages `SimSession` decodes have nowhere to go, and
all three are writes:

- **`ObjectAdd` → `ServerEvent::RezObject`.** The client rezzes a prim: a
  shape, a position and a rotation, plus the ray-cast the viewer aimed with.
  The simulator mints a region-local id and a full id, adds the object to the
  region, and streams it back as an `ObjectUpdate` (the client's
  `Event::ObjectAdded`). The fake grid's `SceneFixtures::objects` is the
  region's object table already, so the mutation has somewhere obvious to
  land — but it is per-session state today, and a rez that only one session
  sees is a rez no second avatar can be shown.
- **`DeRezObject` → `ServerEvent::DerezObjects`.** Take-into-inventory,
  delete-to-trash, return-to-owner and the rest (`DeRezDestination`). Taking
  mints an agent inventory item — the fake grid's `SimInventoryTree` can hold
  it — and answers `UpdateCreateInventoryItem` echoing the client's
  transaction; every destination kills the object (`KillObject`), which
  `send_kill_object` already does. `send_derez_ack` exists and nothing sends
  it.
- **`UpdateTaskInventory` → `ServerEvent::UpdateTaskInventory`.** Dropping an
  agent inventory item into a prim: the simulator resolves the item by id from
  the agent's own inventory, copies it into the object's task inventory under a
  freshly-minted item id, and **advances the object's contents serial**. The
  serial is the whole observable — a viewer decides a cached listing is stale
  from it alone — and the listing behind it has to be re-generated as an Xfer
  file, because `UdpAssetFixtures::register_xfer_file` serves bytes stated up
  front rather than bytes derived from live state.

Together they are the first grid-side write path the fake grid has, and the
piece that makes it a *simulator* rather than a replayer of fixtures. The
awkward part is not the three arms; it is that the object table, the agent
inventory and the task inventories are per-session fixtures cloned at login
([`SimState::world`]), so a write has to decide whether it is the session's or
the region's. A region-scoped store shared by its sessions is the honest
answer and the bigger change.

Acceptance: `task-inventory` runs in `fake::OFFLINE_CASES` with its test in
`tests/offline.rs`, rezzing its own container, taking a donor item, dropping it
in, watching the serial advance, reading the listing back over Xfer and
trashing the container — with the region left as it was found.

## What landed

The region-scoped store, because the alternative was a rez only its rezzer
could see. `RegionEntry` now owns one `SceneFixtures` behind one lock
(`RegionWorld`) and every session in the region holds a clone of the `Arc`;
the scenario states what the region *starts* as and this is what it has
*become*. Two regions still hold different worlds, which is what a handover
needs and what `with_world`'s doc used to promise per session.

Nothing sweeps the region for changes, so each write publishes a
`RegionUpdate` (tagged with the session that made it) and a per-session
`run_region_watcher` turns it back into the `ObjectUpdate` / `KillObject` its
own circuit needs, skipping its own. A lagging watcher warns rather than
swallowing: a lost `KillObject` is a ghost object for good.

Three things the task did not anticipate:

- **The task inventories had to move.** They were `UdpAssetFixtures`
  fixtures — bytes stated up front — and a contents serial only means
  anything if the store that answers it is the store a write advances. They
  are now `SceneFixtures::task_inventories`, keyed by region-local id, with
  the `ObjectKey` the reply carries read off the object rather than restated
  beside it. `TaskInventoryFixture` is gone; `TaskInventory::write` is the
  only way in, because it is the only way that also bumps the serial.
- **`DeRezDestination` decides both halves, and OpenSim disagrees with the
  obvious guess.** `agent_folder()` names the folder an item is minted in and
  `removes_from_world()` says whether the world copy goes; the pair is
  `Scene.DeRezObjects`'s `takeCopyGroups` / `takeDeleteGroups` split, which
  means a *take copy* and a *god take copy* leave the object standing and the
  three attachment destinations do nothing at all here.
- **`ObjectAdd` carries no position.** Where the prim lands is `ray_end`, so
  `AddPrimParams` fills `shape.position` from it and keeps the ray fields
  besides.

`ObjectAdd` and `DeRezObject` left the `RAW_FORWARDED` ledger, so their
assertions moved out of the object family test and into
`sim_session.rs::object_rez_and_take_round_trip`. `send_inventory_item_created`
is new (`UpdateCreateInventoryItem`), and is what every server-side inventory
creation should go through from here.

Not done, deliberately: the take mints an asset id and leaves it unbacked —
nothing serialises the object into an asset, so a client that fetched it would
get nothing. The case does not, and inventing an object serialisation format
for a grid with no persistence would be a fixture pretending to be a
simulator. The same is true of the item this task's `UpdateTaskInventory`
writes into a prim, and of most of the seeded agent inventory, so it is not
this path's problem: [[test-fake-grid-asset-round-trip]] is the whole of it,
and [[test-assets-object-asset-codec]] is what the object half waits on.
