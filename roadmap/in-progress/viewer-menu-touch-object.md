---
id: viewer-menu-touch-object
title: Touch an object from the object and attachment menus
topic: viewer
status: in-progress
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
