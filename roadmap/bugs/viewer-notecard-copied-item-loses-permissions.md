---
id: viewer-notecard-copied-item-loses-permissions
title: An item copied out of a notecard arrives with no permissions
topic: viewer
status: bugs
origin: user report (2026-09-13, testing the notecard body)
refs: [viewer-notecard-editor, viewer-notecard-inline-items,
  protocol-audit-legacy-permission-blocks]
---

Context: [context/viewer.md](../context/viewer.md).

Drag a **full-permission** item into a notecard, save it, then click the
embedded item and accept the copy: the item that lands in inventory reads
**(no copy) (no modify) (no transfer)**. The permissions went somewhere between
the item in inventory and the item copied back out of the notecard.

Where it is **not** happening, as far as this can be established offline: the
encode. `edit_notecard`'s `dropped_item_round_trips_through_the_notecard`
asserts that a dragged item's `owner_mask` survives `to_embedded_item` plus a
full `Notecard::encode` / `decode` round trip with `0x7fff_ffff` intact, and the
`sl-notecard` crate's own tests cover the permission block's wire form. So the
suspects are, in the order worth checking:

1. **what the viewer puts in `ItemInfo.permissions`** for the dragged item —
   full permissions in the inventory row does not prove the five masks reached
   the drop; check them at the drop, not at the display;
2. **`CopyInventoryFromNotecard`** — the cap POST carries only ids
   (`notecard-id`, `object-id`, `item-id`, `folder-id`, `callback-id`), so the
   permissions the new item gets are the **simulator's** reading of the embedded
   item in the saved asset. Fetch the saved notecard's asset and read its
   permission block directly (`sl-repl`, or the OpenSim asset DB) — that settles
   viewer-side versus grid-side in one step;
3. **next-owner masks** — if the grid is applying next-owner permissions on the
   copy, an item whose `next_owner_mask` is restrictive would arrive exactly
   like this, and the bug is then that we write a next-owner mask the item does
   not have.

Test on **both** grids before concluding: OpenSim's `CopyInventoryFromNotecard`
is its own implementation, and "the local grid strips them" is a different bug
from "we wrote them wrong".
