---
id: protocol-fetch-inventory-items-request
title: Fetching inventory items by id (FetchInventory2's request half)
topic: protocol
status: done
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
  `CAP_FETCH_INVENTORY_ITEM` (`FetchInventory2`, agent) /
  `CAP_FETCH_LIBRARY_ITEM` (`FetchLib2`, library) by the owner of each item,
  mirroring `fetch_folder_contents`' routing;
- the request body — each entry an `{ item_id, owner_id }` map. The builder
  exists already (audit 2026-10-15):
  `sl_wire::build_fetch_inventory_items_request` with `FetchItemRef`,
  round-tripped in `sl-wire/tests/llsd.rs` and parsed by the server side's
  `CapHandler::FetchItems` (`sl-proto/src/sim_caps.rs`); only the client command
  is missing, and the reply parser already exists;
- **both runtimes**, at parity: `sl-client-tokio` and `sl-client-bevy`;
- a `sl-conformance` case (`test-inventory-item-fetch`): fetch an item by id on
  a live grid and check the reply against what a folder fetch says about the
  same item.

If a refetch-on-announcement is ever wired on top of this, mind the tail it
would chase: the item-fetch reply is itself surfaced as an
`InventoryBulkUpdate`, so such a refetch must not treat its own answer as a new
announcement.

## Done (2026-11-26)

- `Command::FetchInventoryItems(Vec<InventoryKey>)`, wired at parity in
  `sl-client-tokio` and `sl-client-bevy` (`fetch_inventory_items` in each
  runtime's `inventory.rs`), plus `fetch_inventory_items` in `sl-repl`.
- The routing is sans-I/O: `Session::item_fetch_requests` splits the ids into
  one `ItemFetchRequest` per tree (the model's Library items to `FetchLib2`
  addressed to the Library owner, everything else — unknown items included —
  to `FetchInventory2` addressed to the agent), each carrying its
  `capability()` and `body()`. `Session::inventory_item_owner` answers which
  tree an item is filed in.
- **Beyond the listed scope:** where the region grants neither capability, the
  runtimes send the request as the legacy UDP `FetchInventory`
  (`Session::request_inventory_items`, 32 blocks to a message), and the
  session now decodes `FetchInventoryReply` into the same nil-transaction
  `Event::InventoryBulkUpdate`, each item folded into its folder's tree. This
  mirrors `fetch_folder_contents`, which falls back to UDP the same way.
- **Found:** a Library item cannot be fetched by id on OpenSim. It grants no
  `FetchLib2`, and its UDP `FetchInventory` handler
  (`AsyncInventorySender.ProcessQueue`) reads `InventoryService.GetItem`, which
  does not hold the in-memory Library, so nothing comes back — measured live
  (the `FetchInventory` naming the Library owner went out, no reply). The fake
  grid's OpenSim flavour had answered Library items there; it now answers the
  agent's tree only, as OpenSim does.
- Conformance case `inventory-item-fetch` (1av): three items of each tree plus
  an unknown id in one command; every agent item (and on aditi every Library
  item) must come back matching its folder listing field by field, the unknown
  id must not, and on OpenSim the Library items must stay unanswered for 5 s.
  Passed on OpenSim and aditi.
- Tests: `item_fetch_requests_route_each_item_to_its_tree` (`sim_caps.rs`) and
  `session_udp_item_fetch_is_answered_and_merged` (`sim_session.rs`).

Not done here, as the task says: refetching an item on its announcement.
