---
id: viewer-inventory-worn-tab-items-without-folders
title: Worn tab lists Skin / Hair / Eyes / Shirt without their containing folders
topic: viewer
status: bugs
origin: user report (2026-07-31, aditi live testing)
---

Context: [context/viewer.md](../context/viewer.md).

## Symptom

In the inventory **Worn** tab, some worn items (observed: Skin, Hair, Eyes, and
a Shirt) appear **without their containing folder hierarchy** — listed loose
rather than nested under (or grouped by) the folder that actually holds them.
The Worn tab is meant to place each worn item within its real folder path (the
viewer kicks off `request_all_agent_folders` so every worn item can be located
in its hierarchy), so an item whose parent folder is not yet resolved shows
folderless.

## Where to look

- `inventory.rs` — the Worn-tab view construction: how it enumerates worn items
  and resolves each to its parent folder for display (`request_worn_source`,
  `request_all_agent_folders`, and the worn-item → folder placement).
- Whether the affected items' parent folders are simply **not fetched yet** (a
  timing/laziness issue — the placement should wait for or trigger the parent
  folder fetch) or are body-part items filed at a location the Worn view does
  not walk up from.
- Relationship to [[viewer-inventory-worn-markers-show-early]] (both stem from
  the Current-Outfit / worn-set resolution depending on lazily-fetched folders).

## Verify

Live on aditi: open the Worn tab and confirm every worn item (including Skin,
Hair, Eyes, and clothing layers) appears under its correct containing folder,
not loose.
