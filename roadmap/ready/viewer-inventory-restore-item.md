---
id: viewer-inventory-restore-item
title: Restore Item from Trash
topic: viewer
status: ready
origin: split from viewer-inventory-context-actions (2026-07-21) — shipped
  greyed (UNIMPLEMENTED)
refs: [viewer-inventory-context-actions]
---

Context: [context/viewer.md](../context/viewer.md).

The Trash context menu's **"Restore Item"**: move a trashed item / folder back
to where it belongs. The reference restores to the item's **type-default
system folder** (`findCategoryUUIDForType` — a texture back to Textures, a
landmark to Landmarks, …), since the original location is not recorded on the
wire. Delete-to-Trash, Purge and Empty Trash are live
(`inventory_actions.rs`); this is the inverse move plus the type→default-
folder mapping (which the model already knows via `FolderType`).

Reference (Firestorm, read-only): `llinventorybridge.cpp`
(`restoreItem`), `llinventorymodel.cpp` (`findCategoryUUIDForType`).
