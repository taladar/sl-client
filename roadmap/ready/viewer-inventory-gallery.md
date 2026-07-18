---
id: viewer-inventory-gallery
title: Inventory gallery view
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-inventory-ui
blocked_by: [viewer-inventory-folder-tree]
---

Context: [context/viewer.md](../context/viewer.md).

The **gallery** view of inventory: a thumbnail grid over the same folder /
item model the tree ([[viewer-inventory-folder-tree]]) presents, with texture
thumbnails resolved through the existing asset pipeline. An alternate
presentation of one folder's contents, sharing the tree's model and selection.

Builds on the folder tree's model + descendent fetching; this task is the grid
layout and thumbnail rendering.

Reference (Firestorm, read-only): `llinventorygallery`.

Builds on: [[viewer-inventory-folder-tree]].
