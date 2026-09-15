---
id: viewer-inventory-link-drop-to-world-rezzes-nothing
title: Dragging an inventory link onto the ground rezzes nothing
topic: viewer
status: done
origin: user report during the aditi live check of
  [[viewer-rigged-attachments-wearer-not-resolved]] (2026-09-15)
refs: [viewer-inventory-link-tools, viewer-rigged-attachments-wearer-not-resolved]
---

Context: [context/viewer.md](../context/viewer.md).

Dragging an inventory **link** to an object item onto the ground in world does
nothing — no rez, no error. Dragging the original item does rez. The drop
should rez what the link points at, as the reference does.

Likely cause (unconfirmed): the drop path in
`sl-viewer-inventory/src/inventory_drag.rs` gates the rez on
`matches!(item.inv_type, InventoryType::Object | InventoryType::Attachment)`
and builds `rez_object_command` from the dragged row's own fields. A link row
carries the link's asset type (`Link`) and its own item id, so either the gate
rejects it or the command names the link rather than the target.

Reference behaviour to port (Firestorm, read-only): a link resolves through to
its target almost everywhere a drag reads it — `LLInvFVBridge::startDrag` takes
the drag type from `getActualType()`, and `LLViewerInventoryItem`'s getters
(`getAssetUUID`, `getInventoryType`, …) forward to `getLinkedItem()`, while
`getPermissions` deliberately stays the link's own. Check in
`indra/newview/lltooldraganddrop.cpp` (`dad3dRezObjectOnLand`,
`dropObject`) which item id the `RezObject` message carries for a link before
choosing between sending the link's id and the target's.

Verify: drag a link to an object from inventory onto the ground on the local
OpenSim and on aditi — the target object rezzes; a broken link (target gone)
is refused with a message rather than silently ignored. Also check dropping a
link onto an object (contents drop) and onto the avatar (wear), which share the
same gate.

## Root cause (2026-09-15)

The gate was not the problem: a link carries its **target's** inventory type,
so `InventoryType::Object` let it through. The rez then named the link — its
own item id and asset type `Link` — which is not an object the grid can rez,
and the grid ignored it without a word. Give, self-wear and the contents /
notecard drops sent the link's own item the same way.

## The reference does not rez a link at all

The premise above was wrong. In the inventory tree, `LLInvFVBridge::startDrag`
takes the drag type from `getActualType()`, which is `AT_LINK`, so a link
drags as `DAD_LINK` — and every in-world handler registered for `DAD_LINK` in
`lltooldraganddrop.cpp` is `dad3dNULL` (self, avatar, object, land). Firestorm
refuses the drop with a no-drop cursor. (Its inventory *gallery* is
inconsistent: it drags by the resolved `getType()` and so sends a `RezObject`
naming the link's own id, exactly the bug this was.)

The user chose to **resolve through** instead, going beyond the reference.

## Fix

- `resolve_drop_source` (`inventory_drag.rs`): outside the list, an item link
  acts as the item it points at, a folder link as that folder; the Library flag
  is the target's. A link whose target is not in the mirror, or is itself a
  link, is refused by name with a `LocalChatNotice`. Over the list a link still
  moves as itself.
- Resolved once at drag start into `ActiveDrag`, because the hover outline reads
  the sources every frame and a link costs a scan of the mirror.
- Found while verifying the broken-link refusal: **Empty Trash left the Trash's
  sub-folders drawn**, and the items inside them still answered `find_item`, so
  a link to a purged object resolved and the drop silently did nothing again.
  `InventoryModel::merge_folders` only adds and re-parents; a complete
  `InventoryFolderPage` now prunes the sub-folders it no longer lists, with
  their subtree and items (`prune_unlisted_children`).
- Found on aditi while verifying the same:
  **Delete and Empty Trash changed nothing on Second Life.** The AIS3 branches
  of `MoveInventoryItem` (a Delete is a move to the Trash),
  `MoveInventoryFolder`, `RemoveInventoryFolders` and
  `PurgeInventoryDescendents` sent the request but skipped the cache mutation
  their UDP twins apply, so the folder page re-read right after was built from
  an unchanged cache; the reply re-files a moved item but prompts no re-read of
  the folder it left, and its removal lists are not parsed. `sl-proto` now has
  wire-free `move_inventory_items_local` / `move_inventory_folders_local` (with
  the UDP path's cycle / unknown-parent check) /
  `remove_inventory_folders_local` / `purge_inventory_descendents_local`, called
  from the AIS3 branch of **both** `sl-client-bevy` and `sl-client-tokio` (only
  `RemoveInventoryItems` had one).

Unit tests: `a_link_resolves_to_what_it_points_at`,
`a_non_link_resolves_to_itself`,
`a_resolved_link_takes_its_targets_library_flag`,
`a_broken_link_is_refused_by_name`,
`a_complete_page_prunes_removed_sub_folders`,
`b4_local_inventory_mutations_change_the_cache_and_send_nothing`.

## Verified

- Local OpenSim (user): a link rezzes its object on the ground and on a prim,
  attaches when dropped on the own avatar, gives a folder link's folder, and
  still moves as itself between folders; Empty Trash removes the Trash's
  sub-folders; a link to a purged object is refused with the notice.
- aditi (user): a link rezzes its object; Delete moves items and folders out of
  their folder at once, Empty Trash empties the Trash, and a link to a purged
  object is refused with the notice.
