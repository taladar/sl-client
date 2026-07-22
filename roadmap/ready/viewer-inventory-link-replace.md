---
id: viewer-inventory-link-replace
title: Inventory link-replace tool
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's link-replace floater: given a **source** item and a **target**
item, find every inventory *link* pointing at the source (worn outfits'
links, COF entries) and repoint it at the target — the tool for swapping a
discontinued body part / attachment across every outfit at once. With AIS3
link create/delete already implemented, the operation is: enumerate links to
the source from the held inventory model, then batch delete + recreate
against the target, with progress and a summary (N links replaced, outfits
touched), and a dry-run preview list first — the operation is not undoable.

Reference (Firestorm, read-only): `fsfloaterlinkreplace`,
`floater_linkreplace.xml`.

Builds on: the held inventory model + AIS3 mutation (`protocol-30`).
