---
id: viewer-inventory-worn-tab-items-without-folders
title: Worn tab lists Skin / Hair / Eyes / Shirt without their containing folders
topic: viewer
status: done
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

## Resolution

The hypotheses above (parent folder not fetched / body parts filed elsewhere)
were both wrong. The real cause: `InventoryModel::worn_item_keys` **unioned**
the legacy `AgentWearables` set on top of the Current Outfit Folder links. On
modern SL the COF is authoritative, and the legacy set also carries the built-in
*system-default* shape / skin / hair / eyes that back every avatar but are not
real inventory items — so those defaults were listed as worn with no folder.
Tells: the rows were labelled with generic slot names ("Skin"/"Shirt") from
`wearable_label` (not item names, so they came from the legacy branch, not COF);
Firestorm on the same account does not list them; and it was
Skin/Hair/Eyes/Shirt rather than the four mandatory body parts
(Shape/Skin/Hair/Eyes), i.e. avatar-specific BoM legacy-slot noise.

Fix: make `worn_item_keys` a true **fallback** — use the COF alone whenever it
*exists* (keyed on the folder's presence in the skeleton, not its loaded
contents, so a not-yet-fetched or transiently-empty COF page does not briefly
flash the legacy defaults), and fall back to `AgentWearables` only when there is
no COF folder at all (a grid that does not use it). Verified on aditi (phantoms
gone, no flash) and on the local OpenSim grid (worn items still listed — its COF
is populated, so no empty-Worn regression). The brief empty Worn tab before the
COF page arrives is ordinary fetch latency, not a defect.

An abandoned first approach — a targeted `Ais3FetchItem` single-item resolver
(plus a new `Ais3FetchLibraryItem` / LibraryAPIv3 command) to *locate* each
unplaced item's folder — was implemented and then reverted: the agent AIS3 GET
returned these phantoms with a nil parent (the API disowns them), confirming
they are not real items, and the COF fix removes them outright, leaving that
machinery unexercised.
