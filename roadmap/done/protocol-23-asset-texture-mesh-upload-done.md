---
id: protocol-23
title: Asset/texture/mesh upload (done)
topic: protocol
status: done
origin: ROADMAP.md — Tier C
---

Context: [context/protocol.md](../context/protocol.md).

**23. Asset/texture/mesh upload (done) ✅ — CAPS `NewFileAgentInventory`,
`UploadBakedTexture`, `UpdateGestureAgentInventory`; legacy
`AssetUploadRequest`/`SendXferPacket` · 5 pts.** Upload content over **both**
transports — usable as a content uploader on its own. Per the scope decision,
mesh upload is **bytes-only**: the caller supplies the fully-formed mesh asset
and the client uploads it verbatim; the viewer's model-import pipeline (LOD /
physics-shape / cost generation) is deliberately out of scope. Implemented:

- **Legacy UDP** —
  `Session::upload_asset_udp(asset_type, data, temp_file, store_local)` stores
  an asset via `AssetUploadRequest`, returning the asset's **predicted** UUID
  (`combine(transaction_id, secure_session_id)` — the LL `LLUUID::combine` MD5,
  ported as `sl_wire::combine_uuids`). Small assets (≤ 1200 bytes) are inlined;
  larger ones stream over `Xfer` — the simulator answers with a `RequestXfer`
  (whose `VFileID` is that predicted asset id) and the session streams
  `SendXferPacket`s (packet 0 carrying the 4-byte little-endian length prefix,
  the last flagged `0x80000000`), each pulled by the simulator's
  `ConfirmXferPacket`. Terminates as [`Event::AssetUploadComplete`]. This path
  stores only the asset (no inventory item — a viewer follows up with
  `CreateInventoryItem`).
- **Modern CAPS** — the two-step uploader (POST metadata → `uploader` URL → POST
  raw bytes → `{ new_asset, new_inventory_item }`), wired through both runtimes:
  `UploadAsset` (`NewFileAgentInventory`: stores the asset **and** creates an
  inventory item — folder, asset/inventory type, name, permissions, expected
  cost), `UploadBakedTexture` (a temporary baked texture, no inventory item),
  and `UpdateInventoryAsset` (`Update{Gesture,Notecard,Script,Settings}Agent` —
  replacing an existing item's asset, the cap chosen by asset class). Surfaced
  as `Event::AssetUploaded` / `Event::AssetUploadFailed`.

New value types `InventoryType` (the `LLInventoryType` classes, with `caps_name`
/ `to_code` / `from_code`) and `AssetType::caps_asset_name` / `update_item_cap`;
new sl-wire LLSD builders (`build_new_file_agent_inventory_request`,
`build_update_item_asset_request`, `build_upload_baked_texture_request`) and
parser (`parse_asset_upload_response` → `AssetUploadResponse`). All wired as
`Command`/`SlCommand` variants through both runtimes; the CAPS uploads run on a
background task/thread and emit their events directly (like the #19 fetches).
Covered by four sl-wire unit tests (the `combine_uuids` digest, the
`NewFileAgentInventory` body, the two-step + baked + error response parse, the
update-item body) and three `lifecycle.rs` tests (UDP inline upload + complete,
UDP `Xfer`-streamed upload + multi-packet `SendXferPacket` + complete, and the
CAPS completion decode). *Live-verified against the local OpenSim via the new
`asset_upload` tokio example: a notecard uploaded over **both** the legacy UDP
path (`AssetUploadComplete`, the reported asset id matching the predicted
`combine()` id) and the CAPS `NewFileAgentInventory` (`AssetUploaded` returning
a new asset **and** a new inventory item); a 3 KB notecard exercised the multi-
packet `Xfer` upload path end to end (`success=true`). Test: local OpenSim — no
content tooling needed, the example synthesises a notecard.* Deferred:
`UploadBakedTexture` and the `Update*` caps are SL-shaped (OpenSim uses the
legacy bake) so they are unit-tested only; the full mesh model-import pipeline
is out of scope (bytes-only upload).
