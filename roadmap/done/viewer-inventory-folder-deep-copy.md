---
id: viewer-inventory-folder-deep-copy
title: Folder Copy / Paste (recursive deep copy)
topic: viewer
status: done
origin: split from viewer-inventory-context-actions (2026-07-21) — shipped
  greyed (UNIMPLEMENTED)
refs: [viewer-inventory-context-actions]
---

Context: [context/viewer.md](../context/viewer.md).

**Copy** on a folder, and pasting it: create a fresh folder tree at the
destination and `CopyInventoryItem` every copyable item into it, recursively.
Item copy / cut / paste and folder **cut** (move-on-paste) are live
(`inventory_actions.rs`); folder *copy* is greyed because it needs the
recursive walk, per-item copy-permission handling (skip or link no-copy
items, as the reference decides), and creation of the destination subtree
before the items land. Also unlocks copying a Library **folder** out (today
only Library items copy; a dragged Library folder is rejected).

Reference (Firestorm, read-only): `llinventoryfunctions.cpp`
(`copy_inventory_category`), `llinventorybridge.cpp`.

Shipped 2026-07-22: folder Copy is live (user + outfit folders, Library
included — the Library-folder copy-out unlock), Copy prefetches the
subtree pages, and paste / a Library-folder drag plans the reference's
copy_inventory_category walk: create the destination folder first
(client-picked id), copy the loaded copyable items, re-link links, skip
no-copy items, recurse. Only loaded contents copy; the prefetch makes
that the common case.
