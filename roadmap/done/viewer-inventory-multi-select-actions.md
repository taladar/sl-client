---
id: viewer-inventory-multi-select-actions
title: Multi-selection context actions + drag
topic: viewer
status: done
origin: split from viewer-inventory-context-actions (2026-07-21) — selection
  shipped, actions still act on the single clicked row
refs: [viewer-inventory-context-actions]
---

Context: [context/viewer.md](../context/viewer.md).

The inventory tree has full **selection** semantics (click / `Ctrl`-click /
`Shift`-click, `InventorySelection` in `inventory.rs`), and a right-click on
an unselected row retargets the selection — but the context-menu actions and
drag-and-drop still operate on the **single clicked / dragged row**. This
task makes them honour the whole selection, as the reference does:

- **Menu**: conditions become the *intersection* of the selected rows'
  capabilities (Delete enabled only if every row is deletable, Wear hidden
  on a mixed selection, …), and the wired actions loop over the selection —
  the batch wire commands exist (`RemoveInventoryObjects`,
  `MoveInventoryItem` batching, `RezAttachments`).
- **Drag**: dragging any selected row drags the selection (ghost shows a
  count), and the drop applies the move / copy / give to every row.

Reference (Firestorm, read-only): `llinventorybridge.cpp` (the
`LLInventorySort`/multi-select action paths), `llfolderview` (selection).

Shipped 2026-07-22: a right-click inside a >1 selection targets the
whole selection (view order): the menu's conditions are the
intersection of every row's set (mixed types lose the type blocks,
any undeletable row kills Delete, multi withholds Rename; pinned by a
unit test), an all-folder selection gets the folder menu. The batch
arms loop / batch: delete, purge (one RemoveInventoryObjects for a
mixed batch), restore, cut/copy (clipboard holds the whole selection,
paste and paste-as-link loop it), share (all given on one pick),
wear/add/take-off (one SetWearing)/detach, gesture (batched
(De)ActivateGestures), attach-to-point (first replaces, rest add),
open. Dragging a selected row drags the selection — the ghost shows
"N items", each row classifies and applies independently on drop
(list move/copy, give, self-wear, world rez).
