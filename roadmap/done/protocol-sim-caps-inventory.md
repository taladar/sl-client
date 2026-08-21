---
id: protocol-sim-caps-inventory
title: Server-side inventory caps — AISv3 and the legacy fetch caps
topic: protocol
status: done
origin: user request (2026-07) — complete simulator protocol surface
points: 8
blocked_by: [protocol-sim-caps-framework]
---

Context: [context/protocol.md](../context/protocol.md).

AISv3 server side (`InventoryAPIv3`, `LibraryAPIv3`,
`CreateInventoryCategory`) plus the legacy `FetchInventory2` /
`FetchLibrary2` caps, over an in-memory inventory-tree fixture (shared
with the UDP descendents serving in [[protocol-sim-udp-flows]]).

The 17 client-direction functions in `sl-wire/src/inventory.rs` get their
`parse_*_request`/`build_*_response` inverses, per the inverse-pairing
convention; each verified by round-tripping against its client
counterpart in-memory.

Done (2026-08-20): seven `REQUESTED_CAPABILITIES` rows now Served in the
pinned coverage table — the five existing Pending rows plus two the task
added end-to-end, the per-item `FetchInventory2`/`FetchLib2` (maximal
scope; note the real cap name is `FetchLib2`, the task text's
"FetchLibrary2" doesn't exist — verified in Firestorm
`llviewerregion.cpp`). Serving store: a new `SimInventoryTree`
(`sl-proto/src/sim_inventory.rs`), held twice on `SimSession` (agent +
read-only Library); AIS3 mutations apply to the agent tree with per-AIS
`version` bumps reported via `_updated_category_versions`, and each
surfaces one of nine new inventory `ServerEvent`s. Four new `CapHandler`
variants (`FetchDescendents`, `FetchItems`, `Ais3` with full verb ×
sub-path REST routing — `LibraryAPIv3` GET-only — and
`CreateInventoryCategory`); unknown AIS3 targets answer 404, invalid
moves 400, while the batch fetches stay tolerant. The AIS3 codec was
already bidirectional (Tier-F #61); the genuinely new inverses are
`parse_fetch_inventory_request`, `build/parse_fetch_inventory_items_request`,
`parse_ais_create_link_body` (+`AisLinkCreate`), `ais_update_to_llsd`,
and the reply serializers `fetch_inventory_items_to_llsd` (+ client
`_from_llsd` fold and the two new CAP constants folded into
`Event::InventoryBulkUpdate`), `ais_mutation_reply_to_llsd`,
`ais_category_children_reply_to_llsd` (subtree deliberately flattened
into top-level `_embedded` — our client parser reads only that level),
`ais_item_reply_to_llsd`. Verified by ten loopback tests driving the
real client builders/parsers (and `Session` folds) against
`SimCaps::dispatch` (`sl-proto/tests/sim_caps.rs`) plus sl-wire codec
round-trips; book coverage is the new "The inventory handlers" section
of `book/src/comms/caps.md`.
