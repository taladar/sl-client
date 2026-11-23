---
id: inventory-stale-folder-fetch-overwrites-newer-state
title: A stale folder fetch overwrites newer inventory, including the outfit folder
topic: inventory
status: bugs
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
