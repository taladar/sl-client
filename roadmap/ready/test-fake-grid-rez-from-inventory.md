---
id: test-fake-grid-rez-from-inventory
title: The fake grid takes an object but cannot rez one back
topic: test
status: ready
origin: doing test-assets-object-asset-codec (2026-09-06)
points: 3
refs:
  [
    test-assets-object-asset-codec,
    test-fake-grid-object-write-path,
    test-fake-grid-asset-round-trip,
  ]
---

Context: [context/testing.md](../context/testing.md).

`ServerEvent::RezObjectFromInventory` (the client's `RezObject` message, an
item id and a ray placement) reaches `answer_world_request` and falls through:
nothing rezzes it, so a viewer that drags an object out of its inventory
against the fake grid gets silence. Only `ObjectAdd` — the build tool's "new
prim" — works.

Everything it needs now exists. [[test-assets-object-asset-codec]] made the
loop's other half real: a take serialises the object into an asset
(`world::store_taken_asset`) and `sl_object_asset::PrimBlock::to_object` turns
an asset prim back into an `sl_proto::Object` given the ids the region mints
(`RezTarget`). So the arm is: resolve the item out of the agent's inventory,
fetch its asset from `GridAssets`, decode it, mint a region-local id and an
object key, `to_object`, push it into the region's world and stream the
`ObjectUpdate` — the same tail `ServerEvent::RezObject` already has.

Two things to decide rather than assume:

- **what a linkset rezzes as.** The asset can hold several prims (the
  reference's own four-prim example does) and the region has no linking, so
  either the arm rezzes the root only and says so, or the fake grid grows a
  parent/child relationship. The second is the honest one and is what
  `parent_id` in an `ObjectUpdate` is for.
- **whether the item's permissions are applied.** `RezObjectParams` carries the
  masks the client believes in; OpenSim ignores them and looks the item up by id
  alone ([[test-object-rez-derez]] records that). The fake grid should do
  whatever the record says a real grid does.

It would take the `object-rez-derez` conformance case offline — it is currently
live-grid only, and its rez leg is the one thing that cannot run here.

Acceptance: a client that takes an object and rezzes the item back gets an
object with the shape, scale and faces it took, under fresh ids; the
`object-rez-derez` case runs in `fake::OFFLINE_CASES`.
