---
id: server-fake-grid-hide-script-asset-ids
title: Hide a script's asset id from task listings and inventory fetches, as Second Life does
topic: server
status: ready
origin: server-fake-grid-script-compile-on-upload (2026-10-30)
refs: [server-fake-grid-script-compile-on-upload, gridspec-lsl-compile]
---

Context: [context/lsl.md](../context/lsl.md).

Second Life lists a script's asset id as nil in a task-inventory listing and
in an inventory fetch, even to its owner with full permissions; OpenSim
lists it (`book/src/gridspec/lsl.md` § `new_asset` and the item — every
`*_item_asset` of the `script-compile` census is `hidden` on aditi). The
fake grid lists it on both flavours.

That matters to a viewer more than it looks: a viewer that fetches a script's
source by the asset id in the listing works against OpenSim and the fake
grid, and fails against Second Life, where the source has to be asked for by
the task and item (the transfer's `TaskInventory` source resolves the asset
itself). The editor already treats a nil task `asset_id` as "cannot open"
for a script it may not modify; whether it opens one it may modify through
the item alone is what this would test.

Wanted: on the Second Life flavour, a nil `asset_id` for a script in the
`RequestTaskInventory` listing (UDP `Xfer` file and capability), in
`FetchInventory2` / `FetchInventoryDescendents2` and the UDP fetches, and in
`UpdateCreateInventoryItem`; the transfer of a task script by item still
answering with the source. The `script-compile` census records the answer
as `item_asset`, so its fake run (once it has one) holds it.
