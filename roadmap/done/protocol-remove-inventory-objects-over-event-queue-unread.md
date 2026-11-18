---
id: protocol-remove-inventory-objects-over-event-queue-unread
title: The client does not read RemoveInventoryObjects over the event queue
topic: protocol
status: done
origin: server-fake-grid-inventory-offers (2026-10-23)
refs: [gridspec-inventory-offers, server-fake-grid-inventory-offers]
---

Context: [context/protocol.md](../context/protocol.md).

## Observation

Second Life tells a viewer that restoring a no-copy object to its last
position (`RezRestoreToWorld`) took the item away with a
`RemoveInventoryObjects` over the **event queue** (`inventory-offers`,
`book/src/gridspec/inventory.md` § Giving, the no-copy object). The client
decodes the UDP message (`Event::InventoryObjectsRemoved`) but not the
event-queue one: the census records it as `unread event
RemoveInventoryObjects` (a `Diagnostic::UnknownCapsEvent`). The fake grid's
Second Life flavour now sends it the same way.

## Effect

After a restore of a no-copy object the client's inventory model still holds
the item the grid took away, until the folder is fetched again.

## Fix

Read the event-queue body (the UDP blocks as LLSD arrays: `AgentData`,
`FolderData` `FolderID`, `ItemData` `ItemID`; the fake grid's
`remove_inventory_objects_to_llsd` writes that shape) into the same
`Event::InventoryObjectsRemoved`, in both runtimes. Then
`nocopy_object_restore_told_a` in `inventory-offers` reads `items removed: [A's
object item, taken back]` on Second Life: re-measure on aditi and update the
`Measured` constant and the book.

## Done (2026-11-18)

- `sl-proto`: `CapsEvent::RemoveInventoryObjects` and
  `remove_inventory_objects_from_llsd` (the inverse of the fake grid's
  encoder; a body that is not a map is a decode failure, an absent list reads
  as empty, nil ids are dropped) feed the same
  `Event::InventoryObjectsRemoved` as the UDP message. Both runtimes hand
  event-queue bodies to `Session::handle_caps_event`, so neither needed a
  change.
- Beyond the letter of the fix, for its stated effect: the session now drops
  what the grid removed from its own inventory model before it reports it
  (`Session::grid_removed_inventory`), for the event-queue message **and** the
  three UDP ones (`RemoveInventoryItem`, `RemoveInventoryFolder`,
  `RemoveInventoryObjects`), as the reference's handlers delete from
  `gInventory`. None of them pruned the model before, so a removal the grid
  made stayed in every local view until the folder was fetched again.
- `sl-viewer-inventory`: the three removal events re-read the pages that held
  what was removed (an item's folder, a removed folder's parent) among the
  folders the window asked for.
- `inventory-offers` renders `InventoryObjectsRemoved` and its
  `nocopy_object_restore_told_a` on Second Life is now
  `items removed: [A's object item, taken back]`, passing on fake-sl and
  re-measured on aditi.
- Tests: the round trip and lenient shapes of the parser, the event-queue
  removal reaching the client and pruning its model with nothing unread, the
  UDP removals pruning it, and the viewer's choice of folders to re-read.
