---
id: viewer-inventory-protected-folders
title: Protected inventory folders
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-inventory-folder-tree]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's protected folders: mark chosen folders as protected so the
destructive inventory actions — delete, move, cut — are refused (with a
clear toast) unless protection is lifted first. Purely client-side guard
rails against the classic accidental drag of a whole inventory branch; the
protected set persists per account and shows a lock decoration on the folder
row ([[viewer-inventory-row-decorations]] already draws row badges).

Reference (Firestorm, read-only): `fsfloaterprotectedfolders`,
`floater_fs_protectedfolders.xml`.

Builds on: the inventory folder tree + context actions and the account-scoped
settings store.
