---
id: viewer-inventory-context-actions
title: Inventory context actions + drag-and-drop
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-inventory-ui
blocked_by: [viewer-inventory-folder-tree]
---

Context: [context/viewer.md](../context/viewer.md).

The mutating actions on an inventory item: the context menu — **wear**, **rez**,
**give**, **delete**, **rename** — plus **drag-and-drop** (move items between
folders, drop onto an object or the world). Hangs off the folder tree
([[viewer-inventory-folder-tree]]) rows and wires each action to the existing
inventory / rez commands.

The inventory protocol and model already exist; this task is the context menu,
the drag-and-drop plumbing, and the command wiring.

Reference (Firestorm, read-only): `llinventorybridge`,
`llinventoryfunctions`.

Builds on: [[viewer-inventory-folder-tree]] and the `inventory.rs` model.

## Note (2026-07-18)

Unblocked: [[viewer-inventory-folder-tree]] is done. Each `DisplayRow` in
`inventory.rs` already carries its `RowKey` (folder / item key), the payload a
drag or a context action needs. Two constraints to honour here:

- The **Library** subtree is **read-only** — `InventoryModel.library_folders`
  flags those folders; mutation actions (rename / delete / move / rez) must be
  disabled for them.
- Drag-and-drop in SL is more than moving rows: **hover-near-top/bottom
  auto-scrolls**, **hover-over-a-folder auto-expands**, and a drag can leave the
  panel entirely to **rez in-world** or **pass to another avatar**. Check the
  reference (`llinventorybridge`, `llfolderview`) for the exact mechanics.

## Operation set (2026-07-18)

The full mutating-operation set the context menu should cover (beyond the
wear/rez/give/rename/delete above): **create** (new folder / notecard / script /
…), **copy**, **paste**, **paste as link**, **delete**, and **purge / empty
trash**. All are `Session` inventory mutations already on the wire; this task is
the menu and the command wiring. Library rows stay read-only (see the note
above).

Give-by-drag targets a **specific avatar**: dropping an inventory item onto an
avatar in-world, onto a radar / people-list row, or onto their **profile**
([[viewer-social-profiles]]) gives it to them (the give wire path is done).

## Menu shape (2026-07-21)

The context menus for inventory **items and folders** are regular
**drop-down / line menus** (the reusable `crate::menu` widget), exactly as in
the reference — inventory never uses pies. Mirror the **reference's** per-type
entry sets: the reference builds them all from one shared `menu_inventory.xml`
whose entries each bridge type shows / hides / disables
(`llinventorybridge.cpp`, `buildContextMenu` per `LL*Bridge`) — folders get
New/Sort/Delete-category entries, wearables get Wear / Take Off, objects get
Attach / Detach, landmarks Teleport, and so on; the Library subtree stays
read-only. As with the pie menus: reproduce the reference entries, grey out
what our side does not implement yet (the `UNIMPLEMENTED` condition pattern),
and wire the simple ones (wear / detach / rename / delete / copy-paste /
move-to-trash) now. The menu XML is skin-shared (the Vintage skin overrides
none of it), so `default/xui/en/menu_inventory.xml` is authoritative.
