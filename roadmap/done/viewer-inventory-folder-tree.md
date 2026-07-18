---
id: viewer-inventory-folder-tree
title: Inventory folder tree + item icons
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-inventory-ui
blocked_by: [viewer-ui-virtualized-list]
---

Context: [context/viewer.md](../context/viewer.md).

The core inventory browser: the **folder tree** view — expand / collapse
folders, lazily fetch descendents, and render each item with its **icon by asset
type**. Built on the virtualized list ([[viewer-ui-virtualized-list]]) so a
large inventory scrolls cheaply. This is the root inventory task the other
inventory views and actions build on.

The inventory **protocol and model** already exist (`protocol-5` /
`protocol-30`, `inventory.rs`, AIS3 / `FetchInventoryDescendents2`, disk cache);
this task is the tree UI that presents that model.

Reference (Firestorm, read-only): `llinventorypanel`, `llinventoryicon`,
`llinventoryfunctions`.

Builds on: the `inventory.rs` model and `protocol-5/30`.

## Done (2026-07-18)

Implemented as `sl-client-bevy-viewer/src/inventory.rs` (the whole inventory
window, `Ctrl+I`). Everything tab: the folder tree over both roots — agent "My
Inventory" and the read-only shared "Library" — with per-row and toolbar
expand/collapse, emoji icons by inventory/folder type, and lazy per-folder item
fetch through the session's existing high-level bridge (`QueryInventoryFolder`
auto-schedules the background fetcher/cache; `InventoryDescendents` triggers a
re-query for the resolved page). Built on [[viewer-ui-virtualized-list]]. Pure
tree-flatten (`build_rows`) is unit-tested; the registered `inventory-row`
element gives the row layout the harness matrix.
