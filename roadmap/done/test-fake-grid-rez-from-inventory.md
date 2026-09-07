---
id: test-fake-grid-rez-from-inventory
title: The fake grid takes an object but cannot rez one back
topic: test
status: done
origin: doing test-assets-object-asset-codec (2026-09-06)
points: 3
refs:
  [
    test-assets-object-asset-codec,
    test-fake-grid-object-write-path,
    test-fake-grid-asset-round-trip,
  ]
---

Done 2026-09-06. Both open decisions went the honest way: a linkset rezzes as
a linkset, and the item is resolved by id alone. See "What landed" below.

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

## What landed

`ServerEvent::RezObjectFromInventory` is answered in `sl-fake-grid`'s
`world::rez_from_inventory`: resolve the item out of the agent's own inventory,
fetch its asset, decode it, mint an id pair per prim, `to_object`, push into the
region's world and stream the `ObjectUpdate` — with the root moved to the ray's
end point, since the fake grid casts no rays and a drag-out always bypasses the
raycast anyway.

**The linkset decision: whole, both ways.** The premise the task was written on
was wrong — the fake grid *does* link (`object_edits`'s `ObjectsLinked` /
`ObjectsDelinked` back the `object-link-delink` case), and only
`store_taken_asset`'s doc still claimed otherwise. So the honest option was the
only coherent one, and it turned out to be two changes rather than one: the rez
rebuilds the parent relationship from the asset's `linked` markers and each
child's `childpos`, and the **take** now files a linkset whole. It had to: a
viewer derezzes its selection and a selected linkset is named by its root
alone, so before this a take of a linkset filed the root and left the children
standing in the region parented to an object that no longer existed.

**The permissions decision: by id alone.** The masks and the CRC in
`RezObjectParams` are what the *viewer* believes about the item and are not
checked against it, which is what [[test-object-rez-derez]] recorded of OpenSim.
The one permission that *is* read is the item's own: a **no-copy** item is
consumed by the rez and the client told with a `RemoveInventoryItem`, which is
OpenSim's `DoPostRezWhenFromItem` rule. The client's `remove_item` flag is not
consulted — it says what the viewer expected, not what the item permits.
Everything the fake grid mints is full-permission, so that arm bites only for an
item a fixture deliberately seeds without copy.

A rez out of a **prim's** contents (`from_task_id`) is left unanswered, which is
what a simulator does with a rez of an item it cannot resolve.

**In `sl-object-asset`,** three things the fake grid needed and the format crate
owns: `ObjectAsset::linkset` (children first, root last, each marked, and a
linkset of one is a solitary prim rather than a root); `PrimBlock::from_object`
now writes a child's `childpos` / `childrot` from its `parent_id` instead of
always writing a root's velocities; and `PrimBlock::to_properties`, the inverse
of the permission and sale blocks a take writes, so a rezzed object answers a
select with the record the asset describes rather than a synthesised default.

`SimInventoryTree::take_item` is new in `sl-proto` — the public counterpart to
`insert_item` for a simulator that consumed an item, factored with the AIS3
delete so only one path removes an item and bumps its folder's version.

**`object-rez-derez` is in `fake::OFFLINE_CASES`,** so the one case that
exercises `sl-object-asset` end to end runs on every `cargo test`. Its
"no primitive to place against" branch now fails on any grid whose content this
workspace declares (`content_is_ours`) rather than on OpenSim alone.
