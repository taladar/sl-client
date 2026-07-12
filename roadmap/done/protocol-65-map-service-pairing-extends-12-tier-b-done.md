---
id: protocol-65
title: Map service pairing (extends #12, Tier B). Done
topic: protocol
status: done
origin: ROADMAP.md — Tier F
---

Context: [context/protocol.md](../context/protocol.md).

**65. Map service pairing (extends #12, Tier B). ✅ Done.** Server-side encoders
for the `MapBlockReply` / `MapItemReply` payloads the map server returns, the
exact inverse of the client decoders (`map_region_info` / `map_item`) and a
mirror of the client request encoders (`send_map_block_request` /
`send_map_name_request` / `send_map_item_request`). Two free builders in
`session.rs`, re-exported from `sl-proto`: **`build_map_block_reply`** — turns a
`&[MapRegionInfo]` into a `MapBlockReply` (grid coords truncated to the wire
`u16`, name NUL-terminated, maturity → `SimAccess` byte via `to_sim_access`),
emitting the parallel `Size` block for every entry whenever any region is not
the standard 256 m (mirroring OpenSim's `SendMapBlock` `needSizes` logic) and
omitting it when all are 256 m (the size the client assumes for a missing
block); and **`build_map_item_reply`** — turns a `&[MapItem]` plus a
`MapItemType` into a `MapItemReply` (global-metre coords, `extra`/`extra2`
verbatim, name NUL-terminated). Both surfaced on `SimSession` as
**`send_map_block_reply`** / **`send_map_item_reply`** (reliable, agent block
filled from the session's agent id + the request's echoed map-layer flag); the
255-entry wire count byte caps a single reply (longer runs split by the caller).
Same public-doc intra-doc-link gotcha as #54–#64 (the `pub` builders reference
the client events, not the private decoders). *Test: 2 new loopback round-trips
in `sim_session.rs` (`SimSession` → client `Session`) — a standard + a variable
512 m region through `MapBlockReply` (full `MapRegionInfo` round-trip including
the size block), and a `MapItemReply` of `AgentLocations` items — alongside the
12 existing `SimSession` tests.* **Tier F (#52–#65) complete — the
bidirectional protocol surface is now whole.**
