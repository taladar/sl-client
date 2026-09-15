---
id: viewer-inventory-link-drop-to-world-rezzes-nothing
title: Dragging an inventory link onto the ground rezzes nothing
topic: viewer
status: bugs
origin: user report during the aditi live check of
  [[viewer-rigged-attachments-wearer-not-resolved]] (2026-09-15)
refs: [viewer-inventory-link-tools, viewer-rigged-attachments-wearer-not-resolved]
---

Context: [context/viewer.md](../context/viewer.md).

Dragging an inventory **link** to an object item onto the ground in world does
nothing — no rez, no error. Dragging the original item does rez. The drop
should rez what the link points at, as the reference does.

Likely cause (unconfirmed): the drop path in
`sl-viewer-inventory/src/inventory_drag.rs` gates the rez on
`matches!(item.inv_type, InventoryType::Object | InventoryType::Attachment)`
and builds `rez_object_command` from the dragged row's own fields. A link row
carries the link's asset type (`Link`) and its own item id, so either the gate
rejects it or the command names the link rather than the target.

Reference behaviour to port (Firestorm, read-only): a link resolves through to
its target almost everywhere a drag reads it — `LLInvFVBridge::startDrag` takes
the drag type from `getActualType()`, and `LLViewerInventoryItem`'s getters
(`getAssetUUID`, `getInventoryType`, …) forward to `getLinkedItem()`, while
`getPermissions` deliberately stays the link's own. Check in
`indra/newview/lltooldraganddrop.cpp` (`dad3dRezObjectOnLand`,
`dropObject`) which item id the `RezObject` message carries for a link before
choosing between sending the link's id and the target's.

Verify: drag a link to an object from inventory onto the ground on the local
OpenSim and on aditi — the target object rezzes; a broken link (target gone)
is refused with a message rather than silently ignored. Also check dropping a
link onto an object (contents drop) and onto the avatar (wear), which share the
same gate.
