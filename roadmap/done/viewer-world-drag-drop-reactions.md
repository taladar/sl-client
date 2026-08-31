---
id: viewer-world-drag-drop-reactions
title: Inventory drag&drop onto the world
topic: viewer
status: done
origin: user request (2026-07) — drag onto avatar must give the item
points: 5
blocked_by: [viewer-ui-interaction-harness, viewer-world-test-harness]
---

Context: [context/viewer.md](../context/viewer.md).

The one flow that spans both harnesses: `inventory_drag.rs` starts from a
UI row (`Pointer<DragStart>` ghost — the UI interaction tier) and resolves
on `DragEnd` against world targets (the world tier).

Assert:

- dropping an item on the avatar fixture emits `give_command` (and
  refuses library sources);
- dropping on an object's `ContentsDropTarget` moves it into contents;
- dropping on the ground emits `rez_object_command` with the right ray
  and respects the copy permission (no-copy rezzes are moves);
- Escape cancels mid-drag with no command emitted;
- hover highlights track `DragHover`.

The pure classification helpers (`classify_folder_drop`,
`rez_object_command`) are already unit-tested; this tests the *wiring*
end-to-end.

Done (2026-08-31): `drag_drop_tests` in
`sl-client-bevy-viewer/src/world_test.rs`, over a new fixture fold
`world_app_with_ui_and_inventory` — the UI fold plus the **real**
inventory window (the floater manager, the virtualized list, the line-menu
widget, and the inventory / actions / drag / filters / properties
plugins), opened the way the menu bar opens it. The model is fed as a grid
feeds it (`InventoryFolders`, `LibraryInventory`, `InventoryFolderPage`),
so every case drags a row the panel actually drew, found by the label the
user reads on it.

Six cases, all five asserts plus the in-list move as the negative that
makes the world cases mean something (a drop that never left the window
must not rez). Each of the seven branches was mutated in turn and the
matching case failed.

Two things the writing settled:

- the resolution is **not** `MeshRayCast` from the drop observer, as this
  file said: the world half is the tier's own drag pick
  (`DragPickActive` / `DragWorldPick`, `WorldPhase::DragPickResolved`),
  which the CPU resolver answers headlessly like any other pick;
- a drag resting on a prim loses its hover highlight for the one frame
  the prim re-tessellates — the user-visible face of
  [[viewer-prim-rebuild-drops-a-click]], not a new bug. The cases rest
  past it, as the pie negatives do.
