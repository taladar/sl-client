---
id: viewer-inventory-context-actions
title: Inventory context actions + drag-and-drop
topic: viewer
status: done
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

## Done (2026-07-21)

Implemented as `sl-client-bevy-viewer/src/inventory_actions.rs` (context
menus + operations) and `inventory_drag.rs` (drag-and-drop), on the shared
line-menu widget (`OpenContextMenu` grew open-time conditions, like the pie).
One item menu + one folder menu mirroring `menu_inventory.xml`, entries
shown / greyed per type via open-time conditions (entry tables pinned by
tests); Library read-only throughout; Trash shows Purge / Empty Trash.

Wired: inline rename (in-row editor; New Folder starts in it), cut / copy /
paste / paste-as-link (single-entry clipboard), delete-to-trash, purge,
empty trash / lost-and-found, New Script / Notecard / Gesture, landmark
Teleport, gesture Activate / Deactivate, calling-card IM, wear / add /
detach (attachments), wear / add / take-off (wearables), folder Add-To /
Remove-From Current Outfit (wearables **and** attachments, batched).
Drag-and-drop: move / Library-copy between folders with auto-scroll,
hover auto-expand, target highlight, Esc cancel; drop onto an avatar /
name tag / People row gives (self-drop wears); object items drop-rez
in-world honouring the copy bit. Selection (click / Ctrl / Shift) landed in
`inventory.rs` alongside; the Worn tab gained its folder hierarchy.

Split out as follow-ups: [[viewer-inventory-attach-to-point]],
[[viewer-inventory-replace-outfit]], [[viewer-inventory-folder-deep-copy]],
[[viewer-inventory-share-picker]], [[viewer-inventory-open-and-properties]],
[[viewer-inventory-new-wearables]], [[viewer-inventory-restore-item]],
[[viewer-inventory-multi-select-actions]],
[[viewer-inventory-give-via-profile]], [[viewer-inventory-cof-maintenance]].
