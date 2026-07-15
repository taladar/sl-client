---
id: viewer-inventory-outfit-tab
title: Worn / current-outfit tab + recent items
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-inventory-ui
blocked_by: [viewer-inventory-folder-tree]
---

Context: [context/viewer.md](../context/viewer.md).

The **worn / current-outfit** tab — what the avatar is currently wearing, with
detach / take-off actions — and the **recent-items** view (items added since
login). Both are filtered views over the same inventory model the folder tree
([[viewer-inventory-folder-tree]]) presents (the current-outfit folder and a
recency filter).

Builds on the folder tree's model; this task is the two curated tabs.

Reference (Firestorm, read-only): `llinventorypanel`,
`llinventoryfunctions`.

Builds on: [[viewer-inventory-folder-tree]] and the `inventory.rs` model.
