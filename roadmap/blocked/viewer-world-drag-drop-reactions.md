---
id: viewer-world-drag-drop-reactions
title: Inventory drag&drop onto the world
topic: viewer
status: blocked
origin: user request (2026-07) — drag onto avatar must give the item
points: 5
blocked_by: [viewer-ui-interaction-harness, viewer-world-test-harness]
---

Context: [context/viewer.md](../context/viewer.md).

The one flow that spans both harnesses: `inventory_drag.rs` starts from a
UI row (`Pointer<DragStart>` ghost — the UI interaction tier) and resolves
on `DragEnd` against world targets via `MeshRayCast` (the world tier).

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
