---
id: viewer-notecard-copied-item-loses-permissions
title: An item copied out of a notecard arrives with no permissions
topic: viewer
status: done
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

## What was found (2026-09-14)

**The item never lost its permissions. The viewer lost them on the way back
in.** All three suspects were cleared from the record the reported session left
behind on the local grid:

1. *What the viewer wrote into the asset.* The saved notecard is still in
   OpenSim's `Asset.db`, and its permission block is right — the dragged
   landmark went in with `base_mask`/`owner_mask` `0009e000` and
   `next_owner_mask` `0008e007`, in the reference's field order and `%08x`
   form, which is also what OpenSim's `SLUtil.GetEmbeddedItem` parses.
2. *What the grid made of it.* The copy is in `inventory.db` with
   `inventoryBasePermissions` / `inventoryCurrentPermissions` `0009e000` — full
   permissions, on both of the copies the reporting session made. OpenSim's
   `BunchOfCaps.CopyInventoryFromNotecard` skips its permission-propagation
   branch entirely here, because the embedded item's `owner_id` is the copying
   agent.
3. *Next-owner masks.* Not implicated on this grid (see 2), but see below —
   they are why the reference refuses some of these drops in the first place.

So the grid held a full-permission item and the tree drew it "(no copy) (no
modify) (no transfer)". The defect is in **`bulk_update_item_from_llsd`**
(`sl-proto/src/session/conversions.rs`): a `U32` field in an LLSD message is a
**4-byte big-endian binary** element, not an integer — the reference reads one
with `ll_U32_from_sd`, and OpenSim writes one with
`LLSDxmlEncode2.AddElem(name, uint)`. The five masks, `Flags`, `CallbackID` and
`CRC` are all `U32`. Reading them with `as_i32` returns `None` → `0`, so every
mask on an item pushed over the event queue was zero, and `item_suffix` spelled
out all three withheld permissions.

Nothing about this is specific to notecards: the same `BulkUpdateInventory` is
how a give, a paste-copy and a server-side change land, and all of them showed
an item with no permissions and no flags. It also **persisted** rather than
self-correcting, because `cache_inventory` merges the zeroed item into the
session's inventory and the folder re-query the viewer does next is answered
from that cache, not from the grid.

This is the same trap `ParcelProperties.ParcelFlags` sprang; the other
event-queue parsers (`TeleportFinish`, `CrossedRegion`, `EnableSimulator`,
`AgentGroupDataUpdate`) already use the tolerant `llsd_u32` / `llsd_u64`, and
this one had been missed.

## What was done

- `bulk_update_item_from_llsd` reads the five masks and `Flags` with
  `u32_member` (binary big-endian, integer, or decimal/hex string).
- `bulk_update_item_to_llsd` — the simulator direction, which `SimSession` and
  the fake grid serve — now **emits** those fields as the big-endian binary a
  real simulator sends, so a client fed by it is fed the shape it has to
  survive rather than a convenient one.
- The notecard editor refuses a drop whose item's **next-owner** mask is not
  `PERM_ITEM_UNRESTRICTED`, as `LLViewerTextEditor::handleDragAndDrop` does
  (`may_embed` in `edit_notecard.rs`, with the
  `notecard-status-drop-restricted` status line). An item copied back out of a
  notecard is a transfer, so on any grid that propagates permissions the copier
  gets the *next-owner* mask — embedding a restricted item promises a copy that
  arrives stripped, which is exactly the shape of this report.

## How it was verified

- `conversions::caps_serializer_tests::bulk_update_inventory_reads_binary_u32_masks`
  — an OpenSim-shaped `BulkUpdateInventory` body whose masks are written out as
  wire bytes (not through our own encoder, so the byte order is asserted rather
  than assumed). Every mask read `0` before the fix.
- `caps_serializer_tests::bulk_update_inventory_round_trip` — unchanged, and
  still exact across the new binary encoding.
- `edit_notecard::tests::body::a_next_owner_restricted_item_is_not_embedded`,
  and `an_item_dropped_since_the_load_appears_as_a_box` (whose fixture is now a
  droppable item — its next-owner mask used to be move+transfer only, which the
  reference would have refused).
- The grid-side record above, read straight out of `Asset.db` /
  `inventory.db`, which is what settled where the permissions were *not* lost.
