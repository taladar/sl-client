---
id: protocol-remove-inventory-objects-over-event-queue-unread
title: The client does not read RemoveInventoryObjects over the event queue
topic: protocol
status: bugs
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
