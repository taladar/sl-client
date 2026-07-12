---
id: api-df2
title: SimSession decode-side for world-map requests
topic: api
status: done
origin: SL_API_ROAD_MAP.md
---

Context: [context/api.md](../context/api.md).

## DF2 — `SimSession` decode-side for world-map requests

The world-map messages are asymmetric on the server side: `SimSession` has the
reply *encoders* (`send_map_block_reply` / `send_map_item_reply` /
`send_map_layer_reply`) but does **not** decode the incoming `MapBlockRequest` /
`MapNameRequest` / `MapItemRequest` / `MapLayerRequest` into a `ServerEvent`
(they fall through to `ServerEvent::ClientMessage`). This predates G16 — G16
mirrored the existing `RequestMapBlocks` / `RequestMapItems` pattern exactly
rather than adding a one-off. A real map service would want the request surfaced
(the requested rectangle / item type / layer flags) to know what to reply with.
Add the four `ServerEvent` variants + decode arms (e.g.
`ServerEvent::MapBlockRequested { min_x, max_x, min_y, max_y }`,
`MapItemRequested { item_type, region_handle }`, `MapLayerRequested`), keeping
the existing reply encoders. Low priority — only matters for a server
implementation that actually serves the map.

- [x] DF2 world-map request `ServerEvent`s. Added four receive-side variants to
  `ServerEvent` (`sl-proto/src/sim_session.rs`): `MapBlockRequested { min_x,
  max_x, min_y, max_y, flags }`, `MapNameRequested { name, flags }`,
  `MapItemRequested { item_type: MapItemType, region_handle, flags }`, and
  `MapLayerRequested { flags }`, each decoded in `handle_client_message` from
  `MapBlockRequest` / `MapNameRequest` / `MapItemRequest` / `MapLayerRequest`
  (the item type via the existing `MapItemType::from_u32`, the name via
  `trimmed_string`) so the requests no longer fall through to
  `ServerEvent::ClientMessage`. The reply encoders
  (`send_map_block_reply` / `send_map_item_reply` / `send_map_layer_reply`) are
  unchanged. Server-only — no client `Command`/`Event`, so no runtime/REPL
  changes. Tests (`sl-proto/tests/sim_session.rs`): new
  `world_map_requests_surface_server_events` drives all four requests from the
  real client paths and asserts each dedicated `ServerEvent`; the pre-existing
  `unhandled_client_message_is_surfaced` was repointed at `RequestRegionInfo`
  (still genuinely unhandled) since `MapBlockRequest` now has a variant. Book
  `content/world.md` "In this codebase" updated. **All Tier G items and both
  deferred follow-ups (DF1, DF2) are now complete.**
