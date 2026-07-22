---
id: viewer-wearable-favorites
title: Wearable favorites floater
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold, viewer-ui-virtualized-list]
refs: [viewer-inventory-worn-actions, viewer-inventory-attach-to-point]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's favorite-wearables floater: a small, always-handy list of
hand-picked wearables / attachments for quick wear / take-off without diving
into inventory — the "jewellery box" for things swapped many times a day.
Entries are added from the inventory context menu ("Add to wearable
favorites"), the list shows worn state, and clicking toggles wear / detach
via the existing wear machinery ([[viewer-inventory-worn-actions]],
[[viewer-inventory-attach-to-point]]). Persist the list per account
(inventory links in a client-managed folder, as the reference does, so it
survives across viewers).

Reference (Firestorm, read-only): `fsfloaterwearablefavorites`,
`floater_fs_wearable_favorites.xml`.

Builds on: the wear/detach actions and the inventory link machinery.
