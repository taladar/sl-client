---
id: viewer-inventory-ui
title: Inventory browser UI
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-ui-virtualized-list]
---

Context: [context/viewer.md](../context/viewer.md).

The inventory browser: folder tree + gallery views, search / filter, item icons
by asset type, drag-and-drop, context actions (wear / rez / give / delete /
rename), a recent-items view, and a worn/current-outfit tab.

The inventory **protocol and model** already exist (`protocol-5` /
`protocol-30`, `inventory.rs`, AIS3 / `FetchInventoryDescendents2`, disk cache);
this stub is the UI that presents and mutates that model.

Reference (Firestorm, read-only): `llinventorypanel`, `llinventorybridge`,
`llinventorygallery`, `llinventoryfilter`, `llinventoryfunctions`,
`llinventoryicon`.

Builds on: the `inventory.rs` model and `protocol-5/30`.

Deps: [[viewer-ui-virtualized-list]].
