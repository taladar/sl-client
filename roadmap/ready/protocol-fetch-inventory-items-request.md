---
id: protocol-fetch-inventory-items-request
title: Fetching inventory items by id (FetchInventory2's request half)
topic: protocol
status: ready
origin: noticed while chasing viewer-notecard-copied-item-loses-permissions (2026-09-14)
refs: [viewer-notecard-copied-item-loses-permissions]
---

Context: [context/protocol.md](../context/protocol.md).

This workspace can **read** a `FetchInventory2` / `FetchLib2` reply — the
per-item fetch decodes in `handle_caps_event` under `CAP_FETCH_INVENTORY_ITEM`,
merges into the tree and surfaces as an `InventoryBulkUpdate` — but nothing can
**ask**. There is no command, so there is no way to fetch one item by id; the
only read path is by folder (`FetchInventoryDescendents2`).

The reference viewer uses the per-item fetch routinely, and for a reason worth
recording: it does not treat an announced item as complete. Both
`LLInventoryModel::processBulkUpdateInventory` and
`processUpdateCreateInventoryItem` end by refetching every item they were just
handed —

> `// Temporary workaround: just fetch the item using AIS to get missing fields.`

— because the announcement's block predates fields the item now has
(thumbnails, and whatever comes next). We cannot do that at all, and the
folder-sized alternative is too coarse to do casually: refetching a whole
folder to correct one item is fine for a folder of four, wasteful for a folder
of four hundred.

Note what this is **not**. [[viewer-notecard-copied-item-loses-permissions]]
looked exactly like a missing-fields problem — an announced item arriving with
no permissions — and was not: our LLSD parser was reading a binary `U32` as an
integer and getting zero. That bug is fixed at the parser. This task is the
remaining *capability* gap, and should not be justified by that symptom.

Scope:

- `Command::FetchInventoryItems(Vec<InventoryKey>)`, routed to
  `CAP_FETCH_INVENTORY` (agent) / `CAP_FETCH_LIBRARY` (library) by the owner of
  each item, mirroring `fetch_folder_contents`' routing;
- the request body — the same shape the descendents request uses for folders,
  each entry an `{ item_id, owner_id }` map
  (`build_fetch_inventory_request` is the model to follow, and the reply parser
  already exists);
- **both runtimes**, at parity: `sl-client-tokio` and `sl-client-bevy`;
- a `sl-conformance` case (`test-inventory-item-fetch`): fetch an item by id on
  a live grid and check the reply against what a folder fetch says about the
  same item.

If a refetch-on-announcement is ever wired on top of this, mind the tail it
would chase: the item-fetch reply is itself surfaced as an
`InventoryBulkUpdate`, so such a refetch must not treat its own answer as a new
announcement.
