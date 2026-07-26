---
id: viewer-object-contents-copy-refinements
title: Object Contents floater — named copy folder + Copy And Wear
topic: viewer
status: ready
origin: split from viewer-prim-inventory-editing (2026-07)
blocked_by: [viewer-prim-inventory-editing]
---

Context: [context/viewer.md](../context/viewer.md).

Reference-parity refinements to the Object Contents floater's copy actions,
which shipped in a simplified form with [[viewer-prim-inventory-editing]]:

- **Named copy folder**: "Copy To Inventory" currently moves the contents into
  the system *Objects* folder. The reference
  (`LLFloaterOpenObject::moveToInventory`) first creates a
  **new category named after the object** (via `createNewCategory` + a callback
  carrying the new folder id) and moves the contents into that, so each opened
  object's contents land in their own folder. Needs a `CreateInventoryFolder` →
  new-folder-id callback wired through.
- **Copy And Wear auto-wear**: the action currently copies to inventory and
  posts a notice telling the user to wear from there. The reference wears the
  copied wearables / attachments afterward — which needs the moved items'
  freshly-allocated **agent-inventory ids** (they arrive asynchronously as
  inventory updates), then a wear on those, the reference's `LLCatAndWear`
  callback chain.

Reference (Firestorm, read-only): `llfloateropenobject.cpp`
(`moveToInventory`, `callbackCreateInventoryCategory`, `LLCatAndWear`).
