---
id: inventory-stale-folder-fetch-overwrites-newer-state
title: A stale folder fetch overwrites newer inventory, including the outfit folder
topic: inventory
status: done
origin: mixed-channel race audit (2026-11-23)
refs: [viewer-outfit-change-during-bake-is-lost]
---

Context: [context/inventory.md](../context/inventory.md).

Confirmed by reading the code. Not reproduced live.

## Bad order

A descendents fetch is in flight, an AIS reply or `RemoveInventory*` changes
the folder, then the older fetch reply lands. Two quick outfit edits are
enough: the first triggers the refetch and the second lands during it.

## Cause

In `sl-proto/src/session/inventory.rs`, `invalidate_folder` only acts on
`FolderState::Loaded`, so it does nothing while a fetch runs.
`mark_folder_loaded` then stores the reply's version unconditionally, and
`cache_inventory` adds back items that were removed.

Descendents replies also never prune items they no longer list, so the
"reconverges" comment in `session/methods.rs` overstates what the session model
does.

## Effect

A removed link or item reappears and the Current Outfit Folder version moves
backwards. The bake's mismatch retry repairs the grid's side, but the local
model stays wrong until the folder next changes or the next login.

## Fix

Record that a folder went dirty while fetching and fetch it again on the reply,
reject replies older than the held version, and prune absent items from a
current reply.

## Fix (2026-11-24)

Fixed as planned, in `Inventory::fold_descendents`. All three reply paths go
through it: the descendents capabilities, AIS3 fetch listings and UDP.

### Refusing a stale reply

- Every mutation of a folder's contents while its fetch is in flight sets
  `changed_while_fetching`. That covers an item filed, moved, removed, renamed
  or rebound, and a sub-folder added, moved or removed.
- `invalidate_folder` now also acts on a `Fetching` folder: it records the
  change and advances the version at once.
- A reply is refused when the change flag is set or when its version is below
  the one held. The folder goes back to `Unknown` and the session surfaces no
  event for it.
- If the background crawl is off, the session refetches the folder itself, so
  an explicit wait still gets an answer.
- Each refusal counts against `INVENTORY_FETCH_MAX_ATTEMPTS`. Once that is
  spent the reply is folded, so versions that disagree across channels cannot
  cause a refetch loop.
- The other packets of a refused UDP reply are refused with it and do not count.

### Pruning

- A complete reply (capability or AIS3) drops the items and sub-folders it no
  longer lists.
- A UDP packet lists only part of a folder and prunes nothing.

### Tests

- Five unit tests in `sl-proto/src/session/inventory.rs`: the audit's
  removal-during-fetch order, an AIS update during a fetch, pruning, the rest of
  a refused reply, and the retry bound.
- The whole `sl-proto` suite passes.
