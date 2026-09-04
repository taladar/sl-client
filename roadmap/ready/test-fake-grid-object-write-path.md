---
id: test-fake-grid-object-write-path
title: The fake grid has no way to rez, take or fill an object
topic: test
status: ready
origin: split out of test-fake-grid-simulator-request-surfaces (2026-09-04)
points: 5
refs: [test-fake-grid-simulator-request-surfaces]
---

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
