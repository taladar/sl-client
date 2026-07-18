---
id: viewer-inventory-outfit-tab
title: Worn / current-outfit tab + recent items
topic: viewer
status: done
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

## Done (2026-07-18)

The **Recent** and **Worn** tab *views* shipped in
`sl-client-bevy-viewer/src/inventory.rs`. Recent = items received since login
(`InventoryBulkUpdate` / `InventoryItemCreated`), newest-first, deduped,
bounded. Worn = the Current Outfit Folder's contents (authoritative on SL),
falling back to the legacy `AgentWearables` set (type labels) on a grid that
does not populate the COF.

The **detach / take-off actions** on a worn item are a mutation and were left
out (no context menu / mutation actions exist yet) — follow-up
[[viewer-inventory-worn-actions]].
