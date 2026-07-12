---
id: inventory-a5
title: Design cache load / merge-with-skeleton / version-validity
topic: inventory
status: done
origin: INVENTORY_ROADMAP.md
---

Context: [context/inventory.md](../context/inventory.md).

**A5. Design cache load / merge-with-skeleton / version-validity.** The
    login sequence: load the disk cache (if present + version-valid) into the
    model, then receive the skeleton, then **merge** — for each skeleton
    folder, if the cached version equals the skeleton version keep the cached
    contents and mark `Loaded`, else drop the cached contents and mark
    `Unknown` (refetch); add skeleton folders absent from the cache as
    `Unknown`; drop cached folders absent from the skeleton (deleted
    server-side). Items are kept only under a folder that stayed `Loaded`.
    This logic is **pure** (`sl-proto`, no I/O): it takes the loaded model +
    the skeleton and returns the merged model + the set of folders needing
    fetch. Mirror Firestorm `loadSkeleton`.

## Cache load / merge reference (from A5)

Pure (sl-proto, no I/O): a `merge_skeleton(cached, skeleton)` taking the loaded
`Inventory` plus the skeleton `&[InventoryFolder]` and returning the merged
`Inventory` and the `Vec<InventoryFolderKey>` of folders needing fetch —
mirroring Firestorm `loadSkeleton`. Start from `cached`; for each skeleton
folder `s`: if a cached entry exists with the same version, keep its contents
and set `Loaded`; else (absent or version differs) insert/keep the folder
`Unknown` and drop any cached child items/folders beneath it. Drop cached
**agent** folders absent from the skeleton (server-deleted); the **library**
subtree is not in the skeleton and is handled per-owner. Items survive only
under a folder that stayed `Loaded`. Return the merged model and the list of
`Unknown` folders (the initial fetch queue, A6). The runtime shell (A4) loads
the disk file and hands it in as `cached`.

**Algorithm verified against Firestorm + the code (anchors for B5).** The
skeleton `merge_skeleton` consumes is exactly the `Vec<InventoryFolder>` the
login arm already builds via `skeleton_folder` (`conversions.rs:951`, copying
the authoritative `folder.version` at `:957`): the **agent** skeleton at
`methods.rs:1216-1226` (also `Event::InventorySkeleton`) and the **library**
skeleton at `:1207-1213` (`Event::LibraryInventory`). So merge runs **once per
owner** — agent skeleton against the agent cache, library skeleton against the
library cache — mirroring Firestorm, which calls `loadSkeleton(options,
owner_id)` once for the agent and once for `getLibraryOwnerID()`
(`llinventorymodel.cpp:2886`), each with its own cache file. No merge function
exists yet (grep-confirmed) — B5 is net-new.

Per-folder rule confirmed against `loadSkeleton` (`:3025-3171`), with
Firestorm's `VERSION_UNKNOWN = -1` (`llviewerinventory.h:208`) ⇒ our
`FolderState::Unknown`: (1) a cached folder present in the skeleton with an
**equal** version is kept `Loaded` (added to `cached_ids`, `:3055-3070`); (2)
version **differs** ⇒ `setVersion(NO_VERSION)` ⇒ `Unknown`/refetch
(`:3055-3061`); (3) a skeleton folder **absent from cache** is added `Unknown`
(`:3076-3088`); (4) a cached folder **absent from the skeleton** is dropped
("removed from inventory", `:3049-3054`); (5) items survive **only** under a
parent still `version != NO_VERSION`, i.e. `Loaded` (`:3106`) — exactly the
reference's "items kept only under a `Loaded` folder".

Version-validity gate (A4 cross-ref): `loadFromFile` (`:3661`) yields the cached
model only when the 4-byte BE header **equals** `5` (`sCurrentInvCacheVersion`,
`:3694`) **and** the binary parse succeeds; on either failure
`is_cache_obsolete` stays true and `loadSkeleton` takes the `else` branch
(`:3162-3171`) marking **every** skeleton folder `Unknown` — a full refetch. So
B5's `inventory_from_cache_bytes` returns the same "treat as no cache" outcome
on a header/parse mismatch, and `merge_skeleton` against an empty cache yields
every skeleton folder `Unknown`.

One wrinkle B5 may fold or skip: Firestorm force-invalidates a folder whose
cached item set is suspect — an item loaded with asset type `AT_UNKNOWN` adds
its parent to `cats_to_update` (`loadFromFile:3751-3753`), which forces that
folder `NO_VERSION` (`:3043-3046`). It is a corruption-recovery guard that only
ever **adds** to the refetch set (never keeps a stale folder), so B5 may either
mirror it (invalidate a folder whose items fail to decode) or rely on the
converters rejecting malformed items earlier; either is sound.
