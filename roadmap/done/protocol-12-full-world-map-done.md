---
id: protocol-12
title: Full world map (done)
topic: protocol
status: done
origin: ROADMAP.md — Tier B
---

Context: [context/protocol.md](../context/protocol.md).

**12. Full world map (done) ✅ — `MapItemRequest`
(agents/telehubs/events/land-for-sale), `MapNameRequest`, `MapBlockRequest` by
name · 5 pts.** Extends the existing `MapBlockRequest`/`MapBlockReply` to a
complete map: avatar dots, POIs, search-by-name — a live map tool on its own.
`Session::request_map_by_name` (search regions by name → `Event::MapBlock`, the
same reply as a block request) and
`request_map_items(MapItemType, region_handle)` →
`Event::MapItems { item_type, items }`, where `MapItemType` covers
`Telehub`/`AgentLocations`/`LandForSale`/the event types/`Other(u32)` and each
`MapItem` carries global coordinates (with `region_handle()`/`local_x()`/
`local_y()` helpers), an id, the type-specific `extra`/`extra2`, and a name.
Both map requests send the viewer's map-layer flag (`LAYER_FLAG = 2`). Wired
through both runtimes (`Command::RequestMapByName` / `RequestMapItems`).
*Live-verified against local OpenSim with two avatars: `MapNameRequest("East
Region")` resolved the neighbour by name, and an `AgentLocations` request
returned the second avatar's map dot at the right global coordinates. Stock
OpenSim answers agent-locations, telehubs and land-for-sale locally; events and
classifieds are not implemented server-side.*
