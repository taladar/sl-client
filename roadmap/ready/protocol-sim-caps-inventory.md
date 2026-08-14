---
id: protocol-sim-caps-inventory
title: Server-side inventory caps — AISv3 and the legacy fetch caps
topic: protocol
status: ready
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
