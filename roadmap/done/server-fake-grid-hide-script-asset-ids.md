---
id: server-fake-grid-hide-script-asset-ids
title: Hide a script's asset id from task listings and inventory fetches, as Second Life does
topic: server
status: done
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
as `item_asset` and runs offline on both fake flavours; its Second Life
flavour excuses `*_item_asset` and `*_new_asset` meanwhile
(`FAKE_SL_PENDING` in `sl-conformance/src/cases/script_compile.rs`), and
those two entries come out with this.

## Done (2026-11-14)

- `sl_proto::InventoryDialect` decides it: `lists_asset_of` (Second Life
  withholds a `ScriptText` asset, OpenSim lists every one), `listed_item`,
  `listed_asset_id`, `listed_task_item` and `hide_listed_script_asset` for an
  LLSD item.
- `SimSession` applies it wherever an item goes to a viewer: the task listing
  over `Xfer` (`serve_task_inventory`), `UpdateCreateInventoryItem`,
  `FetchInventoryReply`, `InventoryDescendents`, `BulkUpdateInventory` over
  UDP and the event queue. `SimCaps` applies it to `FetchInventoryDescendents2`
  / `FetchInventory2` (`fetched_item`), AIS3 answers (`decorate_ais`) and the
  task-inventory capability (`task_inventory_response` takes the dialect).
  AIS3 and `BulkUpdateInventory` are by extension, not measured.
- The fake grid still resolves a script's source by its item (task or agent)
  and never trusts a requested asset id, so the transfer answers as before;
  the viewer already opens a nil-asset script by its item.
- `FAKE_SL_PENDING` loses its two entries: the census's Second Life flavour
  holds `item_asset` (`hidden`) and `new_asset` (`another`) as aditi measured.
- Tests: `task_inventory_fetch_round_trips` runs per dialect with a script
  and a notecard; `a_fetched_script_names_no_asset_on_second_life` holds the
  fetch rewrite. The book's gridspec (`lsl.md` § `new_asset` and the item)
  and the fake grid's flavour table say so.
