---
id: viewer-inventory-gallery
title: Inventory gallery view
topic: viewer
status: done
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

Shipped 2026-07-22 (inventory_gallery module): a single-folder
thumbnail grid floater with back / forward / up navigation (the
reference's LLInventoryGallery shape, 130 px tiles), toggled from the
gear menu's new "Gallery View" entry (checked while open; opens on the
selected folder). Tiles share the tree's model and selection; a
right-click opens the same context menus (via the refactored shared
opener); double-click descends into folders or opens an item's
preview. Texture / snapshot tiles resolve their own asset as the
thumbnail — the wire model carries no per-item thumbnail ids — and
every other type shows its icon glyph large. List / combination view
modes are not carried over.
