---
id: viewer-menu-touch-object
title: Touch an object from the object and attachment menus
topic: viewer
status: done
origin: audit of menu entries still gated on UNIMPLEMENTED (2026-08-24)
refs: [viewer-object-context-menu, viewer-attachment-context-menu]
---

Context: [context/viewer.md](../context/viewer.md).

**Touch** is greyed in the object menu, the attachment menu and the
inventory's worn-object menu, though the wire side is done and tested
(`test-object-touch-grab`: `ObjectGrab` / `ObjectDeGrab`).

What is missing is the menu path into it, and the two details that make it
more than one message: a touch aimed through a menu has no surface
coordinates, so it sends the object-centre form the reference uses, and a
touch on one's own attachment goes to the attachment rather than to whatever
sits behind it in the world.

## Status (2026-09-24): half done

The object menu and the attachment menu both send `Command::TouchObject` (the
`touch` slice behind `TARGET_TOUCHABLE`, and the attachment menu's `More >`),
carrying the picked surface point since a pie is always opened on one.

**Left:** the inventory's worn-object menu. `menu-inv-touch` is still
`enabled_when(UNIMPLEMENTED)` in `sl-viewer-inventory/src/inventory_actions.rs`
— and it is the harder half this task describes: a touch aimed from an
inventory row has no surface, so it sends the object-centre form the reference
uses, to the attachment itself.

## Done (2026-09-27)

The inventory's worn-object Touch is live. It is enabled when the row's item
is worn, its object has streamed in and handles touch (`FLAGS_HANDLE_TOUCH`),
and only one row is selected. The reference disables the entry on every row
but the first. Picking it sends `Command::TouchObject` with no surface (the
object-centre form) to the attachment **root**, which is what the reference's
`handle_attachment_touch` touches.

The bridge from an inventory row to the worn object is
`ObjectState::worn_attachment_of_item`. It matches the attachment's
`AttachItemID` against the item. It identifies the wearer by the avatar
object the attachment root hangs off (`AvatarState::agent_of`), the test the
HUD routing makes, so an item id echoed on someone else's attachment is never
reached. The first version matched the update's `owner_id` and never
enabled. A `sl-repl` probe showed that OpenSim sends it nil on every worn
attachment, because `ObjectUpdate`'s `OwnerID` is the attached sound's owner.
The `TrackedObject::owner_id` doc, which claimed it names the wearer, is
corrected. A link row, such
as one from the Worn tab or the COF, stands for the item it links to, as the
reference's `getLinkedItemID` has it. The same lookup is asked again when
Touch is picked, so an attachment removed while the menu was open is not
touched. `FLAGS_HANDLE_TOUCH` now lives once in `sl-viewer-world-api`'s
`object_flags`, and both pies import it instead of keeping their own copies.

Two things the reference does are left out on purpose:

- **The transient selection.** The reference selects the attachment before
  touching it only because its touch handler reads the selection. Here the
  command names the object directly.
- **The RLV `canTouch` gate.** No touch path in the viewer consults it yet:
  not the object pie, not the attachment pie, and not a click. The
  `RlvActions` façade has no viewer consumer at all. That gap is filed as
  [[viewer-rlv-send-side-consumers]] rather than wired into this one entry.

The live test also found [[viewer-inventory-worn-before-attach-confirmed]]:
the worn label followed requests rather than the avatar. It was fixed in
the same change.
